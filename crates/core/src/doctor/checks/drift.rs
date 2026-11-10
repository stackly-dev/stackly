//! `config_drift`: a stored `configDrift` blocker, else the read only drift
//! check of spec 0029 against the active revision. Nothing is kept or
//! rendered.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use stackly_domain::InstanceBlocker;
use stackly_domain::doctor::{DoctorFindingCode, DoctorSeverity};

use super::{CheckContext, CheckFailure, CheckFindings, stopped};
use crate::doctor::types::{Action, Finding, FindingFacts, InstanceRef};
use crate::supervisor::drift;

pub(super) fn run(
    context: &CheckContext,
    stop: &Arc<AtomicBool>,
) -> Result<CheckFindings, CheckFailure> {
    let supervisor = context.needs_supervisor()?;
    let registry = context.registry()?;
    let fs = context.sources.platform.fs();
    let mut findings = Vec::new();
    let mut checked = 0usize;
    for instance in context.instances()? {
        if stopped(stop) {
            break;
        }
        if instance.data_initialized_at().is_none() {
            continue;
        }
        checked += 1;
        let files = match supervisor.status(instance.id()).blocker {
            Some(InstanceBlocker::ConfigDrift { files, .. }) => {
                files.iter().map(ToString::to_string).collect()
            }
            _ => {
                let Some(revision) = registry
                    .active_config_revision(instance.id())
                    .ok()
                    .and_then(|versioned| versioned.value)
                else {
                    continue;
                };
                let paths = context.sources.layout.instance_paths(instance.id());
                match drift::check(fs, &paths, revision.id) {
                    Ok(report) if !report.is_clean() => report.paths(),
                    _ => continue,
                }
            }
        };
        let reference = InstanceRef::of(&instance);
        findings.push(
            Finding::new(
                DoctorFindingCode::ConfigDrift,
                DoctorSeverity::Warning,
                FindingFacts::Drift { files },
            )
            .action(Action::ReviewConfig(reference.id))
            .about(reference),
        );
    }
    Ok(CheckFindings::checked(findings, checked))
}
