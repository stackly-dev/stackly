//! Turns the raw records of a run into report records and problems.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use stackly_evidence::{EvidenceRecord, Kind, Problem, ProblemCode, RunContext, Status};
use uuid::Uuid;

/// The records and problems found in a run's `records/` folder.
pub struct Collected {
    pub records: Vec<EvidenceRecord>,
    pub problems: Vec<Problem>,
}

/// Reads every `records/*.json` file (skipping `*.tmp` files that were
/// never renamed), then checks the records against the run.
pub fn collect(
    records_dir: &Path,
    context: &RunContext,
    kind: Kind,
    workspace_root: &Path,
    exit_code: Option<i32>,
) -> Result<Collected> {
    let mut files: Vec<PathBuf> = fs::read_dir(records_dir)
        .with_context(|| format!("cannot read {}", records_dir.display()))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<_, _>>()
        .with_context(|| format!("cannot list {}", records_dir.display()))?;
    files.retain(|path| path.extension().is_some_and(|ext| ext == "json"));
    files.sort();

    let mut decoded = Vec::new();
    let mut problems = Vec::new();
    for path in files {
        let file_name = path
            .file_name()
            .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
        let id = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .and_then(|stem| stem.parse::<Uuid>().ok());
        let parsed = fs::read(&path)
            .map_err(|err| err.to_string())
            .and_then(|bytes| {
                serde_json::from_slice::<EvidenceRecord>(&bytes).map_err(|err| err.to_string())
            });
        match parsed {
            Ok(record) => decoded.push(record),
            Err(err) => problems.push(Problem {
                code: ProblemCode::InvalidRecord,
                record_id: id,
                message: format!("cannot decode records/{file_name}: {err}"),
            }),
        }
    }
    let checked = check(decoded, context, kind, workspace_root, exit_code);
    problems.extend(checked.problems);
    Ok(Collected {
        records: checked.records,
        problems,
    })
}

/// Checks decoded records against the run and completes them: the run
/// context is attached and absolute `test.file` paths become relative to
/// the workspace root.
pub fn check(
    decoded: Vec<EvidenceRecord>,
    context: &RunContext,
    kind: Kind,
    workspace_root: &Path,
    exit_code: Option<i32>,
) -> Collected {
    let mut problems = Vec::new();
    let mut seen = HashSet::new();
    let mut records = Vec::with_capacity(decoded.len());
    for mut record in decoded {
        let mut problem = |code, message: String| {
            problems.push(Problem {
                code,
                record_id: Some(record.id),
                message,
            });
        };
        if let Err(err) = record.validate() {
            problem(ProblemCode::InvalidRecord, err.to_string());
        }
        if record.kind != kind {
            problem(
                ProblemCode::KindMismatch,
                format!(
                    "the record is {} but the run is {}",
                    record.kind.as_str(),
                    kind.as_str()
                ),
            );
        }
        let key = format!("{:?}", record.test.key());
        if !seen.insert(key) {
            problem(
                ProblemCode::DuplicateTest,
                format!("test `{}` wrote more than one record", record.test.name),
            );
        }
        if record.status == Status::NotRun && exit_code == Some(0) {
            problem(
                ProblemCode::LeftNotRun,
                format!(
                    "test `{}` is still NOT_RUN although the command exited 0",
                    record.test.name
                ),
            );
        }
        if let Some(file) = &record.test.file {
            record.test.file = Some(relative_to(file, workspace_root));
        }
        record.context = Some(context.clone());
        records.push(record);
    }
    Collected { records, problems }
}

