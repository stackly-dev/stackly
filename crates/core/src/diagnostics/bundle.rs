//! Building the diagnostic bundle (spec 0034, *Isi bundle*, AC-12): the
//! entries the daemon gathered, every one but the instance logs through the
//! `Redactor` and every one through the home mask, held to the byte budget
//! in its order, then one zip in memory whose entries all carry the time of
//! the preview, so the same content gives the same bytes.
//!
//! JSON entries are redacted value by value before they are written, so a
//! secret that JSON would escape is still found; instance logs come from
//! the durable store, which only holds redacted text (spec 0027), so they
//! are masked only. JSON entries are never left out; a log that does not
//! fit, and every instance log past the first 50, is left out and named in
//! `README.txt`.

use std::io::{Cursor, Write};

use jiff::Timestamp;
use serde_json::Value;
use sha2::{Digest, Sha256};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, DateTime, ZipWriter};

use super::home::HomeMask;
use crate::secret::Redactor;

/// The most bytes of all entries together, before compression.
pub const BUNDLE_BYTES_MAX: u64 = 64 * 1024 * 1024;
/// The most lines of the daemon log.
pub const DAEMON_LOG_LINES_MAX: usize = 2000;
/// The most bytes of the daemon log.
pub const DAEMON_LOG_BYTES_MAX: usize = 4 * 1024 * 1024;
/// The most entries of one instance log.
pub const INSTANCE_LOG_ENTRIES_MAX: usize = 2000;
/// The most bytes of one instance log.
pub const INSTANCE_LOG_BYTES_MAX: usize = 1024 * 1024;
/// The most instance logs in one bundle.
pub const INSTANCE_LOGS_MAX: usize = 50;
/// The bytes kept for `README.txt` while the budget is spent.
const README_RESERVE: u64 = 64 * 1024;

/// What an entry of the bundle is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum BundleEntryKind {
    /// `README.txt`.
    Readme,
    /// `doctor.json`.
    Doctor,
    /// `system.json`.
    System,
    /// `instances.json`.
    Instances,
    /// `runtimes.json`.
    Runtimes,
    /// `operations.json`.
    Operations,
    /// `logs/stacklyd.log`.
    DaemonLog,
    /// `logs/instances/<name>.log`.
    InstanceLog,
    /// A file of `agent/`.
    Agent,
}

/// One entry of a built bundle, as the preview lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct BundleEntrySummary {
    /// The path in the zip, such as `logs/stacklyd.log`.
    pub path: String,
    /// What it is.
    pub kind: BundleEntryKind,
    /// One sentence about it.
    pub description: String,
    /// Its bytes before compression.
    pub bytes: u64,
    /// Its lines, for a text log.
    pub lines: Option<u64>,
    /// True when it holds less than its source had.
    pub truncated: bool,
}

/// A log as the bundle takes it: its lines, oldest first, and whether the
/// source had more.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct LogText {
    /// The lines, oldest first, without line breaks.
    pub lines: Vec<String>,
    /// True when the source had more lines than these.
    pub truncated: bool,
}

impl LogText {
    /// A log of `lines`, `truncated` when the source had more.
    #[must_use]
    pub const fn new(lines: Vec<String>, truncated: bool) -> Self {
        Self { lines, truncated }
    }
}

/// The log of one instance.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct InstanceLogInput {
    /// The instance name, safe as a file name (VAL-001).
    pub name: String,
    /// When its newest entry was written, for the order of the budget.
    pub newest: Option<Timestamp>,
    /// Its lines, `<RFC 3339> <stream> <text>`.
    pub log: LogText,
}

impl InstanceLogInput {
    /// The log of the instance `name`.
    #[must_use]
    pub const fn new(name: String, newest: Option<Timestamp>, log: LogText) -> Self {
        Self { name, newest, log }
    }
}

/// A file of `agent/`, as read from the data root.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct AgentFile {
    /// Its name, such as `background-agent.json`.
    pub name: String,
    /// Its text.
    pub text: String,
}

impl AgentFile {
    /// The file `name` with `text`.
    #[must_use]
    pub const fn new(name: String, text: String) -> Self {
        Self { name, text }
    }
}

