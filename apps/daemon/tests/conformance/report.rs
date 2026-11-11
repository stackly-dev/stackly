//! The `stackly.provider-conformance/v1` report (spec 0036, *Report*,
//! AC-13): built during the run, every path string written with `~` for
//! home and `$TMPDIR` for the temporary directory, and written atomically.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{Map, Value, json};
use stackly_core::provider::PackageCheck;
use uuid::Uuid;

use super::{REPORT_SCHEMA, ScenarioResult, Verdict, expect_name};

/// The runtime a run used.
#[derive(Debug, Clone)]
pub struct RuntimeFacts {
    /// The software of the artifact.
    pub software: String,
    /// The version `runtime.get` reported, else the artifact's.
    pub version: String,
    /// The SHA-256 of what was installed.
    pub sha256: String,
    /// `fixture` or `managed`.
    pub source: &'static str,
}

/// What the end of the run did.
#[derive(Debug, Clone, Default)]
pub struct Cleanup {
    /// Both root folders were removed.
    pub root_removed: bool,
    /// The data folder was kept (`--keep`), with its path.
    pub kept: Option<PathBuf>,
    /// The processes that still named the root after the daemon stopped.
    pub processes_left: Vec<Value>,
    /// Why a folder was left in place.
    pub refused: Option<String>,
}

/// A conformance report.
#[derive(Debug, Clone)]
pub struct Report {
    run_id: Uuid,
    started_at: String,
    finished_at: Option<String>,
    kind: Option<&'static str>,
    package: Value,
    validation: Value,
    validate_failure: Option<String>,
    /// The runtime of the run, once installed.
    pub runtime: Option<RuntimeFacts>,
    /// Every scenario in run order.
    pub scenarios: Vec<ScenarioResult>,
    /// `proven` or `unproven`, from the first `handover` step reached.
    pub handover: Option<&'static str>,
    /// The end of the run.
    pub cleanup: Cleanup,
    blocked: Option<(String, String)>,
    /// A signal stopped the run.
    pub interrupted: bool,
    /// The two folders of the run's test root, once made; never written to
    /// the report, so a test can check that nothing still names them.
    pub root_folders: Vec<PathBuf>,
    written: Option<PathBuf>,
    value: Option<Value>,
}

impl Report {
    /// An empty report of run `run_id`, started now.
    #[must_use]
    pub fn new(run_id: Uuid) -> Self {
        Self {
            run_id,
            started_at: jiff::Timestamp::now().to_string(),
            finished_at: None,
            kind: None,
            package: Value::Null,
            validation: Value::Null,
            validate_failure: None,
            runtime: None,
            scenarios: Vec::new(),
            handover: None,
            cleanup: Cleanup::default(),
            blocked: None,
            interrupted: false,
            root_folders: Vec::new(),
            written: None,
            value: None,
        }
    }

    /// The run ID.
    #[must_use]
    pub const fn run_id(&self) -> Uuid {
        self.run_id
    }

    /// Notes the package identity and the validation.
    pub fn package(&mut self, check: &PackageCheck) {
        let identity = check.package.as_ref().map(|package| {
            let key = package.key();
            (
                key.provider().to_string(),
                key.revision().to_string(),
                key.manifest_sha256().to_string(),
            )
        });
        self.package = json!({
            "folder": check.folder,
            "provider": identity.as_ref().map(|(provider, _, _)| provider),
            "revision": identity.as_ref().map(|(_, revision, _)| revision),
            "manifestSha256": identity.as_ref().map(|(_, _, sha)| sha),
            "apiVersion": check.api_version,
        });
        let findings: Vec<Value> = check
            .findings
            .iter()
            .map(|finding| {
                json!({
                    "severity": format!("{:?}", finding.severity).to_lowercase(),
                    "origin": format!("{:?}", finding.origin).to_lowercase(),
                    "file": finding.issue.file,
                    "location": finding.issue.location.to_string(),
                    "code": finding.issue.code.to_string(),
                    "message": finding.issue.message,
                })
            })
            .collect();
        self.validation = json!({
            "loadable": check.loadable(),
            "publishable": check.publishable(),
            "findings": findings,
        });
    }

    /// Ends the run at `validate`: no root was made.
    pub fn refuse_validate(&mut self, reason: &str) {
        self.validate_failure = Some(reason.to_owned());
    }

