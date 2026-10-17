//! `cargo xtask runtime check`: rules 1 to 8 of spec 0003 (*Key
//! invariants*) on `runtimes/inventory.toml` and `evidence/runtime/`.
//! Read only; runs on any OS and in the `linux` CI job on every PR.

use std::fs;
use std::io;
use std::path::Path;
use std::process::ExitCode;

use anyhow::{Context, Result};
use stackly_evidence::{Kind, REPORT_SCHEMA, RunReport, Status, Summary};

use super::inventory::{Inventory, Violation};

/// Run folders of native runtime evidence, relative to the repository root.
pub const EVIDENCE_DIR: &str = "evidence/runtime";
/// The file every `evidencePath` folder must hold.
const SPIKE_RESULT: &str = "spike-result.json";

pub fn run(root: &Path) -> Result<ExitCode> {
    let inventory = Inventory::load(root)?;
    let mut violations = inventory.violations(root);
    let evidence = read_evidence(&root.join(EVIDENCE_DIR))?;
    let runs = evidence.runs;
    violations.extend(evidence.violations);
    violations.extend(latest_record_violations(&inventory, &evidence.reports));
    if violations.is_empty() {
        println!(
            "runtime check: {} artifact(s) and {runs} evidence run(s) follow the rules",
            inventory.artifacts.len()
        );
        return Ok(ExitCode::SUCCESS);
    }
    for violation in &violations {
        println!("{violation}");
    }
    eprintln!(
        "runtime check: {} rule violation(s), see spec 0003 (Key invariants)",
        violations.len()
    );
    Ok(ExitCode::FAILURE)
}

/// What `evidence/runtime/` holds.
struct Evidence {
    /// The number of run folders.
    runs: usize,
    /// Rule 7 violations.
    violations: Vec<Violation>,
    /// Every report that could be decoded, for rule 8.
    reports: Vec<RunReport>,
}

/// Rule 7 on every run folder in `dir`: the number of run folders, and
/// the violations. A missing `dir` has no runs.
#[cfg(test)]
fn evidence_violations(dir: &Path) -> Result<(usize, Vec<Violation>)> {
    let evidence = read_evidence(dir)?;
    Ok((evidence.runs, evidence.violations))
}

/// Reads every run folder in `dir` and checks rule 7 on it.
fn read_evidence(dir: &Path) -> Result<Evidence> {
    let mut evidence = Evidence {
        runs: 0,
        violations: Vec::new(),
        reports: Vec::new(),
    };
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(evidence),
        Err(err) => return Err(err).with_context(|| format!("cannot read {}", dir.display())),
    };
    let mut names = Vec::new();
    for entry in entries {
        let entry = entry.with_context(|| format!("cannot read {}", dir.display()))?;
        let file_type = entry
            .file_type()
            .with_context(|| format!("cannot read {}", entry.path().display()))?;
        // Plain files (a README, Finder's .DS_Store) are not runs.
        if file_type.is_dir() || file_type.is_symlink() {
            names.push((
                entry.file_name().to_string_lossy().into_owned(),
                file_type.is_symlink(),
            ));
        }
    }
    names.sort();
    for (name, is_symlink) in &names {
        let mut v = RunRules {
            subject: format!("{EVIDENCE_DIR}/{name}"),
            out: &mut evidence.violations,
        };
        if *is_symlink {
            v.push("a run must be a folder, not a symbolic link".to_owned());
            continue;
        }
        if let Some(report) = check_run(&dir.join(name), name, &mut v)? {
            evidence.reports.push(report);
        }
    }
    evidence.runs = names.len();
    Ok(evidence)
}

