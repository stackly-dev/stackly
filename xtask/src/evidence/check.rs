//! `cargo xtask evidence check`: the static evidence rules of spec 0002.
//!
//! It validates the manual records in `evidence/manual/` and the baseline
//! in `evidence/baseline-host.json`, then scans the Rust and Swift sources
//! for the native test markers. It never reads `evidence/runtime/`; that is
//! `cargo xtask runtime check` (spec 0003).

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result};
use stackly_evidence::{BaselineHost, EvidenceRecord, Method};
use uuid::Uuid;

use super::context::BASELINE_FILE;
use super::{rust_scan, swift_scan};

/// Manual records, relative to the repository root.
pub(super) const MANUAL_DIR: &str = "evidence/manual";
/// Folders scanned for Rust sources, and the folder names skipped in them.
const RUST_ROOTS: [&str; 3] = ["crates", "apps", "xtask"];
const RUST_SKIP: [&str; 3] = ["target", ".build", "DerivedData"];
/// Folders scanned for Swift sources, and the folder names skipped in them.
const SWIFT_ROOTS: [&str; 2] = ["clients", "apps/macos"];
const SWIFT_SKIP: [&str; 4] = [".build", ".swiftpm", "DerivedData", "Generated"];
/// A Swift test target for native tests has a name with this suffix.
const NATIVE_TARGET_SUFFIX: &str = "NativeTests";

pub fn run(root: &Path, args: &[&str]) -> Result<ExitCode> {
    if let Some(extra) = args.first() {
        anyhow::bail!("`evidence check` takes no arguments, found `{extra}`");
    }
    let mut violations = Vec::new();
    let manual = check_manual(root, &mut violations)?;
    let baseline = check_baseline(root, &mut violations)?;

    let rust_files = source_files(root, &RUST_ROOTS, &RUST_SKIP, "rs")?;
    for file in &rust_files {
        let (relative, source) = read_source(root, file)?;
        violations.extend(rust_scan::scan(&relative, &source));
    }
    let swift_files = source_files(root, &SWIFT_ROOTS, &SWIFT_SKIP, "swift")?;
    for file in &swift_files {
        let (relative, source) = read_source(root, file)?;
        violations.extend(swift_scan::scan(
            &relative,
            &source,
            in_native_target(root, file),
        ));
    }

    if violations.is_empty() {
        println!(
            "evidence check: {manual} manual record(s), baseline {}, {} Rust and {} Swift file(s) follow the rules",
            if baseline {
                "valid"
            } else {
                "not recorded yet"
            },
            rust_files.len(),
            swift_files.len(),
        );
        return Ok(ExitCode::SUCCESS);
    }
    for violation in &violations {
        println!("{violation}");
    }
    eprintln!(
        "evidence check: {} rule violation(s), see spec 0002 (Penanda test native, Record manual)",
        violations.len()
    );
    Ok(ExitCode::FAILURE)
}

/// Validates every `evidence/manual/*.json` and returns how many there are.
fn check_manual(root: &Path, violations: &mut Vec<String>) -> Result<usize> {
    let dir = root.join(MANUAL_DIR);
    if !dir.exists() {
        return Ok(0);
    }
    let mut names = Vec::new();
    for entry in fs::read_dir(&dir).with_context(|| format!("cannot read {}", dir.display()))? {
        let entry = entry.with_context(|| format!("cannot read {}", dir.display()))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        // `files/` holds evidence files, and `*.tmp` is an unfinished write.
        if entry.path().is_file() && name.ends_with(".json") {
            names.push(name);
        }
    }
    names.sort();
    let mut seen: BTreeMap<Uuid, String> = BTreeMap::new();
    for name in &names {
        let relative = format!("{MANUAL_DIR}/{name}");
        let bytes = fs::read(dir.join(name)).with_context(|| format!("cannot read {relative}"))?;
        let record: EvidenceRecord = match serde_json::from_slice(&bytes) {
            Ok(record) => record,
            Err(err) => {
                violations.push(format!(
                    "{relative}: not a stackly.evidence/v1 record: {err}"
                ));
                continue;
            }
        };
        for problem in manual_violations(root, &relative, &record) {
            violations.push(format!("{relative}: {problem}"));
        }
        if let Some(first) = seen.insert(record.id, relative.clone()) {
            violations.push(format!(
                "{relative}: id {} is already used by {first}; a new review needs a new record from `cargo xtask evidence new`",
                record.id
            ));
        }
    }
    Ok(names.len())
}

