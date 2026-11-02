//! The DTOs of `instance.get_metrics` (spec 0028, *API surface*): the
//! samples of CPU, memory, and Disk I/O of a running instance, the process
//! list of the newest sample, and the facts the Metrics screen and `stackly
//! instance metrics` show next to them.
//!
//! A number that was not measured is a `Metric` with `type` `unavailable`
//! and a `reason`, never 0. Samples never travel as events: a client asks
//! with the `historyId` and `afterSeq` it holds and gets only what is new.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use typeshare::typeshare;
use uuid::Uuid;

use super::{InstanceHealth, InstanceLastFailure, InstanceLifecycle, InstanceReadiness};
use crate::error::FieldError;
use crate::json::{FieldCode, Validate};
use crate::wire::{Count, Sequence, Timestamp};

/// The longest `historyId` a request may carry, in bytes.
pub const METRICS_HISTORY_ID_MAX: usize = 64;

/// Params of `instance.get_metrics`.
///
/// Without `historyId`, or with one the daemon no longer holds, every
/// sample is sent and `historyReset` is true. With the current one, only
/// samples with a `seq` above `afterSeq` are sent.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GetMetricsParams {
    /// The instance to read.
    pub instance_id: Uuid,
    /// The `historyId` of the last reply the client holds. Opaque text.
    #[serde(default)]
    pub history_id: Option<String>,
    /// The `seq` of the newest sample the client holds in that history.
    #[serde(default)]
    pub after_seq: Option<Sequence>,
}

impl Validate for GetMetricsParams {
    /// `historyId` is 1 to [`METRICS_HISTORY_ID_MAX`] bytes, and
    /// `afterSeq` needs a `historyId` to belong to.
    fn validate(&self) -> Vec<FieldError> {
        let mut fields = Vec::new();
        if let Some(history_id) = &self.history_id {
            if history_id.is_empty() {
                fields.push(FieldError::new("historyId", FieldCode::TooShort));
            } else if history_id.len() > METRICS_HISTORY_ID_MAX {
                fields.push(FieldError::new("historyId", FieldCode::TooLong));
            }
        }
        if self.after_seq.is_some() && self.history_id.is_none() {
            fields.push(FieldError::new("afterSeq", FieldCode::Required));
        }
        fields
    }
}

/// Whether an instance has live metrics. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MetricsState {
    /// The instance is `running` with a verified main process.
    Live,
    /// Any other case; `lifecycle` says which.
    NotRunning,
    /// A state this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// Why a number has no value. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum UnavailableReason {
    /// The first sample of a process: there is nothing to take a
    /// difference from yet.
    WarmingUp,
    /// The OS refused to read the process.
    NotPermitted,
    /// The process exited while it was being sampled.
    ProcessExited,
    /// The platform cannot read it.
    NotSupported,
    /// The data folder was never scanned.
    NotScanned,
    /// A reason this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// Which processes a [`ResourceUsage`] covers. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum UsageScope {
    /// The main process alone.
    Main,
    /// The main process and every descendant of the same user.
    Tree,
    /// A scope this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// The content of an unavailable number or size.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UnavailableContent {
    /// Why there is no value.
    pub reason: UnavailableReason,
}

/// A measured fractional number such as a CPU percent, or why there is
/// none. Tagged `type` and `content`:
/// `{ "type": "value", "content": 12.5 }` or
/// `{ "type": "unavailable", "content": { "reason": "warming_up" } }`.
/// Open: a type this build does not know decodes as `unrecognized`.
///
/// The daemon only sends finite numbers, so `Eq` holds.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Serialize, JsonSchema)]
#[serde(tag = "type", content = "content", rename_all = "snake_case")]
pub enum NumberMetric {
    /// The measured number.
    Value(f64),
    /// No number, and why.
    Unavailable(UnavailableContent),
    /// A type this build does not know. The daemon never sends it.
    Unrecognized,
}

impl Eq for NumberMetric {}

/// A measured whole number such as bytes, or why there is none. Same shape
/// as [`NumberMetric`]. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(tag = "type", content = "content", rename_all = "snake_case")]
pub enum CountMetric {
    /// The measured number.
    Value(Count),
    /// No number, and why.
    Unavailable(UnavailableContent),
    /// A type this build does not know. The daemon never sends it.
    Unrecognized,
}

