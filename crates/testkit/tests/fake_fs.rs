//! Behavior of `FakeFilesystem`: confinement to the test roots, fault
//! injection, the mutation log, and the shared `fs_conformance` and
//! `fs_cross_device_conformance` suites.
#![cfg(unix)]

use std::os::unix::fs::symlink;
use std::path::Path;

use stackly_platform::{Filesystem, PlatformError, RemoveReport};
use stackly_testkit::{FakeFilesystem, Fault, FsOp, TestRoot, fs_conformance};

fn fake() -> (TestRoot, FakeFilesystem) {
    let root = TestRoot::new().expect("a test root can be created");
    let fs = FakeFilesystem::new(&root);
    (root, fs)
}

#[test]
fn fake_filesystem_passes_the_conformance_suite() {
    let (root, fs) = fake();

    fs_conformance(&fs, &root);

    assert_eq!(fs.violations(), 0);
}

#[test]
fn paths_outside_the_test_roots_fail_and_count_as_violations() {
    let (root, fs) = fake();
    let other = TestRoot::new().unwrap();

    let outside = fs.symlink_metadata(other.path());
    let relative = fs.create_dir_private(Path::new("relative/dir"));
    let dot_dot = fs.create_dir_private(&root.path().join("..").join("escaped"));

    for result in [outside.map(|_| ()), relative, dot_dot] {
        assert!(
            matches!(result, Err(PlatformError::OutsideTestRoots { .. })),
            "{result:?}"
        );
    }
    assert_eq!(fs.violations(), 3);
    assert!(!root.path().parent().unwrap().join("escaped").exists());
}

#[test]
fn the_test_root_its_run_folder_and_missing_paths_inside_are_allowed() {
    let (root, fs) = fake();

    assert!(fs.symlink_metadata(root.path()).unwrap().is_some());
    assert!(fs.symlink_metadata(root.run_dir()).unwrap().is_some());
    let deep = root.path().join("a").join("b").join("c");
    assert!(fs.symlink_metadata(&deep).unwrap().is_none());
    assert!(matches!(
        fs.create_dir_private(&deep),
        Err(PlatformError::NotFound { .. })
    ));
    assert_eq!(fs.violations(), 0);
}

#[test]
fn canonicalize_through_a_symlink_that_leaves_the_root_is_a_violation() {
    let (root, fs) = fake();
    let other = TestRoot::new().unwrap();
    let link = root.path().join("out");
    symlink(other.path(), &link).unwrap();

    let result = fs.canonicalize(&link);

    assert!(
        matches!(result, Err(PlatformError::OutsideTestRoots { .. })),
        "{result:?}"
    );
    assert_eq!(fs.violations(), 1);
    // The symlink itself sits inside the root, so reading it is fine.
    assert!(fs.symlink_metadata(&link).unwrap().is_some());
    assert_eq!(fs.violations(), 1);
}

#[test]
fn a_symlinked_parent_that_leaves_the_root_is_a_violation() {
    let (root, fs) = fake();
    let other = TestRoot::new().unwrap();
    symlink(other.path(), root.path().join("out")).unwrap();

    let result = fs.create_dir_private(&root.path().join("out").join("dir"));

    assert!(
        matches!(result, Err(PlatformError::OutsideTestRoots { .. })),
        "{result:?}"
    );
    assert!(!other.path().join("dir").exists());
}

#[test]
fn fail_next_fails_one_call_only() {
    let (root, fs) = fake();
    fs.fail_next(FsOp::CreateDirPrivate, Fault::NoSpace);

    let first = fs.create_dir_private(&root.path().join("a"));
    let second = fs.create_dir_private(&root.path().join("b"));

    assert!(
        matches!(first, Err(PlatformError::NoSpace { .. })),
        "{first:?}"
    );
    second.unwrap();
    assert!(!root.path().join("a").exists());
}

#[test]
fn fail_when_matches_whole_trailing_components_until_cleared() {
    let (root, fs) = fake();
    fs.fail_when(FsOp::CreateDirPrivate, "config", Fault::PermissionDenied);

    let config = fs.create_dir_private(&root.path().join("config"));
    let history = fs.create_dir_private(&root.path().join("config-history"));
    let again = fs.create_dir_private(&root.path().join("config"));
    fs.clear_faults();
    let cleared = fs.create_dir_private(&root.path().join("config"));

    assert!(
        matches!(config, Err(PlatformError::PermissionDenied { .. })),
        "{config:?}"
    );
    history.unwrap();
    assert!(
        matches!(again, Err(PlatformError::PermissionDenied { .. })),
        "{again:?}"
    );
    cleared.unwrap();
}

