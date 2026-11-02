//! The histories the sampler fills and the handlers read.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use jiff::Timestamp;
use stackly_domain::{HistoryId, InstanceId, MemberRead, MetricsHistory};
use std::num::NonZeroUsize;

use super::RunningMain;

/// The metrics history of every running instance, shared between the
/// sampler that writes it and the readers of `instance.get_metrics`.
///
/// Cloning gives another handle to the same histories. A history exists
/// only while its instance runs: the sampler drops it when the instance
/// stops or its main process changes.
#[derive(Debug, Clone, Default)]
pub struct MetricsStore {
    histories: Arc<Mutex<HashMap<InstanceId, MetricsHistory>>>,
}

impl MetricsStore {
    /// An empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Reads the history of `instance` under the lock, or `None` when the
    /// instance has none. Keep `read` short: the sampler waits for it.
    pub fn with_history<R>(
        &self,
        instance: InstanceId,
        read: impl FnOnce(Option<&MetricsHistory>) -> R,
    ) -> R {
        read(self.lock().get(&instance))
    }

    /// Returns the instances that have a history now.
    #[must_use]
    pub fn instances(&self) -> Vec<InstanceId> {
        self.lock().keys().copied().collect()
    }

    /// Drops every history, as when the daemon stops.
    pub fn clear(&self) {
        self.lock().clear();
    }

    /// Records one tick: keeps the histories of `running` main processes
    /// only, starts a new history for a main process that is new or changed,
    /// and adds the sample of each.
    pub(super) fn record_tick(
        &self,
        running: &[RunningMain],
        sampled_at: Timestamp,
        reads: &[(RunningMain, Vec<MemberRead>)],
        capacity: NonZeroUsize,
        rows: usize,
    ) {
        let mut histories = self.lock();
        histories.retain(|instance, history| {
            running.iter().any(|main| {
                main.instance == *instance && HistoryId::new(main.pid, main.start) == history.id()
            })
        });
        for (main, members) in reads {
            let id = HistoryId::new(main.pid, main.start);
            histories
                .entry(main.instance)
                .or_insert_with(|| MetricsHistory::new(id, capacity))
                .record(sampled_at, members, rows);
        }
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<InstanceId, MetricsHistory>> {
        self.histories
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
}
