//! The DTOs of the `instance` area (spec 0010, *Status dan event*) built
//! from the registry row and the supervisor's status. A domain type never
//! reaches the wire.

use std::collections::BTreeMap;

use stackly_core::supervisor::{FAILURE_EXCERPT_LINE_CHARS, InstanceStatus, ProcessStatus};
use stackly_domain::{
    DesiredState, Endpoint, HealthState, IdentityCheck, IdentityCheckName as DomainCheckName,
    IdentityEvidence as DomainEvidence, Instance, InstanceBlocker as DomainBlocker, InstanceId,
    InstanceLifecycle as DomainLifecycle, LockState, PortRequest as DomainPortRequest,
    ProcessStart, ReadinessState, Transport,
};
use stackly_protocol::instance::{
    EndpointPortMode, EndpointStatus, EndpointTransport, IdentityBlocker, IdentityCheckEntry,
    IdentityCheckName, IdentityCheckResult, IdentityEvidence, InstanceBlocker, InstanceDesired,
    InstanceHealth, InstanceHealthChanged, InstanceHealthState, InstanceLastFailure,
    InstanceLifecycle, InstanceLockState, InstanceOwnership, InstanceProcess, InstanceReadiness,
    InstanceReadinessState, InstanceRestart, InstanceSummary, ProviderPin,
};
use stackly_protocol::{Sequence, Timestamp};

use crate::ipc::operation::dto::revision;
use crate::ipc::secret::needs_secret_blocker;

/// The snapshot of `instance` with the status the supervisor shows now.
/// `excerpt` is the failure excerpt, which only an instance with a
/// recorded failure carries.
///
/// `runtimeVersion` is the exact version that initialized the data of the
/// instance, which equals the version of its runtime record until a data
/// migration moves the instance to another runtime (feature 55).
#[must_use]
pub fn summary(
    instance: &Instance,
    status: &InstanceStatus,
    excerpt: Vec<String>,
    sockets: &BTreeMap<String, String>,
) -> InstanceSummary {
    let key = instance.provider_revision();
    InstanceSummary {
        id: instance.id().as_uuid(),
        name: instance.name().to_string(),
        display_name: instance.display_name().map(ToString::to_string),
        revision: revision(instance.revision()),
        software: instance.software().to_string(),
        provider: ProviderPin {
            name: key.provider().to_string(),
            revision: key.revision().to_string(),
            manifest_sha256: key.manifest_sha256().to_string(),
        },
        runtime_id: instance.runtime_id().as_uuid(),
        runtime_version: instance.data_version().to_string(),
        lifecycle: lifecycle(instance.lifecycle()),
        desired: desired(instance.desired()),
        readiness: readiness(&status.readiness),
        health: health(&status.health),
        status_seq: Sequence::saturating(status.status_seq),
        process: status.process.as_ref().map(process),
        last_failure: instance.last_failure().map(|code| InstanceLastFailure {
            code: code.as_str().to_owned(),
            // The values of the failure in memory are not kept by the
            // status yet; they are null here, as after a daemon restart.
            at: None,
            exit_code: None,
            signal: None,
            excerpt,
        }),
        restart: restart(status),
        blocker: status.blocker.as_ref().and_then(blocker),
        endpoints: instance
            .endpoints()
            .iter()
            .map(|endpoint| endpoint_status(endpoint, status.ports_final, sockets))
            .collect(),
        leftover_processes: status.leftover.iter().map(|pid| pid.get()).collect(),
        logs_unavailable: status.logs_unavailable,
        ownership: InstanceOwnership::Manual,
        initialized: Some(instance.data_initialized_at().is_some()),
    }
}

/// The content of `instance.health_changed` for `status`.
#[must_use]
pub fn health_changed(id: InstanceId, status: &InstanceStatus) -> InstanceHealthChanged {
    InstanceHealthChanged {
        instance_id: id.as_uuid(),
        status_seq: Sequence::saturating(status.status_seq),
        readiness: readiness(&status.readiness),
        health: health(&status.health),
        blocker: status.blocker.as_ref().and_then(blocker),
        restart: restart(status),
        process: status.process.as_ref().map(process),
        logs_unavailable: status.logs_unavailable,
    }
}

