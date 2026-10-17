//! The macOS adapter on the real filesystem: the default and override roots,
//! override validation, the `fs_conformance` suite for `MacosFilesystem`,
//! the capability statuses, and `prepare_layout` through
//! `MacosPlatform::with_roots`. Every write goes to a `TestRoot`; the real
//! per user folders are only ever read.
#![cfg(target_os = "macos")]

use std::fs::Permissions;
use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::Command;

use stackly_core::{ensure_instance_run_dir, prepare_layout};
use stackly_domain::{InstanceId, Os};
use stackly_platform::{
    CapabilityStatus, Filesystem, PlatformCapability, PlatformError, RUNTIME_ROOT_MAX, UserId,
    UserIdentity, validate_root_override,
};
use stackly_platform_macos::{MacosFilesystem, MacosIdentity, MacosPlatform};
use stackly_testkit::{TestRoot, fs_conformance};

/// Runs a command and returns its trimmed stdout.
fn stdout_of(program: &str, args: &[&str]) -> String {
    let output = Command::new(program)
        .args(args)
        .output()
        .expect("the macOS command runs");
    assert!(output.status.success(), "{program} {args:?}: {output:?}");
    String::from_utf8(output.stdout)
        .expect("the command prints UTF-8")
        .trim()
        .to_owned()
}

/// The user's private temporary directory, as `getconf` reports it.
fn user_temp_dir() -> PathBuf {
    PathBuf::from(stdout_of("getconf", &["DARWIN_USER_TEMP_DIR"]))
}

/// The user's home directory from the directory service, not from `HOME`.
fn user_home_dir() -> PathBuf {
    let user = stdout_of("id", &["-un"]);
    let record = stdout_of(
        "dscl",
        &[".", "-read", &format!("/Users/{user}"), "NFSHomeDirectory"],
    );
    let home = record
        .strip_prefix("NFSHomeDirectory:")
        .expect("dscl prints the attribute name first")
        .trim();
    PathBuf::from(home)
}

fn private_dir(path: &Path) {
    std::fs::create_dir(path).expect("the test root accepts a new folder");
    std::fs::set_permissions(path, Permissions::from_mode(0o700))
        .expect("the test owns the folder it just made");
}

#[test]
fn default_roots_come_from_the_user_database_and_the_user_temp_dir() {
    let platform = MacosPlatform::new(None).unwrap();
    let roots = platform.directories().roots().unwrap();

    let home = user_home_dir();
    assert_eq!(
        roots.data(),
        home.join("Library/Application Support/Stackly")
    );
    assert_eq!(roots.logs(), home.join("Library/Logs/Stackly"));
    assert_eq!(roots.cache(), home.join("Library/Caches/Stackly"));
    assert_eq!(roots.runtime(), user_temp_dir().join("stackly"));
    assert!(roots.runtime().as_os_str().len() <= RUNTIME_ROOT_MAX);
}

#[test]
fn an_override_moves_every_root_and_keeps_the_runtime_root_in_the_user_temp_dir() {
    let root = TestRoot::new().unwrap();

    let platform = MacosPlatform::new(Some(root.path().into())).unwrap();
    let roots = platform.directories().roots().unwrap();

    assert_eq!(roots.data(), root.path().join("data"));
    assert_eq!(roots.logs(), root.path().join("logs"));
    assert_eq!(roots.cache(), root.path().join("cache"));
    assert_eq!(roots.runtime().parent().unwrap(), user_temp_dir().as_path());
    assert!(roots.runtime().as_os_str().len() <= RUNTIME_ROOT_MAX);
    assert!(!roots.runtime().exists());
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}

#[test]
fn an_empty_or_relative_override_is_rejected() {
    for (value, reason) in [("", "empty"), ("relative/root", "not absolute")] {
        let result = MacosPlatform::new(Some(value.into()));

        assert!(
            matches!(
                result,
                Err(PlatformError::InvalidRootOverride { reason: actual, .. }) if actual == reason
            ),
            "{value}: {result:?}"
        );
    }
}

#[test]
fn override_validation_rejects_symlinks_open_folders_and_the_default_data_root() {
    let root = TestRoot::new().unwrap();
    let default_data = root.path().join("Stackly");
    private_dir(&default_data);
    let inside = default_data.join("tests");
    private_dir(&inside);
    let target = root.path().join("target");
    private_dir(&target);
    let link = root.path().join("link");
    symlink(&target, &link).unwrap();
    let open = root.path().join("open");
    private_dir(&open);
    std::fs::set_permissions(&open, Permissions::from_mode(0o777)).unwrap();
    let validate =
        |path: &Path| validate_root_override(path, &MacosFilesystem, &MacosIdentity, &default_data);

    for (path, reason) in [
        (&link, "is a symlink"),
        (&open, "writable by group or others"),
        (&default_data, "inside the default data root"),
        (&inside, "inside the default data root"),
        (&root.path().join("missing"), "does not exist"),
    ] {
        let result = validate(path);
        assert!(
            matches!(
                &result,
                Err(PlatformError::InvalidRootOverride { reason: actual, .. }) if *actual == reason
            ),
            "{}: {result:?}",
            path.display()
        );
    }
    assert_eq!(validate(&target).unwrap(), target);
}

#[test]
fn macos_filesystem_passes_the_conformance_suite() {
    let root = TestRoot::new().unwrap();

    fs_conformance(&MacosFilesystem, &root);
}

#[test]
fn identity_is_the_uid_that_owns_new_files() {
    let root = TestRoot::new().unwrap();
    let file = root.path().join("owned");
    std::fs::write(&file, b"").unwrap();

    let user = MacosIdentity.current_user().unwrap();

    assert_eq!(user, UserId::Unix(std::fs::metadata(&file).unwrap().uid()));
}

#[test]
fn the_four_built_ports_are_supported_and_the_rest_name_their_feature() {
    let platform = MacosPlatform::with_roots(TestRoot::new().unwrap().roots());
    let info = platform.info();
    let caps = info.capabilities();

    assert_eq!(info.target().os(), Os::Macos);
    for capability in PlatformCapability::ALL {
        let status = caps.status(capability);
        match capability {
            PlatformCapability::Directories
            | PlatformCapability::Filesystem
            | PlatformCapability::Clock
            | PlatformCapability::UserIdentity => {
                assert_eq!(status, CapabilityStatus::Supported, "{capability}");
            }
            _ => assert!(
                matches!(status, CapabilityStatus::NotSupported { reason } if reason.contains("(feature ")),
                "{capability}: {status:?}"
            ),
        }
    }
}

#[test]
fn prepare_layout_makes_a_private_layout_on_the_real_filesystem() {
    let root = TestRoot::new().unwrap();
    let platform = MacosPlatform::with_roots(root.roots());

    let prepared = prepare_layout(&platform).unwrap();
    let again = prepare_layout(&platform).unwrap();
    let run_dir =
        ensure_instance_run_dir(&platform, &prepared, InstanceId::generate().short_key()).unwrap();

    assert_eq!(again, prepared);
    let layout = prepared.layout();
    let mut dirs: Vec<PathBuf> = layout
        .owned_roots()
        .iter()
        .map(|dir| dir.to_path_buf())
        .collect();
    dirs.extend(layout.standard_dirs());
    dirs.push(run_dir);
    let uid = std::fs::metadata(root.path()).unwrap().uid();
    for dir in &dirs {
        let info = MacosFilesystem.symlink_metadata(dir).unwrap().unwrap();
        assert_eq!(info.mode, Some(0o700), "{}", dir.display());
        assert_eq!(info.owner, Some(UserId::Unix(uid)), "{}", dir.display());
    }
}
