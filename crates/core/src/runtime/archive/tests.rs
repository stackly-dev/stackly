// The fake tree writer of `stackly-testkit` needs Unix file modes.
#![cfg(unix)]

use std::collections::BTreeSet;
use std::fs;
use std::io::{self, Read, Seek, SeekFrom};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use stackly_evidence::{Kind, evidence};
use stackly_platform::Filesystem;
use stackly_testkit::{FakeFilesystem, Fault, FsOp, TestRoot};

use super::*;
use crate::runtime::samples::{self, ArchiveSample, MemoryFile, TarBuilder, ZipBuilder, ZipEntry};

/// A test root with an empty stage folder 0700 and a sentinel folder next
/// to it, which a planted symlink in a sample points at.
struct Stage {
    root: TestRoot,
    fs: FakeFilesystem,
    stage: PathBuf,
    sentinel: PathBuf,
}

impl Stage {
    fn new() -> Self {
        let root = TestRoot::new().unwrap();
        let fs = FakeFilesystem::new(&root);
        let stage = root.path().join("stage");
        fs.create_dir_private(&stage).unwrap();
        let sentinel = root.path().join("sentinel");
        fs::create_dir(&sentinel).unwrap();
        fs::write(sentinel.join("keep"), b"sentinel").unwrap();
        Self {
            root,
            fs,
            stage,
            sentinel,
        }
    }

    fn extract(&self, sample: &ArchiveSample) -> Result<ExtractReport, ArchiveError> {
        let mut writer = self.fs.open_tree_writer(&self.stage).unwrap();
        extract(
            sample.format,
            &mut MemoryFile::new(sample.bytes.clone()),
            &sample.spec,
            writer.as_mut(),
            &StopFlag::new(),
        )
    }

    /// Fails unless the sentinel and the test root hold exactly what they
    /// held before the extraction.
    fn assert_untouched_outside(&self, name: &str) {
        let top: BTreeSet<_> = fs::read_dir(self.root.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(
            top,
            BTreeSet::from(["sentinel".into(), "stage".into()]),
            "{name}"
        );
        let inside: Vec<_> = fs::read_dir(&self.sentinel)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(inside, ["keep"], "{name}");
        assert_eq!(
            fs::read(self.sentinel.join("keep")).unwrap(),
            b"sentinel",
            "{name}"
        );
    }
}

fn mode_of(path: &Path) -> u32 {
    fs::symlink_metadata(path).unwrap().permissions().mode() & 0o7777
}

fn record<'a>(report: &'a ExtractReport, path: &str) -> &'a EntryRecord {
    report
        .entries
        .iter()
        .find(|entry| entry.path.as_str() == path)
        .unwrap_or_else(|| panic!("no entry {path} in {:#?}", report.entries))
}

fn final_mode(report: &ExtractReport, path: &str) -> Option<u32> {
    record(report, path).mode.map(FileMode::bits)
}

fn spec(root: Option<&str>, executables: &[&str]) -> ExtractSpec {
    ExtractSpec::new(
        root.map(str::to_owned),
        executables
            .iter()
            .map(|path| EntryPath::new(path).unwrap())
            .collect(),
        samples::sample_limits(),
    )
}

fn scan_bytes(
    format: ArtifactFormat,
    bytes: Vec<u8>,
    spec: &ExtractSpec,
) -> Result<ExtractReport, ArchiveError> {
    scan(format, &mut MemoryFile::new(bytes), spec)
}

fn reason_of(result: Result<ExtractReport, ArchiveError>) -> RejectReason {
    match result {
        Err(ArchiveError::Rejected { reason, .. }) => reason,
        other => panic!("expected a rejection, got {other:?}"),
    }
}