/// Everything the daemon gathered for one bundle.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct BundleInput {
    /// When the preview was made; every zip entry carries this time.
    pub created_at: Timestamp,
    /// The Stackly version.
    pub stackly_version: String,
    /// The protocol version, `major.minor`.
    pub protocol_version: String,
    /// Whether the registry is in recovery mode.
    pub recovery: bool,
    /// The doctor report of the preview, as its DTO.
    pub doctor: Value,
    /// `system.json`.
    pub system: Value,
    /// `instances.json`, or `None` when the registry could not be read.
    pub instances: Option<Value>,
    /// `runtimes.json`, likewise.
    pub runtimes: Option<Value>,
    /// `operations.json`, likewise.
    pub operations: Option<Value>,
    /// The files of `agent/` that exist.
    pub agent: Vec<AgentFile>,
    /// The daemon log, or `None` when it could not be read.
    pub daemon_log: Option<LogText>,
    /// The instance logs that could be read.
    pub instance_logs: Vec<InstanceLogInput>,
    /// Sentences about what could not be gathered, for `README.txt`.
    pub notes: Vec<String>,
}

impl BundleInput {
    /// The input made at `created_at` by Stackly `stackly_version` with
    /// protocol `protocol_version`, with nothing gathered yet.
    #[must_use]
    pub const fn new(
        created_at: Timestamp,
        stackly_version: String,
        protocol_version: String,
        recovery: bool,
    ) -> Self {
        Self {
            created_at,
            stackly_version,
            protocol_version,
            recovery,
            doctor: Value::Null,
            system: Value::Null,
            instances: None,
            runtimes: None,
            operations: None,
            agent: Vec::new(),
            daemon_log: None,
            instance_logs: Vec::new(),
            notes: Vec::new(),
        }
    }
}

/// A bundle built in memory.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct BuiltBundle {
    /// The zip.
    pub bytes: Vec<u8>,
    /// Its sha256, lowercase hex.
    pub sha256: String,
    /// Its entries, in zip order.
    pub entries: Vec<BundleEntrySummary>,
    /// The bytes of all entries before compression.
    pub total_bytes: u64,
}

/// The zip could not be written.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("the bundle zip cannot be written: {0}")]
#[non_exhaustive]
pub struct BundleError(pub String);

/// The file name a bundle made at `created_at` suggests:
/// `stackly-diagnostics-<YYYYMMDD>T<HHMMSS>Z.zip`, in UTC.
#[must_use]
pub fn suggested_file_name(created_at: Timestamp) -> String {
    format!(
        "stackly-diagnostics-{}.zip",
        created_at.strftime("%Y%m%dT%H%M%SZ")
    )
}

/// One entry before it is written.
struct Staged {
    summary: BundleEntrySummary,
    bytes: Vec<u8>,
}

impl Staged {
    fn new(path: String, kind: BundleEntryKind, description: String, bytes: Vec<u8>) -> Self {
        Self {
            summary: BundleEntrySummary {
                path,
                kind,
                description,
                bytes: len_u64(bytes.len()),
                lines: None,
                truncated: false,
            },
            bytes,
        }
    }

    fn log(path: String, kind: BundleEntryKind, description: String, log: &LogText) -> Self {
        let mut text = log.lines.join("\n");
        if !text.is_empty() {
            text.push('\n');
        }
        let mut staged = Self::new(path, kind, description, text.into_bytes());
        staged.summary.lines = Some(len_u64(log.lines.len()));
        staged.summary.truncated = log.truncated;
        staged
    }
}

fn len_u64(len: usize) -> u64 {
    u64::try_from(len).unwrap_or(u64::MAX)
}

