//! `daemon`: one Warning per subsystem the daemon reports degraded.

use stackly_domain::doctor::{DoctorFindingCode, DoctorSeverity};

use super::{CheckContext, CheckFindings};
use crate::doctor::types::{Finding, FindingFacts, SummaryFacts};

pub(super) fn run(context: &CheckContext) -> CheckFindings {
    let findings = context
        .sources
        .daemon
        .degraded
        .iter()
        .map(|subsystem| {
            Finding::new(
                DoctorFindingCode::SubsystemDegraded,
                DoctorSeverity::Warning,
                FindingFacts::Subsystem(subsystem.clone()),
            )
            .subject(subsystem.name.clone())
        })
        .collect();
    CheckFindings::new(findings, SummaryFacts::Daemon)
}
