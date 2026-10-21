//! Replies of `daemon.get_info`, `daemon.get_health`, and
//! `daemon.get_capabilities`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use typeshare::typeshare;
use uuid::Uuid;

use crate::error::FieldError;
use crate::json::{FieldCode, Validate};
use crate::secret::SecretStoreStatus;
use crate::version::ProtocolVersion;
use crate::wire::{Count, Timestamp};

/// Reply of `daemon.get_info`: who the daemon is and where it keeps things.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DaemonInfo {
    /// The daemon's version.
    pub version: String,
    /// The protocol version the daemon speaks.
    pub protocol: ProtocolVersion,
    /// The daemon's contract digest.
    pub contract_digest: String,
    /// The UUID v7 of this daemon run.
    pub server_instance_id: Uuid,
    /// The daemon's process ID.
    pub pid: u32,
    /// The daemon's Unix user ID, or null on a platform without one.
    #[serde(default)]
    pub uid: Option<u32>,
    /// When this daemon run started.
    pub started_at: Timestamp,
    /// The four resolved roots.
    pub roots: DaemonRoots,
    /// The resolved path of the daemon socket.
    pub socket_path: String,
    /// The registry's schema version once the registry opened ready, or
    /// null while it opens, in recovery mode, or from an older daemon
    /// (spec 0005).
    #[serde(default)]
    pub registry_schema: Option<u32>,
    /// The secret store and the keychain chosen at start, or null from an
    /// older daemon (spec 0011).
    #[serde(default)]
    pub secret_store: Option<SecretStoreStatus>,
    /// Who started this daemon run, or null from an older daemon
    /// (spec 0015).
    #[serde(default)]
    pub launch: Option<DaemonLaunch>,
}

/// Who started the daemon and from which app bundle (spec 0015).
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DaemonLaunch {
    /// How the process was started.
    pub context: LaunchContextKind,
    /// The launchd job label for a `launchd` start, else null.
    #[serde(default)]
    pub label: Option<String>,
    /// The canonical app bundle folder the daemon runs from, or null when
    /// it runs outside a bundle, as lossy UTF-8 text.
    #[serde(default)]
    pub bundle_path: Option<String>,
}

/// How a daemon process was started. Open: newer daemons may add values.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LaunchContextKind {
    /// launchd started it as a LaunchAgent job.
    Launchd,
    /// Anything else: a terminal, a test, or an orphaned process.
    Manual,
    /// A context this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// Params of `daemon.shutdown`.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ShutdownParams {
    /// Whether the daemon only stops or stops so launchd starts it again.
    pub mode: ShutdownMode,
}

impl Validate for ShutdownParams {
    /// Refuses a mode this build does not know with `mode`
    /// (`request.invalid_params`).
    fn validate(&self) -> Vec<FieldError> {
        if self.mode == ShutdownMode::Unrecognized {
            vec![FieldError::new("mode", FieldCode::UnknownValue)]
        } else {
            Vec::new()
        }
    }
}

/// What `daemon.shutdown` asks for. Open: newer daemons may add values.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ShutdownMode {
    /// Stop and stay stopped; the process exits 0.
    Stop,
    /// Stop so launchd starts the daemon again; the process exits 75. Only
    /// a `launchd` daemon accepts it.
    Restart,
    /// A mode this build does not know. A client never sends it.
    #[serde(other)]
    Unrecognized,
}

/// Reply of `daemon.shutdown`, queued before the first step of the stop
/// order so it is flushed with the connections.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ShutdownAccepted {
    /// The daemon run that accepted the request.
    pub server_instance_id: Uuid,
    /// The mode it accepted.
    pub mode: ShutdownMode,
}

/// The daemon's resolved (not canonical) roots, as lossy UTF-8 text.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DaemonRoots {
    /// The data root.
    pub data: String,
    /// The logs root.
    pub logs: String,
    /// The cache root.
    pub cache: String,
    /// The runtime root, which holds the socket.
    pub runtime: String,
    /// Whether `STACKLY_ROOT` was set when the daemon started.
    pub root_override: bool,
}

