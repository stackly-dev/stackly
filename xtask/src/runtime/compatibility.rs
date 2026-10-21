//! Compatibility records (spec 0018, *Compatibility record*; PRD BR-025).
//!
//! The generator turns native evidence into the `compatibility` field of an
//! artifact in `providers/<package>/versions.json`: scope `runtime` from
//! `evidence/runtime/<runId>/` (the `spike-result.json` of each record),
//! scope `provider` from `evidence/compatibility/<runId>/` (the
//! `compatibility.json` of each BR-025 record). `runtime check` rule 12
//! holds every report in `evidence/compatibility/` to the same rules
//! `runtime compat-record` refuses a report with.

use std::cmp::Reverse;
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use stackly_evidence::{EvidenceRecord, Kind, REPORT_SCHEMA, RunReport, Source, Status, Summary};

use super::check::EVIDENCE_DIR as RUNTIME_EVIDENCE_DIR;
use super::inventory::Violation;

/// Run folders of provider compatibility evidence, relative to the
/// repository root.
pub const COMPAT_EVIDENCE_DIR: &str = "evidence/compatibility";
/// The file each kept record of a provider run holds.
pub const COMPAT_FILE: &str = "compatibility.json";
/// The schema of [`COMPAT_FILE`].
pub const COMPAT_SCHEMA: &str = "stackly.compatibility-evidence/v1";
/// The PRD ID a record needs to count as compatibility evidence.
pub const COMPAT_COVERS: &str = "BR-025";
/// Rule 12 of `runtime check`.
const RULE: u8 = 12;
/// The longest `summary` of a record, in characters.
const SUMMARY_MAX: usize = 256;
/// The most records one artifact carries.
const RECORDS_MAX: usize = 32;
/// The most codes in `checks` or `knownLimitations`, and the longest code.
const CODES_MAX: usize = 64;
const SPIKE_RESULT: &str = "spike-result.json";
/// The one limitation every runtime record carries.
const RUNTIME_LIMITATION: &str = "provider_integration_not_covered";

/// The scope of a record. The order is the order records are written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    /// The artifact ran under a LaunchAgent; no provider was involved.
    Runtime,
    /// A provider package ran against the artifact.
    Provider,
}

/// The result of a record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    /// Every check passed.
    Pass,
    /// A check failed.
    Fail,
}

/// The provider package a `provider` record was tested with.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RevisionRef {
    /// The package id, for example `official/postgresql`.
    pub id: String,
    /// The package revision.
    pub revision: String,
    /// The SHA-256 of the manifest, 64 lowercase hex characters.
    pub manifest_sha256: String,
}

/// One compatibility record, in the field order of `versions.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogRecord {
    scope: Scope,
    result: Outcome,
    provider_revision: Option<RevisionRef>,
    os: String,
    os_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    os_build: Option<String>,
    arch: String,
    checks: Vec<String>,
    known_limitations: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    summary: Option<String>,
    tested_at: String,
    evidence: String,
}

/// `compatibility.json`, schema [`COMPAT_SCHEMA`].
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CompatFile {
    schema: String,
    provider_revision: RevisionRef,
    checks: Vec<String>,
    known_limitations: Vec<String>,
}

/// The parts of `spike-result.json` a record reads.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SpikeResult {
    #[serde(default)]
    steps: Vec<Check>,
    #[serde(default)]
    closure_probes: Vec<Check>,
}

#[derive(Debug, Deserialize)]
struct Check {
    name: String,
    #[serde(default)]
    status: Option<String>,
}

/// One kept run folder.
struct Run {
    name: String,
    dir: PathBuf,
    report: RunReport,
}

/// The native evidence of both folders.
pub struct Evidence {
    runtime: Vec<Run>,
    provider: Vec<Run>,
}

