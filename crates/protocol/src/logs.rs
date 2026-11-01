//! The log methods of the `instance` area (spec 0027, *Method IPC*):
//! `instance.logs_read`, `instance.logs_follow`, `instance.logs_unfollow`,
//! and the `log` frame a follow pushes.
//!
//! Every entry comes from the durable log store and is redacted before it
//! was stored; nothing here carries a raw spool byte. A cursor is
//! `{storeId, seq}`: the `seq` rises per instance across both streams, and
//! a cursor of another store reads as the gap `store_reset`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use typeshare::typeshare;
use uuid::Uuid;

use crate::error::FieldError;
use crate::json::{FieldCode, Validate};
use crate::wire::{Count, LogSeq, Timestamp};

/// The entries a page holds when `limit` is left out.
pub const LOG_READ_LIMIT_DEFAULT: u32 = 200;
/// The fewest entries a page may ask for.
pub const LOG_READ_LIMIT_MIN: u32 = 1;
/// The most entries a page may ask for.
pub const LOG_READ_LIMIT_MAX: u32 = 1000;
/// The longest `contains`, in characters.
pub const LOG_FILTER_MAX_CHARS: usize = 256;

/// A position in one instance's log store.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LogCursor {
    /// The store the `seq` belongs to.
    pub store_id: Uuid,
    /// The record, which may no longer exist.
    pub seq: LogSeq,
}

/// The content of a `before` or `after` position.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LogAnchor {
    /// The cursor the page is read next to.
    pub cursor: LogCursor,
}

/// Where a page starts. Closed: an older daemon refuses the method before
/// it reads the params.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", content = "content", rename_all = "snake_case")]
pub enum LogPosition {
    /// The newest entries.
    #[default]
    Newest,
    /// The entries with a `seq` below the cursor, the closest ones.
    Before(LogAnchor),
    /// The entries with a `seq` above the cursor, the closest ones.
    After(LogAnchor),
}

/// An output stream of the main process. Open: newer daemons may add
/// values.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LogStream {
    /// Standard output.
    Stdout,
    /// Standard error.
    Stderr,
    /// A stream this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// How serious a line looks, from a keyword rule on its text. Open: newer
/// daemons may add values.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LogLevel {
    /// Nothing matched.
    Normal,
    /// A warning keyword matched.
    Warning,
    /// An error keyword matched.
    Error,
    /// A level this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// One entry of a page or a `log` frame.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    /// Unique for the instance and rising across both streams.
    pub seq: LogSeq,
    /// When the daemon read the line or saw the event, to the millisecond.
    pub at: Timestamp,
    /// The stream; null for a marker of the process run.
    #[serde(default)]
    pub stream: Option<LogStream>,
    /// What the entry holds.
    pub kind: LogEntryKind,
}

/// What a [`LogEntry`] holds. Open: newer daemons may add kinds, and a
/// kind this build does not know decodes as `unrecognized` whatever its
/// content.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(tag = "type", content = "content", rename_all = "snake_case")]
pub enum LogEntryKind {
    /// One piece of output, already redacted.
    Line(LogLineEntry),
    /// Output of the stream that was dropped unread.
    Truncated(LogTruncated),
    /// A new main process started.
    Started(LogStarted),
    /// The main process exited.
    Exited(LogExited),
    /// Output written while Stackly was not running could not be read.
    Unavailable,
    /// A kind this build does not know. The daemon never sends it.
    Unrecognized,
}

impl<'de> Deserialize<'de> for LogEntryKind {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Tagged {
            #[serde(rename = "type")]
            kind: String,
            #[serde(default)]
            content: Option<serde_json::Value>,
        }
        fn content<T: serde::de::DeserializeOwned, E: serde::de::Error>(
            value: Option<serde_json::Value>,
        ) -> Result<T, E> {
            serde_json::from_value(value.unwrap_or(serde_json::Value::Null)).map_err(E::custom)
        }
        let tagged = Tagged::deserialize(deserializer)?;
        Ok(match tagged.kind.as_str() {
            "line" => Self::Line(content(tagged.content)?),
            "truncated" => Self::Truncated(content(tagged.content)?),
            "started" => Self::Started(content(tagged.content)?),
            "exited" => Self::Exited(content(tagged.content)?),
            "unavailable" => Self::Unavailable,
            _ => Self::Unrecognized,
        })
    }
}

/// The content of a `line` entry.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LogLineEntry {
    /// The redacted text, without its line ending.
    pub text: String,
    /// The level.
    pub level: LogLevel,
    /// The process had not ended the line yet.
    #[serde(default)]
    pub partial: bool,
    /// The piece continues the line of the entry before it.
    #[serde(default)]
    pub continued: bool,
    /// The init step whose output this is, by its number.
    #[serde(default)]
    pub step: Option<String>,
}

/// The content of a `truncated` entry.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LogTruncated {
    /// How many bytes were dropped.
    pub bytes: Count,
}