/// What an open tagged value holds before its `type` is looked at.
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

impl<'de> Deserialize<'de> for NumberMetric {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let tagged = Tagged::deserialize(deserializer)?;
        Ok(match tagged.kind.as_str() {
            "value" => Self::Value(content(tagged.content)?),
            "unavailable" => Self::Unavailable(content(tagged.content)?),
            _ => Self::Unrecognized,
        })
    }
}

impl<'de> Deserialize<'de> for CountMetric {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let tagged = Tagged::deserialize(deserializer)?;
        Ok(match tagged.kind.as_str() {
            "value" => Self::Value(content(tagged.content)?),
            "unavailable" => Self::Unavailable(content(tagged.content)?),
            _ => Self::Unrecognized,
        })
    }
}

/// The usage of the main process or of the whole tree in one sample.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResourceUsage {
    /// Which processes it covers.
    pub scope: UsageScope,
    /// The processes found: 1 for the main process.
    pub process_count: Count,
    /// The processes counted in CPU and Disk I/O.
    pub measured_count: Count,
    /// CPU as a percent of one logical core, to one decimal; a busy tree
    /// is over 100.
    pub cpu: NumberMetric,
    /// Physical footprint in bytes, the number Activity Monitor calls
    /// Memory.
    pub memory: CountMetric,
    /// Bytes read per second.
    pub disk_read: CountMetric,
    /// Bytes written per second.
    pub disk_write: CountMetric,
}

/// One sample of an instance.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MetricsSample {
    /// 1 for the first sample of a history, then one more per sample.
    pub seq: Sequence,
    /// When the sample was taken, from the daemon clock.
    pub sampled_at: Timestamp,
    /// The main process alone.
    pub main: ResourceUsage,
    /// The main process and its descendants.
    pub tree: ResourceUsage,
}

/// One process of the tree, in the newest sample.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProcessRow {
    /// The process ID.
    pub pid: u32,
    /// Its parent; null for the main process.
    #[serde(default)]
    pub parent_pid: Option<u32>,
    /// Its distance from the main process.
    pub depth: u32,
    /// The file name of its verified executable, never its arguments.
    pub name: String,
    /// True for the main process.
    pub is_main: bool,
    /// CPU as a percent of one logical core.
    pub cpu: NumberMetric,
    /// Physical footprint in bytes.
    pub memory: CountMetric,
}

/// The main process the history belongs to.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MetricsProcess {
    /// The process ID.
    pub pid: u32,
    /// When it started.
    pub started_at: Timestamp,
}

/// The content of a scanned size.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ScannedUsage {
    /// The allocated bytes of every entry counted.
    pub bytes: Count,
    /// False when the scan stopped at a limit, so `bytes` is a lower bound.
    pub complete: bool,
    /// When the scan finished.
    pub scanned_at: Timestamp,
}

/// The size of the data folder as the last scan left it, or why there is
/// none. Tagged `type` and `content`. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(tag = "type", content = "content", rename_all = "snake_case")]
pub enum DataUsageMetric {
    /// The result of the last scan.
    Scanned(ScannedUsage),
    /// No scan result; `reason` is `not_scanned` before the first scan.
    Unavailable(UnavailableContent),
    /// A type this build does not know. The daemon never sends it.
    Unrecognized,
}

impl<'de> Deserialize<'de> for DataUsageMetric {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let tagged = Tagged::deserialize(deserializer)?;
        Ok(match tagged.kind.as_str() {
            "scanned" => Self::Scanned(content(tagged.content)?),
            "unavailable" => Self::Unavailable(content(tagged.content)?),
            _ => Self::Unrecognized,
        })
    }
}