/// Builds the bundle of `input`: the entries through `redactor` (all but
/// the instance logs) and `home`, held to [`BUNDLE_BYTES_MAX`] in the order
/// of *Isi bundle*, with `README.txt` first.
///
/// # Errors
///
/// Returns [`BundleError`] when the zip writer fails, which only an
/// allocation failure could cause for a zip in memory.
pub fn build(
    input: BundleInput,
    redactor: &Redactor,
    home: &HomeMask,
) -> Result<BuiltBundle, BundleError> {
    let text = |value: &str| home.mask(&redactor.redact(value)).into_owned();
    let mut staged: Vec<Staged> = Vec::new();
    let mut left_out: Vec<String> = Vec::new();
    let mut notes = input.notes.clone();

    staged.push(Staged::new(
        "doctor.json".to_owned(),
        BundleEntryKind::Doctor,
        "The doctor report of this preview.".to_owned(),
        json(input.doctor, redactor, home),
    ));
    staged.push(Staged::new(
        "system.json".to_owned(),
        BundleEntryKind::System,
        "Stackly and protocol versions, macOS, the launch context, capacity, and the Stackly folders.".to_owned(),
        json(input.system, redactor, home),
    ));
    for (path, kind, description, value) in [
        (
            "instances.json",
            BundleEntryKind::Instances,
            "Every instance with its provider, runtime, state, last failure, blockers, endpoints, and settings; secrets appear only as references.",
            input.instances,
        ),
        (
            "runtimes.json",
            BundleEntryKind::Runtimes,
            "Every runtime with its software, version, source, state, digest, and folder.",
            input.runtimes,
        ),
        (
            "operations.json",
            BundleEntryKind::Operations,
            "The 100 newest operations and every one that needs recovery.",
            input.operations,
        ),
    ] {
        match value {
            Some(value) => staged.push(Staged::new(
                path.to_owned(),
                kind,
                description.to_owned(),
                json(value, redactor, home),
            )),
            None if input.recovery => notes.push(format!(
                "{path} is left out: the registry is in recovery mode, so Stackly reads nothing from it."
            )),
            None => notes.push(format!("{path} is left out: the registry could not be read.")),
        }
    }
    for file in input.agent {
        let description = match file.name.as_str() {
            "background-agent.json" => "The launch record of the background agent.".to_owned(),
            "daemon-start-failure.json" => {
                "The record of the last daemon start that failed.".to_owned()
            }
            name => format!("The agent file {name}."),
        };
        staged.push(Staged::new(
            format!("agent/{}", file.name),
            BundleEntryKind::Agent,
            description,
            text(&file.text).into_bytes(),
        ));
    }

    let mut used = README_RESERVE + staged.iter().map(|entry| entry.summary.bytes).sum::<u64>();
    let fits = |used: u64, bytes: u64| used.saturating_add(bytes) <= BUNDLE_BYTES_MAX;
    match input.daemon_log {
        Some(log) => {
            let log = LogText::new(
                log.lines.iter().map(|line| text(line)).collect(),
                log.truncated,
            );
            let entry = Staged::log(
                "logs/stacklyd.log".to_owned(),
                BundleEntryKind::DaemonLog,
                format!("The last {DAEMON_LOG_LINES_MAX} lines of the stacklyd log."),
                &log,
            );
            if fits(used, entry.summary.bytes) {
                used += entry.summary.bytes;
                staged.push(entry);
            } else {
                left_out.push("logs/stacklyd.log (over the 64 MiB limit)".to_owned());
            }
        }
        None => notes
            .push("logs/stacklyd.log is left out: the stacklyd log could not be read.".to_owned()),
    }

    let mut logs = input.instance_logs;
    logs.sort_by(|a, b| b.newest.cmp(&a.newest).then_with(|| a.name.cmp(&b.name)));
    for (index, log) in logs.into_iter().enumerate() {
        let path = format!("logs/instances/{}.log", log.name);
        if index >= INSTANCE_LOGS_MAX {
            left_out.push(format!(
                "{path} (past the first {INSTANCE_LOGS_MAX} instances)"
            ));
            continue;
        }
        let masked = LogText::new(
            log.log
                .lines
                .iter()
                .map(|line| home.mask(line).into_owned())
                .collect(),
            log.log.truncated,
        );
        let entry = Staged::log(
            path.clone(),
            BundleEntryKind::InstanceLog,
            format!(
                "The last {INSTANCE_LOG_ENTRIES_MAX} log entries of {}, every stream.",
                log.name
            ),
            &masked,
        );
        if fits(used, entry.summary.bytes) {
            used += entry.summary.bytes;
            staged.push(entry);
        } else {
            left_out.push(format!("{path} (over the 64 MiB limit)"));
        }
    }

    let readme = readme(
        &input.created_at,
        &input.stackly_version,
        &input.protocol_version,
        input.recovery,
        &staged,
        &left_out,
        &notes,
    );
    let mut readme = text(&readme).into_bytes();
    readme.truncate(usize::try_from(README_RESERVE).unwrap_or(usize::MAX));
    staged.insert(
        0,
        Staged::new(
            "README.txt".to_owned(),
            BundleEntryKind::Readme,
            "What this bundle holds and what it leaves out.".to_owned(),
            readme,
        ),
    );

    let bytes = write_zip(&staged, input.created_at)?;
    let sha256 = hex(&Sha256::digest(&bytes));
    let total_bytes = staged.iter().map(|entry| entry.summary.bytes).sum();
    Ok(BuiltBundle {
        bytes,
        sha256,
        entries: staged.into_iter().map(|entry| entry.summary).collect(),
        total_bytes,
    })
}

