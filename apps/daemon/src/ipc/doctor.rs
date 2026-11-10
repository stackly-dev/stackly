//! `doctor.run` (spec 0034): the read only checks of `stackly_core::doctor`
//! with the sources this daemon holds, mapped to the wire with their text.
//!
//! A run is not an operation (spec 0007) and is not audited (spec 0035):
//! it writes nothing, holds no lock, and signals nothing. It is served in
//! recovery mode, where it never touches the registry.

mod copy;
mod map;

use std::time::Instant;

use stackly_core::catalog::CatalogState;
use stackly_core::doctor::{
    CapacityFailure, CapacityOutcome, DaemonFacts, DegradedSubsystem, DiskReading, DoctorError,
    DoctorMode, DoctorRequest, DoctorSources,
};
use stackly_domain::InstanceId;
use stackly_domain::doctor::CheckUnavailableReason;
use stackly_protocol::codes::{internal, resource};
use stackly_protocol::daemon::SubsystemStatus;
use stackly_protocol::doctor::{DoctorParams, DoctorReport};
use stackly_protocol::{ErrorBody, ResourceKind, ResourceRef};
use uuid::Uuid;

use crate::dispatch::HandlerFuture;
use crate::map::registry_error;
use crate::state::{DaemonCore, RegistryState};
use crate::supervisor::Availability;

/// Runs the doctor and answers its report.
///
/// # Errors
///
/// `resource.not_found` when `instanceId` names no instance, and the
/// registry errors of reading it.
pub fn run(core: &DaemonCore, params: DoctorParams) -> HandlerFuture<'_, DoctorReport> {
    Box::pin(async move {
        let instance = match params.instance_id {
            Some(id) => Some(InstanceId::from_uuid(id).map_err(|_| not_found(id))?),
            None => None,
        };
        let checks = params
            .checks
            .iter()
            .filter_map(|check| map::domain_check(*check))
            .collect();
        let request = DoctorRequest::new(instance, checks);
        let report = core
            .doctor()
            .run(sources(core), request, capacity(core))
            .await
            .map_err(|error| match error {
                DoctorError::InstanceNotFound(id) => not_found(id.as_uuid()),
                DoctorError::Registry(error) => Box::new(registry_error(&error)),
                _ => Box::new(ErrorBody::new(
                    &internal::UNEXPECTED,
                    "the doctor could not start",
                )),
            })?;
        Ok(map::report(core, &report))
    })
}

/// What the checks read: the platform, the layout, the registry through the
/// supervisor (or the recovery report), and the daemon's own health.
fn sources(core: &DaemonCore) -> DoctorSources {
    let platform = std::sync::Arc::new(core.platform().clone());
    let layout = std::sync::Arc::new(core.prepared().clone());
    let redactor = core.redactor();
    let mode = match core.registry_state() {
        RegistryState::Recovery { recovery } => {
            let schema = recovery
                .db_schema_version
                .map_or_else(|| "unknown".to_owned(), |schema| schema.to_string());
            DoctorMode::Recovery {
                message: redactor
                    .redact(&format!(
                        "Stackly opened the registry in recovery mode ({}): its schema is {schema} and this build knows {}.",
                        recovery.reason, recovery.app_schema_version
                    ))
                    .into_owned(),
                reason: recovery.reason,
            }
        }
        RegistryState::Ready { schema_version } => match core.supervision().availability() {
            Availability::Ready(supervisor) => DoctorMode::Ready {
                supervisor,
                schema_version,
            },
            _ => DoctorMode::Unavailable {
                message: "stacklyd is still starting".to_owned(),
            },
        },
        _ => DoctorMode::Unavailable {
            message: "the registry is not open".to_owned(),
        },
    };
    let degraded = core
        .health(Instant::now())
        .subsystems
        .into_iter()
        .filter(|subsystem| {
            matches!(
                subsystem.status,
                SubsystemStatus::Degraded | SubsystemStatus::Unavailable
            )
        })
        .map(|subsystem| {
            let status = serde_json::to_value(subsystem.status)
                .ok()
                .and_then(|value| value.as_str().map(str::to_owned))
                .unwrap_or_else(|| "degraded".to_owned());
            DegradedSubsystem::new(
                subsystem.name,
                status,
                subsystem
                    .detail
                    .map(|detail| redactor.redact(&detail).into_owned()),
            )
        })
        .collect();
    let catalog = core.catalog().and_then(|state| match state.as_ref() {
        CatalogState::Loaded(catalog) => Some(std::sync::Arc::clone(catalog)),
        _ => None,
    });
    DoctorSources::new(platform, layout, mode, DaemonFacts::new(degraded)).with_catalog(catalog)
}

/// The free space through `storage.get_capacity`, so the doctor shares its
/// single flight (spec 0022).
async fn capacity(core: &DaemonCore) -> CapacityOutcome {
    let report = crate::storage::get_capacity(core).await.map_err(|error| {
        let timeout = error
            .details()
            .iter()
            .any(|detail| detail.key() == "reason" && detail.value() == "timeout");
        CapacityFailure::new(
            if timeout {
                CheckUnavailableReason::Timeout
            } else {
                CheckUnavailableReason::Io
            },
            core.redactor().redact(error.message()).into_owned(),
        )
    })?;
    DiskReading::new(
        report.checked_at.as_jiff(),
        u64::from(report.total_bytes),
        u64::from(report.available_bytes),
    )
    .ok_or_else(|| {
        CapacityFailure::new(
            CheckUnavailableReason::Io,
            "the volume reported no size".to_owned(),
        )
    })
}

fn not_found(id: Uuid) -> Box<ErrorBody> {
    Box::new(
        ErrorBody::new(&resource::NOT_FOUND, "the instance does not exist").with_resource(
            ResourceRef {
                kind: ResourceKind::Instance,
                id: id.to_string(),
            },
        ),
    )
}
