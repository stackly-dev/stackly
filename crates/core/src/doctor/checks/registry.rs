//! `registry`: recovery mode from what the daemon holds since open, else
//! `PRAGMA quick_check` and the scan for codes this build does not know,
//! both through the read connection and cut by the stop flag.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use stackly_domain::QuickCheck;
use stackly_domain::doctor::{DoctorFindingCode, DoctorSeverity};

use super::{CheckContext, CheckFailure, CheckFindings};
use crate::doctor::types::{DoctorMode, Finding, FindingFacts, SummaryFacts};

pub(super) fn run(
    context: &CheckContext,
    stop: &Arc<AtomicBool>,
) -> Result<CheckFindings, CheckFailure> {
    let schema_version = match &context.sources.mode {
        DoctorMode::Recovery { reason, message } => {
            let finding = Finding::new(
                DoctorFindingCode::RecoveryMode,
                DoctorSeverity::Error,
                FindingFacts::Recovery {
                    reason: reason.clone(),
                    message: message.clone(),
                },
            );
            return Ok(CheckFindings::new(
                vec![finding],
                SummaryFacts::Registry {
                    schema_version: None,
                },
            ));
        }
        DoctorMode::Ready { schema_version, .. } => *schema_version,
        DoctorMode::Unavailable { message } => return Err(CheckFailure::io(message.clone())),
    };
    let registry = context.registry()?;
    let mut findings = Vec::new();
    let check = registry
        .quick_check(Arc::clone(stop))
        .map_err(|error| CheckFailure::registry(&error))?;
    if let QuickCheck::Failed { messages } = check {
        findings.push(Finding::new(
            DoctorFindingCode::QuickCheckFailed,
            DoctorSeverity::Error,
            FindingFacts::QuickCheck { messages },
        ));
    }
    let values = registry
        .scan_unrecognized(Arc::clone(stop))
        .map_err(|error| CheckFailure::registry(&error))?;
    findings.extend(values.into_iter().map(|value| {
        let subject = format!("{}.{}#{}", value.table, value.column, value.row_id);
        Finding::new(
            DoctorFindingCode::UnrecognizedValue,
            DoctorSeverity::Error,
            FindingFacts::Unrecognized(value),
        )
        .subject(subject)
    }));
    Ok(CheckFindings::new(
        findings,
        SummaryFacts::Registry {
            schema_version: Some(schema_version),
        },
    ))
}