/// Reply of `daemon.get_health`.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DaemonHealth {
    /// The overall status.
    pub status: DaemonHealthStatus,
    /// Each subsystem, by its fixed name (see [`subsystem`]).
    #[serde(default)]
    pub subsystems: Vec<SubsystemHealth>,
    /// Counters since daemon start.
    pub counters: DaemonCounters,
    /// The recovery report while the registry is in recovery mode, else
    /// null (spec 0005, spec 0006 AC-18).
    #[serde(default)]
    pub recovery: Option<RegistryRecovery>,
}

/// Why the registry is in recovery mode: the report `open_registry` wrote
/// (spec 0005). The daemon leaves the registry untouched until a restart
/// after it was repaired.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RegistryRecovery {
    /// The recovery reason code of spec 0005, such as
    /// `schema_newer_than_app`.
    pub reason: String,
    /// The recovery report file, as resolved lossy UTF-8 text.
    pub report_path: String,
    /// When the problem was detected.
    pub detected_at: Timestamp,
    /// The newest registry schema version this daemon knows.
    pub app_schema_version: u32,
    /// The registry's schema version, or null when it could not be read.
    #[serde(default)]
    pub db_schema_version: Option<u32>,
}

/// The overall daemon status. Open: newer daemons may add values. Named
/// apart from the health of an instance (spec 0004 and 0012).
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DaemonHealthStatus {
    /// The daemon is bound and running its start steps.
    Starting,
    /// Every built subsystem is `ok`.
    Ready,
    /// A built subsystem is `degraded` or `unavailable`.
    Degraded,
    /// The daemon is shutting down.
    Stopping,
    /// A status this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// The health of one subsystem.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SubsystemHealth {
    /// The fixed subsystem name (see [`subsystem`]).
    pub name: String,
    /// The subsystem's status.
    pub status: SubsystemStatus,
    /// One safe English sentence, such as `socket missing, retrying`.
    #[serde(default)]
    pub detail: Option<String>,
}

/// The status of one subsystem. Open: newer daemons may add values.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SubsystemStatus {
    /// Working.
    Ok,
    /// Working with losses, such as dropped log lines.
    Degraded,
    /// Not working.
    Unavailable,
    /// Not built yet in this release; it never lowers the overall status.
    NotBuilt,
    /// A status this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// The fixed subsystem names. Features 5, 7, and 10 take over `registry`,
/// `operations`, and `supervisor`, which stay `not_built` until then.
pub mod subsystem {
    /// The socket listener; `unavailable` while a rebind fails.
    pub const IPC: &str = "ipc";
    /// The event bus; always `ok`, losses show in the counters.
    pub const EVENT_BUS: &str = "event_bus";
    /// The log writer; `degraded` after a dropped line or failed write in
    /// the last 60 seconds.
    pub const LOG_SINK: &str = "log_sink";
    /// The registry (feature 5).
    pub const REGISTRY: &str = "registry";
    /// The operation engine (feature 7).
    pub const OPERATIONS: &str = "operations";
    /// The process supervisor (feature 10).
    pub const SUPERVISOR: &str = "supervisor";
    /// The provider catalog (feature 17); `unavailable` without a usable
    /// catalog, `degraded` while an entry failed validation.
    pub const CATALOG: &str = "catalog";
    /// Every name, in the order `get_health` lists them.
    pub const ALL: [&str; 7] = [
        IPC, EVENT_BUS, LOG_SINK, REGISTRY, OPERATIONS, SUPERVISOR, CATALOG,
    ];
}

/// Counters since daemon start.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DaemonCounters {
    /// Connections open now.
    pub connections_open: Count,
    /// Every accepted connection.
    pub connections_total: Count,
    /// Rejected peers, connections over the limit, and failed handshakes.
    pub connections_rejected: Count,
    /// Frames answered with a framing or payload error.
    pub frames_rejected: Count,
    /// Published events.
    pub events_published: Count,
    /// Events lagging subscribers lost; may exceed `eventsPublished`.
    pub events_dropped: Count,
    /// Log lines dropped because the queue was full or a write failed.
    pub log_lines_dropped: Count,
    /// Times the socket was bound again after it went missing.
    pub socket_rebinds: Count,
}