/// Reply of `instance.get_metrics`.
///
/// When `state` is `not_running`, `historyId`, `process`, `samples`, and
/// `latestProcesses` are empty and `historyReset` is true: the client drops
/// what it holds.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MetricsReply {
    /// The instance.
    pub instance_id: Uuid,
    /// Whether the instance has live metrics.
    pub state: MetricsState,
    /// The lifecycle of the instance now.
    pub lifecycle: InstanceLifecycle,
    /// The seconds between two samples.
    pub interval_seconds: u32,
    /// The most samples a history keeps.
    pub capacity: Count,
    /// Names the history of the live main process; null when not running.
    /// Opaque text a client sends back in its next request.
    #[serde(default)]
    pub history_id: Option<String>,
    /// The live main process; null when not running.
    #[serde(default)]
    pub process: Option<MetricsProcess>,
    /// The samples after the cursor, oldest first.
    pub samples: Vec<MetricsSample>,
    /// The process list of the newest sample, as a tree: children below
    /// their parent, siblings by PID. Empty when there is no sample yet.
    #[serde(default)]
    pub latest_processes: Vec<ProcessRow>,
    /// True when the tree had more processes than the list holds.
    #[serde(default)]
    pub processes_truncated: bool,
    /// True when the client must drop the samples it holds before it adds
    /// these.
    pub history_reset: bool,
    /// The restarts that followed a crash since the instance was created.
    pub restart_total: Count,
    /// The last failure, if there was one.
    #[serde(default)]
    pub last_failure: Option<InstanceLastFailure>,
    /// Readiness as read now.
    pub readiness: InstanceReadiness,
    /// Health as read now.
    pub health: InstanceHealth,
    /// The size of the data folder from the last scan; this method never
    /// starts a scan.
    pub data_usage: DataUsageMetric,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(history_id: Option<&str>, after_seq: Option<u64>) -> GetMetricsParams {
        GetMetricsParams {
            instance_id: Uuid::nil(),
            history_id: history_id.map(str::to_owned),
            after_seq: after_seq.and_then(Sequence::new),
        }
    }

    #[test]
    fn after_seq_without_a_history_id_is_required() {
        let fields = params(None, Some(3)).validate();

        assert_eq!(fields.len(), 1);
        assert_eq!(
            (fields[0].path(), fields[0].code()),
            ("afterSeq", "required")
        );
        assert!(params(None, None).validate().is_empty());
        assert!(params(Some("42-1.0"), Some(3)).validate().is_empty());
    }

    #[test]
    fn a_history_id_is_one_to_sixty_four_bytes() {
        let long = "x".repeat(METRICS_HISTORY_ID_MAX + 1);

        let empty = params(Some(""), None).validate();
        let too_long = params(Some(&long), None).validate();

        assert_eq!(empty[0].code(), "too_short");
        assert_eq!(too_long[0].code(), "too_long");
        assert!(
            params(Some(&"x".repeat(METRICS_HISTORY_ID_MAX)), None)
                .validate()
                .is_empty()
        );
    }

    #[test]
    fn a_metric_reads_both_known_types_and_an_unknown_one_as_unrecognized() {
        let value: NumberMetric =
            serde_json::from_str(r#"{"type":"value","content":12.5}"#).expect("a value");
        let missing: CountMetric =
            serde_json::from_str(r#"{"type":"unavailable","content":{"reason":"warming_up"}}"#)
                .expect("an unavailable");
        let newer: CountMetric =
            serde_json::from_str(r#"{"type":"estimated","content":{"x":1}}"#).expect("unknown");
        let reason: UnavailableContent =
            serde_json::from_str(r#"{"reason":"hibernating"}"#).expect("an unknown reason");

        assert_eq!(value, NumberMetric::Value(12.5));
        assert_eq!(
            missing,
            CountMetric::Unavailable(UnavailableContent {
                reason: UnavailableReason::WarmingUp
            })
        );
        assert_eq!(newer, CountMetric::Unrecognized);
        assert_eq!(reason.reason, UnavailableReason::Unrecognized);
    }

    #[test]
    fn a_metric_writes_the_tagged_shape_of_the_spec() {
        let written = serde_json::to_value(NumberMetric::Unavailable(UnavailableContent {
            reason: UnavailableReason::NotPermitted,
        }))
        .expect("encodes");

        assert_eq!(
            written,
            serde_json::json!({"type": "unavailable", "content": {"reason": "not_permitted"}})
        );
    }
}