#[test]
fn a_write_that_fails_before_the_rename_keeps_the_old_file_and_no_temp_file() {
    let (root, fs) = fake();
    let file = root.path().join("manifest.json");
    fs.write_file_atomic(&file, b"old").unwrap();
    fs.fail_next(FsOp::WriteFileAtomic, Fault::Io);

    let result = fs.write_file_atomic(&file, b"new");

    assert!(
        matches!(result, Err(PlatformError::Io { .. })),
        "{result:?}"
    );
    assert_eq!(std::fs::read(&file).unwrap(), b"old");
    let names: Vec<_> = std::fs::read_dir(root.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(names, ["manifest.json"]);
}

#[test]
fn mutations_log_created_directories_and_written_files_only() {
    let (root, fs) = fake();
    let dir = root.path().join("dir");
    let file = dir.join("file");

    fs.create_dir_private(&dir).unwrap();
    fs.write_file_atomic(&file, b"x").unwrap();
    fs.read_file_limited(&file, 10).unwrap();
    fs.read_dir(&dir).unwrap();
    fs.symlink_metadata(&file).unwrap();
    fs.canonicalize(&file).unwrap();
    let _ = fs.create_dir_private(&dir);

    let log: Vec<_> = fs.mutations().into_iter().map(|m| (m.op, m.path)).collect();
    assert_eq!(
        log,
        [(FsOp::CreateDirPrivate, dir), (FsOp::WriteFileAtomic, file)]
    );
}

#[test]
fn faults_and_reads_on_other_operations_leave_the_log_empty() {
    let (root, fs) = fake();
    fs.fail_next(FsOp::ReadDir, Fault::Io);

    let result = fs.read_dir(root.path());

    assert!(
        matches!(result, Err(PlatformError::Io { op: "read_dir", .. })),
        "{result:?}"
    );
    assert!(fs.mutations().is_empty());
}

#[test]
fn remove_tree_outside_the_test_roots_fails_and_removes_nothing() {
    let (root, fs) = fake();
    let other = TestRoot::new().unwrap();
    std::fs::create_dir(other.path().join("x")).unwrap();
    std::fs::write(other.path().join("x").join("file"), b"keep").unwrap();
    symlink(other.path(), root.path().join("out")).unwrap();

    let outside_root = fs.remove_tree(other.path(), Path::new("x"));
    let through_link = fs.remove_tree(root.path(), Path::new("out/x"));

    for result in [outside_root, through_link] {
        assert!(
            matches!(result, Err(PlatformError::OutsideTestRoots { .. })),
            "{result:?}"
        );
    }
    assert_eq!(fs.violations(), 2);
    assert_eq!(
        std::fs::read(other.path().join("x").join("file")).unwrap(),
        b"keep"
    );
    assert!(fs.mutations().is_empty());
}

#[test]
fn a_remove_tree_fault_fires_before_anything_is_removed() {
    let (root, fs) = fake();
    let tree = root.path().join("tree");
    std::fs::create_dir(&tree).unwrap();
    std::fs::write(tree.join("file"), b"x").unwrap();
    fs.fail_when(FsOp::RemoveTree, "tree", Fault::PermissionDenied);

    let result = fs.remove_tree(root.path(), Path::new("tree"));

    assert!(
        matches!(result, Err(PlatformError::PermissionDenied { .. })),
        "{result:?}"
    );
    assert!(tree.join("file").exists());
    assert!(fs.mutations().is_empty());
}

#[test]
fn mutations_log_a_removed_tree_but_not_a_missing_one() {
    let (root, fs) = fake();
    let tree = root.path().join("tree");
    std::fs::create_dir_all(tree.join("sub")).unwrap();
    std::fs::write(tree.join("sub").join("file"), b"x").unwrap();

    let removed = fs.remove_tree(root.path(), Path::new("tree")).unwrap();
    let missing = fs.remove_tree(root.path(), Path::new("tree")).unwrap();

    assert_eq!(
        (removed, missing),
        (RemoveReport::removed(1, 2), RemoveReport::missing())
    );
    let log: Vec<_> = fs.mutations().into_iter().map(|m| (m.op, m.path)).collect();
    assert_eq!(log, [(FsOp::RemoveTree, tree)]);
}

#[cfg(target_os = "macos")]
#[test]
fn fake_remove_tree_stops_at_another_device() {
    use stackly_testkit::evidence::{Kind, evidence};
    use stackly_testkit::{CrossDevice, fs_cross_device_conformance};

    let ev = evidence!(
        covers = ["VAL-007"],
        kind = Kind::Fixture,
        expected = "FakeFilesystem::remove_tree stops with CrossesDevice at a disk image attached inside, as, or on the way to the target, and the file on the image stays intact (VAL-007)",
    );
    let (root, fs) = fake();

    match fs_cross_device_conformance(&fs, &root) {
        CrossDevice::Checked => ev.pass(),
        CrossDevice::Blocked(reason) => ev.blocked(
            &format!("hdiutil cannot attach a disk image on this host: {reason}"),
            "feature 7",
        ),
        other => panic!("unexpected cross device outcome {other:?}"),
    }
    assert_eq!(fs.violations(), 0);
}
