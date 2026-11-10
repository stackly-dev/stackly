//! `instance_files`: the read only integrity checks of reconciliation
//! (spec 0030) for every instance they apply to: the data folder and the
//! runtime executable. No `ReconcileFinding` is sent.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use stackly_domain::doctor::{DoctorFindingCode, DoctorSeverity};

use super::{CheckContext, CheckFailure, CheckFindings, stopped};
use crate::doctor::types::{Action, Finding, FindingFacts, InstanceRef};
use crate::supervisor::integrity::{applies, data_is_missing, runtime_is_missing};

pub(super) fn run(
    context: &CheckContext,
    stop: &Arc<AtomicBool>,
) -> Result<CheckFindings, CheckFailure> {
    let supervisor = context.needs_supervisor()?;
    let mut findings = Vec::new();
    let mut checked = 0usize;
    for instance in context.instances()? {
        if stopped(stop) {
            break;
        }
        if !applies(&instance) {
            continue;
        }
        checked += 1;
        let (data_gone, runtime_gone) = context.runtime.block_on(async {
            (
                data_is_missing(supervisor, &instance).await,
                runtime_is_missing(supervisor, &instance).await,
            )
        });
        let reference = InstanceRef::of(&instance);
        for (gone, code) in [
            (data_gone, DoctorFindingCode::DataMissing),
            (runtime_gone, DoctorFindingCode::RuntimeMissing),
        ] {
            if gone {
                findings.push(
                    Finding::new(code, DoctorSeverity::Error, FindingFacts::Missing)
                        .about(reference.clone())
                        .action(Action::ShowInstance(reference.id)),
                );
            }
        }
    }
    Ok(CheckFindings::checked(findings, checked))
}