/// Reply of `daemon.get_capabilities`.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Capabilities {
    /// Each platform capability and whether this Mac supports it.
    #[serde(default)]
    pub platform: Vec<PlatformCapabilityStatus>,
    /// Every method the daemon knows (`Call::METHODS`).
    #[serde(default)]
    pub methods: Vec<String>,
    /// Every event kind the daemon knows (`EventKind::KINDS`).
    #[serde(default)]
    pub event_kinds: Vec<String>,
}

/// One platform capability (spec 0004) and its support.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PlatformCapabilityStatus {
    /// The capability code, such as `ipc_endpoint`.
    pub capability: String,
    /// Whether this platform supports it.
    pub supported: bool,
    /// Why not, when it is not supported.
    #[serde(default)]
    pub reason: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn foreign_statuses_decode_as_unrecognized() {
        let health: DaemonHealthStatus = serde_json::from_str("\"hibernating\"").unwrap();
        assert_eq!(health, DaemonHealthStatus::Unrecognized);
        let sub: SubsystemStatus = serde_json::from_str("\"paused\"").unwrap();
        assert_eq!(sub, SubsystemStatus::Unrecognized);
        assert_eq!(
            serde_json::to_string(&SubsystemStatus::NotBuilt).unwrap(),
            "\"not_built\""
        );
    }

    #[test]
    fn health_and_info_from_an_older_daemon_decode_without_the_new_fields() {
        let health: DaemonHealth = serde_json::from_value(serde_json::json!({
            "status": "ready",
            "counters": DaemonCounters::default(),
        }))
        .unwrap();
        assert_eq!(health.recovery, None);
        assert!(health.subsystems.is_empty());
    }

    #[test]
    fn recovery_encodes_in_camel_case_with_a_null_db_schema_version() {
        let recovery = RegistryRecovery {
            reason: "integrity_failed".to_owned(),
            report_path: "/data/recovery/report.json".to_owned(),
            detected_at: "2026-10-18T03:04:05Z".parse().unwrap(),
            app_schema_version: 2,
            db_schema_version: None,
        };
        let json = serde_json::to_value(&recovery).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "reason": "integrity_failed",
                "reportPath": "/data/recovery/report.json",
                "detectedAt": "2026-10-18T03:04:05.000Z",
                "appSchemaVersion": 2,
                "dbSchemaVersion": null,
            })
        );
    }

    #[test]
    fn counters_encode_in_camel_case() {
        let json = serde_json::to_value(DaemonCounters::default()).unwrap();
        assert_eq!(json["logLinesDropped"], 0);
        assert_eq!(json["socketRebinds"], 0);
    }

    #[test]
    fn an_unknown_shutdown_mode_decodes_as_unrecognized_and_fails_validation() {
        let params: ShutdownParams = serde_json::from_str(r#"{"mode":"pause"}"#).unwrap();
        assert_eq!(params.mode, ShutdownMode::Unrecognized);
        let fields = params.validate();
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].path(), "mode");
        assert_eq!(fields[0].code(), "unknown_value");
        for mode in [ShutdownMode::Stop, ShutdownMode::Restart] {
            assert!(ShutdownParams { mode }.validate().is_empty());
        }
    }

    #[test]
    fn a_launch_context_this_build_does_not_know_decodes_as_unrecognized() {
        let launch: DaemonLaunch =
            serde_json::from_str(r#"{"context":"systemd","label":null,"bundlePath":null}"#)
                .unwrap();
        assert_eq!(launch.context, LaunchContextKind::Unrecognized);
        let json = serde_json::to_value(DaemonLaunch {
            context: LaunchContextKind::Launchd,
            label: Some("com.ojiepermana.stackly.daemon".to_owned()),
            bundle_path: None,
        })
        .unwrap();
        assert_eq!(json["context"], "launchd");
        assert_eq!(json["bundlePath"], serde_json::Value::Null);
    }
}