impl Evidence {
    /// Reads `evidence/runtime/` and `evidence/compatibility/` under
    /// `root`. A missing folder holds no runs, and a folder without a
    /// `report.json` is skipped (`runtime check` reports it).
    ///
    /// # Errors
    ///
    /// Returns an error when a folder cannot be read or a report does not
    /// decode.
    pub fn load(root: &Path) -> Result<Self> {
        Ok(Self {
            runtime: load_runs(&root.join(RUNTIME_EVIDENCE_DIR))?,
            provider: load_runs(&root.join(COMPAT_EVIDENCE_DIR))?,
        })
    }

    /// The records of the artifact with `sha256`: for each key (scope,
    /// provider revision, macOS major, architecture) the record of the
    /// latest evidence, whatever its status, ordered by scope and then
    /// `testedAt`, newest first.
    ///
    /// # Errors
    ///
    /// Returns an error when a record misses what the generator reads, or
    /// the artifact has more than 32 records.
    pub fn records_for(&self, sha256: &str) -> Result<Vec<CatalogRecord>> {
        type Key = (Scope, Option<RevisionRef>, String, String);
        let mut latest: BTreeMap<Key, (jiff::Timestamp, usize, CatalogRecord)> = BTreeMap::new();
        let mut order = 0;
        let runs = self
            .runtime
            .iter()
            .map(|run| (Scope::Runtime, RUNTIME_EVIDENCE_DIR, run))
            .chain(
                self.provider
                    .iter()
                    .map(|run| (Scope::Provider, COMPAT_EVIDENCE_DIR, run)),
            );
        for (scope, folder, run) in runs {
            for record in &run.report.records {
                let kept = matches!(record.status, Status::Pass | Status::Fail)
                    && record
                        .subjects
                        .iter()
                        .any(|subject| subject.digest.as_deref() == Some(sha256))
                    && (scope == Scope::Runtime
                        || record.covers.iter().any(|id| id == COMPAT_COVERS));
                if !kept {
                    continue;
                }
                let made = build(scope, folder, run, record)
                    .with_context(|| format!("{folder}/{}: record {}", run.name, record.id))?;
                let finished = record.finished_at.unwrap_or(run.report.finished_at);
                let key = (
                    made.scope,
                    made.provider_revision.clone(),
                    major(&made.os_version).to_owned(),
                    made.arch.clone(),
                );
                order += 1;
                let newer = latest
                    .get(&key)
                    .is_none_or(|(seen, seen_order, _)| (finished, order) > (*seen, *seen_order));
                if newer {
                    latest.insert(key, (finished, order, made));
                }
            }
        }
        let mut records: Vec<CatalogRecord> =
            latest.into_values().map(|(_, _, record)| record).collect();
        records.sort_by(|a, b| {
            (a.scope, Reverse(&a.tested_at)).cmp(&(b.scope, Reverse(&b.tested_at)))
        });
        if records.len() > RECORDS_MAX {
            bail!(
                "{sha256}: {} compatibility records; at most {RECORDS_MAX}",
                records.len()
            );
        }
        Ok(records)
    }
}

fn major(os_version: &str) -> &str {
    os_version.split('.').next().unwrap_or(os_version)
}

fn load_runs(dir: &Path) -> Result<Vec<Run>> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err).with_context(|| format!("cannot read {}", dir.display())),
    };
    let mut runs = Vec::new();
    for entry in entries {
        let entry = entry.with_context(|| format!("cannot read {}", dir.display()))?;
        let file_type = entry
            .file_type()
            .with_context(|| format!("cannot read {}", entry.path().display()))?;
        if !file_type.is_dir() {
            continue;
        }
        let path = entry.path();
        let bytes = match fs::read(path.join("report.json")) {
            Ok(bytes) => bytes,
            Err(err)
                if matches!(
                    err.kind(),
                    io::ErrorKind::NotFound | io::ErrorKind::IsADirectory
                ) =>
            {
                continue;
            }
            Err(err) => {
                return Err(err).with_context(|| format!("cannot read {}", path.display()));
            }
        };
        let report: RunReport = serde_json::from_slice(&bytes).with_context(|| {
            format!(
                "{}/report.json is not an evidence report; run `cargo xtask runtime check`",
                path.display()
            )
        })?;
        runs.push(Run {
            name: entry.file_name().to_string_lossy().into_owned(),
            dir: path,
            report,
        });
    }
    runs.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(runs)
}

