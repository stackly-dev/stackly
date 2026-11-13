//! The ports of Up (spec 0046, *Operasi anak*): `UpChildren` starts the
//! existing install, create, and start operations as children, through the
//! same code as `runtime.install`, `instance.create`, and `instance.start`;
//! `UpReads` reads the plan and an instance as the daemon sees them. Core
//! knows no IPC handler, so `stacklyd` implements both.

use std::path::PathBuf;

use stackly_domain::{
    EnvironmentId, FailureError, InstanceId, InstanceLifecycle, ManifestLocation, MemberAlias,
    NeedsSecretItem, ProviderName, RuntimeId, Sha256Digest,
};

use super::report::{BlockedItem, UpError};
use crate::environment::children::{ChildOrigin, ChildStart};
use crate::environment::{Plan, Validation};
use crate::instance::create::{CreateDraft, EnvironmentBinding};
use crate::operation::BoxFuture;

/// Starts the child operations of an Up.
pub trait UpChildren: Send + Sync + 'static {
    /// Starts `install_runtime` for one artifact, as `runtime.install`
    /// does, with `BusyPolicy::Queue`.
    fn install(
        &self,
        parent: &ChildOrigin,
        provider: &ProviderName,
        artifact: Sha256Digest,
    ) -> BoxFuture<'_, Result<ChildStart<RuntimeId>, FailureError>>;

    /// Starts `create_instance` for one service, bound to the environment,
    /// with `BusyPolicy::Reject`.
    fn create(
        &self,
        parent: &ChildOrigin,
        draft: CreateDraft,
        binding: EnvironmentBinding,
    ) -> BoxFuture<'_, Result<ChildStart<InstanceId>, FailureError>>;

    /// Starts `start_instance` with the environment trigger, with
    /// `BusyPolicy::Queue`.
    fn start(
        &self,
        parent: &ChildOrigin,
        instance: InstanceId,
    ) -> BoxFuture<'_, Result<ChildStart<InstanceId>, FailureError>>;
}

/// Which manifest to plan, and how.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanRequest {
    /// The manifest file, absolute.
    pub path: PathBuf,
    /// The registered environment the request named, if it named one.
    pub registered: Option<EnvironmentId>,
    /// `locked`, as the request sent it (spec 0045).
    pub locked: bool,
}

/// A manifest read and planned as `env.plan` plans it.
#[derive(Debug, Clone)]
pub struct PlannedUp {
    /// The validation of the bytes read, with their digest.
    pub validation: Validation,
    /// Where the manifest lives.
    pub location: ManifestLocation,
    /// The plan.
    pub plan: Plan,
    /// One entry per `plan.blockers`, in the same order: what
    /// `env.plan_blocked` lists for it.
    pub blocked: Vec<BlockedItem>,
    /// The references of the instances whose blocker `env.instance_blocked`
    /// has `reason` `needs_secret`, from the supervisor's `NeedsSecret`
    /// blocker, by service.
    pub instance_needs: Vec<(MemberAlias, Vec<NeedsSecretItem>)>,
    /// The references of `create` items whose status is `missing` or
    /// `unregistered`, with `reason` `missing`.
    pub create_needs: Vec<NeedsSecretItem>,
}

/// An instance as a service's start reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstanceNow {
    /// The lifecycle the registry row holds, `unknown` for a live one
    /// whose exit went unrecorded.
    pub lifecycle: InstanceLifecycle,
    /// Whether the supervisor reads its readiness `ready` now.
    pub ready: bool,
    /// Whether a blocker refuses its start (identity, data in use, config
    /// drift); a `needsSecret` blocker does not, the start retries it.
    pub refuses_start: bool,
}

impl InstanceNow {
    /// Returns whether it runs and is ready.
    #[must_use]
    pub fn running_and_ready(&self) -> bool {
        self.lifecycle == InstanceLifecycle::Running && self.ready
    }
}

/// Reads what an Up needs from the daemon.
pub trait UpReads: Send + Sync + 'static {
    /// Reads the manifest of `request` and plans it with the lock and the
    /// local overrides next to it, as `env.plan` does.
    fn plan(&self, request: &PlanRequest) -> BoxFuture<'_, Result<PlannedUp, UpError>>;

    /// Reads one instance now: its registry row and the supervisor's
    /// status; `None` when the row is gone.
    fn instance(&self, id: InstanceId) -> BoxFuture<'_, Option<InstanceNow>>;
}