#[test]
fn every_sample_extracts_or_stops_with_its_reason_and_writes_nothing_outside_the_stage() {
    let ev = evidence!(
        covers = ["VAL-011", "AC-057", "AC-060"],
        kind = Kind::Fixture,
        expected = "every sample tar.gz, zip, and binary artifact in stackly_core::runtime::samples either extracts through the fake tree writer or stops with its fixed runtime.archive_rejected reason (one sample or more per reason), and the sentinel folder next to the stage and the test root stay as they were (VAL-011, AC-057, AC-060)",
    );
    let samples = samples::archive_samples();
    for sample in &samples {
        let stage = Stage::new();
        let result = stage.extract(sample);
        match (sample.expected, &result) {
            (None, Ok(_)) => {}
            (Some(expected), Err(ArchiveError::Rejected { reason, .. })) if *reason == expected => {
            }
            (expected, got) => panic!("{}: expected {expected:?}, got {got:?}", sample.name),
        }
        stage.assert_untouched_outside(sample.name);
    }
    ev.pass_with(&format!("{} samples", samples.len()));
}

#[test]
fn every_reject_reason_has_a_sample() {
    let covered: BTreeSet<&str> = samples::archive_samples()
        .iter()
        .filter_map(|sample| sample.expected.map(RejectReason::as_code))
        .collect();
    let all: BTreeSet<&str> = RejectReason::ALL
        .iter()
        .map(|reason| reason.as_code())
        .collect();
    assert_eq!(covered, all);
}

#[test]
fn scan_reports_what_extract_writes() {
    for sample in samples::archive_samples() {
        let stage = Stage::new();
        let written = stage.extract(&sample);
        let scanned = scan_bytes(sample.format, sample.bytes.clone(), &sample.spec);
        assert_eq!(scanned, written, "{}", sample.name);
    }
}

#[test]
fn the_tar_gz_layout_writes_its_tree_and_reports_final_modes() {
    let stage = Stage::new();
    let sample = &samples::archive_samples()[0];
    assert_eq!(sample.name, "tar_gz_layout");

    let report = stage.extract(sample).unwrap();

    let long_name = format!("share/{}", "n".repeat(120));
    assert_eq!(
        (report.files, report.dirs, report.symlinks),
        (5, 3, 2),
        "{report:#?}"
    );
    assert_eq!(report.skipped_metadata, 2);
    assert_eq!(report.stripped_special_bits, 1);
    assert_eq!(report.entry_count(), 12);
    assert_eq!(final_mode(&report, "bin/tool"), Some(0o555));
    assert_eq!(final_mode(&report, "bin/helper"), Some(0o555));
    assert_eq!(final_mode(&report, "lib/libz.1.dylib"), Some(0o444));
    assert_eq!(final_mode(&report, "lib"), Some(0o755));
    assert_eq!(record(&report, "lib/libz.dylib").kind, EntryKind::Symlink);
    assert_eq!(record(&report, "lib/libz.dylib").mode, None);
    assert_eq!(
        record(&report, "bin/libz.1.dylib").target.as_deref(),
        Some("../lib/libz.1.dylib")
    );
    assert_eq!(record(&report, &long_name).size, 13);
    assert_eq!(
        fs::read(stage.stage.join("share/from-pax.txt")).unwrap(),
        b"pax path wins"
    );
    assert!(!stage.stage.join("share/ignored-name").exists());
    assert!(!stage.stage.join("bin/._tool").exists());
    assert!(!stage.stage.join("__MACOSX").exists());
    // Modes during extraction: folders 0700, files 0600; the final modes
    // come later, in the validate step.
    assert_eq!(mode_of(&stage.stage.join("bin")), 0o700);
    assert_eq!(mode_of(&stage.stage.join("bin/helper")), 0o600);
    assert_eq!(
        fs::read_link(stage.stage.join("bin/libz.1.dylib")).unwrap(),
        Path::new("../lib/libz.1.dylib")
    );
    assert_eq!(
        fs::read(stage.stage.join("bin/libz.1.dylib")).unwrap(),
        b"library bytes"
    );
}

#[test]
fn the_zip_layout_trusts_mode_bits_from_unix_only_and_marks_executables() {
    let stage = Stage::new();
    let sample = samples::archive_samples()
        .into_iter()
        .find(|sample| sample.name == "zip_layout")
        .unwrap();

    let report = stage.extract(&sample).unwrap();

    assert_eq!((report.files, report.dirs, report.symlinks), (4, 2, 1));
    assert_eq!(report.skipped_metadata, 2);
    assert_eq!(final_mode(&report, "bin/server"), Some(0o555));
    assert_eq!(final_mode(&report, "lib/data.txt"), Some(0o444));
    assert_eq!(record(&report, "lib/dos-link").kind, EntryKind::File);
    assert_eq!(
        fs::read(stage.stage.join("lib/descriptor.txt")).unwrap(),
        b"descriptor"
    );
    assert_eq!(
        fs::read(stage.stage.join("lib/data.txt")).unwrap(),
        b"deflated ".repeat(64)
    );
    assert_eq!(
        fs::read_link(stage.stage.join("lib/current")).unwrap(),
        Path::new("data.txt")
    );
}

