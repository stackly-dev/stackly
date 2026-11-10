//! `instance_failures`: every `failed` instance with its last failure as
//! the instance summary shows it (spec 0033), and every `running` instance
//! whose readiness is `not_ready`. Provider remedies come with milestone 2.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use stackly_domain::doctor::{DoctorFindingCode, DoctorSeverity};
use stackly_domain::{InstanceLifecycle, ReadinessState};

use super::{CheckContext, CheckFailure, CheckFindings, stopped};
use crate::doctor::types::{Action, FailureSnapshot, Finding, FindingFacts, InstanceRef};

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
                Finding::new(
                    DoctorFindingCode::StartFailed,
                    DoctorSeverity::Error,
                    FindingFacts::Failure(Box::new(FailureSnapshot {
                        instance: instance.clone(),
                        status,
                        excerpt,
                    })),
                )
            }
            InstanceLifecycle::Running => {
                let status = supervisor.status(instance.id());
                if status.readiness.state() != ReadinessState::NotReady {
                    continue;
                }
                Finding::new(
                    DoctorFindingCode::NotReady,
                    DoctorSeverity::Warning,
                    FindingFacts::NotReady {
                        reason: status.readiness.reason().to_string(),
                        detail: status
                            .readiness
                            .detail()
                            .map(|detail| detail.as_str().to_owned()),
                    },
                )
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
