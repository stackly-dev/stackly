//! Guarded commits: a caller reads the registry and writes in one
//! transaction, so nothing commits between the check and the write (spec
//! 0020, "Commit terjaga di langkah `register`").

use std::any::Any;
use std::fmt;

use crate::id::RuntimeId;
use crate::instance::Instance;
use crate::provider::ProviderRevisionKey;
use crate::registry::batch::{Committed, WriteOp};
use crate::registry::error::RegistryError;
use crate::registry::query::InstanceFilter;
use crate::registry::record::ProviderRevisionRecord;
use crate::runtime::Runtime;

/// What a commit guard may read: the registry inside the guarded commit's
/// own write transaction. Reads see every batch committed before the
/// guarded commit began, and nothing commits while the guard runs.
pub trait GuardView {
    /// Returns the instances `filter` selects, ordered by name and then ID.
    ///
    /// # Errors
    ///
    /// Returns `Corrupt`, `Busy`, `Unavailable`, or `Io`.
    fn instances(&self, filter: InstanceFilter) -> Result<Vec<Instance>, RegistryError>;

    /// Returns a runtime.
    ///
    /// # Errors
    ///
    /// Returns `Corrupt`, `Busy`, `Unavailable`, or `Io`.
    fn runtime(&self, id: RuntimeId) -> Result<Option<Runtime>, RegistryError>;

    /// Returns a provider revision by its key.
    ///
    /// # Errors
    ///
    /// Returns `Corrupt`, `Busy`, `Unavailable`, or `Io`.
    fn provider_revision(
        &self,
        key: &ProviderRevisionKey,
    ) -> Result<Option<ProviderRevisionRecord>, RegistryError>;
}

/// What a guard decided after reading.
#[must_use]
pub enum GuardDecision {
    /// Apply these ops, in order, in the same transaction.
    Write(Vec<WriteOp>),
    /// Apply nothing; the reason is the caller's own type, handed back as
    /// [`Guarded::Refused`].
    Refuse(Box<dyn Any + Send>),
}

impl fmt::Debug for GuardDecision {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Write(ops) => f.debug_tuple("Write").field(&ops.len()).finish(),
            Self::Refuse(_) => f.write_str("Refuse(..)"),
        }
    }
}

/// A guard: called exactly once, on the registry's own thread, inside the
/// write transaction.
///
/// A guard is pure. It reads only through the [`GuardView`], does no other
/// IO, never sleeps or waits (`await`, channel, or a lock that is not its
/// own), and never calls the [`Registry`](crate::Registry): the registry's
/// thread would wait for itself.
pub type CommitGuard =
    Box<dyn FnOnce(&dyn GuardView) -> Result<GuardDecision, RegistryError> + Send>;

/// What a guarded commit returns.
#[must_use]
pub enum Guarded {
    /// The ops applied and the generation rose by 1.
    Committed(Committed),
    /// The guard refused: nothing applied and the generation did not move.
    /// The caller downcasts the reason to its own type.
    Refused(Box<dyn Any + Send>),
}

impl fmt::Debug for Guarded {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Committed(committed) => f.debug_tuple("Committed").field(committed).finish(),
            Self::Refused(_) => f.write_str("Refused(..)"),
        }
    }
}