    /// Ends the run as `blocked`.
    pub fn block(&mut self, reason: &str, owner: &str) {
        self.blocked = Some((reason.to_owned(), owner.to_owned()));
    }

    /// Notes the kind of run: `fixture` or `native`.
    pub fn set_kind(&mut self, kind: &'static str) {
        self.kind = Some(kind);
    }

    /// The verdict of *Report*.
    #[must_use]
    pub fn verdict(&self) -> Verdict {
        if self.blocked.is_some() {
            return Verdict::Blocked;
        }
        let cleanup_failed =
            self.cleanup.refused.is_some() || !self.cleanup.processes_left.is_empty();
        if self.validate_failure.is_some()
            || self.interrupted
            || cleanup_failed
            || self.scenarios.iter().any(|scenario| !scenario.passed())
        {
            return Verdict::Fail;
        }
        Verdict::Pass
    }

    /// The scenario called `name`.
    #[must_use]
    pub fn scenario(&self, name: &str) -> Option<&ScenarioResult> {
        self.scenarios.iter().find(|scenario| scenario.name == name)
    }

    /// The file the report was written to.
    #[must_use]
    pub fn written(&self) -> Option<&Path> {
        self.written.as_deref()
    }

    /// The report as written, after [`finish_and_write`](Self::finish_and_write).
    #[must_use]
    pub fn value(&self) -> Option<&Value> {
        self.value.as_ref()
    }

    /// Stamps `finishedAt`, builds the JSON, and writes it atomically to
    /// `path` (a temporary file in the same folder, then a rename). A
    /// write that fails leaves `written` empty.
    pub fn finish_and_write(&mut self, path: &Path) {
        self.finished_at = Some(jiff::Timestamp::now().to_string());
        let mut value = self.to_value();
        sanitize(&mut value, &Sanitizer::new());
        let written = write_atomic(path, &value).ok().map(|()| path.to_path_buf());
        self.value = Some(value);
        self.written = written;
    }

    fn to_value(&self) -> Value {
        let (commit, dirty) = source_commit();
        let scenarios: Vec<Value> = self.scenarios.iter().map(scenario_value).collect();
        let mut root = json!({
            "schema": REPORT_SCHEMA,
            "runId": self.run_id.to_string(),
            "startedAt": self.started_at,
            "finishedAt": self.finished_at,
            "kind": self.kind,
            "host": host(),
            "sourceCommit": commit,
            "dirty": dirty,
            "package": self.package,
            "runtime": self.runtime.as_ref().map(|runtime| json!({
                "software": runtime.software,
                "version": runtime.version,
                "sha256": runtime.sha256,
                "source": runtime.source,
            })),
            "validation": self.validation,
            "scenarios": scenarios,
            "handover": self.handover,
            "cleanup": {
                "rootRemoved": self.cleanup.root_removed,
                "kept": self.cleanup.kept.as_ref().map(|path| path.display().to_string()),
                "processesLeft": self.cleanup.processes_left,
                "refused": self.cleanup.refused,
            },
            "verdict": self.verdict().as_str(),
            "blocked": self.blocked.as_ref().map(|(reason, owner)| json!({
                "reason": reason,
                "owner": owner,
            })),
            "interrupted": self.interrupted,
        });
        if let Some(reason) = &self.validate_failure
            && let Some(map) = root.as_object_mut()
        {
            // The run ended before any scenario: the one failed step is
            // `validate`, outside every scenario.
            map.insert(
                "failure".to_owned(),
                json!({ "step": "validate", "reason": "not_runnable", "message": reason }),
            );
        }
        root
    }
}

fn scenario_value(scenario: &ScenarioResult) -> Value {
    let steps: Vec<Value> = scenario
        .steps
        .iter()
        .map(|step| {
            let mut map = Map::new();
            map.insert("name".to_owned(), json!(step.name));
            map.insert("status".to_owned(), json!(step.status.as_str()));
            map.insert("durationMs".to_owned(), json!(step.duration_ms));
            if let Some(detail) = &step.detail {
                map.insert("detail".to_owned(), detail.clone());
            }
            Value::Object(map)
        })
        .collect();
    let failure = scenario.failure().map(|step| {
        let failure = step.failure.clone().unwrap_or_default();
        json!({
            "step": step.name,
            "reason": failure.reason,
            "code": failure.code,
            "message": failure.message,
        })
    });
    let mut map = Map::new();
    map.insert("name".to_owned(), json!(scenario.name));
    map.insert("expect".to_owned(), json!(expect_name(scenario.expect)));
    map.insert(
        "outcome".to_owned(),
        json!(if scenario.passed() { "pass" } else { "fail" }),
    );
    map.insert("steps".to_owned(), Value::Array(steps));
    if let Some(failure) = failure {
        map.insert("failure".to_owned(), failure);
    }
    Value::Object(map)
}

