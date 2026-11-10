//! `processes`: the passive identity of each recorded process (spec 0034,
//! *Hasil check processes*), by the first row of the table that matches.
//! It never probes or takes the instance lock and never signals: the
//! process, the data, and `instance.lock` stay as they are.
//!
//! A recorded observation is looked at only while the lifecycle says a
//! process should be alive (`starting`, `running`, `recovering`,
//! `stopping`, or `unknown`), or while an identity blocker is stored:
//! observations stay in the registry after a normal stop, and a PID reused
//! since then claims nothing.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use stackly_domain::doctor::{DoctorFindingCode, DoctorSeverity};
use stackly_domain::{
    IdentityCheck, IdentityCheckName, Instance, InstanceBlocker, InstanceLifecycle,
};

use super::{CheckContext, CheckFailure, CheckFindings, claims_process, stopped};
use crate::doctor::types::{
    Action, Finding, FindingFacts, IdentityFacts, IdentityReason, InstanceRef,
};
use crate::supervisor::evidence::{PassiveEvidence, observe_passive};

pub(super) fn run(
    context: &CheckContext,
    stop: &Arc<AtomicBool>,
) -> Result<CheckFindings, CheckFailure> {
    let supervisor = context.needs_supervisor()?;
    let registry = context.registry()?;
    let uid = context.uid().ok();
    let mut findings = Vec::new();
    let mut checked = 0usize;
    for instance in context.instances()? {
        if stopped(stop) {
            break;
        }
        let status = supervisor.status(instance.id());
        let blocker = match &status.blocker {
            Some(
                blocker @ (InstanceBlocker::DataInUse { .. }
                | InstanceBlocker::IdentityUnproven { .. }),
            ) => Some(blocker.clone()),
            _ => None,
        };
        let recorded = if blocker.is_some() || claims_process(instance.lifecycle()) {
            registry
                .latest_process_observation(instance.id())
                .map_err(|error| CheckFailure::registry(&error))?
                .value
                .map(|observation| observation.identity)
        } else {
            None
        };
        if blocker.is_none() && recorded.is_none() {
            continue;
        }
        checked += 1;
        let paths = context.sources.layout.instance_paths(instance.id());
        let passive = observe_passive(&context.sources.platform, &paths.lock, recorded.as_ref());
        let Some((code, severity, reason)) = judge(&instance, blocker.as_ref(), &passive) else {
            continue;
        };
        let (evidence, observed_at) = match &blocker {
            Some(blocker) => {
                let evidence = match blocker.evidence().and_then(|stored| stored.lock()) {
                    Some(lock) => passive.evidence.clone().with_lock(lock),
                    None => passive.evidence.clone(),
                };
                (evidence, blocker.observed_at())
            }
            None => (passive.evidence.clone(), context.now()),
        };
        let reference = InstanceRef::of(&instance);
        findings.push(
            Finding::new(
                code,
                severity,
                FindingFacts::Identity(Box::new(IdentityFacts {
                    reason,
                    evidence,
                    recorded,
                    current: passive.facts.clone(),
                    current_other_user: passive.other_user,
                    uid: uid.clone(),
                    observed_at,
                })),
            )
            .action(Action::ShowInstance(reference.id))
            .about(reference),
        );
    }
    Ok(CheckFindings::checked(findings, checked))
}

/// The first row of the table that matches, or `None` when nothing is
/// wrong.
fn judge(
    instance: &Instance,
    blocker: Option<&InstanceBlocker>,
    passive: &PassiveEvidence,
) -> Option<(DoctorFindingCode, DoctorSeverity, IdentityReason)> {
    let error = |code| Some((code, DoctorSeverity::Error, IdentityReason::Blocker));
    match blocker {
        Some(InstanceBlocker::DataInUse { .. }) => return error(DoctorFindingCode::DataInUse),
        Some(InstanceBlocker::IdentityUnproven { .. }) => {
            return error(DoctorFindingCode::IdentityUnproven);
        }
        _ => {}
    }
    let unknown = instance.lifecycle() == InstanceLifecycle::Unknown;
    let check = |name| passive.evidence.check(name);
    let unproven = |reason| {
        Some((
            DoctorFindingCode::IdentityUnproven,
            DoctorSeverity::Error,
            reason,
        ))
    };
    if check(IdentityCheckName::BootMatches) == IdentityCheck::Failed {
        return if unknown {
            unproven(IdentityReason::BootChanged)
        } else {
            None
        };
    }
    if check(IdentityCheckName::PidExists) == IdentityCheck::Failed {
        return if unknown {
            unproven(IdentityReason::ProcessGone)
        } else {
            None
        };
    }
    let reused = passive.other_user
        || [
            IdentityCheckName::StartMatches,
            IdentityCheckName::ExecutableMatches,
            IdentityCheckName::UserMatches,
        ]
        .into_iter()
        .any(|name| check(name) == IdentityCheck::Failed);
    if check(IdentityCheckName::PidExists) == IdentityCheck::Passed && reused {
        return Some((
            DoctorFindingCode::PidReused,
            DoctorSeverity::Error,
            IdentityReason::PidReused,
        ));
    }
    if check(IdentityCheckName::HoldsLock) == IdentityCheck::Failed {
        return unproven(IdentityReason::LockNotHeld);
    }
    if passive.facts.is_some() && passive.evidence.stopped() {
        return Some((
            DoctorFindingCode::ProcessSuspended,
            DoctorSeverity::Warning,
            IdentityReason::Suspended,
        ));
    }
    None
}
