//! `instance_failures`: every `failed` instance with its last failure as
//! the instance summary shows it (spec 0033), and every `running` instance
//! whose readiness is `not_ready`, each with the provider remedy that
//! matches it (AC-10).

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use stackly_domain::doctor::{DoctorFindingCode, DoctorSeverity};
use stackly_domain::{InstanceLifecycle, ReadinessState};

use super::{CheckContext, CheckFailure, CheckFindings, remedy, stopped};
use crate::doctor::types::{Action, FailureSnapshot, Finding, FindingFacts, InstanceRef};
use crate::provider::RemedySubject;

pub(super) fn run(
    context: &CheckContext,
    stop: &Arc<AtomicBool>,
) -> Result<CheckFindings, CheckFailure> {
    let supervisor = context.needs_supervisor()?;
    let mut findings = Vec::new();
    let instances = context.instances()?;
    let checked = instances.len();
    for instance in instances {
        if stopped(stop) {
            break;
        }
        let reference = InstanceRef::of(&instance);
        let finding = match instance.lifecycle() {
            InstanceLifecycle::Failed => {
                let status = supervisor.status(instance.id());
                let excerpt = if instance.last_failure().is_some() {
                    supervisor.failure_excerpt(instance.id())
                } else {
                    Vec::new()
                };
                let remedy = remedy::find(
                    context,
                    &instance,
                    &RemedySubject::Failure {
                        code: instance.last_failure(),
                        excerpt: &excerpt,
                    },
                );
                Finding::new(
                    DoctorFindingCode::StartFailed,
                    DoctorSeverity::Error,
                    FindingFacts::Failure(Box::new(FailureSnapshot {
                        instance: instance.clone(),
                        status,
                        excerpt,
                    })),
                )
                .remedy(remedy)
            }
            InstanceLifecycle::Running => {
                let status = supervisor.status(instance.id());
                if status.readiness.state() != ReadinessState::NotReady {
                    continue;
                }
                let reason = status.readiness.reason().to_string();
                let detail = status
                    .readiness
                    .detail()
                    .map(|detail| detail.as_str().to_owned());
                let remedy = remedy::find(
                    context,
                    &instance,
                    &RemedySubject::Readiness {
                        reason: &reason,
                        detail: detail.as_deref(),
                    },
                );
                Finding::new(
                    DoctorFindingCode::NotReady,
                    DoctorSeverity::Warning,
                    FindingFacts::NotReady { reason, detail },
                )
                .remedy(remedy)
            }
            _ => continue,
        };
        findings.push(
            finding
                .action(Action::ShowInstance(reference.id))
                .about(reference),
        );
    }
    Ok(CheckFindings::checked(findings, checked))
}
