//! One function per daemon check (spec 0034, *Daftar check*). Each runs on
//! a blocking thread, reads its sources through the ports, and looks at
//! its stop flag between items; past its limit nobody waits for it, so
//! what it finds then is dropped.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use jiff::Timestamp;
use stackly_domain::doctor::{CheckUnavailableReason, DoctorCheckId, DoctorSeverity};
use stackly_domain::{
    Instance, InstanceFilter, InstanceId, InstanceLifecycle, Registry, RegistryError,
};
use stackly_platform::UserId;

use super::types::{DiskReading, DoctorMode, DoctorSources, Finding, SummaryFacts};
use crate::supervisor::Supervisor;

mod daemon;
pub(super) mod disk;
mod drift;
mod failures;
mod files;
mod operations;
mod packages;
mod permissions;
mod ports;
mod processes;
mod registry;
mod remedy;
mod runtimes;
mod secrets;

/// What a check found.
#[derive(Debug, Clone)]
pub(super) struct CheckFindings {
    /// Every finding, in any order; the runner orders and caps them.
    pub(super) findings: Vec<Finding>,
    /// The severity of each finding the check left out itself.
    pub(super) omitted: Vec<DoctorSeverity>,
    /// What the summary line says.
    pub(super) summary: SummaryFacts,
    /// The reading of `disk_space`.
    pub(super) capacity: Option<DiskReading>,
}

impl CheckFindings {
    pub(super) const fn new(findings: Vec<Finding>, summary: SummaryFacts) -> Self {
        Self {
            findings,
            omitted: Vec::new(),
            summary,
            capacity: None,
        }
    }

    /// The count summary of a check that looked at `checked` things.
    pub(super) fn checked(findings: Vec<Finding>, checked: usize) -> Self {
        Self::new(
            findings,
            SummaryFacts::Checked(u64::try_from(checked).unwrap_or(u64::MAX)),
        )
    }
}

/// Why a check could not run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CheckFailure {
    pub(super) reason: CheckUnavailableReason,
    pub(super) message: Option<String>,
}

impl CheckFailure {
    /// A read that failed, with `message`.
    pub(super) fn io(message: impl Into<String>) -> Self {
        Self {
            reason: CheckUnavailableReason::Io,
            message: Some(message.into()),
        }
    }

    /// A registry read that failed.
    pub(super) fn registry(error: &RegistryError) -> Self {
        Self::io(error.to_string())
    }
}

type Outcome = Result<CheckFindings, CheckFailure>;

/// What every check of one run shares.
pub(super) struct CheckContext {
    pub(super) sources: DoctorSources,
    /// The instance the run is limited to.
    pub(super) scope: Option<InstanceId>,
    /// The runtime, for the few async reads (the integrity checks of spec
    /// 0030) a blocking thread waits for.
    pub(super) runtime: tokio::runtime::Handle,
}

impl CheckContext {
    pub(super) const fn new(
        sources: DoctorSources,
        scope: Option<InstanceId>,
        runtime: tokio::runtime::Handle,
    ) -> Self {
        Self {
            sources,
            scope,
            runtime,
        }
    }

    /// Why `check` does not run at all in this mode, if it does not.
    pub(super) fn refusal(&self, check: DoctorCheckId) -> Option<Outcome> {
        match &self.sources.mode {
            DoctorMode::Ready { .. } => None,
            DoctorMode::Recovery { .. } => {
                (!check.runs_in_recovery()).then_some(Err(CheckFailure {
                    reason: CheckUnavailableReason::RecoveryMode,
                    message: None,
                }))
            }
            DoctorMode::Unavailable { message } => (check.needs_registry()
                && check != DoctorCheckId::Permissions)
                .then(|| Err(CheckFailure::io(message.clone()))),
        }
    }

    /// The supervisor, when the registry is open.
    pub(super) fn supervisor(&self) -> Option<&Supervisor> {
        match &self.sources.mode {
            DoctorMode::Ready { supervisor, .. } => Some(supervisor),
            _ => None,
        }
    }

    /// The supervisor, or the failure of a check that needs it.
    pub(super) fn needs_supervisor(&self) -> Result<&Supervisor, CheckFailure> {
        self.supervisor()
            .ok_or_else(|| CheckFailure::io("the registry is not available"))
    }

    /// The registry, or the failure of a check that needs it.
    pub(super) fn registry(&self) -> Result<Arc<dyn Registry>, CheckFailure> {
        self.needs_supervisor()
            .map(|supervisor| Arc::clone(&supervisor.deps().registry))
    }

    /// Every instance that is not retained, by name.
    pub(super) fn all_instances(&self) -> Result<Vec<Instance>, CheckFailure> {
        self.registry()?
            .instances(InstanceFilter {
                include_retained: false,
            })
            .map(|versioned| versioned.value)
            .map_err(|error| CheckFailure::registry(&error))
    }

    /// The instances the run looks at: every one that is not retained, or
    /// only the one it is limited to.
    pub(super) fn instances(&self) -> Result<Vec<Instance>, CheckFailure> {
        let mut instances = self.all_instances()?;
        if let Some(scope) = self.scope {
            instances.retain(|instance| instance.id() == scope);
        }
        Ok(instances)
    }

    /// The user the daemon runs as.
    pub(super) fn uid(&self) -> Result<UserId, CheckFailure> {
        self.sources
            .platform
            .identity()
            .current_user()
            .map_err(|error| CheckFailure::io(error.to_string()))
    }

    /// The time now, from the platform clock.
    pub(super) fn now(&self) -> Timestamp {
        self.sources.platform.clock().now()
    }
}

/// Whether the runner gave up on this check.
pub(super) fn stopped(stop: &AtomicBool) -> bool {
    stop.load(Ordering::SeqCst)
}

/// Whether the lifecycle of `instance` says a process of it should be
/// alive, so a recorded observation still claims one.
pub(super) const fn claims_process(lifecycle: InstanceLifecycle) -> bool {
    matches!(
        lifecycle,
        InstanceLifecycle::Starting
            | InstanceLifecycle::Running
            | InstanceLifecycle::Recovering
            | InstanceLifecycle::Stopping
            | InstanceLifecycle::Unknown
    )
}

/// Runs `check`. `disk_space` runs in the runner, which awaits its reading.
pub(super) fn run(check: DoctorCheckId, context: &CheckContext, stop: &Arc<AtomicBool>) -> Outcome {
    match check {
        DoctorCheckId::Daemon => Ok(daemon::run(context)),
        DoctorCheckId::Registry => registry::run(context, stop),
        DoctorCheckId::Permissions => permissions::run(context, stop),
        DoctorCheckId::Runtimes => runtimes::run(context, stop),
        DoctorCheckId::ProviderPackages => packages::run(context, stop),
        DoctorCheckId::Endpoints => ports::run(context, stop),
        DoctorCheckId::Processes => processes::run(context, stop),
        DoctorCheckId::InstanceFiles => files::run(context, stop),
        DoctorCheckId::InstanceFailures => failures::run(context, stop),
        DoctorCheckId::ConfigDrift => drift::run(context, stop),
        DoctorCheckId::Secrets => secrets::run(context, stop),
        DoctorCheckId::Operations => operations::run(context, stop),
        DoctorCheckId::DiskSpace
        | DoctorCheckId::Connection
        | DoctorCheckId::Protocol
        | DoctorCheckId::BackgroundAgent => Err(CheckFailure {
            reason: CheckUnavailableReason::NotSupported,
            message: None,
        }),
    }
}