/// The content of a `started` entry.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LogStarted {
    /// The PID of the new process.
    pub pid: u32,
}

/// The content of an `exited` entry: a code or a signal, or neither when
/// the status is not known.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LogExited {
    /// The exit code.
    #[serde(default)]
    pub code: Option<i32>,
    /// The signal that ended the process.
    #[serde(default)]
    pub signal: Option<i32>,
}

/// Why the entries after a cursor are not the next ones.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LogGap {
    /// The reason.
    pub reason: LogGapReason,
}

/// Why a reader missed entries. Open: newer daemons may add values.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LogGapReason {
    /// The entries at the cursor were rotated out.
    Rotated,
    /// The cursor belongs to another store of the instance.
    StoreReset,
    /// A reason this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// How much a store keeps.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LogRetention {
    /// The segment files one stream keeps.
    pub segments_per_stream: Count,
    /// The size at which a segment is closed.
    pub segment_max_bytes: Count,
}

/// The bytes a store holds per stream.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LogStreamBytes {
    /// The segments of stdout.
    pub stdout: Count,
    /// The segments of stderr.
    pub stderr: Count,
}

/// Params of `instance.logs_read`.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LogsReadParams {
    /// The instance, retained ones included.
    pub instance: Uuid,
    /// The streams; empty means both. No duplicates.
    #[serde(default)]
    pub streams: Vec<LogStream>,
    /// Keeps lines that hold this text, in any case: 1 to 256 characters
    /// without a line break.
    #[serde(default)]
    pub contains: Option<String>,
    /// Where the page starts; the newest entries when left out.
    #[serde(default)]
    pub position: Option<LogPosition>,
    /// The entries at most, 1 to 1000; 200 when left out.
    #[serde(default)]
    pub limit: Option<u32>,
}

impl Validate for LogsReadParams {
    fn validate(&self) -> Vec<FieldError> {
        let mut fields = filter_errors(&self.streams, self.contains.as_deref());
        if self
            .limit
            .is_some_and(|limit| !(LOG_READ_LIMIT_MIN..=LOG_READ_LIMIT_MAX).contains(&limit))
        {
            fields.push(FieldError::new("limit", FieldCode::OutOfRange));
        }
        fields
    }
}

/// Params of `instance.logs_follow`.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LogsFollowParams {
    /// The instance, retained ones included.
    pub instance: Uuid,
    /// The streams; empty means both. No duplicates.
    #[serde(default)]
    pub streams: Vec<LogStream>,
    /// Keeps lines that hold this text, in any case.
    #[serde(default)]
    pub contains: Option<String>,
    /// Starts after this cursor; after the current head when left out.
    #[serde(default)]
    pub after: Option<LogCursor>,
}

impl Validate for LogsFollowParams {
    fn validate(&self) -> Vec<FieldError> {
        filter_errors(&self.streams, self.contains.as_deref())
    }
}

/// The field errors of the shared filter: a `contains` that is empty, too
/// long, or holds a line break, and a stream listed twice or unknown.
fn filter_errors(streams: &[LogStream], contains: Option<&str>) -> Vec<FieldError> {
    let mut fields = Vec::new();
    if let Some(text) = contains {
        if text.is_empty() {
            fields.push(FieldError::new("contains", FieldCode::TooShort));
        } else if text.chars().count() > LOG_FILTER_MAX_CHARS {
            fields.push(FieldError::new("contains", FieldCode::TooLong));
        } else if text.contains(['\n', '\r']) {
            fields.push(FieldError::new("contains", FieldCode::InvalidFormat));
        }
    }
    let mut seen = Vec::new();
    for (index, stream) in streams.iter().enumerate() {
        if *stream == LogStream::Unrecognized {
            fields.push(FieldError::new(
                format!("streams[{index}]"),
                FieldCode::UnknownValue,
            ));
        } else if seen.contains(stream) {
            fields.push(FieldError::new("streams", FieldCode::InvalidValue));
            break;
        }
        seen.push(*stream);
    }
    fields
}

/// Params of `instance.logs_unfollow`.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LogsUnfollowParams {
    /// The follow to end.
    pub subscription_id: u32,
}

impl Validate for LogsUnfollowParams {}

/// Reply of `instance.logs_read`.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LogPage {
    /// The store; null before the instance wrote anything.
    #[serde(default)]
    pub store_id: Option<Uuid>,
    /// The entries, `seq` rising.
    #[serde(default)]
    pub entries: Vec<LogEntry>,
    /// The cursor of the page before; null when nothing older is left.
    #[serde(default)]
    pub earlier: Option<LogCursor>,
    /// The cursor of the page after; null when the page reached the head.
    #[serde(default)]
    pub later: Option<LogCursor>,
    /// The newest entry of the store.
    #[serde(default)]
    pub head: Option<LogCursor>,
    /// Applies before the entries.
    #[serde(default)]
    pub gap: Option<LogGap>,
    /// The read stopped after scanning its budget; the cursors continue
    /// from where it stopped.
    #[serde(default)]
    pub scan_limited: bool,
    /// How much the store keeps.
    pub retention: LogRetention,
    /// The bytes the store holds now.
    pub stored_bytes: LogStreamBytes,
    /// Whole lines of the store that were not records, met by this read.
    #[serde(default)]
    pub skipped_records: Count,
}