/// Builds the record of `record` in `run`.
fn build(scope: Scope, folder: &str, run: &Run, record: &EvidenceRecord) -> Result<CatalogRecord> {
    let context = record.context.as_ref().unwrap_or(&run.report.context);
    let host = &context.host;
    let Some(os_version) = host.os_version.clone() else {
        bail!("the host has no osVersion");
    };
    let arch = match host.arch.as_str() {
        "arm64" => "aarch64".to_owned(),
        other => other.to_owned(),
    };
    let finished = record.finished_at.unwrap_or(run.report.finished_at);
    let record_dir = record.evidence_path.trim_end_matches('/');
    let artifact = run.dir.join(record_dir);
    let (provider_revision, checks, known_limitations) = match scope {
        Scope::Runtime => {
            let path = artifact.join(SPIKE_RESULT);
            let result: SpikeResult = read_json(&path)?;
            let mut checks: Vec<String> = result
                .steps
                .iter()
                .chain(&result.closure_probes)
                .filter(|check| check.status.as_deref() == Some("PASS"))
                .map(|check| check_code(&check.name))
                .collect();
            checks.sort();
            checks.dedup();
            (None, checks, vec![RUNTIME_LIMITATION.to_owned()])
        }
        Scope::Provider => {
            let file: CompatFile = read_json(&artifact.join(COMPAT_FILE))?;
            let mut checks = file.checks;
            checks.sort();
            checks.dedup();
            let mut limits = file.known_limitations;
            limits.sort();
            limits.dedup();
            (Some(file.provider_revision), checks, limits)
        }
    };
    Ok(CatalogRecord {
        scope,
        result: if record.status == Status::Pass {
            Outcome::Pass
        } else {
            Outcome::Fail
        },
        provider_revision,
        os: host.os.clone(),
        os_version,
        os_build: host.os_build.clone(),
        arch,
        checks,
        known_limitations,
        summary: record
            .actual
            .as_deref()
            .map(|actual| actual.chars().take(SUMMARY_MAX).collect()),
        tested_at: finished.strftime("%Y-%m-%dT%H:%M:%S%.3fZ").to_string(),
        evidence: format!(
            "{}/{}/{}",
            folder.trim_start_matches("evidence/"),
            run.name,
            record_dir.rsplit('/').next().unwrap_or(record_dir)
        ),
    })
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T> {
    let bytes = fs::read(path).with_context(|| format!("cannot read {}", path.display()))?;
    serde_json::from_slice(&bytes).with_context(|| format!("cannot decode {}", path.display()))
}

/// A step or probe name as a check code: lowercase with `_` for every
/// other character, and `launch` as `launch_agent`.
fn check_code(name: &str) -> String {
    let code: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    if code == "launch" {
        "launch_agent".to_owned()
    } else {
        code
    }
}

/// Why `record` of a provider run cannot be kept as compatibility
/// evidence: not native, no managed subject with a digest, or no valid
/// [`COMPAT_FILE`] in its folder. `run_dir` is the folder of the run.
pub fn record_problems(run_dir: &Path, record: &EvidenceRecord) -> Vec<String> {
    let mut problems = Vec::new();
    if record.kind != Kind::Native {
        problems.push("is not a native record".to_owned());
    }
    if !record
        .subjects
        .iter()
        .any(|subject| subject.source == Source::Managed && subject.digest.is_some())
    {
        problems.push("has no managed subject with a digest".to_owned());
    }
    match read_compat_file(run_dir, &record.evidence_path) {
        Ok(file) => problems.extend(file_problems(&file)),
        Err(problem) => problems.push(problem),
    }
    problems
}

/// Reads and decodes the [`COMPAT_FILE`] of a record folder under
/// `artifacts/`.
fn read_compat_file(run_dir: &Path, evidence_path: &str) -> Result<CompatFile, String> {
    let folder = evidence_path.trim_end_matches('/');
    if !evidence_path.starts_with("artifacts/")
        || folder.split('/').any(|part| part == ".." || part == ".")
    {
        return Err(format!(
            "evidencePath `{evidence_path}` must be a folder under artifacts/ that holds {COMPAT_FILE}"
        ));
    }
    let path = run_dir.join(folder).join(COMPAT_FILE);
    if !fs::symlink_metadata(&path).is_ok_and(|meta| meta.is_file()) {
        return Err(format!(
            "evidencePath `{evidence_path}` must be a folder under artifacts/ that holds {COMPAT_FILE}"
        ));
    }
    let bytes = fs::read(&path).map_err(|err| format!("cannot read {COMPAT_FILE}: {err}"))?;
    serde_json::from_slice(&bytes)
        .map_err(|err| format!("{COMPAT_FILE} is not a {COMPAT_SCHEMA} file: {err}"))
}

fn file_problems(file: &CompatFile) -> Vec<String> {
    let mut problems = Vec::new();
    if file.schema != COMPAT_SCHEMA {
        problems.push(format!(
            "{COMPAT_FILE} schema must be `{COMPAT_SCHEMA}`, found `{}`",
            file.schema
        ));
    }
    let revision = &file.provider_revision;
    if revision.id.is_empty() || revision.revision.is_empty() {
        problems.push(format!(
            "{COMPAT_FILE} providerRevision needs an id and a revision"
        ));
    }
    if revision.manifest_sha256.len() != 64
        || !revision
            .manifest_sha256
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    {
        problems.push(format!(
            "{COMPAT_FILE} providerRevision.manifestSha256 must be 64 lowercase hex characters"
        ));
    }
    for (name, codes) in [
        ("checks", &file.checks),
        ("knownLimitations", &file.known_limitations),
    ] {
        if codes.len() > CODES_MAX {
            problems.push(format!(
                "{COMPAT_FILE} {name} has {} codes; at most {CODES_MAX}",
                codes.len()
            ));
        }
        for code in codes {
            if !is_code(code) {
                problems.push(format!(
                    "{COMPAT_FILE} {name} code `{code}` must be snake_case, 1 to {CODES_MAX} characters"
                ));
            }
        }
    }
    problems
}

fn is_code(code: &str) -> bool {
    let mut bytes = code.bytes();
    code.len() <= CODES_MAX
        && bytes.next().is_some_and(|b| b.is_ascii_lowercase())
        && bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// Rule 12: the number of run folders in `evidence/compatibility/` under
/// `root`, and the violations. Every report there follows the rules of
/// `runtime compat-record`: the folder is named after its runId, the
/// summary counts the records, and every record is a native `PASS` or
/// `FAIL` of BR-025 with a managed subject and a valid [`COMPAT_FILE`].
///
/// # Errors
///
/// Returns an error when the folder cannot be read.
pub fn violations(root: &Path) -> Result<(usize, Vec<Violation>)> {
    let dir = root.join(COMPAT_EVIDENCE_DIR);
    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok((0, Vec::new())),
        Err(err) => return Err(err).with_context(|| format!("cannot read {}", dir.display())),
    };
    let mut names = Vec::new();
    for entry in entries {
        let entry = entry.with_context(|| format!("cannot read {}", dir.display()))?;
        let file_type = entry
            .file_type()
            .with_context(|| format!("cannot read {}", entry.path().display()))?;
        if file_type.is_dir() || file_type.is_symlink() {
            names.push((
                entry.file_name().to_string_lossy().into_owned(),
                file_type.is_symlink(),
            ));
        }
    }
    names.sort();
    let mut out = Vec::new();
    for (name, is_symlink) in &names {
        let mut push = |message: String| {
            out.push(Violation {
                subject: format!("{COMPAT_EVIDENCE_DIR}/{name}"),
                rule: RULE,
                message,
            });
        };
        if *is_symlink {
            push("a run must be a folder, not a symbolic link".to_owned());
            continue;
        }
        run_problems(&dir.join(name), name)?
            .into_iter()
            .for_each(&mut push);
    }
    Ok((names.len(), out))
}

fn run_problems(run_dir: &Path, name: &str) -> Result<Vec<String>> {
    let mut problems = Vec::new();
    let bytes = match fs::read(run_dir.join("report.json")) {
        Ok(bytes) => bytes,
        Err(err)
            if matches!(
                err.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::IsADirectory
            ) =>
        {
            return Ok(vec!["report.json is missing".to_owned()]);
        }
        Err(err) => {
            return Err(err)
                .with_context(|| format!("cannot read {}/report.json", run_dir.display()));
        }
    };
    let report: RunReport = match serde_json::from_slice(&bytes) {
        Ok(report) => report,
        Err(err) => {
            return Ok(vec![format!(
                "report.json is not a {REPORT_SCHEMA} report: {err}"
            )]);
        }
    };
    if report.schema != REPORT_SCHEMA {
        problems.push(format!(
            "report.json schema must be `{REPORT_SCHEMA}`, found `{}`",
            report.schema
        ));
    }
    if report.run_id.to_string() != name {
        problems.push(format!(
            "the folder name must be the runId {}",
            report.run_id
        ));
    }
    if report.summary != Summary::of(&report.records) {
        problems.push("summary does not match the records".to_owned());
    }
    if report.records.is_empty() {
        problems.push("the report holds no record of BR-025".to_owned());
    }
    for record in &report.records {
        let id = record.id;
        if let Err(err) = record.validate() {
            problems.extend(
                err.violations
                    .iter()
                    .map(|rule| format!("record {id}: {rule}")),
            );
        }
        if !matches!(record.status, Status::Pass | Status::Fail) {
            problems.push(format!(
                "record {id} is {}; only PASS and FAIL records are kept",
                record.status.as_str()
            ));
        }
        if !record.covers.iter().any(|covered| covered == COMPAT_COVERS) {
            problems.push(format!("record {id} does not cover {COMPAT_COVERS}"));
        }
        problems.extend(
            record_problems(run_dir, record)
                .into_iter()
                .map(|problem| format!("record {id} {problem}")),
        );
    }
    Ok(problems)
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use serde_json::{Value, json};

    pub const SHA: &str = "88bb7533e639e852e5258c293aa42f0abf40aed42d18f0f714cb47a9354b6590";
    pub const MANIFEST_SHA: &str =
        "1111111111111111111111111111111111111111111111111111111111111111";

    pub fn uuid(n: u32) -> String {
        format!("019a0000-0000-7000-8000-{n:012}")
    }

    fn context(arch: &str, os_version: &str) -> Value {
        json!({
            "sourceCommit": "0123456789abcdef0123456789abcdef01234567",
            "sourceDirty": false,
            "runKind": "native",
            "host": { "os": "macos", "arch": arch, "osVersion": os_version, "osBuild": "26A428" },
            "toolchain": { "rustc": "1.99.1" }
        })
    }

    /// A record of a run: `name` is the test name, `finished` the end time.
    pub fn record(id: u32, status: &str, covers: &[&str], finished: &str) -> Value {
        json!({
            "schema": "stackly.evidence/v1",
            "id": uuid(id),
            "covers": covers,
            "kind": "native",
            "method": "automated",
            "test": { "language": "rust", "package": "xtask", "target": "native_runtime",
                      "name": "postgresql_18_runs", "file": "xtask/tests/native_runtime.rs", "line": 9 },
            "status": status,
            "expected": "it runs",
            "actual": "postgresql 18.6 ready in 493 ms, stopped in 184 ms",
            "subjects": [{ "provider": "postgresql", "version": "18.6", "digest": SHA, "source": "managed" }],
            "startedAt": "2026-10-18T11:00:00Z",
            "finishedAt": finished,
            "evidencePath": format!("artifacts/{}/", uuid(id)),
            "context": context("aarch64", "27.0")
        })
    }

    pub fn report(run: u32, records: &[Value]) -> Value {
        let typed: Vec<EvidenceRecord> = records
            .iter()
            .map(|record| serde_json::from_value(record.clone()).unwrap())
            .collect();
        json!({
            "schema": "stackly.evidence-report/v1",
            "runId": uuid(run),
            "kind": "native",
            "wrapperVersion": "0.1.0",
            "context": context("aarch64", "27.0"),
            "startedAt": "2026-10-18T11:00:00Z",
            "finishedAt": "2026-10-18T11:30:00Z",
            "commandExitCode": 0,
            "interrupted": false,
            "summary": Summary::of(&typed),
            "problems": [],
            "records": records
        })
    }

    /// Writes `evidence/<folder>/<run>/` with the report and, for every
    /// record, `file` holding `body`.
    pub fn write_run(
        root: &Path,
        folder: &str,
        run: u32,
        records: &[Value],
        file: &str,
        body: &Value,
    ) {
        let dir = root.join(folder).join(uuid(run));
        for record in records {
            let artifact = dir.join(record["evidencePath"].as_str().unwrap());
            fs::create_dir_all(&artifact).unwrap();
            fs::write(
                artifact.join(file),
                serde_json::to_vec_pretty(body).unwrap(),
            )
            .unwrap();
        }
        fs::write(
            dir.join("report.json"),
            serde_json::to_vec_pretty(&report(run, records)).unwrap(),
        )
        .unwrap();
    }

    pub fn spike(passed: &[&str]) -> Value {
        json!({
            "steps": passed.iter().map(|n| json!({"name": n, "status": "PASS"})).chain([json!({"name": "ready", "status": "FAIL"})]).collect::<Vec<_>>(),
            "closureProbes": [{"name": "icu", "status": "PASS"}, {"name": "icu", "status": "PASS"}]
        })
    }

    pub fn compat(checks: &[&str]) -> Value {
        json!({
            "schema": COMPAT_SCHEMA,
            "providerRevision": { "id": "official/postgresql", "revision": "3", "manifestSha256": MANIFEST_SHA },
            "checks": checks,
            "knownLimitations": ["no_replication"]
        })
    }

    #[test]
    fn a_runtime_run_gives_one_record_with_snake_case_checks_and_its_evidence_path() {
        let root = tempfile::tempdir().unwrap();
        let records = [record(
            2,
            "PASS",
            &["AC-078"],
            "2026-10-18T11:07:19.973842Z",
        )];
        write_run(
            root.path(),
            "evidence/runtime",
            1,
            &records,
            "spike-result.json",
            &spike(&["launch", "static audit", "stop", "launch"]),
        );

        let evidence = Evidence::load(root.path()).unwrap();
        let found = evidence.records_for(SHA).unwrap();

        assert_eq!(
            serde_json::to_value(&found).unwrap(),
            json!([{
                "scope": "runtime", "result": "pass", "providerRevision": null,
                "os": "macos", "osVersion": "27.0", "osBuild": "26A428", "arch": "aarch64",
                "checks": ["icu", "launch_agent", "static_audit", "stop"],
                "knownLimitations": ["provider_integration_not_covered"],
                "summary": "postgresql 18.6 ready in 493 ms, stopped in 184 ms",
                "testedAt": "2026-10-18T11:07:19.973Z",
                "evidence": format!("runtime/{}/{}", uuid(1), uuid(2))
            }])
        );
        assert!(evidence.records_for(&"f".repeat(64)).unwrap().is_empty());
    }

    #[test]
    fn the_latest_record_per_key_wins_even_when_it_is_a_fail() {
        let root = tempfile::tempdir().unwrap();
        let old = [record(2, "PASS", &["AC-078"], "2026-10-18T11:00:00Z")];
        let new = [record(4, "FAIL", &["AC-078"], "2026-10-19T11:00:00Z")];
        write_run(
            root.path(),
            "evidence/runtime",
            1,
            &old,
            "spike-result.json",
            &spike(&["stop"]),
        );
        write_run(
            root.path(),
            "evidence/runtime",
            3,
            &new,
            "spike-result.json",
            &spike(&["stop"]),
        );

        let found = Evidence::load(root.path())
            .unwrap()
            .records_for(SHA)
            .unwrap();

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].result, Outcome::Fail);
        assert_eq!(
            found[0].evidence,
            format!("runtime/{}/{}", uuid(3), uuid(4))
        );
    }

    #[test]
    fn provider_records_come_after_runtime_ones_by_scope_and_keep_the_revision() {
        let root = tempfile::tempdir().unwrap();
        let runtime = [record(2, "PASS", &["AC-078"], "2026-10-18T11:00:00Z")];
        let provider = [
            record(4, "PASS", &["BR-025", "AC-020"], "2026-10-20T11:00:00Z"),
            record(5, "PASS", &["AC-021"], "2026-10-20T11:01:00Z"),
        ];
        write_run(
            root.path(),
            "evidence/runtime",
            1,
            &runtime,
            "spike-result.json",
            &spike(&[]),
        );
        write_run(
            root.path(),
            "evidence/compatibility",
            3,
            &provider,
            "compatibility.json",
            &compat(&["start", "backup", "start"]),
        );

        let found = Evidence::load(root.path())
            .unwrap()
            .records_for(SHA)
            .unwrap();

        let scopes: Vec<Scope> = found.iter().map(|record| record.scope).collect();
        assert_eq!(scopes, [Scope::Runtime, Scope::Provider]);
        assert_eq!(found[1].checks, ["backup", "start"]);
        assert_eq!(found[1].known_limitations, ["no_replication"]);
        assert_eq!(found[1].provider_revision.as_ref().unwrap().revision, "3");
        assert_eq!(
            found[1].evidence,
            format!("compatibility/{}/{}", uuid(3), uuid(4))
        );
    }

    #[test]
    fn another_macos_major_or_architecture_is_another_key() {
        let root = tempfile::tempdir().unwrap();
        let mut other = record(4, "PASS", &["AC-078"], "2026-10-19T11:00:00Z");
        other["context"]["host"]["osVersion"] = json!("28.1");
        let mut intel = record(6, "PASS", &["AC-078"], "2026-10-19T12:00:00Z");
        intel["context"]["host"]["arch"] = json!("arm64");
        let records = [
            record(2, "PASS", &["AC-078"], "2026-10-18T11:00:00Z"),
            other,
            intel,
        ];
        write_run(
            root.path(),
            "evidence/runtime",
            1,
            &records,
            "spike-result.json",
            &spike(&[]),
        );

        let found = Evidence::load(root.path())
            .unwrap()
            .records_for(SHA)
            .unwrap();

        // arm64 is written aarch64, so the third record replaces the first.
        let versions: Vec<&str> = found.iter().map(|r| r.os_version.as_str()).collect();
        assert_eq!(versions, ["27.0", "28.1"]);
        assert!(found.iter().all(|record| record.arch == "aarch64"));
    }

    #[test]
    fn a_record_without_a_spike_result_names_the_record() {
        let root = tempfile::tempdir().unwrap();
        let records = [record(2, "PASS", &["AC-078"], "2026-10-18T11:00:00Z")];
        write_run(
            root.path(),
            "evidence/runtime",
            1,
            &records,
            "other.json",
            &spike(&[]),
        );

        let err = Evidence::load(root.path())
            .unwrap()
            .records_for(SHA)
            .unwrap_err();

        assert!(format!("{err:#}").contains(&format!(
            "evidence/runtime/{}: record {}",
            uuid(1),
            uuid(2)
        )));
    }

    fn rule_12(root: &Path) -> Vec<String> {
        violations(root)
            .unwrap()
            .1
            .iter()
            .map(ToString::to_string)
            .collect()
    }

    #[test]
    fn rule_12_accepts_a_clean_provider_run_and_an_absent_folder() {
        let root = tempfile::tempdir().unwrap();
        assert_eq!(violations(root.path()).unwrap().0, 0);
        let records = [record(2, "PASS", &["BR-025"], "2026-10-18T11:00:00Z")];
        write_run(
            root.path(),
            "evidence/compatibility",
            1,
            &records,
            "compatibility.json",
            &compat(&["start"]),
        );

        assert_eq!(rule_12(root.path()), Vec::<String>::new());
        assert_eq!(violations(root.path()).unwrap().0, 1);
    }

    #[test]
    fn rule_12_reports_each_way_a_report_breaks_the_compat_record_rules() {
        let root = tempfile::tempdir().unwrap();
        let mut fixture = record(2, "PASS", &["AC-1"], "2026-10-18T11:00:00Z");
        fixture["kind"] = json!("fixture");
        fixture["subjects"][0]["source"] = json!("fixture");
        let records = [fixture];
        let mut bad = compat(&["Start"]);
        bad["schema"] = json!("stackly.compatibility-evidence/v2");
        write_run(
            root.path(),
            "evidence/compatibility",
            1,
            &records,
            "compatibility.json",
            &bad,
        );

        let found = rule_12(root.path());

        let subject = format!("{COMPAT_EVIDENCE_DIR}/{}", uuid(1));
        let id = uuid(2);
        for part in [
            format!("record {id} does not cover BR-025"),
            format!("record {id} is not a native record"),
            format!("record {id} has no managed subject with a digest"),
            format!(
                "record {id} compatibility.json schema must be `{COMPAT_SCHEMA}`, found `stackly.compatibility-evidence/v2`"
            ),
            format!(
                "record {id} compatibility.json checks code `Start` must be snake_case, 1 to 64 characters"
            ),
        ] {
            assert!(
                found.contains(&format!("{subject}: {part} (rule 12)")),
                "missing: {part}\n{found:#?}"
            );
        }
    }

    #[test]
    fn rule_12_refuses_a_missing_file_a_wrong_folder_name_and_a_link() {
        let root = tempfile::tempdir().unwrap();
        let records = [record(2, "PASS", &["BR-025"], "2026-10-18T11:00:00Z")];
        write_run(
            root.path(),
            "evidence/compatibility",
            1,
            &records,
            "other.json",
            &compat(&[]),
        );
        let from = root.path().join(COMPAT_EVIDENCE_DIR).join(uuid(1));
        fs::rename(
            &from,
            root.path().join(COMPAT_EVIDENCE_DIR).join("2026-10-18-run"),
        )
        .unwrap();
        fs::create_dir(root.path().join(COMPAT_EVIDENCE_DIR).join("empty")).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(
            root.path(),
            root.path().join(COMPAT_EVIDENCE_DIR).join("link"),
        )
        .unwrap();

        let found = rule_12(root.path());

        let id = uuid(2);
        let run = uuid(1);
        assert!(found.contains(&format!(
            "{COMPAT_EVIDENCE_DIR}/2026-10-18-run: the folder name must be the runId {run} (rule 12)"
        )));
        assert!(found.contains(&format!(
            "{COMPAT_EVIDENCE_DIR}/2026-10-18-run: record {id} evidencePath `artifacts/{id}/` must be a folder under artifacts/ that holds compatibility.json (rule 12)"
        )));
        assert!(found.contains(&format!(
            "{COMPAT_EVIDENCE_DIR}/empty: report.json is missing (rule 12)"
        )));
        assert!(found.contains(&format!(
            "{COMPAT_EVIDENCE_DIR}/link: a run must be a folder, not a symbolic link (rule 12)"
        )));
    }
}
