//! Read filters and the paged operation query.

use jiff::Timestamp;

use crate::id::OperationId;
use crate::operation::{Operation, OperationResource};
use crate::registry::error::RegistryError;

/// Which instances a list returns, ordered by name and then ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct InstanceFilter {
    /// Whether retained instances are listed too.
    pub include_retained: bool,
}

/// Which leases of an instance a list returns, ordered by `acquired_at`
/// and then ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LeaseFilter {
    /// Every lease.
    All,
    /// The leases that are not released: `active` and `suppressed`.
    Unreleased,
}

/// Where a page of operations ends: the next page starts after it. Pages go
/// from the newest `(created_at, id)` down.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct OperationCursor {
    /// When the last operation of the page was created, to the nanosecond.
    pub created_at: Timestamp,
    /// Its ID, which breaks ties.
    pub id: OperationId,
}

impl OperationCursor {
    /// Returns the cursor that points at `operation`.
    #[must_use]
    pub const fn of(operation: &Operation) -> Self {
        Self {
            created_at: operation.created_at(),
            id: operation.id(),
        }
    }
}

/// A page of operations, newest `(created_at, id)` first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OperationQuery {
    /// Only operations that are not in a final state.
    pub active_only: bool,
    /// Only operations on this resource.
    pub resource: Option<OperationResource>,
    /// The most operations on the page, 1 to [`OperationQuery::LIMIT_MAX`].
    pub limit: u16,
    /// Start after this cursor, for the next page.
    pub before: Option<OperationCursor>,
}

impl OperationQuery {
    /// The largest page.
    pub const LIMIT_MAX: u16 = 500;

    /// Checks the page limit.
    ///
    /// # Errors
    ///
    /// Returns [`RegistryError::InvalidQuery`] when `limit` is outside 1 to
    /// [`OperationQuery::LIMIT_MAX`].
    pub const fn validate(&self) -> Result<(), RegistryError> {
        if self.limit == 0 || self.limit > Self::LIMIT_MAX {
            return Err(RegistryError::InvalidQuery {
                reason: "the page limit must be 1 to 500",
            });
        }
        Ok(())
    }
}

/// One page of operations, and the cursor of the next page while rows
/// remain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationPage {
    /// The operations, newest `(created_at, id)` first.
    pub items: Vec<Operation>,
    /// Where the next page starts, or `None` on the last page.
    pub next: Option<OperationCursor>,
}