/// Rule 8: every `available` artifact has a record whose subject carries
/// its digest, and the latest such record (by `finishedAt`) is a clean
/// native `PASS` from macOS 27 on arm64, outside a pull request, with the
/// inventory version.
fn latest_record_violations(inventory: &Inventory, reports: &[RunReport]) -> Vec<Violation> {
    let mut out = Vec::new();
    for (index, artifact) in inventory.artifacts.iter().enumerate() {
        if artifact.status.as_deref() != Some("available") {
            continue;
        }
        let Some(sha256) = artifact.sha256.as_deref().filter(|sha| !sha.is_empty()) else {
            continue;
        };
        let mut push = |message: String| {
            out.push(Violation {
                subject: artifact.label(index),
                rule: 8,
                message,
            });
        };
        let latest = reports
            .iter()
            .flat_map(|report| report.records.iter().map(move |record| (report, record)))
            .filter(|(_, record)| {
                record
                    .subjects
                    .iter()
                    .any(|subject| subject.digest.as_deref() == Some(sha256))
            })
            .max_by_key(|(_, record)| record.finished_at);
        let Some((report, record)) = latest else {
            push(format!(
                "no native record in {EVIDENCE_DIR}/ for sha256 {sha256}; run the native test and `cargo xtask runtime record`"
            ));
            continue;
        };
        let id = record.id;
        let context = record.context.as_ref().unwrap_or(&report.context);
        if record.status != Status::Pass {
            push(format!(
                "the latest record {id} for this sha256 is {}, not PASS",
                record.status.as_str()
            ));
        }
        if record.kind != Kind::Native {
            push(format!("the latest record {id} is not native"));
        }
        if context.source_dirty {
            push(format!("the latest record {id} comes from a dirty tree"));
        }
        if context.ci.as_ref().and_then(|ci| ci.event.as_deref()) == Some("pull_request") {
            push(format!(
                "the latest record {id} comes from a pull_request run"
            ));
        }
        let os_version = context.host.os_version.as_deref().unwrap_or("unknown");
        if os_version.split('.').next() != Some("27") {
            push(format!(
                "the latest record {id} ran on macOS {os_version}, not 27"
            ));
        }
        if !matches!(context.host.arch.as_str(), "arm64" | "aarch64") {
            push(format!(
                "the latest record {id} ran on {}, not arm64",
                context.host.arch
            ));
        }
        let version = artifact.version.as_deref().unwrap_or_default();
        for subject in &record.subjects {
            if subject.digest.as_deref() == Some(sha256) && subject.version != version {
                push(format!(
                    "the latest record {id} reports version {}, the inventory says {version}",
                    subject.version
                ));
            }
        }
    }
    out
}

/// Collects the rule 7 violations of one run folder.
struct RunRules<'a> {
    subject: String,
    out: &'a mut Vec<Violation>,
}

impl RunRules<'_> {
    fn push(&mut self, message: String) {
        self.out.push(Violation {
            subject: self.subject.clone(),
            rule: 7,
            message,
        });
    }
}

/// Rule 7 on one run folder named `name`; the report when it decodes.
fn check_run(run_dir: &Path, name: &str, v: &mut RunRules<'_>) -> Result<Option<RunReport>> {
    let report_path = run_dir.join("report.json");
    let bytes = match fs::read(&report_path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            v.push("report.json is missing".to_owned());
            return Ok(None);
        }
        Err(err) if err.kind() == io::ErrorKind::IsADirectory => {
            v.push("report.json must be a file".to_owned());
            return Ok(None);
        }
        Err(err) => {
            return Err(err).with_context(|| format!("cannot read {}", report_path.display()));
        }
    };
    let report: RunReport = match serde_json::from_slice(&bytes) {
        Ok(report) => report,
        Err(err) => {
            v.push(format!(
                "report.json is not a {REPORT_SCHEMA} report: {err}"
            ));
            return Ok(None);
        }
    };
    if report.schema != REPORT_SCHEMA {
        v.push(format!(
            "report.json schema must be `{REPORT_SCHEMA}`, found `{}`",
            report.schema
        ));
    }
    if report.run_id.to_string() != name {
        v.push(format!(
            "the folder name must be the runId {}",
            report.run_id
        ));
    }
    if report.summary != Summary::of(&report.records) {
        v.push("summary does not match the records".to_owned());
    }
    let canonical_run = run_dir
        .canonicalize()
        .with_context(|| format!("cannot resolve {}", run_dir.display()))?;
    for record in &report.records {
        let id = record.id;
        if let Err(err) = record.validate() {
            for rule in err.violations {
                v.push(format!("record {id}: {rule}"));
            }
        }
        if !matches!(record.status, Status::Pass | Status::Fail) {
            v.push(format!(
                "record {id} is {}; only PASS and FAIL records are kept",
                record.status.as_str()
            ));
        }
        if !holds_spike_result(&canonical_run, &record.evidence_path) {
            v.push(format!(
                "record {id} evidencePath `{}` must be a folder in this run that holds {SPIKE_RESULT}",
                record.evidence_path
            ));
        }
    }
    Ok(Some(report))
}

