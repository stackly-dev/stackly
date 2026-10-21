//! The background agent through `SMAppService` (spec 0015).
//!
//! The adapter gets the canonical path of the executable it runs from and
//! finds the bundle around it (`AppBundle::enclosing`). It never reads
//! `NSBundle.mainBundle`: through a symlink that value is the symlink's
//! folder without an identifier (spike B2 of spec 0015), while
//! `SMAppService` itself answers for the real bundle. Before any
//! `SMAppService` call it checks that the bundle's `Info.plist` names the
//! Stackly app and that the bundle holds both the LaunchAgent property list
//! and `stacklyd`; a process outside the bundle (a cargo build, a test) or a
//! bundle without the helper (a build from the Xcode IDE) gets an answer
//! without the OS being asked, so it can never register anything.

use std::path::{Path, PathBuf};

use objc2::rc::{Retained, autoreleasepool};
use objc2_foundation::{NSBundle, NSError, NSString};
// The SDK names of the error codes are lower camel case, which a match
// pattern would flag, so they get upper case aliases here.
use objc2_service_management::{
    SMAppService, SMAppServiceErrorDomain, SMAppServiceStatus,
    kSMErrorAlreadyRegistered as ALREADY_REGISTERED, kSMErrorInvalidSignature as INVALID_SIGNATURE,
    kSMErrorJobMustBeEnabled as JOB_MUST_BE_ENABLED, kSMErrorJobNotFound as JOB_NOT_FOUND,
    kSMErrorJobPlistNotFound as JOB_PLIST_NOT_FOUND,
    kSMErrorLaunchDeniedByUser as LAUNCH_DENIED_BY_USER,
};
use stackly_platform::bundle::{
    APP_BUNDLE_ID, AppBundle, DAEMON_BUNDLE_PROGRAM, DAEMON_PLIST_FILE, LAUNCH_AGENTS_DIR,
};
use stackly_platform::{
    AgentOp, AgentOsErrorKind, AgentProbe, AgentRegistration, BackgroundAgent, BackgroundAgentError,
};

/// The daemon's LaunchAgent, registered with `SMAppService.agent`.
///
/// Every method runs inside its own autorelease pool, so it is safe on any
/// thread; the CLI calls it from a blocking thread with a time limit.
#[derive(Debug, Clone)]
pub struct MacosBackgroundAgent {
    executable: PathBuf,
}

impl MacosBackgroundAgent {
    /// Creates the adapter for the process that runs from `executable`, the
    /// canonical path (symlinks resolved) that `main` read once.
    #[must_use]
    pub fn new(executable: PathBuf) -> Self {
        Self { executable }
    }

    /// Creates an adapter that does not know its executable, the one
    /// `MacosPlatform` installs so the capability report is honest. Every
    /// method answers [`AgentProbe::NotBundled`] with an empty path without
    /// asking the OS.
    #[must_use]
    pub fn unlocated() -> Self {
        Self::new(PathBuf::new())
    }

    /// Finds the bundle and its service, then runs `call`; answers without
    /// the OS when the executable is not inside a complete Stackly bundle.
    fn with_service<F>(&self, call: F) -> Result<AgentProbe, BackgroundAgentError>
    where
        F: FnOnce(&SMAppService, &Path) -> Result<AgentProbe, BackgroundAgentError>,
    {
        autoreleasepool(|_| {
            let bundle = match self.locate() {
                Ok(bundle) => bundle,
                Err(probe) => return Ok(probe),
            };
            let name = NSString::from_str(DAEMON_PLIST_FILE);
            // SAFETY: `agentServiceWithPlistName:` only builds a service
            // object for a property list name; `name` is a valid NSString
            // that outlives the call, and the method has no other
            // precondition.
            let service = unsafe { SMAppService::agentServiceWithPlistName(&name) };
            call(&service, &bundle)
        })
    }

    /// Returns the bundle folder when the executable sits in a Stackly
    /// bundle that holds the property list and `stacklyd`, else the probe
    /// to answer.
    fn locate(&self) -> Result<PathBuf, AgentProbe> {
        let not_bundled = || AgentProbe::NotBundled {
            executable: self.executable.clone(),
        };
        let bundle = AppBundle::enclosing(&self.executable).ok_or_else(not_bundled)?;
        let root = bundle.root().to_path_buf();
        let path = NSString::from_str(&root.to_string_lossy());
        let identifier = NSBundle::bundleWithPath(&path)
            .and_then(|bundle| bundle.bundleIdentifier())
            .map(|id| id.to_string());
        if identifier.as_deref() != Some(APP_BUNDLE_ID) {
            return Err(not_bundled());
        }
        if helper_present(&root) {
            Ok(root)
        } else {
            Err(AgentProbe::HelperMissing { bundle: root })
        }
    }
}