/// The rules one manual record breaks. `relative` is its path from the
/// repository root.
fn manual_violations(root: &Path, relative: &str, record: &EvidenceRecord) -> Vec<String> {
    let mut problems = match record.validate() {
        Ok(()) => Vec::new(),
        Err(err) => err.violations,
    };
    if record.method != Method::Manual {
        problems.push("a record in evidence/manual has method manual".to_owned());
    }
    if record.context.is_none() {
        problems
            .push("a manual record needs context; `cargo xtask evidence new` fills it".to_owned());
    }
    let path = record.evidence_path.as_str();
    let files = format!("{MANUAL_DIR}/files/{}", record.id);
    let own_files = path.trim_end_matches('/') == files;
    if path != relative && !own_files {
        problems.push(format!(
            "evidencePath `{path}` must be the record itself ({relative}) or {files}/"
        ));
    } else if !root.join(path).exists() {
        problems.push(format!("evidencePath `{path}` does not exist"));
    }
    problems
}

/// Validates `evidence/baseline-host.json` when it exists, and returns
/// whether it is there.
fn check_baseline(root: &Path, violations: &mut Vec<String>) -> Result<bool> {
    let path = root.join(BASELINE_FILE);
    if !path.exists() {
        return Ok(false);
    }
    let bytes = fs::read(&path).with_context(|| format!("cannot read {BASELINE_FILE}"))?;
    match serde_json::from_slice::<BaselineHost>(&bytes) {
        Ok(baseline) => {
            if let Err(err) = baseline.validate() {
                violations.extend(
                    err.violations
                        .into_iter()
                        .map(|problem| format!("{BASELINE_FILE}: {problem}")),
                );
            }
        }
        Err(err) => violations.push(format!(
            "{BASELINE_FILE}: not a stackly.evidence-baseline/v1 file: {err}"
        )),
    }
    Ok(true)
}

/// Every file with `extension` below `roots`, skipping folders named in
/// `skip` and symbolic links, sorted.
fn source_files(
    root: &Path,
    roots: &[&str],
    skip: &[&str],
    extension: &str,
) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let mut pending: Vec<PathBuf> = roots
        .iter()
        .map(|dir| root.join(dir))
        .filter(|dir| dir.is_dir())
        .collect();
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(&dir).with_context(|| format!("cannot read {}", dir.display()))? {
            let entry = entry.with_context(|| format!("cannot read {}", dir.display()))?;
            let file_type = entry.file_type()?;
            let path = entry.path();
            if file_type.is_dir() {
                if !skip.iter().any(|name| entry.file_name() == *name) {
                    pending.push(path);
                }
            } else if file_type.is_file() && path.extension().is_some_and(|ext| ext == extension) {
                files.push(path);
            }
        }
    }
    files.sort();
    files.dedup();
    Ok(files)
}

fn read_source(root: &Path, file: &Path) -> Result<(String, String)> {
    let relative = file
        .strip_prefix(root)
        .unwrap_or(file)
        .to_string_lossy()
        .into_owned();
    let source = fs::read_to_string(file).with_context(|| format!("cannot read {relative}"))?;
    Ok((relative, source))
}

