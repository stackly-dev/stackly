//! `operations`: one Error per operation that needs recovery (spec 0007),
//! and one Warning per folder in `<data>/operations/` without an operation
//! row (or whose name is not an operation ID). A folder modified less than
//! [`SCRATCH_MIN_AGE`] ago, or that names an operation active in the
//! engine, is skipped, so an operation being created is not counted. The
//! doctor removes nothing.
//!
//! The age is the folder's own modification time, which moves when an
//! entry is added or removed in it. The engine makes a scratch folder only
//! from the work of an operation whose row is already committed, so a
//! folder without a row is never one an operation still writes to.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use jiff::Timestamp;
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
        if young(entry.info.modified, now) {
            continue;
        }
        if let Some(id) = Uuid::parse_str(&name)
            .ok()
            .and_then(|uuid| OperationId::from_uuid(uuid).ok())
        {
            if active.contains(&id) {
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

/// Whether `modified` is less than [`SCRATCH_MIN_AGE`] before `now`, or
/// after it. A folder whose time the OS does not report counts as old, so
/// it is still shown.
fn young(modified: Option<Timestamp>, now: Timestamp) -> bool {
    let Some(modified) = modified else {
        return false;
    };
    let age = now.duration_since(modified);
    age < jiff::SignedDuration::try_from(SCRATCH_MIN_AGE).unwrap_or(jiff::SignedDuration::MAX)
}
