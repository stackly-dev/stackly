//! [`DurableLogSink`]: the [`LogSink`] over the durable log store, which
//! the daemon puts in the supervisor's sink set next to the tail sink.

use stackly_domain::InstanceId;

use super::writer::DurableLog;
use crate::supervisor::{LogBatch, LogSink, RunEvent, SinkOutcome};

/// Queues every batch and run event for the writer thread. It never
/// blocks: a full queue answers [`SinkOutcome::Busy`] (spec 0027, AC-3).
#[derive(Debug, Clone)]
pub struct DurableLogSink {
    log: DurableLog,
}

impl DurableLogSink {
    /// Returns the sink over `log`.
    #[must_use]
    pub const fn new(log: DurableLog) -> Self {
        Self { log }
    }
}

impl LogSink for DurableLogSink {
    fn accept(&self, instance: InstanceId, batch: LogBatch) -> SinkOutcome {
        self.log.accept(instance, batch)
    }

    fn run_event(&self, instance: InstanceId, event: RunEvent) {
        self.log.run_event(instance, event);
    }
}