impl BackgroundAgent for MacosBackgroundAgent {
    fn status(&self) -> Result<AgentProbe, BackgroundAgentError> {
        self.with_service(|service, bundle| Ok(probe(service, bundle)))
    }

    fn register(&self) -> Result<AgentProbe, BackgroundAgentError> {
        self.with_service(|service, bundle| {
            register(service, AgentOp::Register)?;
            Ok(probe(service, bundle))
        })
    }

    fn unregister(&self) -> Result<AgentProbe, BackgroundAgentError> {
        self.with_service(|service, bundle| {
            unregister(service, AgentOp::Unregister)?;
            Ok(probe(service, bundle))
        })
    }

    fn relaunch(&self) -> Result<AgentProbe, BackgroundAgentError> {
        self.with_service(|service, bundle| {
            // A job the OS no longer knows has nothing to unregister; the
            // register step that follows still starts it.
            match unregister(service, AgentOp::Relaunch) {
                Err(BackgroundAgentError::Os {
                    kind: AgentOsErrorKind::NotFound,
                    ..
                })
                | Ok(()) => {}
                Err(error) => return Err(error),
            }
            match register(service, AgentOp::Relaunch) {
                Err(BackgroundAgentError::Os {
                    kind: AgentOsErrorKind::AlreadyRegistered,
                    ..
                })
                | Ok(()) => {}
                Err(error) => return Err(error),
            }
            Ok(probe(service, bundle))
        })
    }
}

/// Returns the parent process ID of this process, which
/// `LaunchContext::detect` compares with 1 (launchd). It lives here because
/// the daemon may not name `std::os::unix::process` (`cargo xtask
/// check-deps`, spec 0010 step 15).
#[must_use]
pub fn parent_pid() -> u32 {
    // SAFETY: `getppid` takes no arguments, never fails, and only reads the
    // process table entry of the caller.
    let pid = unsafe { libc::getppid() };
    u32::try_from(pid).unwrap_or(0)
}

/// Returns whether the bundle at `root` holds the LaunchAgent property list
/// and the daemon executable.
fn helper_present(root: &Path) -> bool {
    root.join(LAUNCH_AGENTS_DIR)
        .join(DAEMON_PLIST_FILE)
        .is_file()
        && root.join(DAEMON_BUNDLE_PROGRAM).is_file()
}

/// Reads the status of `service` as a probe for `bundle`.
fn probe(service: &SMAppService, bundle: &Path) -> AgentProbe {
    // SAFETY: `status` is a read only property of a valid service object
    // and has no precondition.
    let SMAppServiceStatus(raw) = unsafe { service.status() };
    AgentProbe::Registration {
        registration: map_status(raw),
        bundle: bundle.to_path_buf(),
    }
}

fn register(service: &SMAppService, op: AgentOp) -> Result<(), BackgroundAgentError> {
    // SAFETY: `registerAndReturnError:` is called on a valid service object
    // and reports failure through the returned error, never by unwinding.
    unsafe { service.registerAndReturnError() }.map_err(|error| os_error(op, &error))
}

fn unregister(service: &SMAppService, op: AgentOp) -> Result<(), BackgroundAgentError> {
    // SAFETY: `unregisterAndReturnError:` is called on a valid service
    // object and reports failure through the returned error, never by
    // unwinding.
    unsafe { service.unregisterAndReturnError() }.map_err(|error| os_error(op, &error))
}

/// Maps an `NSError` from `SMAppService` to the port error.
fn os_error(op: AgentOp, error: &Retained<NSError>) -> BackgroundAgentError {
    let domain = error.domain().to_string();
    let code = i64::try_from(error.code()).unwrap_or(i64::MIN);
    // SAFETY: `SMAppServiceErrorDomain` is an immutable NSString constant
    // that the ServiceManagement framework exports for the life of the
    // process.
    let service_domain = unsafe { SMAppServiceErrorDomain }.to_string();
    let kind = if domain == service_domain {
        map_error_code(code)
    } else {
        AgentOsErrorKind::Other
    };
    BackgroundAgentError::Os {
        op,
        kind,
        domain,
        code,
        message: error.localizedDescription().to_string(),
    }
}