/// Reply of `instance.logs_follow`: sent before the first `log` frame.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LogsFollowing {
    /// The follow, for `instance.logs_unfollow` and the frames.
    pub subscription_id: u32,
    /// The store; null before the instance wrote anything.
    #[serde(default)]
    pub store_id: Option<Uuid>,
    /// The newest entry when the follow started.
    #[serde(default)]
    pub head: Option<LogCursor>,
}

/// A `log` frame: entries of one follow, a gap before them, or its end.
/// Never all three empty.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LogFrame {
    /// The follow the frame belongs to.
    pub subscription_id: u32,
    /// The store the entries belong to.
    #[serde(default)]
    pub store_id: Option<Uuid>,
    /// The new entries, `seq` rising.
    #[serde(default)]
    pub entries: Vec<LogEntry>,
    /// Applies before the entries.
    #[serde(default)]
    pub gap: Option<LogGap>,
    /// The follow ended; no frame follows.
    #[serde(default)]
    pub ended: Option<LogFollowEnd>,
}

/// Why a follow ended.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LogFollowEnd {
    /// The reason.
    pub reason: LogFollowEndReason,
}

/// Why a follow ended. Open: newer daemons may add values.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LogFollowEndReason {
    /// The instance was deleted.
    InstanceRemoved,
    /// A reason this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(json: serde_json::Value) -> LogsReadParams {
        serde_json::from_value(json).unwrap()
    }

    #[test]
    fn read_params_default_to_the_newest_page_of_both_streams() {
        let params =
            read(serde_json::json!({ "instance": "0192f2c4-7b8a-7cc0-8a51-1f0c5b0a7e01" }));

        assert!(params.streams.is_empty());
        assert_eq!(params.position, None);
        assert!(params.validate().is_empty());
    }

    #[test]
    fn a_bad_filter_or_limit_names_its_field() {
        let mut params =
            read(serde_json::json!({ "instance": "0192f2c4-7b8a-7cc0-8a51-1f0c5b0a7e01" }));
        params.contains = Some("a\nb".to_owned());
        params.limit = Some(0);
        params.streams = vec![LogStream::Stdout, LogStream::Stdout];

        let paths: Vec<String> = params
            .validate()
            .iter()
            .map(|field| field.path().to_owned())
            .collect();

        assert_eq!(paths, ["contains", "streams", "limit"]);
        params.contains = Some("x".repeat(LOG_FILTER_MAX_CHARS + 1));
        assert_eq!(params.validate()[0].code(), FieldCode::TooLong.as_str());
        params.contains = Some(String::new());
        assert_eq!(params.validate()[0].code(), FieldCode::TooShort.as_str());
    }

    #[test]
    fn a_position_and_an_entry_have_the_tagged_wire_shape() {
        let cursor = LogCursor {
            store_id: "0192f2c4-7b8a-7cc0-8a51-1f0c5b0a7e01".parse().unwrap(),
            seq: LogSeq::new(42).unwrap(),
        };
        let position = serde_json::to_value(LogPosition::Before(LogAnchor { cursor })).unwrap();
        assert_eq!(
            position,
            serde_json::json!({ "type": "before", "content": { "cursor": { "storeId": "0192f2c4-7b8a-7cc0-8a51-1f0c5b0a7e01", "seq": 42 } } })
        );

        let entry: LogEntry = serde_json::from_value(serde_json::json!({
            "seq": 7, "at": "2026-11-01T07:03:22.118Z", "stream": "stderr",
            "kind": { "type": "line", "content": { "text": "x", "level": "error" } }
        }))
        .unwrap();
        assert!(
            matches!(entry.kind, LogEntryKind::Line(ref line) if line.level == LogLevel::Error)
        );
    }

    #[test]
    fn unknown_values_from_a_newer_daemon_fall_back_to_unrecognized() {
        let entry: LogEntry = serde_json::from_value(serde_json::json!({
            "seq": 7, "at": "2026-11-01T07:03:22.118Z", "stream": "stdin",
            "kind": { "type": "rotated_in", "content": { "x": 1 } }
        }))
        .unwrap();
        assert_eq!(entry.stream, Some(LogStream::Unrecognized));
        assert_eq!(entry.kind, LogEntryKind::Unrecognized);
        let gap: LogGap = serde_json::from_value(serde_json::json!({ "reason": "later" })).unwrap();
        assert_eq!(gap.reason, LogGapReason::Unrecognized);
    }
}