#[test]
fn a_binary_artifact_is_written_to_its_executable() {
    let stage = Stage::new();
    let sample = samples::archive_samples()
        .into_iter()
        .find(|sample| sample.name == "binary_layout")
        .unwrap();

    let report = stage.extract(&sample).unwrap();

    assert_eq!(report.files, 1);
    assert_eq!(report.file_bytes, sample.bytes.len() as u64);
    assert_eq!(final_mode(&report, "server"), Some(0o555));
    assert_eq!(fs::read(stage.stage.join("server")).unwrap(), sample.bytes);
}

#[test]
fn a_tar_gz_executable_keeps_the_mode_the_archive_gives_it() {
    let bytes = TarBuilder::new().file("bin/tool", 0o644, b"x").finish_gz();

    let report = scan_bytes(ArtifactFormat::TarGz, bytes, &spec(None, &["bin/tool"])).unwrap();

    assert_eq!(final_mode(&report, "bin/tool"), Some(0o444));
}

#[test]
fn a_raised_stop_flag_stops_before_the_next_member() {
    let stage = Stage::new();
    let sample = &samples::archive_samples()[0];
    let stop = StopFlag::new();
    stop.stop();
    let mut writer = stage.fs.open_tree_writer(&stage.stage).unwrap();

    let result = extract(
        sample.format,
        &mut MemoryFile::new(sample.bytes.clone()),
        &sample.spec,
        writer.as_mut(),
        &stop,
    );

    assert_eq!(result, Err(ArchiveError::Stopped));
    assert_eq!(fs::read_dir(&stage.stage).unwrap().count(), 0);
}

#[test]
fn no_space_while_writing_a_file_is_a_write_error() {
    let stage = Stage::new();
    stage
        .fs
        .fail_when(FsOp::TreeCreateFile, "bin/tool", Fault::NoSpace);

    let result = stage.extract(&samples::archive_samples()[0]);

    assert!(
        matches!(
            result,
            Err(ArchiveError::Write(PlatformError::NoSpace { .. }))
        ),
        "{result:?}"
    );
}

#[test]
fn names_that_differ_only_in_letter_case_are_one_entry() {
    let bytes = TarBuilder::new()
        .file("Foo", 0o644, b"x")
        .file("foo", 0o644, b"y")
        .finish_gz();
    let sample_spec = spec(None, &[]);

    let scanned = scan_bytes(ArtifactFormat::TarGz, bytes.clone(), &sample_spec);

    assert_eq!(reason_of(scanned), RejectReason::DuplicateEntry);
    let probe = TestRoot::new().unwrap();
    fs::write(probe.path().join("Probe"), b"x").unwrap();
    if probe.path().join("probe").exists() {
        let stage = Stage::new();
        let mut writer = stage.fs.open_tree_writer(&stage.stage).unwrap();
        let written = extract(
            ArtifactFormat::TarGz,
            &mut MemoryFile::new(bytes),
            &sample_spec,
            writer.as_mut(),
            &StopFlag::new(),
        );
        assert_eq!(reason_of(written), RejectReason::DuplicateEntry);
    }
}