/// Maps a raw `SMAppServiceStatus` value to the port's registration.
#[must_use]
pub(crate) fn map_status(raw: isize) -> AgentRegistration {
    match SMAppServiceStatus(raw) {
        SMAppServiceStatus::Enabled => AgentRegistration::Enabled,
        SMAppServiceStatus::RequiresApproval => AgentRegistration::RequiresApproval,
        SMAppServiceStatus::NotRegistered => AgentRegistration::NotRegistered,
        SMAppServiceStatus::NotFound => AgentRegistration::NotFound,
        SMAppServiceStatus(other) => {
            AgentRegistration::Unrecognized(i64::try_from(other).unwrap_or(i64::MIN))
        }
    }
}

/// Maps an error code of `SMAppServiceErrorDomain` to its kind, with the
/// table *Hasil pendaftaran* of spec 0015. The values come from the SDK
/// header (`ServiceManagement/SMErrors.h`) through the binding, never typed
/// by hand.
#[must_use]
pub(crate) fn map_error_code(code: i64) -> AgentOsErrorKind {
    let Ok(code) = u32::try_from(code) else {
        return AgentOsErrorKind::Other;
    };
    match code {
        ALREADY_REGISTERED => AgentOsErrorKind::AlreadyRegistered,
        LAUNCH_DENIED_BY_USER => AgentOsErrorKind::LaunchDenied,
        JOB_MUST_BE_ENABLED => AgentOsErrorKind::MustBeEnabled,
        INVALID_SIGNATURE => AgentOsErrorKind::InvalidSignature,
        JOB_NOT_FOUND | JOB_PLIST_NOT_FOUND => AgentOsErrorKind::NotFound,
        _ => AgentOsErrorKind::Other,
    }
}

#[cfg(test)]
mod tests {
    use objc2_service_management::{
        kSMErrorAlreadyRegistered, kSMErrorInternalFailure, kSMErrorInvalidPlist,
        kSMErrorInvalidSignature, kSMErrorJobMustBeEnabled, kSMErrorJobNotFound,
        kSMErrorJobPlistNotFound, kSMErrorLaunchDeniedByUser,
    };
    use stackly_testkit::TestRoot;

    use super::*;

    #[test]
    fn known_statuses_map_to_their_registration() {
        assert_eq!(map_status(0), AgentRegistration::NotRegistered);
        assert_eq!(map_status(1), AgentRegistration::Enabled);
        assert_eq!(map_status(2), AgentRegistration::RequiresApproval);
        assert_eq!(map_status(3), AgentRegistration::NotFound);
    }

    #[test]
    fn an_unknown_status_keeps_its_raw_value() {
        assert_eq!(map_status(42), AgentRegistration::Unrecognized(42));
        assert_eq!(map_status(-1), AgentRegistration::Unrecognized(-1));
    }

    #[test]
    fn header_error_codes_map_to_the_spec_table() {
        let cases = [
            (
                kSMErrorAlreadyRegistered,
                AgentOsErrorKind::AlreadyRegistered,
            ),
            (kSMErrorLaunchDeniedByUser, AgentOsErrorKind::LaunchDenied),
            (kSMErrorJobMustBeEnabled, AgentOsErrorKind::MustBeEnabled),
            (kSMErrorInvalidSignature, AgentOsErrorKind::InvalidSignature),
            (kSMErrorJobNotFound, AgentOsErrorKind::NotFound),
            (kSMErrorJobPlistNotFound, AgentOsErrorKind::NotFound),
            (kSMErrorInternalFailure, AgentOsErrorKind::Other),
            (kSMErrorInvalidPlist, AgentOsErrorKind::Other),
        ];
        for (code, kind) in cases {
            assert_eq!(map_error_code(i64::from(code)), kind, "code {code}");
        }
    }

    #[test]
    fn a_negative_or_huge_code_is_other() {
        assert_eq!(map_error_code(-5), AgentOsErrorKind::Other);
        assert_eq!(map_error_code(i64::MAX), AgentOsErrorKind::Other);
    }

