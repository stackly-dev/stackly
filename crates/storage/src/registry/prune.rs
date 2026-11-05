//! Retention: deleting history beyond a [`RetentionPolicy`] (NFR-011),
//! at most [`PRUNE_BATCH_ROWS`] rows per transaction, and at most
//! [`OPERATION_PRUNE_BATCH_ROWS`] operations, whose journal rows go with
//! them by cascade (spec 0007). A final operation with a created item
//! that is unsettled or whose compensation failed is never pruned.
//!
//! Each batch recomputes which rows lie beyond the limits. The set is
//! stable while batches delete it: a row past an age limit is older than
//! every row within it, and deleting rows ranked past a row limit leaves
//! the ranks of the kept rows as they were.

use jiff::{SignedDuration, Timestamp};
use rusqlite::{Connection, TransactionBehavior};
use stackly_domain::{RegistryError, RetentionPolicy};

use super::column::{sql_error, ts};
use super::read::FINAL_OPERATION_STATES;

/// The most rows one prune transaction deletes.
pub(super) const PRUNE_BATCH_ROWS: usize = 5_000;

/// The most operations one prune transaction deletes. Each one cascades to
/// its journal, steps, created items, and lock keys.
pub(super) const OPERATION_PRUNE_BATCH_ROWS: usize = 500;

/// The `operation_created` states that keep a final operation from being
/// pruned: an item never settled, or one whose compensation failed, may
/// still exist on disk or in the Keychain, and its row is the only record
/// of it (spec 0007 AC-17, audit note 145).
pub(super) const UNRESOLVED_CREATED_STATES: &str = "('intended', 'created', 'compensation_failed')";

/// One kind of history a prune deletes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum History {
    /// Operations in a final state, except those a restart plan refers to
    /// or with a created item in [`UNRESOLVED_CREATED_STATES`].
    Operations,
    /// Audit events.
    AuditEvents,
    /// Released leases.
    ReleasedLeases,
    /// Process observations.
    ProcessObservations,
    /// Final daemon restart plans, whose instance rows go with them by
    /// cascade (spec 0030). An operation a plan refers to is never pruned,
    /// so it goes only after its plan.
    RestartPlans,
}

impl History {
    /// Every kind, in the order a prune visits them.
    pub(super) const ALL: [Self; 5] = [
        Self::RestartPlans,
        Self::Operations,
        Self::AuditEvents,
        Self::ReleasedLeases,
        Self::ProcessObservations,
    ];

    /// The most rows of this kind one transaction deletes.
    pub(super) const fn batch_rows(self) -> usize {
        match self {
            Self::Operations => OPERATION_PRUNE_BATCH_ROWS,
            Self::AuditEvents
            | Self::ReleasedLeases
            | Self::ProcessObservations
            | Self::RestartPlans => PRUNE_BATCH_ROWS,
        }
    }
}

/// A [`RetentionPolicy`] measured from one `now`, in the form the delete
/// statements bind.
#[derive(Debug, Clone)]
pub(super) struct Limits {
    operation_cutoff: String,
    audit_cutoff: String,
    released_lease_cutoff: String,
    operation_max_rows: i64,
    audit_max_rows: i64,
    observations_per_instance: i64,
    restart_plans: i64,
}

impl Limits {
    /// Measures `policy` from `now`.
    pub(super) fn new(policy: &RetentionPolicy, now: Timestamp) -> Self {
        Self {
            operation_cutoff: cutoff(now, policy.operation_max_age),
            audit_cutoff: cutoff(now, policy.audit_max_age),
            released_lease_cutoff: cutoff(now, policy.released_lease_max_age),
            operation_max_rows: i64::from(policy.operation_max_rows),
            audit_max_rows: i64::from(policy.audit_max_rows),
            observations_per_instance: i64::from(policy.observations_per_instance),
            restart_plans: i64::from(policy.restart_plans),
        }
    }
}

