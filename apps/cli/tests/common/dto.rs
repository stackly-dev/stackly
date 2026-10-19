//! Fixed DTOs for the scripted daemon, so text and goldens never depend on
//! this machine.

use stackly_protocol::daemon::{
    DaemonCounters, DaemonHealth, DaemonHealthStatus, DaemonInfo, DaemonRoots, RegistryRecovery,
    SubsystemHealth, SubsystemStatus,
};
use stackly_protocol::{CONTRACT_DIGEST, ProtocolVersion, Timestamp};

use super::scripted::SERVER_INSTANCE;

/// When the scripted daemon started.
pub const STARTED_AT: &str = "2026-10-18T03:04:05Z";

/// `daemon.get_info` of the scripted daemon.
pub fn info(root_override: bool) -> DaemonInfo {
    DaemonInfo {
        version: "0.1.0".to_owned(),
        protocol: ProtocolVersion::CURRENT,
        contract_digest: CONTRACT_DIGEST.to_owned(),
        server_instance_id: SERVER_INSTANCE,
        pid: 4242,
        uid: Some(501),
        started_at: STARTED_AT
            .parse::<Timestamp>()
            .expect("a fixed time parses"),
        roots: DaemonRoots {
            data: "/stackly/data".to_owned(),
            logs: "/stackly/logs".to_owned(),
            cache: "/stackly/cache".to_owned(),
            runtime: "/stackly/run".to_owned(),
            root_override,
        },
        socket_path: "/stackly/run/stacklyd.sock".to_owned(),
        registry_schema: Some(2),
    }
}

/// `daemon.get_health` with `status`; every subsystem `ok` but the
/// supervisor, which is `not_built`.
pub fn health(status: DaemonHealthStatus) -> DaemonHealth {
    let subsystem = |name: &str, status: SubsystemStatus, detail: Option<&str>| SubsystemHealth {
        name: name.to_owned(),
        status,
        detail: detail.map(str::to_owned),
    };
    DaemonHealth {
        status,
        subsystems: vec![
            subsystem("ipc", SubsystemStatus::Ok, None),
            subsystem("registry", SubsystemStatus::Ok, None),
            subsystem("operations", SubsystemStatus::Ok, None),
            subsystem(
                "supervisor",
                SubsystemStatus::NotBuilt,
                Some("arrives with feature 10"),
            ),
        ],
        counters: DaemonCounters::default(),
        recovery: None,
    }
}

/// A degraded health in recovery mode: the registry schema is newer than
/// the app.
pub fn recovery_health() -> DaemonHealth {
    let mut health = health(DaemonHealthStatus::Degraded);
    health.subsystems[1] = SubsystemHealth {
        name: "registry".to_owned(),
        status: SubsystemStatus::Unavailable,
        detail: Some("the registry needs recovery (schema_newer_than_app)".to_owned()),
    };
    health.recovery = Some(RegistryRecovery {
        reason: "schema_newer_than_app".to_owned(),
        report_path: "/stackly/data/recovery/report.json".to_owned(),
        detected_at: STARTED_AT.parse().expect("a fixed time parses"),
        app_schema_version: 2,
        db_schema_version: Some(3),
    });
    health
}
