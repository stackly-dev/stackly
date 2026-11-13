//! What `env.get` and `env.list` add for Up (spec 0046 AC-16): the
//! operation of an environment that has not ended, the last Up report
//! (from the daemon's memory while it runs, from the registry afterwards),
//! and the reasons a service is not ready (*Alasan belum ready*).

use stackly_core::environment::up::LiveUp;
use stackly_core::operation::OperationEngine;
use stackly_domain::{
    Environment, InstanceBlocker, InstanceId, OperationKind, OperationQuery, OperationResource,
    ServiceReport, UpReport, UpState as DomainState,
};
use stackly_protocol::Timestamp;
use stackly_protocol::env::{
    DependencyFailure, EnvOperationRef, EnvUpReport, NotReadyReason, ServiceNotReady,
    ServiceUpState, UpPhase, UpStage, UpState,
};
use stackly_protocol::secret::{NeedsSecretItem, NeedsSecretReason};

use crate::ipc::operation::dto::by_code;
use crate::state::DaemonCore;

/// What an environment's Up looks like now.
#[derive(Debug, Clone, Default)]
pub(crate) struct UpNow {
    /// Its `environment_up` or `environment_down` that has not ended.
    pub active: Option<EnvOperationRef>,
    /// The last Up report.
    pub last_up: Option<EnvUpReport>,
}

/// Every active operation on an environment, one engine read for a list.
pub(crate) async fn active_operations(
    engine: Option<&OperationEngine>,
    resource: Option<OperationResource>,
) -> Vec<(OperationResource, EnvOperationRef)> {
    let Some(engine) = engine else {
        return Vec::new();
    };
    let query = OperationQuery {
        active_only: true,
        resource,
        limit: OperationQuery::LIMIT_MAX,
        before: None,
    };
    let Ok(page) = engine.list(query).await else {
        return Vec::new();
    };
    page.items
        .iter()
        .filter(|view| {
            matches!(
                view.operation.kind(),
                OperationKind::EnvironmentUp | OperationKind::EnvironmentDown
            )
        })
        .map(|view| {
            (
                view.operation.resource(),
                EnvOperationRef {
                    id: view.operation.id().as_uuid(),
                    kind: by_code(view.operation.kind().as_code()),
                },
            )
        })
        .collect()
}

/// The Up of `environment` now, from the active operations of the engine.
pub(crate) fn up_now(
    core: &DaemonCore,
    environment: &Environment,
    active: &[(OperationResource, EnvOperationRef)],
) -> UpNow {
    let active = active
        .iter()
        .find(|(resource, _)| *resource == OperationResource::Environment(environment.id()))
        .map(|(_, reference)| reference.clone());
    let live = core.up_board().get(environment.id()).filter(|live| {
        active
            .as_ref()
            .is_some_and(|active| active.id == live.operation.as_uuid())
    });
    let last_up = match live {
        Some(live) => Some(live_report(&live)),
        None => environment.last_up().map(|report| {
            let running_up = active
                .as_ref()
                .is_some_and(|active| active.id == report.operation().as_uuid());
            stored_report(report, running_up)
        }),
    };
    UpNow { active, last_up }
}

fn stage(service: &ServiceReport) -> Option<UpStage> {
    service.stage().map(|stage| by_code(stage.as_code()))
}

fn service_state(
    service: &ServiceReport,
    waiting_on: &[stackly_domain::MemberAlias],
) -> ServiceUpState {
    ServiceUpState {
        alias: service.alias().to_string(),
        phase: by_code::<UpPhase>(service.phase().as_code()),
        stage: stage(service),
        code: service.code().map(ToString::to_string),
        dependency: service.dependency().map(ToString::to_string),
        dependency_reason: service
            .reason()
            .map(|reason| by_code::<DependencyFailure>(reason.as_code())),
        waiting_on: waiting_on.iter().map(ToString::to_string).collect(),
        operation_id: service.operation().map(|id| id.as_uuid()),
        references: service
            .references()
            .iter()
            .map(|reference| NeedsSecretItem {
                reference: reference.to_string(),
                reason: NeedsSecretReason::Missing,
                generate_allowed: true,
            })
            .collect(),
    }
}

