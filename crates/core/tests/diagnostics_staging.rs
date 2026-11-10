//! Staging and export of the diagnostic bundle (spec 0034 AC-12, AC-13) on
//! the real disk of a `TestRoot` through `FakePlatform`: previews in
//! `<cache>/diagnostics/` with mode 0600 in a 0700 folder, at most two, gone
//! after 15 minutes or when a newer one pushes them out; an export copies
//! the exact bytes through a partial file and a rename, refuses what it
//! must, replaces only when asked, and never leaves a partial; the daemon
//! log tail spans the rotated files.
#![cfg(unix)]

use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

use jiff::{SignedDuration, Timestamp};
use serde_json::json;
use stackly_core::diagnostics::{
    BundleInput, DAEMON_LOG_LINES_MAX, DestinationProblem, ExportError, HomeMask, LogText, Staging,
    WriteReason, build, daemon_log, export,
};
use stackly_core::secret::Redactor;
use stackly_testkit::{FakePlatform, TestRoot};
use uuid::Uuid;

const T0: &str = "2026-11-10T10:00:00Z";

fn at(minutes: i64) -> Timestamp {
    T0.parse::<Timestamp>()
        .expect("T0 is a time")
        .checked_add(SignedDuration::from_mins(minutes))
        .expect("a test time is in range")
}

fn bundle(note: &str) -> stackly_core::diagnostics::BuiltBundle {
    let mut input = BundleInput::new(at(0), "0.1.0".to_owned(), "3.5".to_owned(), false);
    input.doctor = json!({ "note": note });
    input.system = json!({});
    input.instances = Some(json!([]));
    input.runtimes = Some(json!([]));
    input.operations = Some(json!([]));
    input.daemon_log = Some(LogText::new(vec!["one".to_owned()], false));
    build(input, &Redactor::new(), &HomeMask::none()).expect("a small bundle builds")
}

struct Site {
    root: TestRoot,
    platform: FakePlatform,
    staging: Staging,
}

impl Site {
    fn new() -> Self {
        let root = TestRoot::new().expect("a test root can be made");
        let cache = root.path().join("cache");
        std::fs::create_dir(&cache).expect("the cache root can be made");
        std::fs::set_permissions(&cache, std::fs::Permissions::from_mode(0o700))
            .expect("the cache root mode can be set");
        let platform = FakePlatform::new(&root);
        Self {
            staging: Staging::new(cache),
            root,
            platform,
        }
    }

    fn out(&self) -> PathBuf {
        let out = self.root.path().join("out");
        if !out.exists() {
            std::fs::create_dir(&out).expect("the export folder can be made");
        }
        out
    }
}

fn mode(path: &Path) -> u32 {
    std::fs::symlink_metadata(path)
        .expect("the path exists")
        .mode()
        & 0o7777
}

// covers: AC-12 (spec 0034)
#[test]
fn a_preview_is_staged_private_and_a_third_one_pushes_out_the_oldest() {
    let site = Site::new();
    let fs = site.platform.fs();
    let (a, b, c) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
    let first = site.staging.stage(fs, a, at(0), &bundle("a")).unwrap();
    assert_eq!(mode(&site.staging.dir()), 0o700);
    assert_eq!(mode(first.path()), 0o600);
    assert_eq!(std::fs::read(first.path()).unwrap(), bundle("a").bytes);
    assert_eq!(first.expires_at, at(15));
    site.staging.stage(fs, b, at(1), &bundle("b")).unwrap();
    site.staging.stage(fs, c, at(2), &bundle("c")).unwrap();

    assert_eq!(site.staging.ids(), [b, c]);
    assert!(!first.path().exists(), "the pushed out preview is unlinked");
    assert!(site.staging.get(fs, a, at(2)).is_none());
    let out = site.out().join("from-the-open-file.zip");
    let copied = export(fs, &first, out.to_str().unwrap(), false).unwrap();
    assert_eq!(
        copied.sha256,
        bundle("a").sha256,
        "an open preview still exports"
    );
    assert_eq!(std::fs::read(&out).unwrap(), bundle("a").bytes);
}

