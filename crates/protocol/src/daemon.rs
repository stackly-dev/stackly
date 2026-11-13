//! Replies of `daemon.get_info`, `daemon.get_health`, and
//! `daemon.get_capabilities`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use typeshare::typeshare;
use uuid::Uuid;

use crate::error::FieldError;
use crate::json::{FieldCode, Validate};
use crate::restart::{RestartPlanView, is_plan_digest};
use crate::secret::SecretStoreStatus;
use crate::stop_all::StopAllResult;
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
    /// The open restart plan, or the last plan when it became final less
    /// than ten minutes ago, else null (spec 0030).
    #[serde(default)]
    pub restart: Option<Box<RestartPlanView>>,
    /// The login session of this daemon run; null for a daemon that launchd
    /// did not start, when the session could not be read, and from an older
    /// daemon (spec 0031).
    #[serde(default)]
    pub session: Option<DaemonSessionInfo>,
}

/// How this daemon run relates to the stored login sessions. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SessionRotationKind {
    /// No session row existed; only the row was written.
    First,
    /// A new login: the startup policy of every instance became desired
    /// state.
    New,
    /// The daemon came back in the same session.
    Continued,
    /// A value this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// How a login session ended. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SessionEndKind {
    /// Every instance stopped cleanly before the daemon exited.
    Clean,
    /// The daemon exited while an instance was still stopping.
    Interrupted,
    /// The next session found the row still open.
    Unknown,
    /// A value this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// The login session of a daemon that launchd started (spec 0031).
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DaemonSessionInfo {
    /// The session ID: the boot UUID and the audit session.
    pub id: String,
    /// When the session began, which is not when this daemon run began.
    pub started_at: Timestamp,
    /// What this run's rotation was.
    pub rotation: SessionRotationKind,
    /// How the session before this one ended, or null when there is none.
    #[serde(default)]
    pub previous_end: Option<SessionEndKind>,
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
///
/// A `restart` starts the `daemon_restart` operation (spec 0030), so it
/// needs the request's `idempotencyKey`; a `stop` does not.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ShutdownParams {
    /// Whether the daemon only stops or stops so launchd starts it again.
    pub mode: ShutdownMode,
    /// The `digest` of the `daemon.restart_preview` the person confirmed;
    /// a restart whose plan has another digest is refused with
    /// `daemon.restart_plan_changed`. Only read for `restart`.
    #[serde(default)]
    pub expected_plan_digest: Option<String>,
    /// Restart even when an operation that cannot resume still runs after
    /// the quiesce wait. Only read for `restart`.
    #[serde(default)]
    pub force: bool,
    /// Stop the instances an environment lease holds too, so the daemon
    /// can exit; their leases stay active (spec 0047 AC-17). Only read for
    /// `agent_off`.
    #[serde(default)]
    pub override_leases: bool,
}

impl ShutdownParams {
    /// Params for `mode` with no expected digest, no force, and no lease
    /// override.
    #[must_use]
    pub const fn new(mode: ShutdownMode) -> Self {
        Self {
            mode,
            expected_plan_digest: None,
            force: false,
            override_leases: false,
        }
    }
}

impl Validate for ShutdownParams {
    /// Refuses a mode this build does not know with `mode` and a digest
    /// that is not 64 lowercase hex digits with `expectedPlanDigest`
    /// (`request.invalid_params`).
    fn validate(&self) -> Vec<FieldError> {
        let mut fields = Vec::new();
        if self.mode == ShutdownMode::Unrecognized {
            fields.push(FieldError::new("mode", FieldCode::UnknownValue));
        }
        if self
            .expected_plan_digest
            .as_deref()
            .is_some_and(|digest| !is_plan_digest(digest))
        {
            fields.push(FieldError::new(
                "expectedPlanDigest",
                FieldCode::InvalidFormat,
            ));
        }
        fields
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
    /// Stop the instances the daemon can stop, then exit 0 only when none is
    /// left, so the client can unregister the background agent (spec 0031,
    /// *Matikan agent*). It needs the request's `idempotencyKey`.
    AgentOff,
    /// A mode this build does not know. A client never sends it.
    #[serde(other)]
    Unrecognized,
}

/// Reply of `daemon.shutdown`, queued before the first step of the stop
/// order so it is flushed with the connections.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ShutdownAccepted {
    /// The daemon run that accepted the request.
    pub server_instance_id: Uuid,
    /// The mode it accepted.
    pub mode: ShutdownMode,
    /// The restart plan it wrote, for `restart`; its progress follows in
    /// `daemon.restart_plan` events (spec 0030).
    #[serde(default)]
    pub plan_id: Option<Uuid>,
    /// Whether the daemon goes on to exit. Always true for `stop` and
    /// `restart`; for `agent_off` false when an instance could not be
    /// stopped, and the daemon stays up (spec 0031, *Matikan agent*).
    #[serde(default)]
    pub exiting: bool,
    /// What `agent_off` stopped, failed on, and skipped; null for `stop` and
    /// `restart`.
    #[serde(default)]
    pub stop_all: Option<StopAllResult>,
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
    /// The durable log store of the instances (feature 27); `degraded`
    /// while a write fails and for 60 seconds after the last failure.
    pub const INSTANCE_LOGS: &str = "instance_logs";
    /// The login session (feature 31). It is listed only while it is
    /// `degraded`, when the session of a launchd daemon cannot be read
    /// (`platform.session_unavailable`), so it is not in [`ALL`].
    pub const SESSION: &str = "session";
    /// Every name that is always listed, in the order `get_health` lists
    /// them.
    pub const ALL: [&str; 8] = [
        IPC,
        EVENT_BUS,
        LOG_SINK,
        REGISTRY,
        OPERATIONS,
        SUPERVISOR,
        CATALOG,
        INSTANCE_LOGS,
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
    /// `truncated` records the instance log store wrote (spec 0027).
    #[serde(default)]
    pub instance_log_truncations: Count,
    /// The bytes those `truncated` records stand for.
    #[serde(default)]
    pub instance_log_bytes_truncated: Count,
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
            assert!(ShutdownParams::new(mode).validate().is_empty());
        }
    }

    #[test]
    fn a_restart_digest_must_be_sixty_four_lowercase_hex_digits() {
        let mut params = ShutdownParams::new(ShutdownMode::Restart);
        params.expected_plan_digest = Some("ABC".to_owned());
        let fields = params.validate();
        assert_eq!(fields.len(), 1);
        assert_eq!(
            (fields[0].path(), fields[0].code()),
            ("expectedPlanDigest", "invalid_format")
        );
        params.expected_plan_digest = Some("0f".repeat(32));
        assert!(params.validate().is_empty());
    }

    #[test]
    fn shutdown_params_from_an_older_client_decode_with_defaults() {
        let params: ShutdownParams = serde_json::from_str(r#"{"mode":"restart"}"#).unwrap();
        assert_eq!(params, ShutdownParams::new(ShutdownMode::Restart));
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
