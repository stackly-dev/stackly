//! `cargo xtask runtime compat-record <report.json>`: keeps the BR-025
//! evidence of one clean provider run in `evidence/compatibility/<runId>/`
//! (spec 0018, AC-15).
//!
//! The refusals of `runtime record` apply: a run that is dirty, a fixture
//! run, a run with problems or an interruption, or a run of another commit
//! is refused, and so is a folder that already exists. Only `PASS` and
//! `FAIL` records that cover BR-025 are kept; the rest of a full native run
//! of a provider is skipped. A kept record must be native, name a managed
//! subject with a digest, and have a valid `compatibility.json`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use stackly_evidence::{EvidenceRecord, Kind, RunReport, Status, write_json_atomic};

use super::compatibility::{COMPAT_COVERS, COMPAT_EVIDENCE_DIR, COMPAT_FILE, record_problems};
use super::record::{Sanitizer, git_head};

/// The most of `output.log` that is kept: its last 1 MiB.
const OUTPUT_LOG_MAX: usize = 1024 * 1024;

pub fn run(repo: &Path, report_path: &Path) -> Result<ExitCode> {
    let head = git_head(repo)?;
    let sanitizer = Sanitizer::from_env(repo)?;
    let dest = keep(
        report_path,
        &head,
        &sanitizer,
        &repo.join(COMPAT_EVIDENCE_DIR),
    )?;
    println!(
        "runtime compat-record: kept the evidence in {}; commit that folder, then run `cargo xtask codegen`",
        dest.strip_prefix(repo).unwrap_or(&dest).display()
    );
    Ok(ExitCode::SUCCESS)
}

