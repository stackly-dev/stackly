//! The spike result `stackly.runtime-spike-result/v1` (spec 0003): what the
//! worker saw, written as camelCase JSON and kept in the test's
//! `artifact_dir()`.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use stackly_evidence::write_json_atomic;

use super::plan::Redaction;

/// The `schema` value of a spike result.
pub const RESULT_SCHEMA: &str = "stackly.runtime-spike-result/v1";

/// The outcome of one spike.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SpikeStatus {
    /// Every step passed.
    #[serde(rename = "PASS")]
    Pass,
    /// A step failed; `failure` says which and why.
    #[serde(rename = "FAIL")]
    Fail,
    /// A prerequisite is missing, such as a GUI login session.
    #[serde(rename = "BLOCKED")]
    Blocked,
}

impl SpikeStatus {
    /// The driver's exit code: 0 PASS, 1 FAIL, 2 BLOCKED.
    pub fn exit_code(self) -> u8 {
        match self {
            Self::Pass => 0,
            Self::Fail => 1,
            Self::Blocked => 2,
        }
    }
}

/// The outcome of one step or probe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StepStatus {
    /// It worked.
    #[serde(rename = "PASS")]
    Pass,
    /// It did not.
    #[serde(rename = "FAIL")]
    Fail,
}

/// The whole result.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpikeResult {
    /// Always [`RESULT_SCHEMA`].
    pub schema: String,
    /// The inventory ID.
    pub artifact_id: String,
    /// The pinned digest.
    pub sha256: String,
    /// The outcome.
    pub status: SpikeStatus,
    /// The step that failed and why, for `FAIL` and `BLOCKED`.
    pub failure: Option<Failure>,
    /// Where the worker ran.
    pub launch: Option<Launch>,
    /// The version the running binary reported, normalized per provider.
    pub observed_version: Option<String>,
    /// The version text as the binary printed it.
    pub raw_version: Option<String>,
    /// `minos` of the main executable, from `vtool`.
    pub min_os_measured: Option<String>,
    /// The signature observed on each executable before any re-signing.
    pub signature: Vec<ExecutableSignature>,
    /// What the extracted archive holds.
    pub archive: Option<ArchiveReport>,
    /// The static and runtime library audits.
    pub libraries: Option<Libraries>,
    /// Every TCP listener of the server process.
    pub listeners: Vec<Listener>,
    /// Provider probes and the dependency closure check.
    pub closure_probes: Vec<Probe>,
    /// The quarantine probe (AC-9).
    pub quarantine: Option<Quarantine>,
    /// How the server stopped.
    pub stop: Option<Stop>,
    /// Every step in order.
    pub steps: Vec<Step>,
    /// The last output of each provider process.
    pub log_tail: Vec<LogTail>,
}

/// Which step failed and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Failure {
    /// The step name, such as `ready`.
    pub step: String,
    /// The reason, such as `digest mismatch`.
    pub reason: String,
}

/// The launch context of the worker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Launch {
    /// `gui/<uid>`.
    pub domain: String,
    /// True when the worker's parent is launchd (PID 1).
    pub parent_is_launchd: bool,
    /// True when the worker could open `/dev/tty`.
    pub controlling_tty: bool,
}

/// The signature of one executable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutableSignature {
    /// The path inside the artifact root.
    pub executable: String,
    /// `developer_id`, `apple`, `certificate`, `adhoc`, `linker_signed`, or
    /// `unsigned`.
    pub kind: String,
    /// The leaf authority of a certificate signature.
    pub authority: Option<String>,
    /// The team identifier, when set.
    pub team_id: Option<String>,
    /// True when the spike signed every Mach-O file of the copy ad hoc.
    pub adhoc_signed_at_install: bool,
}

/// What the extracted archive holds.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveReport {
    /// The number of regular files.
    pub files: u64,
    /// The number of symbolic links that stay inside the artifact.
    pub symlinks_internal: u64,
    /// Links that point outside the artifact, as `<path> -> <target>`.
    pub symlinks_escaping: Vec<String>,
    /// Files with the setuid or setgid bit.
    pub setuid: Vec<String>,
    /// Anything that is not a file, a folder, or a link.
    pub special_files: Vec<String>,
}

