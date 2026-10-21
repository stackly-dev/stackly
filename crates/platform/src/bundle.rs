//! The app bundle that carries `stacklyd` and `stackly` (spec 0015).
//!
//! Every identifier here is temporary until OQ-001 settles the final names
//! before a public release. The Swift app keeps the same values in
//! `StacklyUI/Agent/AgentIdentifiers.swift`; both are checked against one
//! shared fixture, so changing a value means one commit that changes the
//! fixture and both sources.
//!
//! [`AppBundle::enclosing`] is pure: it looks only at the shape of a path the
//! caller already made canonical, and never touches the filesystem. Binaries
//! read `std::env::current_exe()` once in `main`, canonicalize it, and pass
//! the result in; library code never asks the process for its own path.

use std::path::{Path, PathBuf};

/// The bundle identifier of the Stackly app (spec 0001).
pub const APP_BUNDLE_ID: &str = "com.ojiepermana.stackly";

/// The launchd label of the daemon's LaunchAgent.
pub const DAEMON_LABEL: &str = "com.ojiepermana.stackly.daemon";

/// The file name of the LaunchAgent property list inside the bundle, the
/// name `SMAppService` takes.
pub const DAEMON_PLIST_FILE: &str = "com.ojiepermana.stackly.daemon.plist";

/// The code signing identifier of `stacklyd`.
pub const DAEMON_SIGNING_ID: &str = "com.ojiepermana.stackly.daemon";

/// The code signing identifier of `stackly`.
pub const CLI_SIGNING_ID: &str = "com.ojiepermana.stackly.cli";

/// The prefix of the launchd labels that native tests give their temporary
/// LaunchAgents, so cleanup can find them and the daemon treats them as
/// started by launchd.
pub const TEST_AGENT_LABEL_PREFIX: &str = "dev.stackly.test.agent.";

/// The file name of the app executable in `Contents/MacOS`.
pub const APP_EXECUTABLE: &str = "Stackly";

/// The file name of the daemon executable in `Contents/MacOS`.
pub const DAEMON_EXECUTABLE: &str = "stacklyd";

/// The file name of the CLI executable in `Contents/MacOS`. It is not
/// `stackly`: on a case insensitive volume (the APFS default) that would be
/// the same file as the app executable `Stackly`. The command people type is
/// still `stackly`; `~/.local/bin/stackly` is a symlink to this file.
pub const CLI_EXECUTABLE: &str = "stackly-cli";

/// The folder of the bundle's executables, relative to the bundle root.
pub const MACOS_DIR: &str = "Contents/MacOS";

/// The folder of the bundle's LaunchAgent property lists, relative to the
/// bundle root.
pub const LAUNCH_AGENTS_DIR: &str = "Contents/Library/LaunchAgents";

/// The folder of the bundle's resources, relative to the bundle root.
pub const RESOURCES_DIR: &str = "Contents/Resources";

/// The `BundleProgram` of the LaunchAgent: the daemon, relative to the
/// bundle root.
pub const DAEMON_BUNDLE_PROGRAM: &str = "Contents/MacOS/stacklyd";

/// The `ThrottleInterval` of the LaunchAgent in seconds: launchd starts the
/// job again no sooner than this after it started. `cargo xtask build-app`
/// writes it into the property list, and `stackly daemon restart` counts
/// with it to tell when a respawn is overdue, so the two never differ
/// (spec 0015).
pub const LAUNCHD_THROTTLE_INTERVAL_SECS: u64 = 10;

/// The extension of an app bundle folder.
const APP_EXTENSION: &str = "app";

/// A `Stackly.app` (or any `<X>.app`) folder found around an executable.
///
/// It only names paths; it never checks that they exist.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AppBundle {
    root: PathBuf,
}

impl AppBundle {
    /// Returns the bundle that holds `exe`, when `exe` has exactly the shape
    /// `<X>.app/Contents/MacOS/<file>`.
    ///
    /// `exe` must already be absolute and canonical (symlinks resolved); a
    /// relative path never matches. The `.app` extension is compared without
    /// regard to ASCII case, as the default macOS filesystem does.
    #[must_use]
    pub fn enclosing(exe: &Path) -> Option<Self> {
        if !exe.is_absolute() {
            return None;
        }
        exe.file_name()?;
        let macos = exe.parent()?;
        if macos.file_name()? != "MacOS" {
            return None;
        }
        let contents = macos.parent()?;
        if contents.file_name()? != "Contents" {
            return None;
        }
        let root = contents.parent()?;
        let extension = root.extension()?.to_str()?;
        if !extension.eq_ignore_ascii_case(APP_EXTENSION) || root.file_stem().is_none() {
            return None;
        }
        Some(Self {
            root: root.to_path_buf(),
        })
    }

    /// Returns the bundle folder, for example `/Applications/Stackly.app`.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Returns `<root>/Contents`.
    #[must_use]
    pub fn contents_dir(&self) -> PathBuf {
        self.root.join("Contents")
    }

    /// Returns `<root>/Contents/MacOS`.
    #[must_use]
    pub fn macos_dir(&self) -> PathBuf {
        self.root.join(MACOS_DIR)
    }