/// True when a folder between the repository root and `file` has a name
/// ending with `NativeTests`.
fn in_native_target(root: &Path, file: &Path) -> bool {
    let relative = file.strip_prefix(root).unwrap_or(file);
    relative.parent().is_some_and(|folders| {
        folders.components().any(|part| {
            part.as_os_str()
                .to_string_lossy()
                .ends_with(NATIVE_TARGET_SUFFIX)
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLES: &str = "schemas/evidence/examples";

    fn repo_root() -> PathBuf {
        crate::repo_root()
    }

    /// A temporary repository root holding `evidence/manual/` with the
    /// shared example `example` saved as `name`.
    fn root_with_manual(example: &str, name: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let manual = dir.path().join(MANUAL_DIR);
        fs::create_dir_all(&manual).unwrap();
        fs::copy(repo_root().join(EXAMPLES).join(example), manual.join(name)).unwrap();
        dir
    }

    fn manual_check(root: &Path) -> Vec<String> {
        let mut violations = Vec::new();
        check_manual(root, &mut violations).unwrap();
        violations
    }

    #[test]
    fn a_new_manual_record_that_points_at_itself_passes() {
        let name = "2026-10-18-menu-bar-shows-a-stopped-instance.json";
        let root = root_with_manual("record-manual-not-run.json", name);
        assert_eq!(manual_check(root.path()), Vec::<String>::new());
    }

    #[test]
    fn a_manual_record_whose_evidence_files_are_missing_fails() {
        let root = root_with_manual("record-manual-native-pass.json", "2026-10-18-postgres.json");
        let found = manual_check(root.path());
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("does not exist"));

        let files = root
            .path()
            .join(MANUAL_DIR)
            .join("files/01a15a41-7b3e-7f28-9a5c-2d3e4f5a6b7c");
        fs::create_dir_all(files).unwrap();
        assert_eq!(manual_check(root.path()), Vec::<String>::new());
    }

    #[test]
    fn a_manual_record_pointing_elsewhere_or_renamed_fails() {
        let root = root_with_manual("record-manual-not-run.json", "renamed.json");
        let found = manual_check(root.path());
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].starts_with("evidence/manual/renamed.json: evidencePath"));
    }

    #[test]
    fn automated_records_without_context_and_rule_breaks_are_reported() {
        let root = root_with_manual("record-rust-fixture-not-run.json", "a.json");
        fs::copy(
            repo_root()
                .join(EXAMPLES)
                .join("invalid-record-final-manual-without-reviewer.json"),
            root.path().join(MANUAL_DIR).join("b.json"),
        )
        .unwrap();
        fs::write(root.path().join(MANUAL_DIR).join("c.json"), "{}").unwrap();
        fs::write(root.path().join(MANUAL_DIR).join("d.json.tmp"), "{").unwrap();
        let found = manual_check(root.path());
        let joined = found.join("\n");
        assert!(joined.contains("a.json: a record in evidence/manual has method manual"));
        assert!(joined.contains("a.json: a manual record needs context"));
        assert!(joined.contains("b.json: a final manual record needs a reviewer"));
        assert!(joined.contains("c.json: not a stackly.evidence/v1 record"));
        assert!(!joined.contains("d.json.tmp"));
    }

    #[test]
    fn a_copied_manual_record_with_the_same_id_fails() {
        let root = root_with_manual(
            "record-manual-deferred.json",
            "2026-10-18-import-from-docker-compose.json",
        );
        fs::copy(
            root.path()
                .join(MANUAL_DIR)
                .join("2026-10-18-import-from-docker-compose.json"),
            root.path().join(MANUAL_DIR).join("2026-10-19-copy.json"),
        )
        .unwrap();
        let found = manual_check(root.path());
        assert!(
            found.iter().any(|v| v.contains("is already used by")),
            "{found:?}"
        );
    }

    #[test]
    fn the_baseline_is_validated_when_present() {
        let dir = tempfile::tempdir().unwrap();
        let mut violations = Vec::new();
        assert!(!check_baseline(dir.path(), &mut violations).unwrap());

        fs::create_dir_all(dir.path().join("evidence")).unwrap();
        fs::copy(
            repo_root()
                .join(EXAMPLES)
                .join("invalid-baseline-host-virtualized.json"),
            dir.path().join(BASELINE_FILE),
        )
        .unwrap();
        assert!(check_baseline(dir.path(), &mut violations).unwrap());
        assert_eq!(
            violations,
            ["evidence/baseline-host.json: the baseline host is a real Mac, not a VM"]
        );
    }

    #[test]
    fn source_walk_skips_build_folders_and_finds_native_targets() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        for file in [
            "clients/swift/Tests/StacklyClientNativeTests/PostgresNativeTests.swift",
            "clients/swift/Tests/StacklyClientTests/X.swift",
            "clients/swift/.build/checkouts/Y.swift",
            "clients/swift/Sources/StacklyClient/Generated/Z.swift",
        ] {
            let path = root.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "").unwrap();
        }
        let files = source_files(root, &SWIFT_ROOTS, &SWIFT_SKIP, "swift").unwrap();
        let native: Vec<bool> = files.iter().map(|f| in_native_target(root, f)).collect();
        assert_eq!(files.len(), 2);
        assert_eq!(native, [true, false]);
    }
}