    /// Builds a fake bundle folder under `root` with an `Info.plist` naming
    /// `identifier` (none when `None`), and returns the path of its
    /// executable. Every case needs its own folder, because `NSBundle` keeps
    /// one object per path for the life of the process.
    fn fake_bundle(root: &Path, name: &str, identifier: Option<&str>, helper: bool) -> PathBuf {
        let bundle = root.join(name);
        let macos = bundle.join("Contents/MacOS");
        std::fs::create_dir_all(&macos).unwrap();
        if let Some(identifier) = identifier {
            let plist = format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<plist version=\"1.0\"><dict>\
                 <key>CFBundleIdentifier</key><string>{identifier}</string></dict></plist>\n"
            );
            std::fs::write(bundle.join("Contents/Info.plist"), plist).unwrap();
        }
        if helper {
            std::fs::create_dir_all(bundle.join(LAUNCH_AGENTS_DIR)).unwrap();
            std::fs::write(bundle.join(LAUNCH_AGENTS_DIR).join(DAEMON_PLIST_FILE), b"").unwrap();
            std::fs::write(bundle.join(DAEMON_BUNDLE_PROGRAM), b"").unwrap();
        }
        let exe = macos.join("stackly");
        std::fs::write(&exe, b"").unwrap();
        exe.canonicalize().unwrap()
    }

    fn canonical_root(test_root: &TestRoot) -> PathBuf {
        test_root.path().canonicalize().unwrap()
    }

    #[test]
    fn a_complete_stackly_bundle_is_located() {
        let test_root = TestRoot::new().unwrap();
        let root = canonical_root(&test_root);
        let exe = fake_bundle(&root, "Complete.app", Some(APP_BUNDLE_ID), true);
        let located = MacosBackgroundAgent::new(exe).locate();
        assert_eq!(located, Ok(root.join("Complete.app")));
    }

    #[test]
    fn another_identifier_is_not_bundled() {
        let test_root = TestRoot::new().unwrap();
        let root = canonical_root(&test_root);
        let exe = fake_bundle(&root, "Other.app", Some("com.example.other"), true);
        assert_eq!(
            MacosBackgroundAgent::new(exe.clone()).locate(),
            Err(AgentProbe::NotBundled { executable: exe })
        );
    }

    #[test]
    fn a_bundle_without_an_info_plist_is_not_bundled() {
        let test_root = TestRoot::new().unwrap();
        let root = canonical_root(&test_root);
        let exe = fake_bundle(&root, "Bare.app", None, true);
        assert_eq!(
            MacosBackgroundAgent::new(exe.clone()).locate(),
            Err(AgentProbe::NotBundled { executable: exe })
        );
    }

    #[test]
    fn a_stackly_bundle_without_the_helper_is_helper_missing() {
        let test_root = TestRoot::new().unwrap();
        let root = canonical_root(&test_root);
        let exe = fake_bundle(&root, "Xcode.app", Some(APP_BUNDLE_ID), false);
        assert_eq!(
            MacosBackgroundAgent::new(exe).locate(),
            Err(AgentProbe::HelperMissing {
                bundle: root.join("Xcode.app")
            })
        );
    }

    #[test]
    fn a_path_canonicalized_from_a_symlink_locates_the_real_bundle() {
        let test_root = TestRoot::new().unwrap();
        let root = canonical_root(&test_root);
        let exe = fake_bundle(&root, "Linked.app", Some(APP_BUNDLE_ID), true);
        let link = root.join("stackly-link");
        std::os::unix::fs::symlink(&exe, &link).unwrap();
        let through_link = link.canonicalize().unwrap();
        assert_eq!(
            MacosBackgroundAgent::new(through_link).locate(),
            Ok(root.join("Linked.app"))
        );
    }

    #[test]
    fn an_unlocated_adapter_answers_not_bundled_with_an_empty_path_for_every_call() {
        let agent = MacosBackgroundAgent::unlocated();
        let expected = AgentProbe::NotBundled {
            executable: PathBuf::new(),
        };
        assert_eq!(agent.status(), Ok(expected.clone()));
        assert_eq!(agent.register(), Ok(expected.clone()));
        assert_eq!(agent.unregister(), Ok(expected.clone()));
        assert_eq!(agent.relaunch(), Ok(expected));
    }

    #[test]
    fn a_folder_with_both_files_has_the_helper() {
        let test_root = TestRoot::new().unwrap();
        let root = test_root.path().join("Stackly.app");
        std::fs::create_dir_all(root.join(LAUNCH_AGENTS_DIR)).unwrap();
        std::fs::create_dir_all(root.join("Contents/MacOS")).unwrap();
        assert!(!helper_present(&root));
        std::fs::write(root.join(LAUNCH_AGENTS_DIR).join(DAEMON_PLIST_FILE), b"").unwrap();
        assert!(!helper_present(&root));
        std::fs::write(root.join(DAEMON_BUNDLE_PROGRAM), b"").unwrap();
        assert!(helper_present(&root));
    }
}