/// `{ os, osVersion, arch }` of this Mac.
fn host() -> Value {
    let version = Command::new("/usr/bin/sw_vers")
        .arg("-productVersion")
        .stdin(Stdio::null())
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned());
    json!({
        "os": std::env::consts::OS,
        "osVersion": version,
        "arch": std::env::consts::ARCH,
    })
}

/// The repository `HEAD` and whether the tree has changes; `(null, true)`
/// outside a git repository.
fn source_commit() -> (Option<String>, bool) {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let git = |args: &[&str]| {
        Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(args)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
    };
    match git(&["rev-parse", "HEAD"]) {
        Some(commit) => {
            let dirty = git(&["status", "--porcelain"]).is_none_or(|status| !status.is_empty());
            (Some(commit), dirty)
        }
        None => (None, true),
    }
}

/// Replaces the home folder and the temporary directory in path strings.
#[derive(Debug, Clone)]
pub struct Sanitizer {
    pairs: Vec<(String, &'static str)>,
}

impl Sanitizer {
    /// The pairs of this Mac: the canonical and the plain temporary
    /// directory first (it can sit under home), then home.
    #[must_use]
    pub fn new() -> Self {
        let mut pairs = Vec::new();
        let temp = std::env::temp_dir();
        for path in [temp.canonicalize().ok(), Some(temp)].into_iter().flatten() {
            let text = path.display().to_string();
            let text = text.trim_end_matches('/').to_owned();
            if !text.is_empty() && !pairs.iter().any(|(known, _)| *known == text) {
                pairs.push((text, "$TMPDIR"));
            }
        }
        if let Some(home) = std::env::var_os("HOME") {
            let text = PathBuf::from(home).display().to_string();
            let text = text.trim_end_matches('/').to_owned();
            if !text.is_empty() {
                pairs.push((text, "~"));
            }
        }
        Self { pairs }
    }

    /// `text` with every known folder replaced.
    #[must_use]
    pub fn apply(&self, text: &str) -> String {
        let mut out = text.to_owned();
        for (from, to) in &self.pairs {
            out = out.replace(from.as_str(), to);
        }
        out
    }
}

impl Default for Sanitizer {
    fn default() -> Self {
        Self::new()
    }
}

/// Runs `sanitizer` over every string of `value`.
pub fn sanitize(value: &mut Value, sanitizer: &Sanitizer) {
    match value {
        Value::String(text) => *text = sanitizer.apply(text),
        Value::Array(items) => items.iter_mut().for_each(|item| sanitize(item, sanitizer)),
        Value::Object(map) => map.values_mut().for_each(|item| sanitize(item, sanitizer)),
        _ => {}
    }
}

/// Writes `value` to a temporary file next to `path`, then renames it.
fn write_atomic(path: &Path, value: &Value) -> std::io::Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| std::io::Error::other("the report path has no folder"))?;
    std::fs::create_dir_all(dir)?;
    let mut text = serde_json::to_string_pretty(value).map_err(std::io::Error::other)?;
    text.push('\n');
    let name = path
        .file_name()
        .ok_or_else(|| std::io::Error::other("the report path has no file name"))?
        .to_string_lossy();
    let temp = dir.join(format!(".{name}.{}.tmp", std::process::id()));
    std::fs::write(&temp, text)?;
    std::fs::rename(&temp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&temp);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sanitizer_writes_home_and_the_temporary_directory_by_name() {
        let sanitizer = Sanitizer {
            pairs: vec![
                ("/private/var/folders/x/T".to_owned(), "$TMPDIR"),
                ("/Users/dev".to_owned(), "~"),
            ],
        };
        let mut value = json!({
            "a": "/private/var/folders/x/T/stackly-test-1/data",
            "b": ["/Users/dev/Library", 3],
        });

        sanitize(&mut value, &sanitizer);

        assert_eq!(
            value,
            json!({ "a": "$TMPDIR/stackly-test-1/data", "b": ["~/Library", 3] })
        );
    }
}
