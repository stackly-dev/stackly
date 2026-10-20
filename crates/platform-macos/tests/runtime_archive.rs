//! The runtime extractor of `stackly-core` writing through
//! `MacosTreeWriter`: every sample archive gives the same outcome as on the
//! fake tree writer, nothing lands outside the stage, and no extracted
//! entry carries an extended attribute but the provenance one macOS adds
//! itself (VAL-011, AC-057). Every path lives in a `TestRoot`.
#![cfg(target_os = "macos")]

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use stackly_core::runtime::archive::{ArchiveError, extract};
use stackly_core::runtime::samples::{self, ArchiveSample, MemoryFile};
use stackly_platform::{Filesystem, StopFlag};
use stackly_platform_macos::MacosFilesystem;
use stackly_testkit::TestRoot;
use stackly_testkit::evidence::{Kind, evidence};

/// The attribute macOS itself puts on files a process creates.
const PROVENANCE: &str = "com.apple.provenance";

struct Stage {
    root: TestRoot,
    stage: PathBuf,
    sentinel: PathBuf,
}

impl Stage {
    fn new() -> Self {
        let root = TestRoot::new().expect("a test root can be created");
        let stage = root.path().join("stage");
        MacosFilesystem
            .create_dir_private(&stage)
            .expect("the stage folder can be created in a fresh test root");
        let sentinel = root.path().join("sentinel");
        fs::create_dir(&sentinel).expect("the sentinel folder can be created");
        fs::write(sentinel.join("keep"), b"sentinel").expect("the sentinel file can be written");
        Self {
            root,
            stage,
            sentinel,
        }
    }

    fn extract(&self, sample: &ArchiveSample) -> Result<(), ArchiveError> {
        let mut writer = MacosFilesystem
            .open_tree_writer(&self.stage)
            .expect("a fresh stage is an empty private folder");
        extract(
            sample.format,
            &mut MemoryFile::new(sample.bytes.clone()),
            &sample.spec,
            writer.as_mut(),
            &StopFlag::new(),
        )
        .map(drop)
    }

    fn assert_untouched_outside(&self, name: &str) {
        let top: BTreeSet<_> = fs::read_dir(self.root.path())
            .expect("the test root can be listed")
            .map(|entry| entry.expect("a listed entry can be read").file_name())
            .collect();
        assert_eq!(
            top,
            BTreeSet::from(["sentinel".into(), "stage".into()]),
            "{name}"
        );
        let inside: Vec<_> = fs::read_dir(&self.sentinel)
            .expect("the sentinel folder can be listed")
            .map(|entry| entry.expect("a listed entry can be read").file_name())
            .collect();
        assert_eq!(inside, ["keep"], "{name}");
    }
}

/// Every path below `dir`, symlinks not followed.
fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(current) = pending.pop() {
        for entry in fs::read_dir(&current).expect("an extracted folder can be listed") {
            let path = entry.expect("a listed entry can be read").path();
            if fs::symlink_metadata(&path)
                .expect("a listed entry exists")
                .is_dir()
            {
                pending.push(path.clone());
            }
            found.push(path);
        }
    }
    found
}

/// The extended attribute names of `path`, without following a symlink.
fn xattr_names(path: &Path) -> Vec<String> {
    let output = Command::new("/usr/bin/xattr")
        .arg("-s")
        .arg(path)
        .output()
        .expect("/usr/bin/xattr is part of macOS");
    assert!(output.status.success(), "{output:?}");
    String::from_utf8(output.stdout)
        .expect("xattr prints attribute names as UTF-8")
        .lines()
        .map(str::to_owned)
        .collect()
}

#[test]
fn every_sample_gives_the_same_outcome_on_the_macos_tree_writer() {
    let ev = evidence!(
        covers = ["VAL-011", "AC-057", "AC-060"],
        kind = Kind::Fixture,
        expected = "every sample artifact of stackly_core::runtime::samples extracts through MacosTreeWriter or stops with the same runtime.archive_rejected reason as on the fake tree writer, and the sentinel folder next to the stage and the test root stay as they were (VAL-011, AC-057, AC-060)",
    );
    let samples = samples::archive_samples();
    for sample in &samples {
        let stage = Stage::new();
        let result = stage.extract(sample);
        match (sample.expected, &result) {
            (None, Ok(())) => {}
            (Some(expected), Err(ArchiveError::Rejected { reason, .. })) if *reason == expected => {
            }
            (expected, got) => panic!("{}: expected {expected:?}, got {got:?}", sample.name),
        }
        stage.assert_untouched_outside(sample.name);
    }
    ev.pass_with(&format!("{} samples", samples.len()));
}

#[test]
fn extracted_entries_carry_no_extended_attribute_from_the_archive() {
    let ev = evidence!(
        covers = ["VAL-011", "AC-057"],
        kind = Kind::Fixture,
        expected = "after extracting the tar.gz sample (a PAX quarantine attribute and AppleDouble members) and the zip sample (AppleDouble members) through MacosTreeWriter, no entry carries an extended attribute other than com.apple.provenance, and no AppleDouble file exists (VAL-011, AC-057)",
    );
    let mut checked = 0;
    for name in ["tar_gz_layout", "zip_layout"] {
        let sample = samples::archive_samples()
            .into_iter()
            .find(|sample| sample.name == name)
            .unwrap();
        let stage = Stage::new();
        stage.extract(&sample).unwrap();
        for path in walk(&stage.stage) {
            let file_name = path.file_name().unwrap().to_string_lossy().into_owned();
            assert!(
                !file_name.starts_with("._") && file_name != "__MACOSX",
                "{}",
                path.display()
            );
            let names: Vec<_> = xattr_names(&path)
                .into_iter()
                .filter(|attribute| attribute != PROVENANCE)
                .collect();
            assert!(names.is_empty(), "{}: {names:?}", path.display());
            checked += 1;
        }
    }
    ev.pass_with(&format!("{checked} entries checked"));
}
