use std::path::{Path, PathBuf};
use std::sync::Arc;

use stackly_domain::{Os, Target};

use crate::capability::{CapabilityStatus, PlatformCapabilities, PlatformCapability};
use crate::clock::SystemClock;
use crate::error::PlatformError;
use crate::fs::{DirEntryInfo, FileInfo, Filesystem, RemoveReport};
use crate::identity::{UserId, UserIdentity};
use crate::info::PlatformInfo;
use crate::layout::{DirectoryResolver, RootSet};
use crate::platform::Platform;
use crate::secret::UnsupportedSecretStore;

/// The platform for an OS that has no adapter yet, such as Windows today.
///
/// Directories, filesystem, and user identity always answer
/// [`PlatformError::NotSupported`], and the secret store is an
/// [`UnsupportedSecretStore`]; the clock works. Every capability but the
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
}

#[cfg(test)]
mod tests {
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