/// Cuts a failure excerpt line to the length the wire allows.
#[must_use]
pub fn excerpt_line(line: &str) -> String {
    line.chars().take(FAILURE_EXCERPT_LINE_CHARS).collect()
}

/// The lifecycle on the wire.
#[must_use]
pub const fn lifecycle(lifecycle: DomainLifecycle) -> InstanceLifecycle {
    match lifecycle {
        DomainLifecycle::Uninitialized => InstanceLifecycle::Uninitialized,
        DomainLifecycle::Initializing => InstanceLifecycle::Initializing,
        DomainLifecycle::Stopped => InstanceLifecycle::Stopped,
        DomainLifecycle::Starting => InstanceLifecycle::Starting,
        DomainLifecycle::Running => InstanceLifecycle::Running,
        DomainLifecycle::Stopping => InstanceLifecycle::Stopping,
        DomainLifecycle::Failed => InstanceLifecycle::Failed,
        DomainLifecycle::Recovering => InstanceLifecycle::Recovering,
        DomainLifecycle::Unknown => InstanceLifecycle::Unknown,
        DomainLifecycle::Retained => InstanceLifecycle::Retained,
    }
}

const fn desired(desired: DesiredState) -> InstanceDesired {
    match desired {
        DesiredState::Running => InstanceDesired::Running,
        DesiredState::Stopped => InstanceDesired::Stopped,
    }
}

fn readiness(readiness: &stackly_domain::Readiness) -> InstanceReadiness {
    InstanceReadiness {
        state: match readiness.state() {
            ReadinessState::Unknown => InstanceReadinessState::Unknown,
            ReadinessState::NotReady => InstanceReadinessState::NotReady,
            ReadinessState::Ready => InstanceReadinessState::Ready,
        },
        reason: readiness.reason().as_str().to_owned(),
        detail: readiness.detail().map(|detail| detail.as_str().to_owned()),
        observed_at: Timestamp::from_jiff(readiness.observed_at()),
    }
}

fn health(health: &stackly_domain::Health) -> InstanceHealth {
    InstanceHealth {
        state: match health.state() {
            HealthState::Unknown => InstanceHealthState::Unknown,
            HealthState::Healthy => InstanceHealthState::Healthy,
            HealthState::Degraded => InstanceHealthState::Degraded,
            HealthState::Unhealthy => InstanceHealthState::Unhealthy,
        },
        reason: health.reason().as_str().to_owned(),
        detail: health.detail().map(|detail| detail.as_str().to_owned()),
        observed_at: Timestamp::from_jiff(health.observed_at()),
    }
}

fn process(process: &ProcessStatus) -> InstanceProcess {
    InstanceProcess {
        pid: process.pid.get(),
        started_at: started_at(process.started_at),
        adopted: process.adopted,
        adoptable: process.adoptable,
    }
}

/// The wall clock time a process started at.
fn started_at(start: ProcessStart) -> Timestamp {
    let seconds = i64::try_from(start.seconds()).unwrap_or(i64::MAX);
    let nanos = i32::try_from(start.micros().saturating_mul(1000)).unwrap_or(0);
    Timestamp::from_jiff(
        jiff::Timestamp::new(seconds, nanos).unwrap_or(jiff::Timestamp::UNIX_EPOCH),
    )
}

fn restart(status: &InstanceStatus) -> InstanceRestart {
    let restart = &status.restart;
    InstanceRestart {
        attempts: restart.attempts,
        budget: restart.budget,
        window_seconds: u32::try_from(restart.window_seconds).unwrap_or(u32::MAX),
        next_attempt_at: restart.next_attempt_at.map(Timestamp::from_jiff),
        exhausted: restart.exhausted,
    }
}

