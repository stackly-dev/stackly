//! Replies of `daemon.get_info`, `daemon.get_health`, and
//! `daemon.get_capabilities`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use typeshare::typeshare;
use uuid::Uuid;

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
    pub status: HealthStatus,
    /// Each subsystem, by its fixed name (see [`subsystem`]).
    #[serde(default)]
    pub subsystems: Vec<SubsystemHealth>,
    /// Counters since daemon start.
    pub counters: DaemonCounters,
}

/// The overall daemon status. Open: newer daemons may add values.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HealthStatus {
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
    Unknown,
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
    Unknown,
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
    /// Every name, in the order `get_health` lists them.
    pub const ALL: [&str; 6] = [IPC, EVENT_BUS, LOG_SINK, REGISTRY, OPERATIONS, SUPERVISOR];
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
    fn unknown_statuses_decode_as_unknown() {
        let health: HealthStatus = serde_json::from_str("\"hibernating\"").unwrap();
        assert_eq!(health, HealthStatus::Unknown);
        let sub: SubsystemStatus = serde_json::from_str("\"paused\"").unwrap();
        assert_eq!(sub, SubsystemStatus::Unknown);
        assert_eq!(
            serde_json::to_string(&SubsystemStatus::NotBuilt).unwrap(),
            "\"not_built\""
        );
    }

    #[test]
    fn counters_encode_in_camel_case() {
        let json = serde_json::to_value(DaemonCounters::default()).unwrap();
        assert_eq!(json["logLinesDropped"], 0);
        assert_eq!(json["socketRebinds"], 0);
    }
}
