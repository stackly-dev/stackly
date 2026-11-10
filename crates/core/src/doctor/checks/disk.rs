//! `disk_space`: the level of the volume that holds the data root, from
//! the same reading as `storage.get_capacity` (spec 0022).

use stackly_domain::CapacityLevel;
use stackly_domain::doctor::{DoctorFindingCode, DoctorSeverity};

use super::CheckFindings;
use crate::doctor::types::{Action, DiskReading, Finding, FindingFacts, SummaryFacts};

/// The findings of a reading: none for `ok`, one for every other level.
pub(in crate::doctor) fn findings(reading: DiskReading) -> CheckFindings {
    let finding = match reading.level {
        CapacityLevel::Filling => Some((DoctorFindingCode::VolumeFilling, DoctorSeverity::Warning)),
        CapacityLevel::Low => Some((DoctorFindingCode::VolumeLow, DoctorSeverity::Warning)),
        CapacityLevel::Critical => Some((DoctorFindingCode::VolumeCritical, DoctorSeverity::Error)),
        _ => None,
    };
    let findings = finding
        .map(|(code, severity)| {
            Finding::new(code, severity, FindingFacts::Capacity(reading))
                .action(Action::OpenStorageSettings)
        })
        .into_iter()
        .collect();
    let mut found = CheckFindings::new(findings, SummaryFacts::Capacity(reading));
    found.capacity = Some(reading);
    found
}
