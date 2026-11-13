//! What a coordinator of an environment hands the ports that start its
//! child operations (spec 0046 *Operasi anak*, spec 0047): the coordinator
//! and its trigger, and the secret caller of the client request. Up and
//! Down share these types.

use stackly_domain::OperationId;

use crate::secret::SecretCaller;

/// Where a child operation of an environment coordinator comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChildOrigin {
    /// The coordinator and a copy of its trigger, for `Origin::Child`.
    pub operation: crate::operation::ChildOrigin,
    /// The secret caller of the coordinator's request: a create binds
    /// with it, and a start uses it for its first try only.
    pub caller: SecretCaller,
}

/// What a port did when asked to start a child.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChildStart<T> {
    /// The operation runs; wait for it with `OperationEngine::wait_final`.
    /// What it made is in its result.
    Started {
        /// The child operation.
        operation: OperationId,
    },
    /// Nothing to do: the runtime is installed or the instance already
    /// runs.
    AlreadyDone(T),
}
