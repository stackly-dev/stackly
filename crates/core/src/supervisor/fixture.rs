//! Test fixtures that put the fake runtime and the fake provider package
//! where the supervisor expects a real one (spec 0010, *Value sourcing*, K5).
//!
//! Only dev dependencies turn the `fixture` feature on (`cargo xtask
//! check-deps`), so no release build holds this module. The functions copy
//! files and return the records a test commits to its registry; they never
//! write the registry themselves.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use jiff::Timestamp;
use sha2::{Digest, Sha256};
use stackly_domain::{
    ArtifactRecord, InstallMode, ProviderRevisionRecord, Runtime, RuntimeId, RuntimeState,
    Sha256Digest, StoredRuntime,
};
use stackly_platform::{DirectoryLayout, Platform};

use crate::provider::{NameCheck, PackageError, ValidatedPackage, VersionArtifact, read_package};

/// The name of the dummy server inside the runtime folder; it is the
/// `runtime.executable` of the fake provider.
pub const FAKE_EXECUTABLE: &str = "stackly-fake-server";

/// Why a fixture could not be installed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum FixtureError {
    /// A file or folder could not be read, copied, or made.
    #[error("cannot {op} {}: {source}", path.display())]
    Io {
        /// What was attempted.
        op: &'static str,
        /// The path.
        path: PathBuf,
        /// The cause.
        #[source]
        source: io::Error,
    },
    /// The package folder is not a valid provider package.
    #[error("the fake provider package is invalid: {0}")]
    Package(#[from] PackageError),
    /// The package has no artifact for the fake runtime.
    #[error("the fake provider package lists no fixture artifact")]
    NoArtifact,
    /// The target folder exists already.
    #[error("{} exists already", path.display())]
    Leftover {
        /// The folder.
        path: PathBuf,
    },
}

impl FixtureError {
    fn io(op: &'static str, path: &Path, source: io::Error) -> Self {
        Self::Io {
            op,
            path: path.to_path_buf(),
            source,
        }
    }
}

/// What [`install_fake`] made: the records a test commits.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct FakeInstall {
    /// The runtime, `installed`.
    pub runtime: Runtime,
    /// Its artifact.
    pub artifact: ArtifactRecord,
    /// The pinned provider revision.
    pub provider: ProviderRevisionRecord,
}

/// Copies the provider package in `dir` to its pinned folder under the
/// provider root of `layout` and returns the record to commit. The copy
/// reads back with the digest the record names.
///
/// # Errors
///
/// Returns [`FixtureError::Package`] for an invalid package,
/// [`FixtureError::Leftover`] when that revision is pinned already, and
/// [`FixtureError::Io`] when a file cannot be written.
pub fn pin_package(
    platform: &Platform,
    layout: &DirectoryLayout,
    dir: &Path,
) -> Result<ProviderRevisionRecord, FixtureError> {
    let package = read_valid(platform, dir)?;
    pin(layout, &package)
}

fn read_valid(platform: &Platform, dir: &Path) -> Result<ValidatedPackage, FixtureError> {
    let files = read_package(platform.fs(), dir)?;
    Ok(ValidatedPackage::from_files(files, NameCheck::Skip)?)
}

fn pin(
    layout: &DirectoryLayout,
    package: &ValidatedPackage,
) -> Result<ProviderRevisionRecord, FixtureError> {
    let target = layout.provider_revision_dir(package.key());
    if target.exists() {
        return Err(FixtureError::Leftover { path: target });
    }
    for (path, bytes) in package.files().iter() {
        let file = target.join(path);
        let parent = file.parent().unwrap_or(&target);
        fs::create_dir_all(parent).map_err(|error| FixtureError::io("create", parent, error))?;
        fs::write(&file, bytes).map_err(|error| FixtureError::io("write", &file, error))?;
    }
    Ok(ProviderRevisionRecord::new(
        package.provider_revision().clone(),
        None,
        None,
        None,
    ))
}

/// Copies the dummy server `fake_server` into the content addressed runtime
/// folder of `layout` and returns the installed runtime with its artifact.
/// The version and line come from the fixture artifact of `package`.
///
/// # Errors
///
/// Returns [`FixtureError::NoArtifact`] when the package lists no fixture
/// artifact, [`FixtureError::Leftover`] when the runtime folder exists, and
/// [`FixtureError::Io`] when the copy fails.
pub fn install_fake_runtime(
    layout: &DirectoryLayout,
    package: &ValidatedPackage,
    fake_server: &Path,
    installed_at: Timestamp,
) -> Result<(Runtime, ArtifactRecord), FixtureError> {
    let artifact: &VersionArtifact = package
        .versions()
        .artifacts()
        .first()
        .ok_or(FixtureError::NoArtifact)?;
    let bytes =
        fs::read(fake_server).map_err(|error| FixtureError::io("read", fake_server, error))?;
    let digest = Sha256Digest::from_bytes(Sha256::digest(&bytes).into());
    let software = package.software().clone();
    let dir = layout.runtime_dir(&software, &artifact.version, artifact.target, &digest);
    if dir.exists() {
        return Err(FixtureError::Leftover { path: dir });
    }
    fs::create_dir_all(&dir).map_err(|error| FixtureError::io("create", &dir, error))?;
    let executable = dir.join(FAKE_EXECUTABLE);
    fs::write(&executable, &bytes)
        .map_err(|error| FixtureError::io("write", &executable, error))?;
    make_executable(&executable)?;
    let runtime = Runtime::restore(StoredRuntime {
        id: RuntimeId::generate(),
        software: software.clone(),
        version: artifact.version.clone(),
        line: artifact.line.clone(),
        target: artifact.target,
        install: InstallMode::Managed {
            artifact_sha256: digest,
        },
        state: RuntimeState::Installed,
        installed_at: Some(installed_at),
        last_verified_at: None,
        depends_on: std::collections::BTreeSet::new(),
        revision: stackly_domain::ResourceRevision::INITIAL,
    })
    .map_err(|error| FixtureError::Io {
        op: "build the runtime",
        path: dir.clone(),
        source: io::Error::other(error.to_string()),
    })?;
    let record = ArtifactRecord::new(
        digest,
        software,
        artifact.version.clone(),
        artifact.target,
        None,
        None,
    );
    Ok((runtime, record))
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<(), FixtureError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))
        .map_err(|error| FixtureError::io("chmod", path, error))
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> Result<(), FixtureError> {
    Ok(())
}

/// Installs the fake runtime and pins the fake provider package, and
/// returns the records to commit (`InsertRuntime`, `InsertArtifact`, and
/// `InsertProviderRevision`). `fake_server` is the path of the
/// `stackly-fake-server` binary (tests find it through
/// `stackly_testkit::fake_server::binary`), and `package` the folder of the
/// package, normally `providers/fake`.
///
/// # Errors
///
/// Returns the errors of [`pin_package`] and [`install_fake_runtime`].
pub fn install_fake(
    platform: &Platform,
    layout: &DirectoryLayout,
    fake_server: &Path,
    package: &Path,
) -> Result<FakeInstall, FixtureError> {
    let validated = read_valid(platform, package)?;
    let installed_at = platform.clock().now();
    let (runtime, artifact) = install_fake_runtime(layout, &validated, fake_server, installed_at)?;
    let provider = pin(layout, &validated)?;
    Ok(FakeInstall {
        runtime,
        artifact,
        provider,
    })
}