/// The stored text of `now` minus `age`: rows with an older time fall
/// outside the age limit. An age reaching past the earliest time jiff
/// knows keeps every row, and the empty text sorts before every stored
/// time.
fn cutoff(now: Timestamp, age: std::time::Duration) -> String {
    SignedDuration::try_from(age)
        .ok()
        .and_then(|age| now.checked_sub(age).ok())
        .map_or_else(String::new, ts)
}

/// Deletes up to [`History::batch_rows`] rows of `history` beyond `limits`
/// in one transaction, and returns how many it deleted.
pub(super) fn delete_batch(
    conn: &mut Connection,
    limits: &Limits,
    history: History,
) -> Result<usize, RegistryError> {
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sql_error)?;
    let batch = i64::try_from(history.batch_rows()).unwrap_or(i64::MAX);
    let deleted = match history {
        History::Operations => tx.execute(
            &format!(
                "DELETE FROM operations WHERE id IN (\
                 SELECT id FROM (SELECT id, updated_at, \
                 row_number() OVER (ORDER BY updated_at DESC, id DESC) AS rank \
                 FROM operations WHERE state IN {FINAL_OPERATION_STATES}) \
                 WHERE (updated_at < ?1 OR rank > ?2) \
                 AND id NOT IN (SELECT operation_id FROM daemon_restart_plan) \
                 AND id NOT IN (SELECT operation_id FROM operation_created \
                 WHERE state IN {UNRESOLVED_CREATED_STATES}) LIMIT ?3)"
            ),
            (&limits.operation_cutoff, limits.operation_max_rows, batch),
        ),
        History::AuditEvents => tx.execute(
            "DELETE FROM audit_events WHERE id IN (\
             SELECT id FROM (SELECT id, occurred_at, \
             row_number() OVER (ORDER BY occurred_at DESC, id DESC) AS rank \
             FROM audit_events) \
             WHERE occurred_at < ?1 OR rank > ?2 LIMIT ?3)",
            (&limits.audit_cutoff, limits.audit_max_rows, batch),
        ),
        History::ReleasedLeases => tx.execute(
            "DELETE FROM instance_demands WHERE id IN (\
             SELECT id FROM instance_demands \
             WHERE state = 'released' AND state_changed_at < ?1 LIMIT ?2)",
            (&limits.released_lease_cutoff, batch),
        ),
        History::ProcessObservations => tx.execute(
            "DELETE FROM process_observations WHERE id IN (\
             SELECT id FROM (SELECT id, row_number() OVER (\
             PARTITION BY instance_id ORDER BY observed_at DESC, id DESC) AS rank \
             FROM process_observations) \
             WHERE rank > ?1 LIMIT ?2)",
            (limits.observations_per_instance, batch),
        ),
        History::RestartPlans => tx.execute(
            "DELETE FROM daemon_restart_plan WHERE id IN (\
             SELECT id FROM (SELECT id, row_number() OVER (\
             ORDER BY created_at DESC, id DESC) AS rank \
             FROM daemon_restart_plan \
             WHERE state NOT IN ('stopping', 'handed_over', 'restoring')) \
             WHERE rank > ?1 LIMIT ?2)",
            (limits.restart_plans, batch),
        ),
    }
    .map_err(sql_error)?;
    tx.commit().map_err(sql_error)?;
    Ok(deleted)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn a_cutoff_is_now_minus_the_age_or_empty_when_it_reaches_too_far_back() {
        let now = Timestamp::constant(1_767_225_600, 0);

        assert_eq!(
            cutoff(now, Duration::from_secs(24 * 60 * 60)),
            "2025-12-31T00:00:00.000000000Z"
        );
        assert_eq!(cutoff(now, Duration::ZERO), ts(now));
        assert_eq!(cutoff(now, Duration::MAX), "");
        assert_eq!(cutoff(now, Duration::from_secs(400_000 * 365 * 86_400)), "");
    }

    #[test]
    fn operations_prune_500_per_transaction_and_other_history_5000() {
        assert_eq!(History::Operations.batch_rows(), 500);
        for history in [
            History::AuditEvents,
            History::ReleasedLeases,
            History::ProcessObservations,
            History::RestartPlans,
        ] {
            assert_eq!(history.batch_rows(), 5_000);
        }
    }
}