// covers: AC-12 (spec 0034)
#[test]
fn a_preview_expires_after_fifteen_minutes_and_its_file_goes() {
    let site = Site::new();
    let fs = site.platform.fs();
    let id = Uuid::now_v7();
    let staged = site.staging.stage(fs, id, at(0), &bundle("a")).unwrap();
    assert!(site.staging.get(fs, id, at(14)).is_some());
    assert!(site.staging.get(fs, id, at(15)).is_none());
    assert!(!staged.path().exists());
    assert!(site.staging.ids().is_empty());
}

// covers: AC-12 (spec 0034)
#[test]
fn clearing_at_start_removes_the_staging_folder_and_only_it() {
    let site = Site::new();
    let fs = site.platform.fs();
    site.staging
        .stage(fs, Uuid::now_v7(), at(0), &bundle("a"))
        .unwrap();
    let keep = site.root.path().join("cache/other");
    std::fs::write(&keep, "x").unwrap();
    site.staging.clear(fs).unwrap();
    assert!(!site.staging.dir().exists());
    assert!(keep.exists());
    site.staging.clear(fs).unwrap();
}

// covers: AC-13 (spec 0034)
#[test]
fn an_export_copies_the_exact_bytes_and_leaves_no_partial() {
    let site = Site::new();
    let fs = site.platform.fs();
    let staged = site
        .staging
        .stage(fs, Uuid::now_v7(), at(0), &bundle("a"))
        .unwrap();
    let out = site.out();
    let target = out.join("stackly-diagnostics-20261110T100000Z.zip");
    let exported = export(fs, &staged, target.to_str().unwrap(), false).unwrap();
    assert_eq!(exported.path, target.canonicalize().unwrap());
    assert_eq!(
        exported.bytes,
        u64::try_from(bundle("a").bytes.len()).unwrap()
    );
    assert_eq!(exported.sha256, staged.sha256);
    assert_eq!(std::fs::read(&target).unwrap(), bundle("a").bytes);
    assert_eq!(mode(&target), 0o600);
    let names: Vec<_> = std::fs::read_dir(&out)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(names, ["stackly-diagnostics-20261110T100000Z.zip"]);
    // Two exports of one preview each get their own file.
    let second = out.join("second.zip");
    export(fs, &staged, second.to_str().unwrap(), false).unwrap();
    assert_eq!(std::fs::read(&second).unwrap(), bundle("a").bytes);
}

// covers: AC-13 (spec 0034)
#[test]
fn an_existing_destination_is_kept_unless_replace_and_a_symlink_is_replaced_as_itself() {
    let site = Site::new();
    let fs = site.platform.fs();
    let staged = site
        .staging
        .stage(fs, Uuid::now_v7(), at(0), &bundle("a"))
        .unwrap();
    let out = site.out();
    let target = out.join("b.zip");
    std::fs::write(&target, "old").unwrap();
    let refused = export(fs, &staged, target.to_str().unwrap(), false).unwrap_err();
    assert!(matches!(refused, ExportError::Exists { .. }), "{refused:?}");
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "old");
    export(fs, &staged, target.to_str().unwrap(), true).unwrap();
    assert_eq!(std::fs::read(&target).unwrap(), bundle("a").bytes);

    let pointed = out.join("pointed.txt");
    std::fs::write(&pointed, "keep me").unwrap();
    let link = out.join("link.zip");
    std::os::unix::fs::symlink(&pointed, &link).unwrap();
    let refused = export(fs, &staged, link.to_str().unwrap(), false).unwrap_err();
    assert!(matches!(refused, ExportError::Exists { .. }));
    export(fs, &staged, link.to_str().unwrap(), true).unwrap();
    assert!(
        std::fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_file()
    );
    assert_eq!(std::fs::read_to_string(&pointed).unwrap(), "keep me");
}

