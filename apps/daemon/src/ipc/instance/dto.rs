//! The DTOs of the `instance` area (spec 0010, *Status dan event*) built
//! from the registry row and the supervisor's status. A domain type never
//! reaches the wire.

use std::collections::BTreeMap;

use stackly_core::supervisor::{FAILURE_EXCERPT_LINE_CHARS, InstanceStatus, ProcessStatus};
use stackly_domain::{
    Capability, DesiredState, Endpoint, ExitKind, HealthState, IdentityCheck,
    IdentityCheckName as DomainCheckName, IdentityEvidence as DomainEvidence, Instance,
    InstanceBlocker as DomainBlocker, InstanceId, InstanceLifecycle as DomainLifecycle, LockState,
    PortRequest as DomainPortRequest, ProcessStart, ReadinessState, Transport,
};
use stackly_protocol::instance::{
    AutostartSession, InstanceAutostart, InstanceCapabilities, InstanceLastExit,
};
use stackly_protocol::instance::{
    ConfigDriftBlocker, EndpointPortMode, EndpointStatus, EndpointTransport, IdentityBlocker,
    IdentityCheckEntry, IdentityCheckName, IdentityCheckResult, IdentityEvidence, InstanceBlocker,
    InstanceDesired, InstanceHealth, InstanceHealthChanged, InstanceHealthState,
    InstanceLastFailure, InstanceLifecycle, InstanceLockState, InstanceOwnership, InstanceProcess,
    InstanceReadiness, InstanceReadinessState, InstanceRestart, InstanceSummary, ProviderPin,
};
use stackly_protocol::{Count, Sequence, Timestamp};

use crate::ipc::operation::dto::revision;
use crate::ipc::secret::needs_secret_blocker;
use crate::map::detail::startup_kind;

/// The wire value of how the process last ended.
const fn last_exit(exit: ExitKind) -> InstanceLastExit {
    match exit {
        ExitKind::Clean => InstanceLastExit::Clean,
        ExitKind::Forced => InstanceLastExit::Forced,
        ExitKind::Crashed => InstanceLastExit::Crashed,
    }
}

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
    autostart: AutostartSession,
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
        lifecycle: shown_lifecycle(instance.lifecycle(), status),
        desired: desired(instance.desired()),
        readiness: readiness(&status.readiness),
        health: health(&status.health),
        status_seq: Sequence::saturating(status.status_seq),
        process: status.process.as_ref().map(process),
        last_failure: instance
            .last_failure()
            .map(|code| last_failure(code, status, excerpt)),
        restart: restart(status),
        restart_total: Count::saturating(instance.restart_total()),
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
        capabilities: InstanceCapabilities::new(
            instance.capabilities().contains(Capability::Reload),
        ),
        last_exit: instance.last_exit().map(last_exit),
        autostart: InstanceAutostart {
            policy: startup_kind(instance.startup()),
            session: autostart,
        },
        // provisional: F44 step 5 fills ownership and environments from the
        // membership cache.
        environments: Vec::new(),
    }
}