    /// Returns `<root>/Contents/Resources`, where the bundled catalog lives
    /// (spec 0017).
    #[must_use]
    pub fn resources_dir(&self) -> PathBuf {
        self.root.join(RESOURCES_DIR)
    }

    /// Returns `<root>/Contents/Library/LaunchAgents`.
    #[must_use]
    pub fn launch_agents_dir(&self) -> PathBuf {
        self.root.join(LAUNCH_AGENTS_DIR)
    }

    /// Returns the daemon executable, `<root>/Contents/MacOS/stacklyd`.
    #[must_use]
    pub fn daemon_executable(&self) -> PathBuf {
        self.macos_dir().join(DAEMON_EXECUTABLE)
    }

    /// Returns the CLI executable, `<root>/Contents/MacOS/stackly-cli`.
    #[must_use]
    pub fn cli_executable(&self) -> PathBuf {
        self.macos_dir().join(CLI_EXECUTABLE)
    }

    /// Returns the LaunchAgent property list,
    /// `<root>/Contents/Library/LaunchAgents/com.ojiepermana.stackly.daemon.plist`.
    #[must_use]
    pub fn daemon_plist(&self) -> PathBuf {
        self.launch_agents_dir().join(DAEMON_PLIST_FILE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bundle_of(exe: &str) -> Option<PathBuf> {
        AppBundle::enclosing(Path::new(exe)).map(|bundle| bundle.root().to_path_buf())
    }

    #[test]
    fn an_executable_in_contents_macos_belongs_to_the_bundle() {
        assert_eq!(
            bundle_of("/Applications/Stackly.app/Contents/MacOS/stacklyd"),
            Some(PathBuf::from("/Applications/Stackly.app"))
        );
    }

    #[test]
    fn the_app_extension_matches_without_regard_to_case() {
        assert_eq!(
            bundle_of("/Applications/Stackly.APP/Contents/MacOS/stackly-cli"),
            Some(PathBuf::from("/Applications/Stackly.APP"))
        );
    }

    #[test]
    fn a_relative_path_never_matches() {
        assert_eq!(bundle_of("Stackly.app/Contents/MacOS/stacklyd"), None);
    }

    #[test]
    fn a_cargo_build_output_is_not_a_bundle() {
        assert_eq!(bundle_of("/work/stackly/target/debug/stacklyd"), None);
    }

    #[test]
    fn a_folder_without_the_app_extension_is_not_a_bundle() {
        assert_eq!(
            bundle_of("/Applications/Stackly/Contents/MacOS/stacklyd"),
            None
        );
    }

    #[test]
    fn a_file_deeper_than_contents_macos_is_not_a_bundle_executable() {
        assert_eq!(
            bundle_of("/Applications/Stackly.app/Contents/MacOS/tools/stackly"),
            None
        );
    }

    #[test]
    fn a_resource_file_is_not_a_bundle_executable() {
        assert_eq!(
            bundle_of("/Applications/Stackly.app/Contents/Resources/stackly"),
            None
        );
    }

    #[test]
    fn a_bare_dot_app_name_is_not_a_bundle() {
        assert_eq!(bundle_of("/.app/Contents/MacOS/stacklyd"), None);
    }

    #[test]
    fn the_bundle_names_every_path_spec_0015_lists() {
        let bundle = AppBundle::enclosing(Path::new(
            "/Users/me/Applications/Stackly.app/Contents/MacOS/Stackly",
        ))
        .expect("the path has the bundle shape");
        let root = Path::new("/Users/me/Applications/Stackly.app");
        assert_eq!(bundle.contents_dir(), root.join("Contents"));
        assert_eq!(bundle.macos_dir(), root.join("Contents/MacOS"));
        assert_eq!(bundle.resources_dir(), root.join("Contents/Resources"));
        assert_eq!(
            bundle.launch_agents_dir(),
            root.join("Contents/Library/LaunchAgents")
        );
        assert_eq!(
            bundle.daemon_executable(),
            root.join("Contents/MacOS/stacklyd")
        );
        assert_eq!(
            bundle.cli_executable(),
            root.join("Contents/MacOS/stackly-cli")
        );
        assert_eq!(
            bundle.daemon_plist(),
            root.join("Contents/Library/LaunchAgents/com.ojiepermana.stackly.daemon.plist")
        );
    }

    #[test]
    fn the_bundle_program_is_the_daemon_executable_relative_to_the_root() {
        let bundle = AppBundle::enclosing(Path::new("/A/Stackly.app/Contents/MacOS/stacklyd"))
            .expect("the path has the bundle shape");
        assert_eq!(
            bundle.root().join(DAEMON_BUNDLE_PROGRAM),
            bundle.daemon_executable()
        );
    }

    #[test]
    fn the_three_executable_names_differ_even_without_regard_to_case() {
        let names = [APP_EXECUTABLE, DAEMON_EXECUTABLE, CLI_EXECUTABLE];
        for (index, first) in names.iter().enumerate() {
            for second in &names[index + 1..] {
                assert!(!first.eq_ignore_ascii_case(second), "{first} and {second}");
            }
        }
    }
}