/// `file` relative to `root` when it is an absolute path under it, with
/// `/` as the separator; otherwise `file` unchanged.
fn relative_to(file: &str, root: &Path) -> String {
    Path::new(file).strip_prefix(root).map_or_else(
        |_| file.to_owned(),
        |relative| {
            relative
                .components()
                .map(|part| part.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/")
        },
    )
}

/// The wrapper's exit code: the command's own failure first (128 plus the
/// signal number when a signal killed it), then 1 when the run has
/// problems, else 0.
pub fn exit_code(command_exit: Option<i32>, signal: Option<i32>, problems: usize) -> u8 {
    if let Some(signal) = signal {
        return u8::try_from(128 + signal).unwrap_or(u8::MAX);
    }
    match command_exit {
        Some(0) if problems == 0 => 0,
        Some(0) | None => 1,
        Some(code) => u8::try_from(code)
            .ok()
            .filter(|code| *code != 0)
            .unwrap_or(1),
    }
}

#[cfg(test)]
mod tests {
    use jiff::Timestamp;
    use stackly_evidence::{Host, RECORD_SCHEMA, Toolchain};

    use super::*;

    fn context() -> RunContext {
        let mut toolchain = Toolchain::default();
        toolchain.rustc = Some("1.99.1".to_owned());
        let mut context = RunContext::new("abc123", false, Host::new("linux", "x86_64"), toolchain);
        context.run_id = Some(Uuid::now_v7());
        context.run_kind = Some(Kind::Fixture);
        context
    }

    /// A raw record as a guard writes it: built from JSON, the way the
    /// wrapper reads it, then adjusted per test.
    fn record(name: &str, kind: Kind, status: Status) -> EvidenceRecord {
        let mut record: EvidenceRecord = serde_json::from_value(serde_json::json!({
            "schema": RECORD_SCHEMA,
            "id": Uuid::now_v7(),
            "covers": ["REQ-051"],
            "kind": kind,
            "method": "automated",
            "test": {
                "language": "rust",
                "package": "stackly-evidence",
                "target": "stackly_evidence",
                "name": name,
                "file": "/repo/crates/evidence/src/guard.rs",
                "line": 7,
            },
            "status": status,
            "expected": "one record per test",
            "startedAt": Timestamp::now(),
            "evidencePath": "output.log",
        }))
        .unwrap();
        if status == Status::Pass {
            record.actual = Some("all assertions passed".to_owned());
        }
        if status.is_final() {
            record.finished_at = Some(Timestamp::now());
        }
        record
    }

    fn codes(collected: &Collected) -> Vec<ProblemCode> {
        collected
            .problems
            .iter()
            .map(|problem| problem.code)
            .collect()
    }

    #[test]
    fn a_clean_run_has_no_problems_and_complete_records() {
        let collected = check(
            vec![record("tests::a", Kind::Fixture, Status::Pass)],
            &context(),
            Kind::Fixture,
            Path::new("/repo"),
            Some(0),
        );
        assert!(collected.problems.is_empty());
        let only = &collected.records[0];
        assert_eq!(
            only.test.file.as_deref(),
            Some("crates/evidence/src/guard.rs")
        );
        assert!(only.context.is_some());
    }

    #[test]
    fn kind_mismatch_duplicate_and_left_not_run_are_problems() {
        let collected = check(
            vec![
                record("tests::a", Kind::Native, Status::Blocked),
                record("tests::b", Kind::Fixture, Status::Pass),
                record("tests::b", Kind::Fixture, Status::Pass),
                record("tests::c", Kind::Fixture, Status::NotRun),
            ],
            &context(),
            Kind::Fixture,
            Path::new("/repo"),
            Some(0),
        );
        assert_eq!(
            codes(&collected),
            [
                // The BLOCKED record also lacks reason and owner.
                ProblemCode::InvalidRecord,
                ProblemCode::KindMismatch,
                ProblemCode::DuplicateTest,
                ProblemCode::LeftNotRun,
            ]
        );
    }

    #[test]
    fn not_run_after_a_failed_command_is_not_a_problem() {
        let collected = check(
            vec![record("tests::crashed", Kind::Fixture, Status::NotRun)],
            &context(),
            Kind::Fixture,
            Path::new("/repo"),
            Some(101),
        );
        assert!(collected.problems.is_empty());
    }

    #[test]
    fn exit_code_prefers_the_command_then_problems() {
        assert_eq!(exit_code(Some(0), None, 0), 0);
        assert_eq!(exit_code(Some(0), None, 2), 1);
        assert_eq!(exit_code(Some(101), None, 0), 101);
        assert_eq!(exit_code(None, Some(15), 0), 143);
        assert_eq!(exit_code(None, None, 0), 1);
        assert_eq!(exit_code(Some(-1), None, 0), 1);
    }

    #[test]
    fn unreadable_files_are_invalid_records() {
        let dir = tempfile::tempdir().unwrap();
        let id = Uuid::now_v7();
        fs::write(dir.path().join(format!("{id}.json")), b"{ not json").unwrap();
        fs::write(dir.path().join(format!("{id}.json.tmp")), b"half written").unwrap();

        let collected = collect(
            dir.path(),
            &context(),
            Kind::Fixture,
            Path::new("/repo"),
            Some(0),
        )
        .unwrap();
        assert_eq!(codes(&collected), [ProblemCode::InvalidRecord]);
        assert_eq!(collected.problems[0].record_id, Some(id));
    }
}
