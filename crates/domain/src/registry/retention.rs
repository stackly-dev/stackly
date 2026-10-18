//! How much history the registry keeps (NFR-011).

use std::time::Duration;

/// Seconds in one day.
const DAY_SECS: u64 = 24 * 60 * 60;

/// Limits on the history [`Registry::prune`](crate::Registry::prune)
/// keeps.
///
/// An age limit deletes rows older than `now` minus the age: operations in
/// a final state by `updated_at`, audit events by `occurred_at`, released
/// leases by when they were released. A row limit counts only rows that
/// may be deleted, newest first, and deletes the rest. Unfinished or
/// `needs_recovery` operations and active or suppressed leases are never
/// deleted or counted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RetentionPolicy {
    /// Oldest finished operation kept.
    pub operation_max_age: Duration,
    /// Oldest audit event kept.
    pub audit_max_age: Duration,
    /// Oldest released lease kept.
    pub released_lease_max_age: Duration,
    /// Most finished operations kept.
    pub operation_max_rows: u32,
    /// Most audit events kept.
    pub audit_max_rows: u32,
    /// Most process observations kept per instance.
    pub observations_per_instance: u32,
}

impl RetentionPolicy {
    /// 30 days and 100,000 rows of operations and audit events, 30 days of
    /// released leases, and 20 process observations per instance, until
    /// Settings (features 32 and 35) make them configurable.
    pub const DEFAULT: Self = Self {
        operation_max_age: Duration::from_secs(30 * DAY_SECS),
        audit_max_age: Duration::from_secs(30 * DAY_SECS),
        released_lease_max_age: Duration::from_secs(30 * DAY_SECS),
        operation_max_rows: 100_000,
        audit_max_rows: 100_000,
        observations_per_instance: 20,
    };
}

impl Default for RetentionPolicy {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// How many rows one prune deleted, per kind of history. A prune never
/// raises the generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub struct PruneReport {
    /// Finished operations.
    pub operations: u64,
    /// Audit events.
    pub audit_events: u64,
    /// Released leases.
    pub released_leases: u64,
    /// Process observations.
    pub process_observations: u64,
}

impl PruneReport {
    /// Returns the rows deleted in all.
    #[must_use]
    pub const fn total(&self) -> u64 {
        self.operations + self.audit_events + self.released_leases + self.process_observations
    }
}