// covers: AC-13 (spec 0034)
#[test]
fn a_destination_is_refused_with_its_reason() {
    let site = Site::new();
    let fs = site.platform.fs();
    let staged = site
        .staging
        .stage(fs, Uuid::now_v7(), at(0), &bundle("a"))
        .unwrap();
    let out = site.out();
    let folder = out.join("folder");
    std::fs::create_dir(&folder).unwrap();
    let file = out.join("file");
    std::fs::write(&file, "f").unwrap();
    let fifo = out.join("fifo");
    stackly_testkit::make_fifo(&fifo).expect("a FIFO can be made");
    let text = |path: &Path| path.to_str().unwrap().to_owned();
    for (destination, reason) in [
        ("relative.zip".to_owned(), DestinationProblem::NotAbsolute),
        (
            format!("{}/./b.zip", text(&out)),
            DestinationProblem::InvalidName,
        ),
        (
            format!("{}/../b.zip", text(&out)),
            DestinationProblem::InvalidName,
        ),
        (
            format!("{}/{}", text(&out), "n".repeat(201)),
            DestinationProblem::InvalidName,
        ),
        (
            format!("{}/missing/b.zip", text(&out)),
            DestinationProblem::ParentMissing,
        ),
        (
            format!("{}/b.zip", text(&file)),
            DestinationProblem::ParentNotDirectory,
        ),
        (text(&folder), DestinationProblem::IsDirectory),
        (text(&fifo), DestinationProblem::NotRegular),
    ] {
        match export(fs, &staged, &destination, true) {
            Err(ExportError::Invalid { reason: got, .. }) => {
                assert_eq!(got, reason, "{destination}");
            }
            other => panic!("{destination}: {other:?}"),
        }
    }
    assert!(folder.is_dir());
}

// covers: AC-13 (spec 0034)
#[test]
fn a_read_only_folder_is_a_permission_failure_without_a_partial() {
    let site = Site::new();
    let fs = site.platform.fs();
    let staged = site
        .staging
        .stage(fs, Uuid::now_v7(), at(0), &bundle("a"))
        .unwrap();
    let out = site.out();
    std::fs::set_permissions(&out, std::fs::Permissions::from_mode(0o500)).unwrap();
    let result = export(fs, &staged, out.join("b.zip").to_str().unwrap(), false);
    std::fs::set_permissions(&out, std::fs::Permissions::from_mode(0o700)).unwrap();
    match result {
        Err(ExportError::Write { reason, .. }) => assert_eq!(reason, WriteReason::Permission),
        other => panic!("{other:?}"),
    }
    assert_eq!(std::fs::read_dir(&out).unwrap().count(), 0);
}

// covers: AC-12 (spec 0034)
#[test]
fn the_daemon_log_tail_spans_the_rotated_files_newest_last() {
    let root = TestRoot::new().unwrap();
    let dir = root.path().join("logs");
    std::fs::create_dir(&dir).unwrap();
    let write = |name: &str, from: usize, to: usize| {
        let text: String = (from..to).map(|n| format!("line {n}\n")).collect();
        std::fs::write(dir.join(name), text).unwrap();
    };
    write("stacklyd.log.2", 0, 1000);
    write("stacklyd.log.1", 1000, 1500);
    write("stacklyd.log", 1500, 2600);
    let platform = FakePlatform::new(&root);
    let log = daemon_log(platform.fs(), &dir).unwrap();
    assert_eq!(log.lines.len(), DAEMON_LOG_LINES_MAX);
    assert_eq!(log.lines.first().map(String::as_str), Some("line 600"));
    assert_eq!(log.lines.last().map(String::as_str), Some("line 2599"));
    assert!(log.truncated);

    std::fs::remove_file(dir.join("stacklyd.log.2")).unwrap();
    std::fs::remove_file(dir.join("stacklyd.log.1")).unwrap();
    write("stacklyd.log", 0, 3);
    let short = daemon_log(platform.fs(), &dir).unwrap();
    assert_eq!(short.lines, ["line 0", "line 1", "line 2"]);
    assert!(!short.truncated);
    let empty = root.path().join("none");
    std::fs::create_dir(&empty).unwrap();
    assert!(daemon_log(platform.fs(), &empty).unwrap().lines.is_empty());
}
