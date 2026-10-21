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
        secret_store: None,
        launch: None,
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

/// The operation most operation tests use.
pub const OPERATION: uuid::Uuid = uuid::Uuid::from_u128(0x0192_f3a0_7c1e_7b2a_9d4e_5f6a_7b8c_9d04);
/// The instance that operation works on.
pub const INSTANCE: uuid::Uuid = uuid::Uuid::from_u128(0x0192_f3a0_7c1e_7b2a_9d4e_5f6a_7b8c_9d05);

/// A `start_instance` summary of `id` in `state` at `revision`.
pub fn summary(
    id: uuid::Uuid,
    state: stackly_protocol::operation::OperationState,
    revision: u64,
) -> stackly_protocol::operation::OperationSummary {
    use stackly_protocol::operation::{
        OperationKind, OperationProgress, OperationSummary, OperationTriggerInfo, TriggerOrigin,
    };
    use stackly_protocol::{ResourceKind, ResourceRef, Revision};
    OperationSummary {
        id,
        kind: OperationKind::StartInstance,
        resource: Some(ResourceRef {
            kind: ResourceKind::Instance,
            id: INSTANCE.to_string(),
        }),
        state,
        stage: None,
        progress: OperationProgress::Indeterminate,
        waiting_on: None,
        cancel_requested_at: None,
        point_of_no_return_at: None,
        failure: None,
        recovery: None,
        result: None,
        trigger: OperationTriggerInfo {
            origin: TriggerOrigin::User,
            client: Some(stackly_protocol::handshake::ClientKind::Cli),
        },
        created_at: STARTED_AT.parse().expect("a fixed time parses"),
        started_at: None,
        updated_at: STARTED_AT.parse().expect("a fixed time parses"),
        revision: Revision::new(revision).expect("test revisions are small"),
    }
}

/// `summary` as an `operation.get` detail without steps.
pub fn detail(
    summary: stackly_protocol::operation::OperationSummary,
) -> stackly_protocol::operation::OperationDetail {
    stackly_protocol::operation::OperationDetail {
        operation: summary,
        steps: Vec::new(),
        created: Vec::new(),
    }
}

/// The error a failed or recovering operation carries.
pub fn operation_error() -> stackly_protocol::ErrorBody {
    super::scripted::error(
        "instance.start_failed",
        "failed",
        "the instance did not become ready",
    )
    .with_hint("check the instance log")
}

/// `summary` with the end state `state`, with or without its error.
pub fn ended(
    state: stackly_protocol::operation::OperationState,
    with_error: bool,
    revision: u64,
) -> stackly_protocol::operation::OperationSummary {
    use stackly_protocol::operation::{
        OperationFailureInfo, OperationState, RecoveryAction, RecoveryCause, RecoveryHint,
        RecoveryInfo,
    };
    let mut summary = summary(OPERATION, state, revision);
    match state {
        OperationState::Failed if with_error => {
            summary.failure = Some(OperationFailureInfo {
                reason: "step_failed".to_owned(),
                retryable: false,
                error: operation_error(),
            });
        }
        OperationState::NeedsRecovery => {
            summary.recovery = Some(RecoveryInfo {
                hint: RecoveryHint::Resume,
                cause: RecoveryCause::StepFailed,
                error: with_error.then(operation_error),
                actions: vec![RecoveryAction::Resume, RecoveryAction::Abandon],
            });
        }
        _ => {}
    }
    summary
}
