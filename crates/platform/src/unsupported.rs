use std::path::{Path, PathBuf};
use std::sync::Arc;

use stackly_domain::{Os, OsVersion, Target};

use crate::capability::{CapabilityStatus, PlatformCapabilities, PlatformCapability};
use crate::clock::SystemClock;
use crate::error::PlatformError;
use crate::fs::{
    AppendFile, DirEntryInfo, FileInfo, Filesystem, ReadSeekFile, RemoveReport, TreeUsage,
    UsageLimits, VolumeFacts, VolumeSpace,
};
use crate::identity::{UserId, UserIdentity};
use crate::info::{HostFacts, PlatformInfo};
use crate::layout::{DirectoryResolver, RootSet};
use crate::platform::Platform;
use crate::process::UnsupportedProcessSupervision;
use crate::secret::UnsupportedSecretStore;
use crate::tree::TreeWriter;

/// The platform for an OS that has no adapter yet, such as Windows today.
///
/// Directories, filesystem, and user identity always answer
/// [`PlatformError::NotSupported`], the secret store is an
/// [`UnsupportedSecretStore`], and process supervision an
/// [`UnsupportedProcessSupervision`]; the clock works. Every capability but the
/// clock reports not supported, so the next port only has to swap the
/// adapter, never core.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnsupportedPlatform {
    target: Target,
}

impl UnsupportedPlatform {
    /// Assembles the platform for `target`.
    #[expect(
        clippy::new_ret_no_self,
        reason = "spec 0004: an adapter's constructor hands back the assembled Platform"
    )]
    #[must_use]
    pub fn new(target: Target) -> Platform {
        let ports = Arc::new(Self { target });
        Platform::new(
            ports.clone(),
            ports.clone(),
            Arc::new(SystemClock),
            ports.clone(),
            ports,
        )
        .with_secrets(Arc::new(UnsupportedSecretStore::new(target.os())))
        .with_processes(Arc::new(UnsupportedProcessSupervision::new(target.os())))
    }

    fn not_supported(&self, capability: PlatformCapability) -> PlatformError {
        PlatformError::NotSupported {
            capability,
            os: self.target.os(),
        }
    }
}

impl DirectoryResolver for UnsupportedPlatform {
    fn roots(&self) -> Result<RootSet, PlatformError> {
        Err(self.not_supported(PlatformCapability::Directories))
    }
}

impl Filesystem for UnsupportedPlatform {
    fn canonicalize(&self, _path: &Path) -> Result<PathBuf, PlatformError> {
        Err(self.not_supported(PlatformCapability::Filesystem))
    }

    fn symlink_metadata(&self, _path: &Path) -> Result<Option<FileInfo>, PlatformError> {
        Err(self.not_supported(PlatformCapability::Filesystem))
    }

    fn create_dir_private(&self, _path: &Path) -> Result<(), PlatformError> {
        Err(self.not_supported(PlatformCapability::Filesystem))
    }

    fn read_dir(&self, _path: &Path) -> Result<Vec<DirEntryInfo>, PlatformError> {
        Err(self.not_supported(PlatformCapability::Filesystem))
    }

    fn read_file_limited(&self, _path: &Path, _max_len: u64) -> Result<Vec<u8>, PlatformError> {
        Err(self.not_supported(PlatformCapability::Filesystem))
    }

    fn write_file_atomic(&self, _path: &Path, _bytes: &[u8]) -> Result<(), PlatformError> {
        Err(self.not_supported(PlatformCapability::Filesystem))
    }

    fn remove_tree(&self, _root: &Path, _relative: &Path) -> Result<RemoveReport, PlatformError> {
        Err(self.not_supported(PlatformCapability::Filesystem))
    }

    fn open_read(&self, _path: &Path) -> Result<Box<dyn ReadSeekFile>, PlatformError> {
        Err(self.not_supported(PlatformCapability::Filesystem))
    }

    fn open_append(&self, _path: &Path) -> Result<Box<dyn AppendFile>, PlatformError> {
        Err(self.not_supported(PlatformCapability::Filesystem))
    }