fn live_report(live: &LiveUp) -> EnvUpReport {
    EnvUpReport {
        operation_id: live.operation.as_uuid(),
        state: UpState::Running,
        started_at: Timestamp::from_jiff(live.started_at),
        finished_at: None,
        services: live
            .services
            .iter()
            .map(|service| service_state(&service.report, &service.waiting_on))
            .collect(),
    }
}

/// The report the registry holds; `running` without an active Up is
/// `interrupted` (AC-15).
fn stored_report(report: &UpReport, running_up: bool) -> EnvUpReport {
    let state = match report.state() {
        DomainState::Running if !running_up => UpState::Interrupted,
        other => by_code(other.as_code()),
    };
    EnvUpReport {
        operation_id: report.operation().as_uuid(),
        state,
        started_at: Timestamp::from_jiff(report.started_at()),
        finished_at: report.finished_at().map(Timestamp::from_jiff),
        services: report
            .services()
            .iter()
            .map(|service| service_state(service, &[]))
            .collect(),
    }
}

/// A reason a service is not ready, with nothing of an Up.
pub(crate) fn plain(reason: NotReadyReason, detail: Option<String>) -> ServiceNotReady {
    ServiceNotReady {
        reason,
        detail,
        phase: None,
        waiting_on: Vec::new(),
        references: Vec::new(),
        dependency: None,
        dependency_reason: None,
        stage: None,
        code: None,
        operation_id: None,
    }
}

/// *Alasan belum ready*: the first rule that matches wins. `base` is the
/// reason of spec 0044, `instance` the service's instance with its
/// lifecycle code and the supervisor's blocker.
pub(crate) fn not_ready(
    base: ServiceNotReady,
    alias: &str,
    instance: Option<(InstanceId, stackly_domain::InstanceLifecycle)>,
    blocker: Option<&InstanceBlocker>,
    last_up: Option<&EnvUpReport>,
) -> ServiceNotReady {
    let entry = last_up.and_then(|report| report.services.iter().find(|s| s.alias == alias));
    // 1. An Up runs and has not finished this service.
    if let (Some(report), Some(entry)) = (last_up, entry)
        && report.state == UpState::Running
        && !matches!(
            entry.phase,
            UpPhase::Ready
                | UpPhase::Failed
                | UpPhase::NeedsSecret
                | UpPhase::Skipped
                | UpPhase::Cancelled
        )
    {
        return ServiceNotReady {
            reason: NotReadyReason::BringingUp,
            phase: Some(entry.phase),
            waiting_on: entry.waiting_on.clone(),
            ..plain(NotReadyReason::BringingUp, None)
        };
    }
    // 2. The instance has a `needsSecret` blocker.
    if let Some(InstanceBlocker::NeedsSecret { references, .. }) = blocker {
        return ServiceNotReady {
            references: references
                .iter()
                .map(crate::ipc::secret::needs_item)
                .collect(),
            ..plain(NotReadyReason::NeedsSecret, None)
        };
    }
    // 3. The instance lives: the reason of spec 0044.
    if let Some((_, lifecycle)) = instance
        && !matches!(
            lifecycle,
            stackly_domain::InstanceLifecycle::Stopped
                | stackly_domain::InstanceLifecycle::Uninitialized
        )
    {
        return base;
    }
    // 4 to 6. The last Up explains a service that is quiet now.
    if let (Some(report), Some(entry)) = (last_up, entry)
        && report.state != UpState::Running
    {
        match entry.phase {
            UpPhase::NeedsSecret => {
                return ServiceNotReady {
                    references: entry.references.clone(),
                    ..plain(NotReadyReason::NeedsSecret, None)
                };
            }
            UpPhase::Skipped => {
                return ServiceNotReady {
                    dependency: entry.dependency.clone(),
                    dependency_reason: entry.dependency_reason,
                    ..plain(NotReadyReason::DependencyNotReady, None)
                };
            }
            UpPhase::Failed => {
                return ServiceNotReady {
                    stage: entry.stage,
                    code: entry.code.clone(),
                    operation_id: entry.operation_id,
                    ..plain(NotReadyReason::UpFailed, None)
                };
            }
            _ => {}
        }
    }
    // 7. Stopped or not created.
    base
}
