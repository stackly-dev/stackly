//! `provider_packages`: every pinned package an instance uses must load
//! through the loader of spec 0009; one Error per package that does not,
//! with every instance that pins it.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use stackly_domain::doctor::{DoctorFindingCode, DoctorSeverity};
use stackly_domain::{Instance, ProviderRevisionKey};

use super::{CheckContext, CheckFailure, CheckFindings, stopped};
use crate::doctor::types::{Action, Finding, FindingFacts, InstanceRef};
use crate::supervisor::prepare::load_package;

pub(super) fn run(
    context: &CheckContext,
    stop: &Arc<AtomicBool>,
) -> Result<CheckFindings, CheckFailure> {
    let instances = context.instances()?;
    let mut by_package: BTreeMap<String, (ProviderRevisionKey, Vec<&Instance>)> = BTreeMap::new();
    for instance in &instances {
        let key = instance.provider_revision();
        by_package
            .entry(key.to_string())
            .or_insert_with(|| (key.clone(), Vec::new()))
            .1
            .push(instance);
    }
    let mut findings = Vec::new();
    let checked = by_package.len();
    for (subject, (key, users)) in by_package {
        if stopped(stop) {
            break;
        }
        let Some(first) = users.first() else {
            continue;
        };
        let Err(error) = load_package(&context.sources.platform, &context.sources.layout, first)
        else {
            continue;
        };
        let reference = InstanceRef::of(first);
        findings.push(
            Finding::new(
                DoctorFindingCode::PackageUnreadable,
                DoctorSeverity::Error,
                FindingFacts::Package {
                    key,
                    reason: error.to_string(),
                    used_by: users.iter().map(|user| user.name().clone()).collect(),
                },
            )
            .subject(subject)
            .action(Action::ShowInstance(reference.id))
            .about(reference),
        );
    }
    Ok(CheckFindings::checked(findings, checked))
}