    fn rename_no_replace(&self, _from: &Path, _to: &Path) -> Result<(), PlatformError> {
        Err(self.not_supported(PlatformCapability::Filesystem))
    }

    fn open_tree_writer(&self, _root: &Path) -> Result<Box<dyn TreeWriter>, PlatformError> {
        Err(self.not_supported(PlatformCapability::Filesystem))
    }

    fn volume_space(&self, _path: &Path) -> Result<VolumeSpace, PlatformError> {
        Err(self.not_supported(PlatformCapability::Filesystem))
    }

    fn volume_facts(&self, _path: &Path) -> Result<VolumeFacts, PlatformError> {
        Err(self.not_supported(PlatformCapability::Filesystem))
    }

    fn tree_usage(&self, _root: &Path, _limits: UsageLimits) -> Result<TreeUsage, PlatformError> {
        Err(self.not_supported(PlatformCapability::Filesystem))
    }
}

impl UserIdentity for UnsupportedPlatform {
    fn current_user(&self) -> Result<UserId, PlatformError> {
        Err(self.not_supported(PlatformCapability::UserIdentity))
    }
}

impl PlatformInfo for UnsupportedPlatform {
    fn target(&self) -> Target {
        self.target
    }

    fn capabilities(&self) -> PlatformCapabilities {
        let reason = match self.target.os() {
            Os::Windows => "windows adapter not built",
            Os::Linux => "linux adapter not built",
            Os::Macos => "macos adapter not in use",
        };
        PlatformCapabilities::none(reason)
            .with(PlatformCapability::Clock, CapabilityStatus::Supported)
    }

