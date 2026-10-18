//! Retention: deleting history beyond a [`RetentionPolicy`] (NFR-011),
//! at most [`PRUNE_BATCH_ROWS`] rows per transaction.
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

/// One kind of history a prune deletes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum History {
    /// Operations in a final state.
    Operations,
    /// Audit events.
    AuditEvents,
    /// Released leases.
    ReleasedLeases,
    /// Process observations.
    ProcessObservations,
}

impl History {
    /// Every kind, in the order a prune visits them.
    pub(super) const ALL: [Self; 4] = [
        Self::Operations,
        Self::AuditEvents,
        Self::ReleasedLeases,
        Self::ProcessObservations,
    ];
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

/// Deletes up to [`PRUNE_BATCH_ROWS`] rows of `history` beyond `limits` in
/// one transaction, and returns how many it deleted.
pub(super) fn delete_batch(
    conn: &mut Connection,
    limits: &Limits,
    history: History,
) -> Result<usize, RegistryError> {
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sql_error)?;
    let batch = i64::try_from(PRUNE_BATCH_ROWS).unwrap_or(i64::MAX);
    let deleted = match history {
        History::Operations => tx.execute(
            &format!(
                "DELETE FROM operations WHERE id IN (\
                 SELECT id FROM (SELECT id, updated_at, \
                 row_number() OVER (ORDER BY updated_at DESC, id DESC) AS rank \
                 FROM operations WHERE state IN {FINAL_OPERATION_STATES}) \
                 WHERE updated_at < ?1 OR rank > ?2 LIMIT ?3)"
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
}