/// A JSON entry: every string value through `redactor` and `home`, then
/// pretty JSON with a final newline.
fn json(mut value: Value, redactor: &Redactor, home: &HomeMask) -> Vec<u8> {
    clean(&mut value, redactor, home);
    let mut bytes =
        serde_json::to_vec_pretty(&value).expect("a JSON value with string keys always serializes");
    bytes.push(b'\n');
    bytes
}

/// Redacts and masks every string of `value` in place.
fn clean(value: &mut Value, redactor: &Redactor, home: &HomeMask) {
    match value {
        Value::String(text) => {
            let cleaned = home.mask(&redactor.redact(text)).into_owned();
            *text = cleaned;
        }
        Value::Array(items) => items
            .iter_mut()
            .for_each(|item| clean(item, redactor, home)),
        Value::Object(map) => map
            .values_mut()
            .for_each(|item| clean(item, redactor, home)),
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

/// The text of `README.txt`.
fn readme(
    created_at: &Timestamp,
    stackly_version: &str,
    protocol_version: &str,
    recovery: bool,
    staged: &[Staged],
    left_out: &[String],
    notes: &[String],
) -> String {
    let mut out = String::new();
    out.push_str("Stackly diagnostic bundle\n\n");
    out.push_str(&format!("Created: {created_at}\n"));
    out.push_str(&format!(
        "Stackly {stackly_version} · protocol {protocol_version}\n"
    ));
    if recovery {
        out.push_str("The registry is in recovery mode.\n");
    }
    out.push_str("\nEntries:\n");
    out.push_str("  README.txt: this file.\n");
    for entry in staged {
        let summary = &entry.summary;
        let mut facts = format!("{} bytes", summary.bytes);
        if let Some(lines) = summary.lines {
            facts.push_str(&format!(", {lines} lines"));
        }
        if summary.truncated {
            facts.push_str(", truncated");
        }
        out.push_str(&format!(
            "  {} ({facts}): {}\n",
            summary.path, summary.description
        ));
    }
    let truncated: Vec<&str> = staged
        .iter()
        .filter(|entry| entry.summary.truncated)
        .map(|entry| entry.summary.path.as_str())
        .collect();
    out.push_str("\nTruncated: ");
    if truncated.is_empty() {
        out.push_str("nothing.\n");
    } else {
        out.push_str(&truncated.join(", "));
        out.push_str(". Each keeps its newest lines.\n");
    }
    out.push_str("\nLeft out: ");
    if left_out.is_empty() && notes.is_empty() {
        out.push_str("nothing.\n");
    } else {
        out.push('\n');
        for line in left_out {
            out.push_str(&format!("  {line}\n"));
        }
        for note in notes {
            out.push_str(&format!("  {note}\n"));
        }
    }
    out.push_str(
        "\nNever included: rendered config files (they can hold secrets in plain text), data folders, the registry file, Keychain values, process environments, and the audit log.\n",
    );
    out.push_str(
        "Secrets Stackly knows and connection strings are redacted, paths in your home folder are written ~, and nothing is uploaded.\n",
    );
    out
}

/// Writes the entries as one zip, every entry deflated, mode 0600, at
/// `created_at`.
fn write_zip(staged: &[Staged], created_at: Timestamp) -> Result<Vec<u8>, BundleError> {
    let failed = |error: &dyn std::fmt::Display| BundleError(error.to_string());
    let civil = created_at.to_zoned(jiff::tz::TimeZone::UTC).datetime();
    let field = |value: i8| u8::try_from(value).unwrap_or(0);
    let when = DateTime::from_date_and_time(
        u16::try_from(civil.year()).unwrap_or(1980),
        field(civil.month()),
        field(civil.day()),
        field(civil.hour()),
        field(civil.minute()),
        field(civil.second()),
    )
    .unwrap_or_default();
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .last_modified_time(when)
        .unix_permissions(0o600);
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    for entry in staged {
        zip.start_file(entry.summary.path.as_str(), options)
            .map_err(|error| failed(&error))?;
        zip.write_all(&entry.bytes)
            .map_err(|error| failed(&error))?;
    }
    Ok(zip.finish().map_err(|error| failed(&error))?.into_inner())
}

/// Lowercase hex of `bytes`.
pub(crate) fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut out, byte| {
            let _ = write!(out, "{byte:02x}");
            out
        })
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use serde_json::json;

    use super::*;

    fn input() -> BundleInput {
        let mut input = BundleInput::new(
            "2026-11-10T10:00:00Z".parse().unwrap(),
            "0.1.0".to_owned(),
            "3.5".to_owned(),
            false,
        );
        input.doctor = json!({ "summary": "Port 5432 by /Users/ojie/bin/x" });
        input.system = json!({ "dataRoot": "/Users/ojie/Library/Application Support/Stackly" });
        input.instances = Some(json!([{ "name": "pg" }]));
        input.runtimes = Some(json!([]));
        input.operations = Some(json!([]));
        input.daemon_log = Some(LogText::new(
            vec!["started at /Users/ojie".to_owned()],
            false,
        ));
        input
    }

    fn home() -> HomeMask {
        HomeMask::new(Some(std::path::Path::new("/Users/ojie")), None)
    }

    fn entry_text(bundle: &BuiltBundle, path: &str) -> String {
        let mut archive = zip::ZipArchive::new(Cursor::new(bundle.bytes.clone())).unwrap();
        let mut file = archive.by_name(path).unwrap();
        let mut text = String::new();
        file.read_to_string(&mut text).unwrap();
        text
    }

    #[test]
    fn the_zip_lists_the_entries_in_budget_order_with_readme_first() {
        let mut input = input();
        input.agent = vec![AgentFile::new(
            "background-agent.json".to_owned(),
            "{}".to_owned(),
        )];
        input.instance_logs = vec![
            InstanceLogInput::new(
                "old".to_owned(),
                Some("2026-11-01T00:00:00Z".parse().unwrap()),
                LogText::new(vec!["a".to_owned()], false),
            ),
            InstanceLogInput::new(
                "new".to_owned(),
                Some("2026-11-09T00:00:00Z".parse().unwrap()),
                LogText::new(vec!["b".to_owned()], true),
            ),
        ];
        let bundle = build(input, &Redactor::new(), &home()).unwrap();
        let paths: Vec<_> = bundle
            .entries
            .iter()
            .map(|entry| entry.path.as_str())
            .collect();
        assert_eq!(
            paths,
            [
                "README.txt",
                "doctor.json",
                "system.json",
                "instances.json",
                "runtimes.json",
                "operations.json",
                "agent/background-agent.json",
                "logs/stacklyd.log",
                "logs/instances/new.log",
                "logs/instances/old.log",
            ]
        );
        let new = &bundle.entries[8];
        assert_eq!((new.lines, new.truncated), (Some(1), true));
        assert_eq!(
            bundle.total_bytes,
            bundle.entries.iter().map(|entry| entry.bytes).sum::<u64>()
        );
        let archive = zip::ZipArchive::new(Cursor::new(bundle.bytes.clone())).unwrap();
        assert_eq!(archive.len(), 10);
        assert_eq!(hex(&Sha256::digest(&bundle.bytes)), bundle.sha256);
    }

    #[test]
    fn the_home_folder_is_masked_and_secrets_are_redacted_in_every_entry() {
        let redactor = Redactor::new();
        redactor.learn(&stackly_platform::SecretValue::new("s3cr\"et-7".to_owned()).unwrap());
        let mut input = input();
        input.doctor = json!({ "explanation": "token s3cr\"et-7 at /Users/ojie/x" });
        input.instance_logs = vec![InstanceLogInput::new(
            "pg".to_owned(),
            None,
            LogText::new(
                vec!["2026-11-10T10:00:00Z stdout /Users/ojie/pg".to_owned()],
                false,
            ),
        )];
        let bundle = build(input, &redactor, &home()).unwrap();
        let doctor = entry_text(&bundle, "doctor.json");
        assert!(!doctor.contains("s3cr"), "{doctor}");
        assert!(!doctor.contains("/Users/ojie"), "{doctor}");
        assert!(doctor.contains("~/x"), "{doctor}");
        assert!(
            entry_text(&bundle, "system.json").contains("~/Library/Application Support/Stackly")
        );
        assert_eq!(entry_text(&bundle, "logs/stacklyd.log"), "started at ~\n");
        assert_eq!(
            entry_text(&bundle, "logs/instances/pg.log"),
            "2026-11-10T10:00:00Z stdout ~/pg\n"
        );
    }

    #[test]
    fn the_same_input_gives_the_same_bytes() {
        let a = build(input(), &Redactor::new(), &home()).unwrap();
        let b = build(input(), &Redactor::new(), &home()).unwrap();
        assert_eq!(a.bytes, b.bytes);
        assert_eq!(a.sha256, b.sha256);
    }

    #[test]
    fn logs_past_fifty_instances_are_left_out_and_named_in_the_readme() {
        let mut input = input();
        input.instance_logs = (0..52)
            .map(|index| {
                InstanceLogInput::new(
                    format!("i{index:02}"),
                    Some(Timestamp::from_second(1_800_000_000 + i64::from(index)).unwrap()),
                    LogText::new(vec!["x".to_owned()], false),
                )
            })
            .collect();
        let bundle = build(input, &Redactor::new(), &home()).unwrap();
        let logs = bundle
            .entries
            .iter()
            .filter(|entry| entry.kind == BundleEntryKind::InstanceLog)
            .count();
        assert_eq!(logs, INSTANCE_LOGS_MAX);
        let readme = entry_text(&bundle, "README.txt");
        assert!(
            readme.contains("logs/instances/i00.log (past the first 50 instances)"),
            "{readme}"
        );
        assert!(
            readme.contains("logs/instances/i01.log (past the first 50 instances)"),
            "{readme}"
        );
        assert!(!readme.contains("i51.log (past"), "{readme}");
    }

    #[test]
    fn a_log_over_the_budget_is_left_out_and_json_entries_never_are() {
        let mut input = input();
        let big = "y".repeat(1024 * 1024);
        input.instances = Some(Value::String("z".repeat(63 * 1024 * 1024)));
        input.instance_logs = vec![InstanceLogInput::new(
            "pg".to_owned(),
            None,
            LogText::new(vec![big.clone(), big], false),
        )];
        let bundle = build(input, &Redactor::new(), &home()).unwrap();
        assert!(
            bundle
                .entries
                .iter()
                .any(|entry| entry.path == "instances.json")
        );
        assert!(
            !bundle
                .entries
                .iter()
                .any(|entry| entry.path == "logs/instances/pg.log")
        );
        assert!(
            entry_text(&bundle, "README.txt")
                .contains("logs/instances/pg.log (over the 64 MiB limit)")
        );
    }

    #[test]
    fn registry_entries_in_recovery_mode_become_a_readme_note() {
        let mut input = input();
        input.recovery = true;
        input.instances = None;
        input.runtimes = None;
        input.operations = None;
        let bundle = build(input, &Redactor::new(), &home()).unwrap();
        assert!(
            !bundle
                .entries
                .iter()
                .any(|entry| entry.kind == BundleEntryKind::Instances)
        );
        let readme = entry_text(&bundle, "README.txt");
        assert!(
            readme.contains("instances.json is left out: the registry is in recovery mode"),
            "{readme}"
        );
        assert!(readme.contains("The registry is in recovery mode."));
    }

    #[test]
    fn the_suggested_file_name_is_the_utc_time_of_the_preview() {
        assert_eq!(
            suggested_file_name("2026-11-10T10:05:09.5Z".parse().unwrap()),
            "stackly-diagnostics-20261110T100509Z.zip"
        );
    }
}
