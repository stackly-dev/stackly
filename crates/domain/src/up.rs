//! The report of one environment Up (spec 0046, *Phase service* and *Model
//! data*): the state of the Up and the phase of every service, as the
//! registry keeps it in `environments.last_up`.

use jiff::Timestamp;

use crate::code::code_enum;
use crate::id::OperationId;
use crate::secret::SecretRef;
use crate::value::{ErrorCodeName, MemberAlias};

code_enum! {
    /// Where one Up stands.
    pub enum UpState: UpState {
        /// It runs now, or the daemon stopped while it ran.
        Running => "running",
        /// Every service is ready.
        Succeeded => "succeeded",
        /// At least one service is not ready.
        Failed => "failed",
        /// It was cancelled before every service ended.
        Cancelled => "cancelled",
    }
}

code_enum! {
    /// Where one service stands in one Up.
    pub enum UpPhase: UpPhase {
        /// Not worked on yet.
        Pending => "pending",
        /// Waiting for the install of its runtime.
        Installing => "installing",
        /// Its create operation runs.
        Creating => "creating",
        /// Waiting for a dependency.
        Waiting => "waiting",
        /// Its start runs, or its running instance is not ready yet.
        Starting => "starting",
        /// Its instance runs and is ready.
        Ready => "ready",
        /// Its install, create, or start failed.
        Failed => "failed",
        /// Its create or start stopped for a secret that is not set.
        NeedsSecret => "needs_secret",
        /// A dependency was not ready, so it never started.
        Skipped => "skipped",
        /// The Up was cancelled before it ended.
        Cancelled => "cancelled",
    }
}

impl UpPhase {
    /// Returns whether the service ended in this phase.
    #[must_use]
    pub const fn is_final(self) -> bool {
        matches!(
            self,
            Self::Ready | Self::Failed | Self::NeedsSecret | Self::Skipped | Self::Cancelled
        )
    }
}

code_enum! {
    /// The stage of an Up a service ended in when it is not ready.
    pub enum UpStage: UpStage {
        /// Installing its runtime.
        Install => "install",
        /// Creating its instance.
        Create => "create",
        /// Starting its instance.
        Start => "start",
        /// Waiting for a dependency.
        Wait => "wait",
    }
}

code_enum! {
    /// Why a service was skipped.
    pub enum SkipReason: SkipReason {
        /// A dependency failed, was skipped, or was cancelled.
        Failed => "failed",
        /// A dependency was not ready within the edge's timeout.
        Timeout => "timeout",
        /// A dependency's instance was not running and ready right before
        /// the service started.
        NotReady => "not_ready",
    }
}

/// What one Up recorded about one service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceReport {
    alias: MemberAlias,
    phase: UpPhase,
    stage: Option<UpStage>,
    code: Option<ErrorCodeName>,
    dependency: Option<MemberAlias>,
    reason: Option<SkipReason>,
    operation: Option<OperationId>,
    references: Vec<SecretRef>,
}

impl ServiceReport {
    /// The most secret references one service keeps; the rest are cut.
    pub const MAX_REFERENCES: usize = 8;

    /// Creates the report of `alias` in `phase`, with nothing else set.
    #[must_use]
    pub const fn new(alias: MemberAlias, phase: UpPhase) -> Self {
        Self {
            alias,
            phase,
            stage: None,
            code: None,
            dependency: None,
            reason: None,
            operation: None,
            references: Vec::new(),
        }
    }

    /// Returns it with the stage it ended in.
    #[must_use]
    pub const fn with_stage(mut self, stage: UpStage) -> Self {
        self.stage = Some(stage);
        self
    }

    /// Returns it with the error code it ended with.
    #[must_use]
    pub fn with_code(mut self, code: ErrorCodeName) -> Self {
        self.code = Some(code);
        self
    }

    /// Returns it with the dependency that was not ready and why.
    #[must_use]
    pub fn with_dependency(mut self, dependency: MemberAlias, reason: SkipReason) -> Self {
        self.dependency = Some(dependency);
        self.reason = Some(reason);
        self
    }

    /// Returns it with the child operation that worked on it last.
    #[must_use]
    pub const fn with_operation(mut self, operation: OperationId) -> Self {
        self.operation = Some(operation);
        self
    }

    /// Returns it with the secret references that are not set, at most
    /// [`ServiceReport::MAX_REFERENCES`].
    #[must_use]
    pub fn with_references(mut self, mut references: Vec<SecretRef>) -> Self {
        references.truncate(Self::MAX_REFERENCES);
        self.references = references;
        self
    }

    /// Returns the service alias.
    #[must_use]
    pub const fn alias(&self) -> &MemberAlias {
        &self.alias
    }

    /// Returns its phase.
    #[must_use]
    pub const fn phase(&self) -> UpPhase {
        self.phase
    }