/// True when `evidence_path` names a folder inside `run_dir` (after
/// resolving links) that holds a regular `spike-result.json`.
fn holds_spike_result(run_dir: &Path, evidence_path: &str) -> bool {
    if evidence_path.is_empty()
        || evidence_path.starts_with('/')
        || evidence_path.split(['/', '\\']).any(|part| part == "..")
    {
        return false;
    }
    let Ok(folder) = run_dir.join(evidence_path).canonicalize() else {
        return false;
    };
    folder != run_dir
        && folder.starts_with(run_dir)
        && folder.is_dir()
        && fs::symlink_metadata(folder.join(SPIKE_RESULT)).is_ok_and(|meta| meta.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    const RUN_ID: &str = "019a0000-0000-7000-8000-000000000001";
    const RECORD_ID: &str = "019a0000-0000-7000-8000-000000000002";

    /// A committed native PASS record as `runtime record` would keep it.
    fn record(status: &str) -> Value {
        json!({
            "schema": "stackly.evidence/v1",
            "id": RECORD_ID,
            "covers": ["AC-078", "REQ-005"],
            "kind": "native",
            "method": "automated",
            "test": {
                "language": "rust",
                "package": "xtask",
                "target": "native_runtime",
                "name": "nats_2_15_runs_from_managed_artifact",
                "file": "xtask/tests/native_runtime.rs",
                "line": 40
            },
            "status": status,
            "expected": "nats 2.15 runs from its managed artifact under a LaunchAgent",
            "actual": "nats 2.15.1 ready in 412 ms, stopped in 95 ms",
            "subjects": [{
                "provider": "nats",
                "version": "2.15.1",
                "digest": "a".repeat(64),
                "source": "managed"
            }],
            "startedAt": "2026-10-17T08:00:00Z",
            "finishedAt": "2026-10-17T08:00:03Z",
            "evidencePath": format!("artifacts/{RECORD_ID}/")
        })
    }

    fn report(records: Vec<Value>) -> Value {
        let mut by_status =
            json!({ "NOT_RUN": 0, "PASS": 0, "FAIL": 0, "BLOCKED": 0, "DEFERRED": 0 });
        for record in &records {
            let status = record["status"].as_str().unwrap();
            by_status[status] = json!(by_status[status].as_u64().unwrap() + 1);
        }
        json!({
            "schema": "stackly.evidence-report/v1",
            "runId": RUN_ID,
            "kind": "native",
            "wrapperVersion": "0.1.0",
            "context": {
                "sourceCommit": "0123456789abcdef0123456789abcdef01234567",
                "sourceDirty": false,
                "runId": RUN_ID,
                "runKind": "native",
                "host": { "os": "macos", "arch": "aarch64", "osVersion": "27.0" },
                "toolchain": { "rustc": "1.99.1" }
            },
            "startedAt": "2026-10-17T08:00:00Z",
            "finishedAt": "2026-10-17T08:01:00Z",
            "commandExitCode": 0,
            "interrupted": false,
            "summary": { "total": records.len(), "byStatus": by_status, "byKind": { "fixture": 0, "native": records.len() } },
            "problems": [],
            "records": records
        })
    }

    /// An `evidence/runtime/` folder with one run folder `name` holding
    /// `report` and the spike result of [`RECORD_ID`].
    fn evidence_with(name: &str, report: &Value) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let run = dir.path().join(name);
        fs::create_dir_all(run.join("artifacts").join(RECORD_ID)).unwrap();
        fs::write(
            run.join("artifacts").join(RECORD_ID).join(SPIKE_RESULT),
            "{}",
        )
        .unwrap();
        fs::write(
            run.join("report.json"),
            serde_json::to_string_pretty(report).unwrap(),
        )
        .unwrap();
        dir
    }

    fn lines(dir: &Path) -> Vec<String> {
        evidence_violations(dir)
            .unwrap()
            .1
            .iter()
            .map(ToString::to_string)
            .collect()
    }

    #[test]
    fn a_missing_evidence_folder_has_no_runs() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(evidence_violations(&dir.path().join("none")).unwrap().0, 0);
    }

    #[test]
    fn a_kept_pass_and_fail_run_follows_rule_7() {
        let dir = evidence_with(RUN_ID, &report(vec![record("PASS")]));
        assert_eq!(lines(dir.path()), Vec::<String>::new());
        assert_eq!(evidence_violations(dir.path()).unwrap().0, 1);
        let mut fail = record("FAIL");
        fail["actual"] = json!("start: nats-server exited 1");
        let dir = evidence_with(RUN_ID, &report(vec![fail]));
        assert_eq!(lines(dir.path()), Vec::<String>::new());
    }

    #[test]
    fn the_folder_name_must_be_the_run_id() {
        let dir = evidence_with("2026-10-17-nats", &report(vec![record("PASS")]));
        assert_eq!(
            lines(dir.path()),
            [format!(
                "evidence/runtime/2026-10-17-nats: the folder name must be the runId {RUN_ID} (rule 7)"
            )]
        );
    }

    #[test]
    fn a_wrong_summary_is_refused() {
        let mut wrong = report(vec![record("PASS")]);
        wrong["summary"]["byStatus"]["PASS"] = json!(2);
        let dir = evidence_with(RUN_ID, &wrong);
        assert_eq!(
            lines(dir.path()),
            [format!(
                "evidence/runtime/{RUN_ID}: summary does not match the records (rule 7)"
            )]
        );
    }

    #[test]
    fn blocked_and_not_run_records_are_never_kept() {
        let mut blocked = record("BLOCKED");
        blocked["reason"] = json!("no gui login session");
        blocked["owner"] = json!("@ojiepermana");
        blocked["actual"] = Value::Null;
        let mut not_run = record("NOT_RUN");
        not_run["finishedAt"] = Value::Null;
        for record in [blocked, not_run] {
            let status = record["status"].as_str().unwrap().to_owned();
            let dir = evidence_with(RUN_ID, &report(vec![record]));
            assert_eq!(
                lines(dir.path()),
                [format!(
                    "evidence/runtime/{RUN_ID}: record {RECORD_ID} is {status}; only PASS and FAIL records are kept (rule 7)"
                )]
            );
        }
    }

    #[test]
    fn an_invalid_record_reports_each_broken_rule() {
        let mut pass = record("PASS");
        pass["subjects"] = json!([]);
        let dir = evidence_with(RUN_ID, &report(vec![pass]));
        assert_eq!(
            lines(dir.path()),
            [format!(
                "evidence/runtime/{RUN_ID}: record {RECORD_ID}: a native PASS needs at least one subject (rule 7)"
            )]
        );
    }

    #[test]
    fn evidence_path_must_hold_a_spike_result_inside_the_run() {
        let dir = evidence_with(RUN_ID, &report(vec![record("PASS")]));
        fs::remove_file(
            dir.path()
                .join(RUN_ID)
                .join("artifacts")
                .join(RECORD_ID)
                .join(SPIKE_RESULT),
        )
        .unwrap();
        let expected = format!(
            "evidence/runtime/{RUN_ID}: record {RECORD_ID} evidencePath `artifacts/{RECORD_ID}/` must be a folder in this run that holds spike-result.json (rule 7)"
        );
        assert_eq!(lines(dir.path()), [expected]);

        let mut outside = record("PASS");
        outside["evidencePath"] = json!("output.log");
        let dir = evidence_with(RUN_ID, &report(vec![outside]));
        assert_eq!(lines(dir.path()).len(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn evidence_path_through_a_link_out_of_the_run_is_refused() {
        let elsewhere = tempfile::tempdir().unwrap();
        fs::write(elsewhere.path().join(SPIKE_RESULT), "{}").unwrap();
        let mut linked = record("PASS");
        linked["evidencePath"] = json!("artifacts/linked/");
        let dir = evidence_with(RUN_ID, &report(vec![linked]));
        std::os::unix::fs::symlink(
            elsewhere.path(),
            dir.path().join(RUN_ID).join("artifacts/linked"),
        )
        .unwrap();
        assert_eq!(lines(dir.path()).len(), 1);
    }

    #[test]
    fn a_run_without_a_readable_report_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join(RUN_ID)).unwrap();
        assert_eq!(
            lines(dir.path()),
            [format!(
                "evidence/runtime/{RUN_ID}: report.json is missing (rule 7)"
            )]
        );
        fs::write(dir.path().join(RUN_ID).join("report.json"), "{}").unwrap();
        let found = lines(dir.path());
        assert_eq!(found.len(), 1);
        assert!(
            found[0].contains("is not a stackly.evidence-report/v1 report"),
            "{found:?}"
        );

        let mut other = report(vec![record("PASS")]);
        other["schema"] = json!("stackly.evidence-report/v2");
        let dir = evidence_with(RUN_ID, &other);
        assert_eq!(
            lines(dir.path()),
            [format!(
                "evidence/runtime/{RUN_ID}: report.json schema must be `stackly.evidence-report/v1`, found `stackly.evidence-report/v2` (rule 7)"
            )]
        );
    }

    #[test]
    fn an_unreadable_report_is_one_line_and_the_other_runs_still_report() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join(RUN_ID)).unwrap();
        fs::write(
            dir.path().join(RUN_ID).join("report.json"),
            [0xff, 0xfe, b'{', 0],
        )
        .unwrap();
        let folder = "019a0000-0000-7000-8000-000000000009";
        fs::create_dir_all(dir.path().join(folder).join("report.json")).unwrap();
        let found = lines(dir.path());
        assert_eq!(found.len(), 2, "{found:#?}");
        assert!(found[0].starts_with(&format!(
            "evidence/runtime/{RUN_ID}: report.json is not a stackly.evidence-report/v1 report"
        )));
        assert_eq!(
            found[1],
            format!("evidence/runtime/{folder}: report.json must be a file (rule 7)")
        );
    }

    #[test]
    fn plain_files_beside_the_runs_are_ignored() {
        let dir = evidence_with(RUN_ID, &report(vec![record("PASS")]));
        fs::write(dir.path().join(".DS_Store"), "").unwrap();
        assert_eq!(lines(dir.path()), Vec::<String>::new());
    }
}