/// The library audits (AC-6).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Libraries {
    /// Every library the Mach-O files reference, resolved.
    #[serde(rename = "static")]
    pub static_: Vec<String>,
    /// Every Mach-O file the running server mapped.
    pub runtime: Vec<String>,
    /// Paths outside the allowlist and the artifact.
    pub foreign: Vec<String>,
    /// Findings that do not fail the spike.
    pub warnings: Vec<String>,
}

/// One TCP listener.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Listener {
    /// The address as `lsof` prints it.
    pub address: String,
    /// The port.
    pub port: u16,
}

/// One probe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Probe {
    /// The probe name.
    pub name: String,
    /// Whether it passed.
    pub status: StepStatus,
    /// What it saw.
    pub detail: String,
}

/// The quarantine probe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Quarantine {
    /// `ran`, `blocked`, `killed`, `timeout`, or `skipped`.
    pub outcome: String,
    /// The exit status, signal, or the folder that already has the result.
    pub detail: String,
}

/// How the server stopped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Stop {
    /// The exit code, when it exited.
    pub exit_code: Option<i32>,
    /// The signal that ended it, when one did.
    pub signal: Option<i32>,
    /// From the stop request to the exit.
    pub duration_ms: u64,
}

/// One step of the worker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Step {
    /// The step name.
    pub name: String,
    /// Whether it passed.
    pub status: StepStatus,
    /// How long it took.
    pub duration_ms: u64,
    /// What it did or why it failed.
    pub detail: Option<String>,
}

/// The last output of one provider process.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogTail {
    /// The process, such as `server` or `init`.
    pub process: String,
    /// At most the last 4 KiB of stdout.
    pub stdout: String,
    /// At most the last 4 KiB of stderr.
    pub stderr: String,
}

impl SpikeResult {
    /// An empty result for one artifact, status `FAIL` until proven.
    pub fn new(artifact_id: &str, sha256: &str) -> Self {
        Self {
            schema: RESULT_SCHEMA.to_owned(),
            artifact_id: artifact_id.to_owned(),
            sha256: sha256.to_owned(),
            status: SpikeStatus::Fail,
            failure: None,
            launch: None,
            observed_version: None,
            raw_version: None,
            min_os_measured: None,
            signature: Vec::new(),
            archive: None,
            libraries: None,
            listeners: Vec::new(),
            closure_probes: Vec::new(),
            quarantine: None,
            stop: None,
            steps: Vec::new(),
            log_tail: Vec::new(),
        }
    }

    /// A result that ends with `status` at `step` for `reason`.
    pub fn ended(
        artifact_id: &str,
        sha256: &str,
        status: SpikeStatus,
        step: &str,
        reason: &str,
    ) -> Self {
        let mut result = Self::new(artifact_id, sha256);
        result.status = status;
        result.failure = Some(Failure {
            step: step.to_owned(),
            reason: reason.to_owned(),
        });
        result
    }

    /// Marks the result failed at `step`, unless it already failed.
    pub fn fail(&mut self, step: &str, reason: &str) {
        self.status = SpikeStatus::Fail;
        self.failure.get_or_insert_with(|| Failure {
            step: step.to_owned(),
            reason: reason.to_owned(),
        });
    }

    /// Reads a result file.
    pub fn read(path: &Path) -> Result<Self> {
        let bytes = fs::read(path).with_context(|| format!("cannot read {}", path.display()))?;
        let result: Self = serde_json::from_slice(&bytes)
            .with_context(|| format!("{} is not a spike result", path.display()))?;
        if result.schema != RESULT_SCHEMA {
            bail!(
                "{} has schema `{}`, not `{RESULT_SCHEMA}`",
                path.display(),
                result.schema
            );
        }
        Ok(result)
    }