#[test]
fn a_folder_named_again_is_one_folder_but_a_file_named_again_is_a_duplicate() {
    let implied_then_named = ZipBuilder::new()
        .entry(ZipEntry::file("a/b", b"x"))
        .entry(ZipEntry::dir("a"))
        .finish();
    // The MySQL tarballs name `bin/` five times.
    let named_three_times = TarBuilder::new()
        .dir("pkg/bin/", 0o755)
        .file("pkg/bin/mysqld", 0o755, b"x")
        .dir("pkg/bin/", 0o755)
        .dir("pkg/bin/", 0o4755)
        .finish_gz();
    let folder_then_file = TarBuilder::new()
        .dir("a/", 0o755)
        .file("a", 0o644, b"x")
        .finish_gz();
    let symlink_twice = TarBuilder::new()
        .file("a", 0o644, b"x")
        .symlink("b", "a")
        .symlink("b", "a")
        .finish_gz();

    let report = scan_bytes(ArtifactFormat::Zip, implied_then_named, &spec(None, &[])).unwrap();
    assert_eq!((report.files, report.dirs), (1, 1));
    let report = scan_bytes(
        ArtifactFormat::TarGz,
        named_three_times,
        &spec(Some("pkg"), &[]),
    )
    .unwrap();
    assert_eq!(
        (report.files, report.dirs, report.stripped_special_bits),
        (1, 1, 0)
    );
    for bytes in [folder_then_file, symlink_twice] {
        assert_eq!(
            reason_of(scan_bytes(ArtifactFormat::TarGz, bytes, &spec(None, &[]))),
            RejectReason::DuplicateEntry
        );
    }
}

#[test]
fn the_root_entry_and_archive_root_itself_are_not_entries() {
    let bytes = TarBuilder::new()
        .dir("./", 0o755)
        .dir("./pkg/", 0o755)
        .file("./pkg/tool", 0o755, b"x")
        .finish_gz();

    let report = scan_bytes(ArtifactFormat::TarGz, bytes, &spec(Some("pkg/"), &["tool"])).unwrap();

    assert_eq!(report.entry_count(), 1);
    assert_eq!(record(&report, "tool").kind, EntryKind::File);
}

#[test]
fn a_spec_that_does_not_fit_the_format_is_refused() {
    let binary = b"\xcf\xfa\xed\xfe".to_vec();
    let cases = [
        (ArtifactFormat::Binary, spec(None, &[])),
        (ArtifactFormat::Binary, spec(None, &["bin/server"])),
        (ArtifactFormat::Binary, spec(Some("pkg"), &["server"])),
        (ArtifactFormat::TarGz, spec(Some("../pkg"), &[])),
    ];
    for (format, bad) in cases {
        let result = scan_bytes(format, binary.clone(), &bad);
        assert!(
            matches!(result, Err(ArchiveError::InvalidSpec { .. })),
            "{format}: {result:?}"
        );
    }
}

/// An artifact file whose reads fail after the first `ok` bytes.
struct FailingFile {
    inner: MemoryFile,
    ok: u64,
}

impl Read for FailingFile {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let position = self.inner.stream_position()?;
        if position >= self.ok {
            return Err(io::Error::other("injected read failure"));
        }
        let room = usize::try_from(self.ok - position).unwrap_or(usize::MAX);
        let limit = buf.len().min(room);
        self.inner.read(&mut buf[..limit])
    }
}

impl Seek for FailingFile {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        self.inner.seek(pos)
    }
}

impl ReadSeekFile for FailingFile {
    fn len(&self) -> u64 {
        self.inner.len()
    }
}

#[test]
fn a_read_failure_of_the_artifact_file_is_not_a_corrupt_archive() {
    for (format, bytes) in [
        (ArtifactFormat::TarGz, samples::tar_gz_layout()),
        (ArtifactFormat::Zip, samples::zip_layout()),
    ] {
        let mut file = FailingFile {
            inner: MemoryFile::new(bytes),
            ok: 40,
        };
        let result = extract(
            format,
            &mut file,
            &spec(None, &[]),
            &mut MemoryTree::default(),
            &StopFlag::new(),
        );
        assert!(
            matches!(result, Err(ArchiveError::Read { .. })),
            "{format}: {result:?}"
        );
    }
}

#[test]
fn a_rejected_entry_names_the_member_cut_to_256_characters() {
    let long = format!("../{}", "x".repeat(300));
    let bytes = TarBuilder::new().file(&long, 0o644, b"x").finish_gz();

    let result = scan_bytes(ArtifactFormat::TarGz, bytes, &spec(None, &[]));

    match result {
        Err(ArchiveError::Rejected { reason, entry }) => {
            assert_eq!(reason, RejectReason::PathTraversal);
            assert_eq!(entry.chars().count(), 256);
            assert!(entry.starts_with("../xxx"));
        }
        other => panic!("{other:?}"),
    }
}