    fn host_facts(&self) -> HostFacts {
        // No path pretends to know the version of an OS without an adapter.
        HostFacts::new(self.target, OsVersion::major_minor(0, 0), None)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use stackly_domain::{Arch, SecretRef};
    use stackly_evidence::{Kind, evidence};

    use super::*;
    use crate::secret::{
        Interaction, SecretBackend, SecretStoreError, SecretValue, StoreState, WriteMode,
    };

    const WINDOWS: Target = Target::new(Os::Windows, Arch::Aarch64);

    fn is_not_supported<T: std::fmt::Debug>(
        result: Result<T, PlatformError>,
        expected: PlatformCapability,
    ) -> bool {
        matches!(
            result,
            Err(PlatformError::NotSupported { capability, os: Os::Windows }) if capability == expected
        )
    }

    #[test]
    fn windows_capabilities_report_not_supported() {
        use PlatformCapability::{Directories, Filesystem as Fs, UserIdentity as Identity};

        let ev = evidence!(
            covers = ["AC-050", "REQ-050"],
            kind = Kind::Fixture,
            expected = "the windows platform reports every capability but the clock as not supported with reason `windows adapter not built`, and every method of the directory, filesystem, and identity ports answers NotSupported naming the capability and windows",
        );
        let platform = UnsupportedPlatform::new(WINDOWS);
        let capabilities = platform.info().capabilities();

        assert_eq!(platform.info().target(), WINDOWS);
        for capability in PlatformCapability::ALL {
            let expected = if capability == PlatformCapability::Clock {
                CapabilityStatus::Supported
            } else {
                CapabilityStatus::NotSupported {
                    reason: "windows adapter not built",
                }
            };
            assert_eq!(capabilities.status(capability), expected, "{capability}");
        }
        let fs = platform.fs();
        let path = Path::new("C:\\stackly");
        assert!(is_not_supported(
            platform.directories().roots(),
            Directories
        ));
        assert!(is_not_supported(fs.canonicalize(path), Fs));
        assert!(is_not_supported(fs.symlink_metadata(path), Fs));
        assert!(is_not_supported(fs.create_dir_private(path), Fs));
        assert!(is_not_supported(fs.read_dir(path), Fs));
        assert!(is_not_supported(fs.read_file_limited(path, 1), Fs));
        assert!(is_not_supported(fs.write_file_atomic(path, b"x"), Fs));
        assert!(is_not_supported(
            fs.remove_tree(path, Path::new("instances")),
            Fs
        ));
        assert!(is_not_supported(fs.open_read(path).map(drop), Fs));
        assert!(is_not_supported(fs.open_append(path).map(drop), Fs));
        assert!(is_not_supported(fs.rename_no_replace(path, path), Fs));
        assert!(is_not_supported(fs.open_tree_writer(path).map(drop), Fs));
        assert!(is_not_supported(fs.volume_space(path), Fs));
        assert!(is_not_supported(fs.volume_facts(path), Fs));
        assert!(is_not_supported(fs.available_space(path), Fs));
        assert!(is_not_supported(
            fs.tree_usage(path, UsageLimits::new(1, Duration::ZERO)),
            Fs
        ));
        assert!(is_not_supported(
            platform.identity().current_user(),
            Identity
        ));
        ev.pass();
    }

    #[test]
    fn the_secret_store_of_an_unsupported_platform_refuses_every_call() {
        let platform = UnsupportedPlatform::new(WINDOWS);
        let secrets = platform.secrets();
        let reference = SecretRef::parse("local:erp/admin").unwrap();
        let value = SecretValue::new("not-a-real-secret".to_owned()).unwrap();
        let not_supported = SecretStoreError::NotSupported { os: Os::Windows };

        assert_eq!(secrets.backend(), SecretBackend::Unsupported);
        assert_eq!(secrets.state(), StoreState::Unsupported);
        assert_eq!(secrets.probe(&reference), Err(not_supported));
        assert_eq!(
            secrets.read(&reference, Interaction::Never).unwrap_err(),
            not_supported
        );
        assert_eq!(
            secrets.write(&reference, &value, WriteMode::Create, Interaction::Never),
            Err(not_supported)
        );
        assert_eq!(
            secrets.delete(&reference, Interaction::Never),
            Err(not_supported)
        );
    }

    #[test]
    fn a_new_platform_has_an_unsupported_secret_store_until_one_is_installed() {
        let ports = Arc::new(UnsupportedPlatform { target: WINDOWS });
        let reference = SecretRef::parse("local:erp/admin").unwrap();
        let platform = Platform::new(
            ports.clone(),
            ports.clone(),
            Arc::new(SystemClock),
            ports.clone(),
            ports,
        );

        assert_eq!(
            platform.secrets().probe(&reference),
            Err(SecretStoreError::NotSupported { os: Os::Windows })
        );
        let linux: Arc<dyn crate::secret::SecretStore> =
            Arc::new(UnsupportedSecretStore::new(Os::Linux));
        let platform = platform.with_secrets(Arc::clone(&linux));
        assert_eq!(
            platform.secrets().probe(&reference),
            Err(SecretStoreError::NotSupported { os: Os::Linux })
        );
        assert!(Arc::ptr_eq(&platform.shared_secrets(), &linux));
        assert!(format!("{platform:?}").contains("secrets: Unsupported"));
    }

    #[test]
    fn the_process_supervision_of_an_unsupported_platform_refuses_every_call() {
        use stackly_domain::{BootId, Pid, ProcessIdentity, ProcessStart};

        use crate::fs::FileIdentity;
        use crate::process::{
            ChildRole, ExitListener, ExitNotice, HelperHandle, OutputTarget, ProcessError,
            ProcessFacts, ProviderSignal, SignalTarget, SpawnRequest, StdinSource, SuspendedChild,
        };

        struct Ignore;
        impl ExitListener for Ignore {
            fn exited(&self, _notice: ExitNotice) {}
        }

        let ev = evidence!(
            covers = ["AC-050"],
            kind = Kind::Fixture,
            expected = "the windows platform's process supervision port answers NotSupported naming process_supervision and windows from every one of its sixteen methods",
        );
        let platform = UnsupportedPlatform::new(WINDOWS);
        let processes = platform.processes();
        let not_supported = || ProcessError::NotSupported {
            capability: PlatformCapability::ProcessSupervision,
            os: Os::Windows,
        };
        let pid = Pid::new(4242).unwrap();
        let start = ProcessStart::new(1_700_000_000, 0).unwrap();
        let path = Path::new("C:\\stackly\\instance.lock");
        let child = || {
            SuspendedChild::new(ProcessFacts {
                pid,
                parent: None,
                group: pid,
                session: pid,
                uid: UserId::Windows("S-1-5-21".to_owned()),
                start,
                executable: PathBuf::from("C:\\stackly\\server.exe"),
                stopped: true,
            })
        };
        let request = |role| SpawnRequest {
            program: Path::new("server.exe"),
            args: &[],
            env: &[],
            cwd: Path::new("C:\\stackly"),
            stdin: StdinSource::Null,
            output: OutputTarget::Capture,
            lock: None,
            role,
        };
        let target = SignalTarget::new(ProcessIdentity {
            pid,
            start,
            executable: PathBuf::from("C:\\stackly\\server.exe"),
            session: pid,
            boot: "6f9619ff-8b86-d011-b42d-00c04fc964ff"
                .parse::<BootId>()
                .unwrap(),
        });
        let file = FileIdentity {
            device: 1,
            inode: 2,
        };

        assert_eq!(
            processes.spawn_main(request(ChildRole::Main)),
            Err(not_supported())
        );
        assert_eq!(processes.resume(child()), Err(not_supported()));
        assert_eq!(processes.discard(child()), Err(not_supported()));
        assert_eq!(
            processes.spawn_helper(request(ChildRole::Probe)),
            Err(not_supported())
        );
        assert_eq!(processes.inspect(pid), Err(not_supported()));
        assert_eq!(processes.holds_file(pid, file), Err(not_supported()));
        assert_eq!(
            processes.signal(&target, ProviderSignal::Term),
            Err(not_supported())
        );
        assert_eq!(processes.continue_stopped(&target), Err(not_supported()));
        assert_eq!(
            processes.terminate_helper_group(&HelperHandle::new(pid, ChildRole::Probe), true),
            Err(not_supported())
        );
        assert_eq!(processes.group_members(pid), Err(not_supported()));
        assert_eq!(processes.boot_id(), Err(not_supported()));
        assert_eq!(
            processes.watch_exit(pid, Arc::new(Ignore)),
            Err(not_supported())
        );
        assert!(matches!(processes.lock_instance(path), Err(e) if e == not_supported()));
        assert_eq!(processes.probe_lock(path), Err(not_supported()));
        assert!(matches!(processes.create_spool(path), Err(e) if e == not_supported()));
        assert!(matches!(processes.open_spool(path), Err(e) if e == not_supported()));
        assert_eq!(
            not_supported().to_string(),
            "process_supervision is not supported on windows"
        );
        ev.pass();
    }

    #[test]
    fn a_new_platform_has_unsupported_process_supervision_until_one_is_installed() {
        use stackly_domain::Pid;

        use crate::process::{ProcessError, UnsupportedProcessSupervision};

        let ports = Arc::new(UnsupportedPlatform { target: WINDOWS });
        let pid = Pid::new(1).unwrap();
        let platform = Platform::new(
            ports.clone(),
            ports.clone(),
            Arc::new(SystemClock),
            ports.clone(),
            ports,
        );
        let refused = |os| ProcessError::NotSupported {
            capability: PlatformCapability::ProcessSupervision,
            os,
        };

        assert_eq!(platform.processes().inspect(pid), Err(refused(Os::Windows)));
        let platform =
            platform.with_processes(Arc::new(UnsupportedProcessSupervision::new(Os::Linux)));
        assert_eq!(platform.processes().inspect(pid), Err(refused(Os::Linux)));
    }

    #[test]
    fn the_clock_still_works_on_an_unsupported_platform() {
        let platform = UnsupportedPlatform::new(WINDOWS);

        let first = platform.clock().now();
        assert!(platform.clock().now() >= first);
    }

    #[test]
    fn not_supported_errors_name_the_capability_and_os() {
        let platform = UnsupportedPlatform::new(WINDOWS);
        let err = platform.identity().current_user().unwrap_err();

        assert_eq!(err.to_string(), "user_identity is not supported on windows");
    }
}