    /// Writes the result atomically after replacing every redaction in
    /// every string, so no root path, home, user, or host name leaks.
    pub fn write(&self, path: &Path, redactions: &[Redaction]) -> Result<()> {
        let mut value = serde_json::to_value(self).context("cannot encode the spike result")?;
        redact_value(&mut value, redactions);
        write_json_atomic(path, &value)
            .with_context(|| format!("cannot write the spike result {}", path.display()))
    }
}

/// Applies `redactions` to every string in `value`, keys included.
pub fn redact_value(value: &mut Value, redactions: &[Redaction]) {
    match value {
        Value::String(text) => *text = redact(text, redactions),
        Value::Array(items) => items
            .iter_mut()
            .for_each(|item| redact_value(item, redactions)),
        Value::Object(map) => {
            let entries = std::mem::take(map);
            for (key, mut item) in entries {
                redact_value(&mut item, redactions);
                map.insert(redact(&key, redactions), item);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

/// Applies `redactions` in order to `text`.
pub fn redact(text: &str, redactions: &[Redaction]) -> String {
    let mut out = text.to_owned();
    for redaction in redactions {
        if redaction.from.is_empty() {
            continue;
        }
        out = if redaction.whole_word {
            replace_word(&out, &redaction.from, &redaction.to)
        } else {
            out.replace(&redaction.from, &redaction.to)
        };
    }
    out
}

/// Replaces `from` only where neither neighbor is a letter, digit, `_`, or
/// `-`, so a short user name never eats part of a longer word.
fn replace_word(text: &str, from: &str, to: &str) -> String {
    let is_word = |c: char| c.is_alphanumeric() || c == '_' || c == '-';
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(from) {
        let before = rest[..at]
            .chars()
            .next_back()
            .or_else(|| out.chars().next_back());
        let after = rest[at + from.len()..].chars().next();
        out.push_str(&rest[..at]);
        if before.is_some_and(is_word) || after.is_some_and(is_word) {
            out.push_str(from);
        } else {
            out.push_str(to);
        }
        rest = &rest[at + from.len()..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn redaction(from: &str, to: &str, whole_word: bool) -> Redaction {
        Redaction {
            from: from.to_owned(),
            to: to.to_owned(),
            whole_word,
        }
    }

    #[test]
    fn every_string_and_key_loses_the_root_home_and_user() {
        let redactions = [
            redaction("/private/var/t/stackly-runtime-spike-1", "$ROOT", false),
            redaction("/var/t/stackly-runtime-spike-1", "$ROOT", false),
            redaction("/Users/dev", "~", false),
            redaction("dev", "$USER", true),
        ];
        let mut value = serde_json::json!({
            "a": "/var/t/stackly-runtime-spike-1/Stackly Spike Ü/x",
            "b": ["/private/var/t/stackly-runtime-spike-1/run", "owned by user \"dev\""],
            "/Users/dev/key": "reads /dev/null and devtools",
            "n": 3
        });
        redact_value(&mut value, &redactions);
        assert_eq!(
            value,
            serde_json::json!({
                "a": "$ROOT/Stackly Spike Ü/x",
                "b": ["$ROOT/run", "owned by user \"$USER\""],
                "~/key": "reads /$USER/null and devtools",
                "n": 3
            })
        );
    }

    #[test]
    fn a_whole_word_redaction_keeps_longer_words() {
        assert_eq!(
            replace_word("ann anna joann ann", "ann", "X"),
            "X anna joann X"
        );
        assert_eq!(replace_word("ann_x x-ann", "ann", "X"), "ann_x x-ann");
    }

    #[test]
    fn a_result_round_trips_with_its_wire_names() {
        let mut result = SpikeResult::new("nats-2.15.1-macos-aarch64", &"a".repeat(64));
        result.libraries = Some(Libraries::default());
        result.fail("ready", "nats-server exited 1 before it was ready");
        let value = serde_json::to_value(&result).unwrap();
        assert_eq!(value["schema"], RESULT_SCHEMA);
        assert_eq!(value["status"], "FAIL");
        assert_eq!(value["failure"]["step"], "ready");
        assert!(value["libraries"].get("static").is_some());
        let back: SpikeResult = serde_json::from_value(value).unwrap();
        assert_eq!(back.failure, result.failure);
    }
}
