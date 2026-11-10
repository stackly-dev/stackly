//! `operations`: one Error per operation that needs recovery (spec 0007),
//! and one Warning per folder in `<data>/operations/` without an operation
//! row. A folder whose UUID version 7 name was made less than
//! [`SCRATCH_MIN_AGE`] ago, or that names an operation active in the
//! engine, is skipped, so an operation being created is not counted. The
//! doctor removes nothing.
//!
//! The age comes from the time inside the UUID version 7 name, which the
//! daemon gives the operation when it creates it: the filesystem port does
//! not read modification times. A name that is not a UUID version 7 was
//! never made by Stackly, so no age applies to it.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use stackly_domain::doctor::{DoctorFindingCode, DoctorSeverity, SCRATCH_MIN_AGE};
use stackly_domain::{OperationId, OperationResource, OperationState};
use uuid::Uuid;

use super::{CheckContext, CheckFailure, CheckFindings, stopped};
use crate::doctor::types::{Action, Finding, FindingFacts, InstanceRef};

/// The code whose error names the path in the way of an install.
const INSTALL_CONFLICT: &str = "runtime.install_conflict";

pub(super) fn run(
    context: &CheckContext,
    stop: &Arc<AtomicBool>,
) -> Result<CheckFindings, CheckFailure> {
    let supervisor = context.needs_supervisor()?;
    let registry = context.registry()?;
    let unfinished = registry
        .unfinished_operations()
        .map_err(|error| CheckFailure::registry(&error))?
        .value;
    let mut findings = Vec::new();
    let mut checked = 0usize;
    for (operation, view) in unfinished {
        if operation.state() != OperationState::NeedsRecovery {
            continue;
        }
        checked += 1;
        let failure = view.journal.failure_error();
        let error_code = failure
            .map(|failure| failure.code().as_str().to_owned())
            .or_else(|| {
                operation
                    .failure()
                    .map(|failure| failure.code().as_str().to_owned())
            });
        let path = failure
            .filter(|failure| failure.code().as_str() == INSTALL_CONFLICT)
            .and_then(|failure| {
                failure
                    .details()
                    .iter()
                    .find(|(key, _)| key.as_str() == "path")
                    .map(|(_, value)| value.clone())
            });
        let instance = match operation.resource() {
            OperationResource::Instance(id) => registry
                .instance(id)
                .ok()
                .and_then(|versioned| versioned.value)
                .map(|instance| InstanceRef::of(&instance)),
            _ => None,
        };
        let id = operation.id();
        let mut finding = Finding::new(
            DoctorFindingCode::NeedsRecovery,
            DoctorSeverity::Error,
            FindingFacts::NeedsRecovery {
                cause: view.journal.recovery_cause(),
                operation: Box::new(operation),
                error_code,
                path,
            },
        )
        .subject(id.to_string())
        .action(Action::ShowOperation(id));
        if let Some(instance) = instance {
            finding = finding.about(instance);
        }
        findings.push(finding);
    }

    let active: BTreeSet<OperationId> = supervisor
        .operations()
        .map(|engine| {
            context
                .runtime
                .block_on(engine.active_operations())
                .map(|active| active.into_iter().map(|operation| operation.id).collect())
                .unwrap_or_default()
        })
        .unwrap_or_default();
    let folder = context.sources.layout.layout().operations_dir();
    let entries = context
        .sources
        .platform
        .fs()
        .read_dir(&folder)
        .unwrap_or_default();
    let now = context.now();
    for entry in entries {
        if stopped(stop) {
            break;
        }
        let name = entry.name.to_string_lossy().into_owned();
        if let Some(id) = Uuid::parse_str(&name)
            .ok()
            .and_then(|uuid| OperationId::from_uuid(uuid).ok())
        {
            if active.contains(&id) || young(id.as_uuid(), now) {
                continue;
            }
            let row = registry
                .operation(id)
                .map_err(|error| CheckFailure::registry(&error))?
                .value;
            if row.is_some() {
                continue;
            }
        }
        findings.push(
            Finding::new(
                DoctorFindingCode::ScratchLeftover,
                DoctorSeverity::Warning,
                FindingFacts::Scratch { name: name.clone() },
            )
            .subject(format!("operations/{name}")),
        );
    }
    Ok(CheckFindings::checked(findings, checked))
}

/// Whether the time inside `uuid` is less than [`SCRATCH_MIN_AGE`] before
/// `now`, or after it.
fn young(uuid: Uuid, now: jiff::Timestamp) -> bool {
    let Some(made) = uuid.get_timestamp() else {
        return false;
    };
    let (seconds, _) = made.to_unix();
    let made = i64::try_from(seconds).unwrap_or(i64::MAX);
    let age = now.as_second().saturating_sub(made);
    age < i64::try_from(SCRATCH_MIN_AGE.as_secs()).unwrap_or(i64::MAX)
}
