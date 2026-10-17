//! `cargo xtask runtime record <report.json>`: keeps the native runtime
//! evidence of one clean run in `evidence/runtime/<runId>/` (spec 0003,
//! AC-10).
//!
//! Only `PASS` and `FAIL` records of `xtask/tests/native_runtime.rs` are
//! kept, with their spike results and a sanitized `output.log`; the
//! records themselves are copied unchanged and the summary is counted
//! again. A run that is dirty, a fixture run, a pull request run, a run
//! with problems or an interruption, or a run of another commit is
//! refused, and so is a folder that already exists.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use anyhow::{Context, Result, bail};
use stackly_evidence::{EvidenceRecord, Kind, Language, RunReport, Status, write_json_atomic};

use super::check::EVIDENCE_DIR;
use super::spike::ROOT_PREFIX;

/// The file every kept `evidencePath` folder holds.
const SPIKE_RESULT: &str = "spike-result.json";
/// The most of `output.log` that is kept: its last 1 MiB.
const OUTPUT_LOG_MAX: usize = 1024 * 1024;

pub fn run(repo: &Path, report_path: &Path) -> Result<ExitCode> {
    let head = git_head(repo)?;
    let sanitizer = Sanitizer::from_env(repo)?;
    let dest = keep(report_path, &head, &sanitizer, &repo.join(EVIDENCE_DIR))?;
    println!(
        "runtime record: kept the evidence in {}; commit that folder",
        dest.strip_prefix(repo).unwrap_or(&dest).display()
    );
    Ok(ExitCode::SUCCESS)
}

