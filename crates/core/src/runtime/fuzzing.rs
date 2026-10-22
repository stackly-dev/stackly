//! Fuzz harnesses of the runtime extractor and the Mach-O parser (spec
//! 0018, AC-19). The cargo-fuzz targets `runtime_archive` and
//! `runtime_macho` in `fuzz/` and the stable regression tests in
//! `crates/core/tests/fuzz_regressions.rs` call these same functions. Each
//! one panics when an invariant breaks, which is what the fuzzer reports as
//! a crash; an input the extractor or the parser refuses is a normal
//! outcome.
//!
//! Both run on bytes in memory: the archive goes through [`scan`], the
//! extractor into a tree in memory that keeps no file contents, so no
//! harness touches a filesystem.

use std::collections::BTreeSet;

use stackly_domain::ArtifactFormat;
use stackly_platform::EntryPath;

use super::archive::{EntryKind, ExtractLimits, ExtractReport, ExtractSpec, scan};
use super::macho::{FAT_ARCHES_MAX, parse_bytes};
use super::samples::MemoryFile;

/// The limits every `runtime_archive` input runs with: small, so one
/// input stays fast and its memory bounded.
pub const ARCHIVE_LIMITS: ExtractLimits = ExtractLimits::new(1 << 20, 1000);

/// The bit of the first `runtime_archive` byte that turns the folder strip
/// off for a `tar_gz` or `zip` input.
pub const NO_ROOT_BIT: u8 = 0b100;

/// The first byte of a `runtime_archive` seed: the format in the low two
/// bits (`0` `tar_gz`, `1` `zip`, anything else `binary`), then
/// [`NO_ROOT_BIT`].
#[must_use]
pub fn archive_selector(format: ArtifactFormat, strips_root: bool) -> u8 {
    let code = match format {
        ArtifactFormat::TarGz => 0,
        ArtifactFormat::Zip => 1,
        _ => 2,
    };
    code | if strips_root { 0 } else { NO_ROOT_BIT }
}

/// `runtime_archive`: the first byte picks the format and whether a
/// `pkg/` folder is stripped, the rest is an artifact. The extractor must
/// not panic, and what it reports must be consistent: the counts add up,
/// the limits hold, no entry keeps a setuid, setgid, or sticky bit, no path
/// repeats, and every entry's folder is in the tree before it.
///
/// # Panics
///
/// Panics when an invariant breaks, which is the crash the fuzzer reports.
pub fn archive(data: &[u8]) {
    let Some((&selector, artifact)) = data.split_first() else {
        return;
    };
    let (format, root, executables): (_, Option<&str>, &[&str]) = match selector & 0b11 {
        0 => (ArtifactFormat::TarGz, Some("pkg"), &["bin/tool"]),
        1 => (ArtifactFormat::Zip, Some("pkg"), &["bin/tool"]),
        _ => (ArtifactFormat::Binary, None, &["tool"]),
    };
    let root = if selector & NO_ROOT_BIT != 0 && format != ArtifactFormat::Binary {
        None
    } else {
        root
    };
    let spec = ExtractSpec::new(
        root.map(str::to_owned),
        executables
            .iter()
            .map(|path| EntryPath::new(path).expect("the harness executables are valid paths"))
            .collect(),
        ARCHIVE_LIMITS,
    );
    let mut input = MemoryFile::new(artifact.to_vec());
    match scan(format, &mut input, &spec) {
        Ok(report) => check_report(&report, ARCHIVE_LIMITS),
        Err(error) => assert!(
            !error.to_string().is_empty(),
            "an extraction error has a message"
        ),
    }
}

fn check_report(report: &ExtractReport, limits: ExtractLimits) {
    assert_eq!(
        report.entries.len() as u64,
        report.files + report.dirs + report.symlinks,
        "the entries are the files, folders, and symlinks"
    );
    let mut bytes = 0_u64;
    let mut seen = BTreeSet::new();
    let mut dirs = BTreeSet::new();
    for entry in &report.entries {
        let path = entry.path.as_str();
        assert!(seen.insert(path), "{path} appears twice");
        if let Some(parent) = entry.path.parent() {
            assert!(
                dirs.contains(parent.as_str()),
                "{path} comes before its folder"
            );
        }
        match entry.kind {
            EntryKind::Dir => {
                dirs.insert(path);
            }
            EntryKind::File => bytes += entry.size,
            EntryKind::Symlink => {
                assert!(entry.mode.is_none(), "{path}: a symlink has no mode");
                assert!(entry.target.is_some(), "{path}: a symlink has a target");
            }
        }
        if let Some(mode) = entry.mode {
            assert_eq!(mode.bits() & 0o7000, 0, "{path}: a special bit survived");
        }
    }
    assert_eq!(bytes, report.file_bytes, "the file sizes add up");
    assert!(
        report.file_bytes <= limits.max_total_file_bytes,
        "the files hold more than the limit"
    );
    assert!(
        report.entry_count() <= limits.max_entries,
        "the entries pass the limit"
    );
}

/// `runtime_macho`: the bytes as a Mach-O file, thin or universal. The
/// parser must not panic, and what it reports must hold together: a
/// universal file lists at most 30 slices, and the arm64 slice, when there
/// is one, is the arm64 or arm64e one listed.
///
/// # Panics
///
/// Panics when an invariant breaks, which is the crash the fuzzer reports.
pub fn macho(data: &[u8]) {
    match parse_bytes(data) {
        Ok(parsed) => {
            assert!(
                !parsed.arches.is_empty() && parsed.arches.len() <= FAT_ARCHES_MAX as usize,
                "a Mach-O file lists 1 to {FAT_ARCHES_MAX} slices"
            );
            if let Some(slice) = &parsed.arm64 {
                assert!(slice.arch.is_arm64(), "the arm64 slice is an arm64 one");
                assert!(
                    parsed.arches.contains(&slice.arch),
                    "the arm64 slice is one the file lists"
                );
            }
        }
        Err(error) => assert!(
            !error.to_string().is_empty(),
            "a Mach-O error has a message"
        ),
    }
}