/// The blocker on the wire: `identityUnproven` before `dataInUse` before
/// `needsSecret`, the one value the supervisor keeps.
fn blocker(blocker: &DomainBlocker) -> Option<InstanceBlocker> {
    match blocker {
        DomainBlocker::IdentityUnproven {
            evidence,
            observed_at,
        } => Some(InstanceBlocker::IdentityUnproven(IdentityBlocker {
            evidence: evidence_dto(evidence),
            observed_at: Timestamp::from_jiff(*observed_at),
        })),
        DomainBlocker::DataInUse {
            evidence,
            observed_at,
        } => Some(InstanceBlocker::DataInUse(IdentityBlocker {
            evidence: evidence_dto(evidence),
            observed_at: Timestamp::from_jiff(*observed_at),
        })),
        other => needs_secret_blocker(other).map(InstanceBlocker::NeedsSecret),
    }
}

/// The evidence of an identity check on the wire.
#[must_use]
pub fn evidence_dto(evidence: &DomainEvidence) -> IdentityEvidence {
    IdentityEvidence {
        checks: evidence
            .checks()
            .map(|(name, result)| IdentityCheckEntry {
                name: check_name(name),
                result: check_result(result),
            })
            .collect(),
        pid: evidence.pid().map(|pid| pid.get()),
        lock: evidence.lock().map(|lock| match lock {
            LockState::Free => InstanceLockState::Free,
            LockState::Held => InstanceLockState::Held,
        }),
        stopped: evidence.stopped(),
        error: evidence.error().map(|error| error.as_str().to_owned()),
    }
}

const fn check_name(name: DomainCheckName) -> IdentityCheckName {
    match name {
        DomainCheckName::BootMatches => IdentityCheckName::BootMatches,
        DomainCheckName::PidExists => IdentityCheckName::PidExists,
        DomainCheckName::StartMatches => IdentityCheckName::StartMatches,
        DomainCheckName::ExecutableMatches => IdentityCheckName::ExecutableMatches,
        DomainCheckName::UserMatches => IdentityCheckName::UserMatches,
        DomainCheckName::SessionMatches => IdentityCheckName::SessionMatches,
        DomainCheckName::HoldsLock => IdentityCheckName::HoldsLock,
    }
}

const fn check_result(result: IdentityCheck) -> IdentityCheckResult {
    match result {
        IdentityCheck::Passed => IdentityCheckResult::Passed,
        IdentityCheck::Failed => IdentityCheckResult::Failed,
        IdentityCheck::NotChecked => IdentityCheckResult::NotChecked,
    }
}

fn endpoint_status(
    endpoint: &Endpoint,
    ports_final: bool,
    sockets: &BTreeMap<String, String>,
) -> EndpointStatus {
    match endpoint {
        Endpoint::Ip(ip) => EndpointStatus {
            name: ip.name().to_string(),
            transport: match ip.transport() {
                Transport::Tcp => EndpointTransport::Tcp,
                Transport::Udp => EndpointTransport::Udp,
            },
            host: Some(ip.address().to_string()),
            port: ip.effective_port().map(|port| port.get()),
            port_mode: match ip.request() {
                DomainPortRequest::Fixed(_) => EndpointPortMode::Fixed,
                DomainPortRequest::Auto => EndpointPortMode::Auto,
            },
            // A port is final only after readiness succeeded for the
            // process that runs now (REQ-010).
            bound: ports_final && ip.effective_port().is_some(),
            socket_path: None,
        },
        Endpoint::Unix(unix) => EndpointStatus {
            name: unix.name().to_string(),
            transport: EndpointTransport::Unix,
            host: None,
            port: None,
            port_mode: EndpointPortMode::Fixed,
            bound: false,
            // Rendered from the pinned package with the effective ports, or
            // null while one of them is not known (spec 0020 AC-12).
            socket_path: sockets.get(unix.name().as_str()).cloned(),
        },
    }
}