fn git_head(repo: &Path) -> Result<String> {
    let output = Command::new("git")
        .current_dir(repo)
        .args(["rev-parse", "HEAD"])
        .output()
        .context("cannot run git")?;
    if !output.status.success() {
        bail!(
            "git rev-parse HEAD failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// True for a record of the native runtime test target.
fn is_runtime_record(record: &EvidenceRecord) -> bool {
    record.test.language == Language::Rust
        && record.test.package.as_deref() == Some("xtask")
        && record.test.target.as_deref() == Some("native_runtime")
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
            matches!(record.status, Status::Pass | Status::Fail) && is_runtime_record(record)
        })
        .cloned()
        .collect();
    if kept.is_empty() {
        bail!("no record to keep");
    }
    for record in &kept {
        if !holds_spike_result(run_dir, &record.evidence_path) {
            bail!(
                "record {} evidencePath `{}` must be a folder under artifacts/ that holds {SPIKE_RESULT}",
                record.id,
                record.evidence_path
            );
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
    let log = sanitizer.clean(&String::from_utf8_lossy(&log))?;

    let staging = evidence_dir.join(format!(".{}.partial", report.run_id));
    if staging.exists() {
        fs::remove_dir_all(&staging)
            .with_context(|| format!("cannot delete {}", staging.display()))?;
    }
    for record in &kept {
        let folder = staging.join(record.evidence_path.trim_end_matches('/'));
        fs::create_dir_all(&folder)
            .with_context(|| format!("cannot create {}", folder.display()))?;
        let from = run_dir
            .join(record.evidence_path.trim_end_matches('/'))
            .join(SPIKE_RESULT);
        fs::copy(&from, folder.join(SPIKE_RESULT))
            .with_context(|| format!("cannot copy {}", from.display()))?;
    }
    fs::write(staging.join("output.log"), log).context("cannot write output.log")?;
    report.set_records(kept, Vec::new());
    write_json_atomic(&staging.join("report.json"), &report).context("cannot write report.json")?;
    fs::rename(&staging, &dest)
        .with_context(|| format!("cannot move the evidence to {}", dest.display()))?;
    Ok(dest)
}

/// True when `evidence_path` is a folder under `artifacts/` in `run_dir`
/// that holds a regular `spike-result.json`.
fn holds_spike_result(run_dir: &Path, evidence_path: &str) -> bool {
    evidence_path.starts_with("artifacts/")
        && !evidence_path
            .split('/')
            .any(|part| part == ".." || part == ".")
        && fs::symlink_metadata(
            run_dir
                .join(evidence_path.trim_end_matches('/'))
                .join(SPIKE_RESULT),
        )
        .is_ok_and(|meta| meta.is_file())
}

/// Cleans `output.log` of the workspace, the home folder, and every spike
/// root before it goes into git.
struct Sanitizer {
    /// The workspace root, as given and resolved.
    workspace: Vec<String>,
    /// The home folder.
    home: String,
    /// The OS user name.
    user: String,
}

impl Sanitizer {
    fn from_env(repo: &Path) -> Result<Self> {
        let mut workspace = vec![repo.to_string_lossy().into_owned()];
        if let Ok(real) = repo.canonicalize() {
            workspace.insert(0, real.to_string_lossy().into_owned());
        }
        let home = std::env::var("HOME").context("HOME is not set")?;
        let user = std::env::var("USER")
            .ok()
            .filter(|user| !user.is_empty())
            .or_else(|| {
                Path::new(&home)
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
            })
            .unwrap_or_default();
        Ok(Self {
            workspace,
            home,
            user,
        })
    }

    fn clean(&self, text: &str) -> Result<String> {
        let mut out = replace_spike_roots(text);
        for workspace in &self.workspace {
            if workspace.len() > 1 {
                out = out.replace(workspace.as_str(), "$WORKSPACE");
            }
        }
        if self.home.len() > 1 {
            out = out.replace(self.home.as_str(), "~");
        }
        let user_path = format!("/Users/{}", self.user);
        if (self.home.len() > 1 && out.contains(self.home.as_str()))
            || (!self.user.is_empty() && out.contains(&user_path))
        {
            bail!("output.log contains user path");
        }
        Ok(keep_tail(out, OUTPUT_LOG_MAX))
    }
}

/// Replaces every `<temp>/stackly-runtime-spike-<uuid>` with `$ROOT`.
fn replace_spike_roots(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(ROOT_PREFIX) {
        let before = &rest[..at];
        let start = before
            .rfind(|c: char| c.is_whitespace() || "\"'=(<[,`".contains(c))
            .map_or(0, |i| {
                i + before[i..].chars().next().map_or(1, char::len_utf8)
            });
        let path_start = if before[start..].starts_with('/') {
            start
        } else {
            at
        };
        out.push_str(&before[..path_start]);
        let after = &rest[at + ROOT_PREFIX.len()..];
        let id_len = after
            .char_indices()
            .take_while(|(_, c)| c.is_ascii_hexdigit() || *c == '-')
            .map(|(i, c)| i + c.len_utf8())
            .last()
            .unwrap_or(0);
        out.push_str("$ROOT");
        rest = &after[id_len..];
    }
    out.push_str(rest);
    out
}

/// The last `max` bytes, starting at a line when the text is cut.
fn keep_tail(text: String, max: usize) -> String {
    if text.len() <= max {
        return text;
    }
    let mut start = text.len() - max;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    let cut = &text[start..];
    cut.find('\n')
        .map_or_else(|| cut.to_owned(), |i| cut[i + 1..].to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    const RUN_ID: &str = "019a0000-0000-7000-8000-000000000001";
    const PASS_ID: &str = "019a0000-0000-7000-8000-000000000002";
    const FAIL_ID: &str = "019a0000-0000-7000-8000-000000000003";
    const BLOCKED_ID: &str = "019a0000-0000-7000-8000-000000000004";
    const HEAD: &str = "0123456789abcdef0123456789abcdef01234567";

    fn context() -> Value {
        json!({
            "sourceCommit": HEAD,
            "sourceDirty": false,
            "runId": RUN_ID,
            "runKind": "native",
            "host": { "os": "macos", "arch": "aarch64", "osVersion": "27.0" },
            "toolchain": { "rustc": "1.99.1" }
        })
    }

    fn record(id: &str, status: &str) -> Value {
        let mut record = json!({
            "schema": "stackly.evidence/v1",
            "id": id,
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
            "expected": "nats 2.15 runs from its managed artifact",
            "actual": "nats 2.15.1 ready in 412 ms, stopped in 95 ms",
            "subjects": [{ "provider": "nats", "version": "2.15.1", "digest": "a".repeat(64), "source": "managed" }],
            "startedAt": "2026-10-17T08:00:00Z",
            "finishedAt": "2026-10-17T08:00:03Z",
            "evidencePath": format!("artifacts/{id}/"),
            "context": context()
        });
        if status == "BLOCKED" {
            record["actual"] = Value::Null;
            record["reason"] = json!("no gui login session");
            record["owner"] = json!("@ojiepermana");
        }
        if status == "FAIL" {
            record["actual"] = json!("test panicked; see output.log");
        }
        record
    }

    fn report(records: Vec<Value>) -> Value {
        let typed: Vec<EvidenceRecord> = records
            .iter()
            .map(|r| serde_json::from_value(r.clone()).unwrap())
            .collect();
        json!({
            "schema": "stackly.evidence-report/v1",
            "runId": RUN_ID,
            "kind": "native",
            "wrapperVersion": "0.1.0",
            "context": context(),
            "startedAt": "2026-10-17T08:00:00Z",
            "finishedAt": "2026-10-17T08:01:00Z",
            "commandExitCode": 101,
            "interrupted": false,
            "summary": stackly_evidence::Summary::of(&typed),
            "problems": [],
            "records": records
        })
    }

    struct Run {
        dir: tempfile::TempDir,
    }

    impl Run {
        /// A wrapper run folder with spike results for every record that
        /// has an artifact folder, and an output log.
        fn new(report: &Value, log: &str) -> Self {
            let dir = tempfile::tempdir().unwrap();
            let run = dir.path().join("target/evidence").join(RUN_ID);
            for record in report["records"].as_array().unwrap() {
                let folder = run.join(record["evidencePath"].as_str().unwrap());
                fs::create_dir_all(&folder).unwrap();
                fs::write(folder.join(SPIKE_RESULT), "{\"status\":\"PASS\"}\n").unwrap();
            }
            fs::write(
                run.join("report.json"),
                serde_json::to_vec_pretty(report).unwrap(),
            )
            .unwrap();
            fs::write(run.join("output.log"), log).unwrap();
            fs::create_dir_all(dir.path().join("evidence/runtime")).unwrap();
            Self { dir }
        }

        fn report_path(&self) -> PathBuf {
            self.dir
                .path()
                .join("target/evidence")
                .join(RUN_ID)
                .join("report.json")
        }

        fn keep(&self) -> Result<PathBuf> {
            keep(
                &self.report_path(),
                HEAD,
                &self.sanitizer(),
                &self.dir.path().join("evidence/runtime"),
            )
        }

        fn sanitizer(&self) -> Sanitizer {
            Sanitizer {
                workspace: vec![self.dir.path().to_string_lossy().into_owned()],
                home: "/Users/dev".to_owned(),
                user: "dev".to_owned(),
            }
        }
    }

    fn refusal(report: Value) -> String {
        Run::new(&report, "ok\n").keep().unwrap_err().to_string()
    }

    #[test]
    fn pass_and_fail_are_kept_unchanged_and_blocked_is_dropped() {
        let records = vec![
            record(PASS_ID, "PASS"),
            record(FAIL_ID, "FAIL"),
            record(BLOCKED_ID, "BLOCKED"),
        ];
        let run = Run::new(&report(records.clone()), "Compiling\n");
        let dest = run.keep().unwrap();
        assert_eq!(dest, run.dir.path().join("evidence/runtime").join(RUN_ID));
        let kept: RunReport =
            serde_json::from_slice(&fs::read(dest.join("report.json")).unwrap()).unwrap();
        assert_eq!(kept.records.len(), 2);
        assert_eq!(kept.summary.total, 2);
        let original: Vec<EvidenceRecord> = records[..2]
            .iter()
            .map(|r| serde_json::from_value(r.clone()).unwrap())
            .collect();
        assert_eq!(
            serde_json::to_string_pretty(&kept.records).unwrap(),
            serde_json::to_string_pretty(&original).unwrap()
        );
        assert!(
            dest.join("artifacts")
                .join(PASS_ID)
                .join(SPIKE_RESULT)
                .is_file()
        );
        assert!(!dest.join("artifacts").join(BLOCKED_ID).exists());
        assert_eq!(
            fs::read_to_string(dest.join("output.log")).unwrap(),
            "Compiling\n"
        );
        assert_eq!(
            run.keep().unwrap_err().to_string(),
            format!(
                "{} already exists; a new run means a new folder",
                dest.display()
            )
        );
    }

    #[test]
    fn every_unfit_run_is_refused() {
        let base = || report(vec![record(PASS_ID, "PASS")]);
        let mut dirty = base();
        dirty["context"]["sourceDirty"] = json!(true);
        assert_eq!(
            refusal(dirty),
            "the run is dirty: commit every change first, then run again"
        );
        let mut fixture = base();
        fixture["kind"] = json!("fixture");
        assert_eq!(
            refusal(fixture),
            "the run is a fixture run, not native evidence"
        );
        let mut problems = base();
        problems["problems"] =
            json!([{ "code": "left_not_run", "message": "a record is still NOT_RUN" }]);
        assert_eq!(
            refusal(problems),
            "the report has 1 problem(s); fix them and run again"
        );
        let mut interrupted = base();
        interrupted["interrupted"] = json!(true);
        assert_eq!(refusal(interrupted), "the run was interrupted");
        let mut other = base();
        other["context"]["sourceCommit"] = json!("f".repeat(40));
        assert_eq!(
            refusal(other),
            format!("the run is from commit {}, not HEAD {HEAD}", "f".repeat(40))
        );
        assert_eq!(
            refusal(report(vec![record(BLOCKED_ID, "BLOCKED")])),
            "no record to keep"
        );
    }

    #[test]
    fn a_record_without_its_spike_result_is_refused() {
        let run = Run::new(&report(vec![record(PASS_ID, "PASS")]), "ok\n");
        let folder = run
            .report_path()
            .parent()
            .unwrap()
            .join("artifacts")
            .join(PASS_ID);
        fs::remove_file(folder.join(SPIKE_RESULT)).unwrap();
        assert_eq!(
            run.keep().unwrap_err().to_string(),
            format!(
                "record {PASS_ID} evidencePath `artifacts/{PASS_ID}/` must be a folder under artifacts/ that holds spike-result.json"
            )
        );
    }

    #[test]
    fn output_log_loses_workspace_home_and_roots_or_is_refused() {
        let run = Run::new(&report(vec![record(PASS_ID, "PASS")]), "");
        let workspace = run.dir.path().to_string_lossy().into_owned();
        let log = format!(
            "Compiling xtask ({workspace}/xtask)\nrustc at /Users/dev/.rustup\nspike: /var/folders/x/T/stackly-runtime-spike-019a0000-0000-7000-8000-00000000000a/Stackly Spike Ü/run\n"
        );
        assert_eq!(
            run.sanitizer().clean(&log).unwrap(),
            "Compiling xtask ($WORKSPACE/xtask)\nrustc at ~/.rustup\nspike: $ROOT/Stackly Spike Ü/run\n"
        );
        let leaking = Sanitizer {
            home: "/home/other".to_owned(),
            ..run.sanitizer()
        };
        assert_eq!(
            leaking.clean("at /Users/dev/x").unwrap_err().to_string(),
            "output.log contains user path"
        );
    }

    #[test]
    fn a_long_log_keeps_its_last_mebibyte_from_a_line_start() {
        let text = format!("{}\nlast line\n", "x".repeat(2 * OUTPUT_LOG_MAX));
        let kept = keep_tail(text, OUTPUT_LOG_MAX);
        assert_eq!(kept, "last line\n");
        assert_eq!(keep_tail("short".to_owned(), 10), "short");
    }
}