/// The stored failure `code` with the facts this daemon run recorded for
/// it (spec 0033, AC-3): its time and exit only while the facts name the
/// same code, so after a daemon restart, or while the registry and the
/// actor disagree for a moment, all three are null.
fn last_failure(
    code: &stackly_domain::FailureCode,
    status: &InstanceStatus,
    excerpt: Vec<String>,
) -> InstanceLastFailure {
    let facts = status
        .last_failure
        .as_ref()
        .filter(|facts| &facts.code == code);
    InstanceLastFailure {
        code: code.as_str().to_owned(),
        at: facts.map(|facts| Timestamp::from_jiff(facts.at)),
        exit_code: facts.and_then(|facts| facts.exit_code),
        signal: facts.and_then(|facts| facts.signal),
        excerpt,
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

/// The lifecycle a snapshot shows: a live lifecycle reads `unknown` while
/// no process runs and the supervisor gave up recording how the last one
/// ended, because the registry may still hold the state from before the
/// exit (spec 0010, *Commit*, audit note 100).
#[must_use]
pub const fn shown_lifecycle(
    stored: DomainLifecycle,
    status: &InstanceStatus,
) -> InstanceLifecycle {
    let live = matches!(
        stored,
        DomainLifecycle::Starting
            | DomainLifecycle::Running
            | DomainLifecycle::Recovering
            | DomainLifecycle::Stopping
    );
    if live && status.exit_unrecorded {
        InstanceLifecycle::Unknown
    } else {
        lifecycle(stored)
    }
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

/// The probe latency in whole milliseconds, rounded up so a quick probe
/// never reads as zero. The supervisor already cut it to the probe timeout.
fn latency_ms(latency: Option<std::time::Duration>) -> Option<u32> {
    latency.map(|latency| {
        let millis = latency.as_millis() + u128::from(latency.subsec_nanos() % 1_000_000 != 0);
        u32::try_from(millis).unwrap_or(u32::MAX)
    })
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
        latency_ms: latency_ms(readiness.latency()),
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
        latency_ms: latency_ms(health.latency()),
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
/// `needsSecret` before `configDrift`, the one value the supervisor keeps.
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
        DomainBlocker::ConfigDrift { files, observed_at } => {
            Some(InstanceBlocker::ConfigDrift(ConfigDriftBlocker {
                files: files.iter().map(ToString::to_string).collect(),
                observed_at: Timestamp::from_jiff(*observed_at),
            }))
        }
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

#[cfg(test)]
mod latency_tests {
    use std::time::Duration;

    use super::latency_ms;

    #[test]
    fn no_probe_means_no_latency() {
        assert_eq!(latency_ms(None), None);
    }

    #[test]
    fn a_latency_rounds_up_to_whole_milliseconds() {
        assert_eq!(latency_ms(Some(Duration::from_micros(1))), Some(1));
        assert_eq!(latency_ms(Some(Duration::from_micros(1500))), Some(2));
        assert_eq!(latency_ms(Some(Duration::from_millis(7))), Some(7));
        assert_eq!(latency_ms(Some(Duration::ZERO)), Some(0));
    }

    #[test]
    fn a_latency_too_large_for_the_wire_saturates() {
        assert_eq!(
            latency_ms(Some(Duration::from_secs(u64::MAX / 2))),
            Some(u32::MAX)
        );
    }
}

#[cfg(test)]
mod tests {
    use stackly_core::supervisor::RestartStatus;
    use stackly_domain::{Health, HealthState, Readiness, ReadinessState, ReasonCode};

    use super::*;

    fn status(exit_unrecorded: bool) -> InstanceStatus {
        let at: jiff::Timestamp = "2026-11-05T00:00:00Z".parse().unwrap();
        InstanceStatus {
            status_seq: 1,
            readiness: Readiness::new(ReadinessState::Unknown, at, ReasonCode::NOT_RUNNING, None),
            health: Health::new(HealthState::Unknown, at, ReasonCode::NOT_RUNNING, None),
            process: None,
            restart: RestartStatus::default(),
            blocker: None,
            leftover: Vec::new(),
            ports_final: false,
            logs_unavailable: false,
            pending_commit: false,
            exit_unrecorded,
            last_failure: None,
        }
    }

    #[test]
    fn failure_facts_fill_the_last_failure_only_while_their_code_is_the_stored_one() {
        // covers: spec 0033 AC-3 (facts of the same code, null otherwise)
        use stackly_core::supervisor::FailureFacts;
        use stackly_domain::FailureCode;
        use stackly_platform::ExitStatus;

        let at: jiff::Timestamp = "2026-11-10T08:00:00Z".parse().unwrap();
        let mut known = status(false);
        known.last_failure = Some(FailureFacts::new(
            FailureCode::CRASH_LOOP,
            at,
            Some(ExitStatus::Exited(3)),
        ));
        let excerpt = vec!["boom".to_owned()];

        let same = last_failure(&FailureCode::CRASH_LOOP, &known, excerpt.clone());
        assert_eq!(same.code, "crash_loop");
        assert_eq!(same.at, Some(Timestamp::from_jiff(at)));
        assert_eq!((same.exit_code, same.signal), (Some(3), None));
        assert_eq!(same.excerpt, excerpt);

        let other = last_failure(&FailureCode::ENDPOINT_IN_USE, &known, Vec::new());
        assert_eq!(other.code, "endpoint_in_use");
        assert_eq!(
            (other.at, other.exit_code, other.signal),
            (None, None, None)
        );

        let restarted = last_failure(&FailureCode::CRASH_LOOP, &status(false), Vec::new());
        assert_eq!(
            (restarted.at, restarted.exit_code, restarted.signal),
            (None, None, None),
            "after a daemon restart the actor holds no facts"
        );

        let mut signaled = status(false);
        signaled.last_failure = Some(FailureFacts::new(
            FailureCode::PROCESS_SIGNALED,
            at,
            Some(ExitStatus::Signaled(11)),
        ));
        let by_signal = last_failure(&FailureCode::PROCESS_SIGNALED, &signaled, Vec::new());
        assert_eq!((by_signal.exit_code, by_signal.signal), (None, Some(11)));
    }

    #[test]
    fn a_live_lifecycle_whose_exit_went_unrecorded_is_shown_unknown() {
        for stored in [
            DomainLifecycle::Starting,
            DomainLifecycle::Running,
            DomainLifecycle::Recovering,
            DomainLifecycle::Stopping,
        ] {
            assert_eq!(
                shown_lifecycle(stored, &status(true)),
                InstanceLifecycle::Unknown
            );
            assert_eq!(shown_lifecycle(stored, &status(false)), lifecycle(stored));
        }
        assert_eq!(
            shown_lifecycle(DomainLifecycle::Stopped, &status(true)),
            InstanceLifecycle::Stopped,
            "a lifecycle that is not live is shown as stored"
        );
    }
}
