//! `secrets`: one Error per reference of a stored `needsSecret` blocker,
//! with the field that uses it. Only references and fields: no value is
//! ever read.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use stackly_domain::doctor::{DoctorFindingCode, DoctorSeverity};
use stackly_domain::{InstanceBlocker, SecretUser};

use super::{CheckContext, CheckFailure, CheckFindings, stopped};
use crate::doctor::types::{Action, Finding, FindingFacts, InstanceRef};

pub(super) fn run(
    context: &CheckContext,
    stop: &Arc<AtomicBool>,
) -> Result<CheckFindings, CheckFailure> {
    let supervisor = context.needs_supervisor()?;
    let registry = context.registry()?;
    let mut findings = Vec::new();
    let instances = context.instances()?;
    let checked = instances.len();
    for instance in instances {
        if stopped(stop) {
            break;
        }
        let Some(InstanceBlocker::NeedsSecret { references, .. }) =
            supervisor.status(instance.id()).blocker
        else {
            continue;
        };
        let uses = registry
            .secret_uses_by(SecretUser::Instance(instance.id()))
            .map(|versioned| versioned.value)
            .unwrap_or_default();
        let reference = InstanceRef::of(&instance);
        for item in references {
            let field = uses
                .iter()
                .find(|secret_use| secret_use.reference == item.reference)
                .map(|secret_use| secret_use.field.as_str().to_owned());
            let subject = field.clone().unwrap_or_else(|| item.reference.to_string());
            findings.push(
                Finding::new(
                    DoctorFindingCode::SecretMissing,
                    DoctorSeverity::Error,
                    FindingFacts::Secret {
                        reference: item.reference,
                        reason: item.reason,
                        field,
                    },
                )
                .about(reference.clone())
                .subject(subject)
                .action(Action::ShowInstance(reference.id)),
            );
        }
    }
    Ok(CheckFindings::checked(findings, checked))
}
