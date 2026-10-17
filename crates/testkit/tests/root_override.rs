//! `validate_root_override` on the fake platform: every rule that rejects
//! an alternative root, and the canonical path it returns when the root is
//! accepted.
#![cfg(unix)]

use std::fs::Permissions;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};

use stackly_platform::{PlatformError, UserId, validate_root_override};
use stackly_testkit::{FakePlatform, TestRoot};

struct Setup {
    root: TestRoot,
    fake: FakePlatform,
    default_data: PathBuf,
}

/// A test root with a private `override` folder and a default data root at
/// `home/Library/Application Support/Stackly`, both inside the test root.
fn setup() -> Setup {
    let root = TestRoot::new().expect("a test root can be created");
    let fake = FakePlatform::new(&root);
    let default_data = root.path().join("home/Library/Application Support/Stackly");
    std::fs::create_dir_all(&default_data).expect("the test root accepts new folders");
    make_private_dir(&root.path().join("override"));
    Setup {
        root,
        fake,
        default_data,
    }
}

fn make_private_dir(path: &Path) {
    std::fs::create_dir(path).expect("the test root accepts a new folder");
    std::fs::set_permissions(path, Permissions::from_mode(0o700))
        .expect("the test owns the folder it just made");
}

fn validate(setup: &Setup, path: &Path) -> Result<PathBuf, PlatformError> {
    let platform = setup.fake.platform();
    validate_root_override(
        path,
        platform.fs(),
        platform.identity(),
        &setup.default_data,
    )
}

fn reason(result: Result<PathBuf, PlatformError>) -> &'static str {
    match result {
        Err(PlatformError::InvalidRootOverride { reason, .. }) => reason,
        other => panic!("expected InvalidRootOverride, got {other:?}"),
    }
}

#[test]
fn a_private_folder_of_the_current_user_is_accepted_as_its_canonical_path() {
    let setup = setup();
    let through_dot_dot = setup.root.path().join("home/../override");

    let accepted = validate(&setup, &through_dot_dot).unwrap();

    assert_eq!(accepted, setup.root.path().join("override"));
    assert_eq!(setup.fake.fs().violations(), 0);
}

#[test]
fn empty_relative_and_missing_roots_are_rejected() {
    let setup = setup();

    assert_eq!(reason(validate(&setup, Path::new(""))), "empty");
    assert_eq!(
        reason(validate(&setup, Path::new("relative/root"))),
        "not absolute"
    );
    let missing = setup.root.path().join("missing");
    assert_eq!(reason(validate(&setup, &missing)), "does not exist");
}

#[test]
fn a_symlink_or_a_file_is_rejected() {
    let setup = setup();
    let link = setup.root.path().join("link");
    symlink(setup.root.path().join("override"), &link).unwrap();
    let file = setup.root.path().join("file");
    std::fs::write(&file, b"").unwrap();

    assert_eq!(reason(validate(&setup, &link)), "is a symlink");
    assert_eq!(reason(validate(&setup, &file)), "not a directory");
}

#[test]
fn a_folder_of_another_user_is_rejected() {
    let setup = setup();
    setup
        .fake
        .identity()
        .set_current(UserId::Unix(u32::MAX - 1));

    let result = validate(&setup, &setup.root.path().join("override"));

    assert_eq!(reason(result), "owned by another user");
}

#[test]
fn a_folder_writable_by_group_or_others_is_rejected() {
    let setup = setup();
    let open = setup.root.path().join("override");
    std::fs::set_permissions(&open, Permissions::from_mode(0o777)).unwrap();

    assert_eq!(
        reason(validate(&setup, &open)),
        "writable by group or others"
    );
}

#[test]
fn the_default_data_root_and_folders_inside_it_are_rejected() {
    let setup = setup();
    std::fs::set_permissions(&setup.default_data, Permissions::from_mode(0o700)).unwrap();
    let inside = setup.default_data.join("tests");
    make_private_dir(&inside);

    assert_eq!(
        reason(validate(&setup, &setup.default_data)),
        "inside the default data root"
    );
    assert_eq!(
        reason(validate(&setup, &inside)),
        "inside the default data root"
    );
}

#[test]
fn a_symlinked_default_data_root_is_compared_by_its_target() {
    let setup = setup();
    let real_data = setup.root.path().join("real-data");
    make_private_dir(&real_data);
    let inside = real_data.join("tests");
    make_private_dir(&inside);
    let linked_default = setup.root.path().join("linked-default");
    symlink(&real_data, &linked_default).unwrap();
    let platform = setup.fake.platform();

    let result =
        validate_root_override(&inside, platform.fs(), platform.identity(), &linked_default);

    assert_eq!(reason(result), "inside the default data root");
}

#[test]
fn a_missing_default_data_root_does_not_block_an_override() {
    let setup = setup();
    let platform = setup.fake.platform();
    let missing_default = setup.root.path().join("no-home/Stackly");

    let accepted = validate_root_override(
        &setup.root.path().join("override"),
        platform.fs(),
        platform.identity(),
        &missing_default,
    );

    assert_eq!(accepted.unwrap(), setup.root.path().join("override"));
}

#[test]
fn rejections_name_the_path_and_the_rule() {
    let setup = setup();
    let missing = setup.root.path().join("missing");

    let message = validate(&setup, &missing).unwrap_err().to_string();

    assert_eq!(
        message,
        format!("root override {missing:?} rejected: does not exist")
    );
}
