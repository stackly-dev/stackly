//! The port of Down (spec 0047, *Operasi `environment_down`*):
//! `DownChildren` starts the existing `stop_instance` as a child, through
//! the same code as `instance.stop` with the request of an environment that
//! goes down. Core knows no IPC handler, so `stacklyd` implements it.

use stackly_domain::{EnvironmentId, FailureError, InstanceId, OperationId};

use crate::environment::children::ChildOrigin;
use crate::operation::BoxFuture;

/// What the port did when asked to stop one instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownChild {
    /// The operation runs; wait for it with `OperationEngine::wait_final`.
    Started {
        /// The child operation.
        operation: OperationId,
    },
    /// Another lease holds the instance; nothing was written.
    Kept,
    /// Nothing runs and the desired state is already `stopped`.
    AlreadyStopped,
}

/// Starts the child operations of a Down.
pub trait DownChildren: Send + Sync + 'static {
    /// Starts `stop_instance` for one instance on behalf of `environment`,
    /// which goes down, with `BusyPolicy::Queue` and the precheck of a
    /// child stop.
    fn stop(
        &self,
        parent: &ChildOrigin,
        instance: InstanceId,
        environment: EnvironmentId,
    ) -> BoxFuture<'_, Result<DownChild, FailureError>>;
}
