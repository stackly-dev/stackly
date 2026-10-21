//! Behavior of `FakeFilesystem`: confinement to the test roots, fault
//! injection, the mutation log, the volume table, and the shared
//! `fs_conformance` and `fs_cross_device_conformance` suites.
#![cfg(unix)]

use std::os::unix::fs::symlink;
use std::path::Path;

use std::io::Write;

use stackly_platform::{
    EntryPath, Filesystem, PlatformError, RemoveReport, VolumeFacts, VolumeSpace,
};
use stackly_testkit::{FakeFilesystem, FakeVolume, Fault, FsOp, TestRoot, fs_conformance};

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

#[test]
fn mutations_log_append_opens_renames_and_tree_writer_creations() {
    let (root, fs) = fake();
    let part = root.path().join("artifact.part");
    let done = root.path().join("artifact");
    let tree = root.path().join("tree");
    std::fs::create_dir(&tree).unwrap();
    std::fs::set_permissions(&tree, std::os::unix::fs::PermissionsExt::from_mode(0o700)).unwrap();

    fs.open_append(&part).unwrap().write_all(b"bytes").unwrap();
    fs.open_read(&part).unwrap();
    fs.rename_no_replace(&part, &done).unwrap();
    let mut writer = fs.open_tree_writer(&tree).unwrap();
    writer.create_dir(&EntryPath::new("bin").unwrap()).unwrap();
    writer
        .create_file(&EntryPath::new("bin/tool").unwrap())
        .unwrap()
        .finish()
        .unwrap();
    writer
        .create_symlink(&EntryPath::new("tool").unwrap(), "bin/tool")
        .unwrap();
    writer
        .set_mode(
            &EntryPath::new("bin/tool").unwrap(),
            stackly_platform::FileMode::new(0o555).unwrap(),
        )
        .unwrap();
    writer.sync().unwrap();

    let log: Vec<_> = fs.mutations().into_iter().map(|m| (m.op, m.path)).collect();
    assert_eq!(
        log,
        [
            (FsOp::OpenAppend, part),
            (FsOp::RenameNoReplace, done),
            (FsOp::TreeCreateDir, tree.join("bin")),
            (FsOp::TreeCreateFile, tree.join("bin/tool")),
            (FsOp::TreeCreateSymlink, tree.join("tool")),
        ]
    );
}

#[test]
fn faults_fire_on_the_new_operations_and_their_tree_entries() {
    let (root, fs) = fake();
    let file = root.path().join("file");
    std::fs::write(&file, b"x").unwrap();
    let tree = root.path().join("tree");
    std::fs::create_dir(&tree).unwrap();
    std::fs::set_permissions(&tree, std::os::unix::fs::PermissionsExt::from_mode(0o700)).unwrap();
    fs.fail_next(FsOp::OpenRead, Fault::Io);
    fs.fail_next(FsOp::VolumeSpace, Fault::PermissionDenied);
    fs.fail_when(FsOp::TreeCreateFile, "bin/tool", Fault::NoSpace);

    let read = fs.open_read(&file).map(drop);
    let space = fs.available_space(root.path());
    let mut writer = fs.open_tree_writer(&tree).unwrap();
    writer.create_dir(&EntryPath::new("bin").unwrap()).unwrap();
    let full = writer
        .create_file(&EntryPath::new("bin/tool").unwrap())
        .map(drop);
    let other = writer
        .create_file(&EntryPath::new("bin/other").unwrap())
        .map(drop);

    assert!(
        matches!(
            read,
            Err(PlatformError::Io {
                op: "open_read",
                ..
            })
        ),
        "{read:?}"
    );
    assert!(
        matches!(space, Err(PlatformError::PermissionDenied { .. })),
        "{space:?}"
    );
    assert!(
        matches!(full, Err(PlatformError::NoSpace { .. })),
        "{full:?}"
    );
    assert!(other.is_ok(), "{other:?}");
    assert!(!tree.join("bin/tool").exists());
    assert!(fs.open_read(&file).is_ok(), "fail_next fires once");
}

#[test]
fn volume_space_answers_from_the_longest_matching_prefix() {
    let (root, fs) = fake();
    let outer = root.path().join("data");
    let inner = outer.join("cache");
    std::fs::create_dir_all(&inner).unwrap();
    fs.set_volume(&outer, FakeVolume::new(1_000, 600));
    fs.set_volume(&inner, FakeVolume::new(1_000, 10));

    assert_eq!(
        fs.volume_space(root.path()).unwrap(),
        VolumeSpace::new(FakeVolume::DEFAULT.total, FakeVolume::DEFAULT.available)
    );
    assert_eq!(
        fs.volume_space(&outer).unwrap(),
        VolumeSpace::new(1_000, 600)
    );
    assert_eq!(fs.available_space(&inner).unwrap(), 10);

    fs.set_volume(&inner, FakeVolume::new(1_000, 0));
    assert_eq!(fs.available_space(&inner).unwrap(), 0);
    assert!(matches!(
        fs.volume_space(&outer.join("missing")),
        Err(PlatformError::NotFound { .. })
    ));
}

#[test]
fn volume_facts_answer_from_the_longest_matching_prefix_and_default_to_local_apfs() {
    let (root, fs) = fake();
    let share = root.path().join("share");
    std::fs::create_dir_all(&share).unwrap();
    fs.set_volume(
        &share,
        FakeVolume::new(1_000, 600).with_kind("smbfs", false),
    );

    assert_eq!(
        fs.volume_facts(root.path()).unwrap(),
        VolumeFacts::new("apfs", true)
    );
    assert_eq!(
        fs.volume_facts(&share).unwrap(),
        VolumeFacts::new("smbfs", false)
    );
    assert_eq!(
        fs.volume_space(&share).unwrap(),
        VolumeSpace::new(1_000, 600)
    );
    assert!(matches!(
        fs.volume_facts(&share.join("missing")),
        Err(PlatformError::NotFound { .. })
    ));

    fs.fail_next(FsOp::VolumeFacts, Fault::PermissionDenied);
    assert!(matches!(
        fs.volume_facts(root.path()),
        Err(PlatformError::PermissionDenied { .. })
    ));
    assert!(fs.volume_facts(root.path()).is_ok(), "fail_next fires once");
}

#[test]
fn a_rename_out_of_or_into_another_root_is_a_violation() {
    let (root, fs) = fake();
    let other = TestRoot::new().unwrap();
    let inside = root.path().join("inside");
    let outside = other.path().join("outside");
    std::fs::write(&inside, b"in").unwrap();
    std::fs::write(&outside, b"out").unwrap();

    let out_of = fs.rename_no_replace(&outside, &root.path().join("taken"));
    let into = fs.rename_no_replace(&inside, &other.path().join("moved"));

    for result in [out_of, into] {
        assert!(
            matches!(result, Err(PlatformError::OutsideTestRoots { .. })),
            "{result:?}"
        );
    }
    assert_eq!(fs.violations(), 2);
    assert!(inside.exists() && outside.exists());
    assert!(fs.mutations().is_empty());
}