    /// Returns the stage it ended in, when not ready.
    #[must_use]
    pub const fn stage(&self) -> Option<UpStage> {
        self.stage
    }

    /// Returns the error code it ended with, if any.
    #[must_use]
    pub const fn code(&self) -> Option<&ErrorCodeName> {
        self.code.as_ref()
    }

    /// Returns the dependency that was not ready, for a skipped service.
    #[must_use]
    pub const fn dependency(&self) -> Option<&MemberAlias> {
        self.dependency.as_ref()
    }

    /// Returns why it was skipped.
    #[must_use]
    pub const fn reason(&self) -> Option<SkipReason> {
        self.reason
    }

    /// Returns the child operation that worked on it last, if any.
    #[must_use]
    pub const fn operation(&self) -> Option<OperationId> {
        self.operation
    }

    /// Returns the secret references that are not set.
    #[must_use]
    pub fn references(&self) -> &[SecretRef] {
        &self.references
    }
}

/// The report of one Up of an environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpReport {
    operation: OperationId,
    state: UpState,
    started_at: Timestamp,
    finished_at: Option<Timestamp>,
    services: Vec<ServiceReport>,
}

impl UpReport {
    /// The most services one report keeps; the rest are cut.
    pub const MAX_SERVICES: usize = 32;

    /// Creates the report of a running Up with no service reported yet.
    #[must_use]
    pub const fn running(operation: OperationId, started_at: Timestamp) -> Self {
        Self {
            operation,
            state: UpState::Running,
            started_at,
            finished_at: None,
            services: Vec::new(),
        }
    }

    /// Creates a report with every field, cutting the services to
    /// [`UpReport::MAX_SERVICES`]. A running report has no finish time,
    /// so `finished_at` is dropped for it.
    #[must_use]
    pub fn new(
        operation: OperationId,
        state: UpState,
        started_at: Timestamp,
        finished_at: Option<Timestamp>,
        mut services: Vec<ServiceReport>,
    ) -> Self {
        services.truncate(Self::MAX_SERVICES);
        Self {
            operation,
            state,
            started_at,
            finished_at: if state == UpState::Running {
                None
            } else {
                finished_at
            },
            services,
        }
    }

    /// Returns the `environment_up` operation.
    #[must_use]
    pub const fn operation(&self) -> OperationId {
        self.operation
    }

    /// Returns where the Up stands.
    #[must_use]
    pub const fn state(&self) -> UpState {
        self.state
    }

    /// Returns when it started.
    #[must_use]
    pub const fn started_at(&self) -> Timestamp {
        self.started_at
    }

    /// Returns when it ended, unless it is running.
    #[must_use]
    pub const fn finished_at(&self) -> Option<Timestamp> {
        self.finished_at
    }

    /// Returns the services, in plan order.
    #[must_use]
    pub fn services(&self) -> &[ServiceReport] {
        &self.services
    }

    /// Returns the report of `alias`, if it has one.
    #[must_use]
    pub fn service(&self, alias: &MemberAlias) -> Option<&ServiceReport> {
        self.services
            .iter()
            .find(|service| service.alias() == alias)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alias(text: &str) -> MemberAlias {
        text.parse().unwrap()
    }

    #[test]
    fn a_report_keeps_at_most_32_services_and_8_references_each() {
        let references: Vec<SecretRef> = (0..10)
            .map(|n| format!("local:shop/db/user-{n}").parse().unwrap())
            .collect();
        let service = ServiceReport::new(alias("db"), UpPhase::NeedsSecret)
            .with_stage(UpStage::Create)
            .with_references(references);
        assert_eq!(service.references().len(), ServiceReport::MAX_REFERENCES);

        let services = (0..40)
            .map(|n| ServiceReport::new(alias(&format!("s{n}")), UpPhase::Ready))
            .collect();
        let report = UpReport::new(
            OperationId::generate(),
            UpState::Succeeded,
            Timestamp::UNIX_EPOCH,
            Some(Timestamp::UNIX_EPOCH),
            services,
        );
        assert_eq!(report.services().len(), UpReport::MAX_SERVICES);
        assert_eq!(
            report.service(&alias("s0")).map(ServiceReport::phase),
            Some(UpPhase::Ready)
        );
    }

    #[test]
    fn a_running_report_has_no_finish_time() {
        let report = UpReport::new(
            OperationId::generate(),
            UpState::Running,
            Timestamp::UNIX_EPOCH,
            Some(Timestamp::UNIX_EPOCH),
            Vec::new(),
        );
        assert_eq!(report.finished_at(), None);
    }

    #[test]
    fn only_the_end_phases_are_final() {
        let finals: Vec<&str> = UpPhase::ALL
            .iter()
            .filter(|phase| phase.is_final())
            .map(|phase| phase.as_code())
            .collect();
        assert_eq!(
            finals,
            ["ready", "failed", "needs_secret", "skipped", "cancelled"]
        );
    }
}