/// Checks the report and writes `<evidence_dir>/<runId>/`. Returns that
/// folder.
fn keep(
    report_path: &Path,
    head: &str,
    sanitizer: &Sanitizer,
    evidence_dir: &Path,
) -> Result<PathBuf> {
    let bytes =
        fs::read(report_path).with_context(|| format!("cannot read {}", report_path.display()))?;
    let mut report: RunReport = serde_json::from_slice(&bytes)
        .with_context(|| format!("{} is not an evidence report", report_path.display()))?;
    let run_dir = report_path.parent().context("the report has no folder")?;
    if report.kind == Kind::Fixture {
        bail!("the run is a fixture run, not native evidence");
    }
    if report.context.source_dirty {
        bail!("the run is dirty: commit every change first, then run again");
    }
    if !report.problems.is_empty() {
        bail!(
            "the report has {} problem(s); fix them and run again",
            report.problems.len()
        );
    }
    if report.interrupted {
        bail!("the run was interrupted");
    }
    if report.context.source_commit != head {
        bail!(
            "the run is from commit {}, not HEAD {head}",
            report.context.source_commit
        );
    }
    let kept: Vec<EvidenceRecord> = report
        .records
        .iter()
        .filter(|record| {
            matches!(record.status, Status::Pass | Status::Fail)
                && record.covers.iter().any(|id| id == COMPAT_COVERS)
        })
        .cloned()
        .collect();
    if kept.is_empty() {
        bail!("no record to keep: no PASS or FAIL record covers {COMPAT_COVERS}");
    }
    for record in &kept {
        if let Some(problem) = record_problems(run_dir, record).first() {
            bail!("record {} {problem}", record.id);
        }
    }
    let dest = evidence_dir.join(report.run_id.to_string());
    if dest.exists() {
        bail!(
            "{} already exists; a new run means a new folder",
            dest.display()
        );
    }
    let log_path = run_dir.join("output.log");
    let log = fs::read(&log_path).with_context(|| format!("cannot read {}", log_path.display()))?;
    let tail = &log[log.len().saturating_sub(OUTPUT_LOG_MAX)..];
    let log = sanitizer.clean(&String::from_utf8_lossy(tail))?;

    let staging = evidence_dir.join(format!(".{}.partial", report.run_id));
    if staging.exists() {
        fs::remove_dir_all(&staging)
            .with_context(|| format!("cannot delete {}", staging.display()))?;
    }
    for record in &kept {
        let relative = record.evidence_path.trim_end_matches('/');
        let folder = staging.join(relative);
        fs::create_dir_all(&folder)
            .with_context(|| format!("cannot create {}", folder.display()))?;
        let from = run_dir.join(relative).join(COMPAT_FILE);
        fs::copy(&from, folder.join(COMPAT_FILE))
            .with_context(|| format!("cannot copy {}", from.display()))?;
    }
    fs::write(staging.join("output.log"), log).context("cannot write output.log")?;
    report.set_records(kept, Vec::new());
    write_json_atomic(&staging.join("report.json"), &report).context("cannot write report.json")?;
    fs::rename(&staging, &dest)
        .with_context(|| format!("cannot move the evidence to {}", dest.display()))?;
    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::super::compatibility::tests::{compat, record, report, uuid, write_run};
    use super::*;
    use serde_json::{Value, json};

    const HEAD: &str = "0123456789abcdef0123456789abcdef01234567";

    /// A wrapper run in `target/evidence/<run>` of a test repo.
    fn setup(records: &[Value]) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        write_run(
            &dir.path().join("target"),
            "evidence",
            1,
            records,
            COMPAT_FILE,
            &compat(&["start"]),
        );
        fs::write(
            dir.path()
                .join("target/evidence")
                .join(uuid(1))
                .join("output.log"),
            "Compiling\n",
        )
        .unwrap();
        let path = dir
            .path()
            .join("target/evidence")
            .join(uuid(1))
            .join("report.json");
        (dir, path)
    }

    fn sanitizer(dir: &Path) -> Sanitizer {
        Sanitizer {
            workspace: vec![dir.to_string_lossy().into_owned()],
            home: "/Users/dev".to_owned(),
            user: "dev".to_owned(),
        }
    }

    fn keep_in(dir: &Path, path: &Path) -> Result<PathBuf> {
        keep(
            path,
            HEAD,
            &sanitizer(dir),
            &dir.join("evidence/compatibility"),
        )
    }

    #[test]
    fn only_pass_and_fail_records_of_br_025_are_kept_with_their_file_and_a_log() {
        let records = [
            record(2, "PASS", &["BR-025", "AC-1"], "2026-10-18T11:00:00Z"),
            record(3, "FAIL", &["BR-025"], "2026-10-18T11:01:00Z"),
            record(4, "PASS", &["AC-9"], "2026-10-18T11:02:00Z"),
        ];
        let (dir, path) = setup(&records);
        fs::create_dir_all(dir.path().join("evidence/compatibility")).unwrap();

        let dest = keep_in(dir.path(), &path).unwrap();

        assert_eq!(
            dest,
            dir.path().join("evidence/compatibility").join(uuid(1))
        );
        let kept: RunReport =
            serde_json::from_slice(&fs::read(dest.join("report.json")).unwrap()).unwrap();
        assert_eq!(kept.records.len(), 2);
        assert_eq!(kept.summary.total, 2);
        assert!(
            dest.join("artifacts")
                .join(uuid(2))
                .join(COMPAT_FILE)
                .is_file()
        );
        assert!(
            dest.join("artifacts")
                .join(uuid(3))
                .join(COMPAT_FILE)
                .is_file()
        );
        assert!(!dest.join("artifacts").join(uuid(4)).exists());
        assert_eq!(
            fs::read_to_string(dest.join("output.log")).unwrap(),
            "Compiling\n"
        );
    }

    fn refusal(report: Value, records: &[Value]) -> String {
        let (dir, path) = setup(records);
        fs::write(&path, serde_json::to_vec(&report).unwrap()).unwrap();
        keep_in(dir.path(), &path).unwrap_err().to_string()
    }

    #[test]
    fn a_report_without_a_br_025_record_has_no_record_to_keep() {
        let records = [record(2, "PASS", &["AC-9"], "2026-10-18T11:00:00Z")];
        let message = refusal(report(1, &records), &records);
        assert!(message.starts_with("no record to keep"), "{message}");
    }

    #[test]
    fn a_dirty_fixture_interrupted_or_other_commit_run_is_refused() {
        let records = [record(2, "PASS", &["BR-025"], "2026-10-18T11:00:00Z")];
        let edit = |field: &str, value: Value| {
            let mut report = report(1, &records);
            match field {
                "dirty" => report["context"]["sourceDirty"] = value,
                "commit" => report["context"]["sourceCommit"] = value,
                other => report[other] = value,
            }
            report
        };
        assert_eq!(
            refusal(edit("kind", json!("fixture")), &records),
            "the run is a fixture run, not native evidence"
        );
        assert_eq!(
            refusal(edit("dirty", json!(true)), &records),
            "the run is dirty: commit every change first, then run again"
        );
        assert_eq!(
            refusal(edit("interrupted", json!(true)), &records),
            "the run was interrupted"
        );
        assert_eq!(
            refusal(edit("commit", json!("f".repeat(40))), &records),
            format!("the run is from commit {}, not HEAD {HEAD}", "f".repeat(40))
        );
    }

    #[test]
    fn a_br_025_record_that_is_not_native_or_has_no_file_is_refused_and_a_kept_run_is_not_redone() {
        let mut fixture = record(2, "PASS", &["BR-025"], "2026-10-18T11:00:00Z");
        fixture["subjects"][0]["digest"] = Value::Null;
        let records = [fixture];
        let message = refusal(report(1, &records), &records);
        assert_eq!(
            message,
            format!("record {} has no managed subject with a digest", uuid(2))
        );

        let records = [record(2, "PASS", &["BR-025"], "2026-10-18T11:00:00Z")];
        let (dir, path) = setup(&records);
        fs::create_dir_all(dir.path().join("evidence/compatibility").join(uuid(1))).unwrap();
        let message = keep_in(dir.path(), &path).unwrap_err().to_string();
        assert!(
            message.ends_with("already exists; a new run means a new folder"),
            "{message}"
        );

        fs::remove_file(
            dir.path()
                .join("target/evidence")
                .join(uuid(1))
                .join("artifacts")
                .join(uuid(2))
                .join(COMPAT_FILE),
        )
        .unwrap();
        let message = keep_in(dir.path(), &path).unwrap_err().to_string();
        assert!(
            message.contains("must be a folder under artifacts/ that holds compatibility.json"),
            "{message}"
        );
    }
}
