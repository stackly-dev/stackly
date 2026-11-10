//! The end of a login session (spec 0031, *Akhir sesi*): what the daemon
//! does when launchd sends `SIGTERM` and no `daemon.shutdown` came first.
//!
//! The daemon first refuses new requests (the daemon's part). Then
//! [`Supervisor::end_session`] cancels the operations that run, starts one
//! `stop_instance` for every instance with a live process, all at the same
//! time, and waits until they commit or the deadline passes. Last,
//! [`Supervisor::close_session`] writes `EndSession`. A session end never
//! sends `SIGKILL`, never suppresses a startup lease, and never changes a
//! desired state, so the next start queue of the same session starts the
//! instances again (AC-13).

use std::sync::atomic::Ordering;
use std::time::Duration;

use stackly_domain::{
    InstanceFilter, InstanceId, InstanceLifecycle, OperationId, OperationKind, OperationState,
    OperationTrigger, SessionEndReason, StopIntent, WriteBatch, WriteOp,
};
use tokio::time::Instant;

use super::Supervisor;
use super::pending::{SESSION_END_INPUT, begin};
use super::persist::blocking;
use super::stop::StopRequest;

/// How often the end of a session looks at the stops it waits for.
const POLL: Duration = Duration::from_millis(20);

/// What [`Supervisor::end_session`] found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct SessionEndReport {
    /// `clean` when every stop committed in time, else `interrupted`.
    pub reason: SessionEndReason,
    /// How many instances got a stop.
    pub stops: usize,
    /// How many of those had not finished when the deadline passed.
    pub unfinished: usize,
}

impl Supervisor {
    /// How long the stops of a session end get, counted from the signal.
    #[must_use]
    pub fn session_end_budget(&self) -> Duration {
        self.deps().config.session_end_stop_budget
    }

    /// Runs steps 2 to 4 of *Akhir sesi* until `deadline` (`t0` plus the
    /// stop budget): cancels the operations that run, stops every instance
    /// with a live process through the provider's strategy, and waits.
    ///
    /// An instance whose lock a cancelled operation still holds waits for
    /// that lock inside the same deadline. A stop that is not done when the
    /// deadline passes is left alone: the instance stays `stopping` and the
    /// next reconciliation reads a `Gone` process as `exited` (spec 0010).
    pub async fn end_session(&self, deadline: Instant) -> SessionEndReport {
        self.shared.session_ending.store(true, Ordering::SeqCst);
        self.cancel_running_operations().await;
        let live = self.live_instances().await;
        let mut stops = Vec::with_capacity(live.len());
        for id in &live {
            let make = || self.stop_work(StopRequest::session_end(*id));
            if let Some(operation) = begin(
                self,
                *id,
                OperationKind::StopInstance,
                SESSION_END_INPUT,
                &make,
                Some(deadline),
            )
            .await
            {
                stops.push(operation);
            }
        }
        let started = stops.len();
        let (succeeded, finished) = self.wait_for_stops(&stops, deadline).await;
        let unfinished = started - finished;
        let clean = live.len() == started && unfinished == 0 && succeeded == started;
        SessionEndReport {
            reason: if clean {
                SessionEndReason::Clean
            } else {
                SessionEndReason::Interrupted
            },
            stops: started,
            unfinished,
        }
    }

    /// The intent of the stop that a cancelled start or restart ends with:
    /// a person's cancel is a choice and writes desired `stopped`, the cancel
    /// of a session end is not and leaves it alone.
    pub(crate) fn cancel_intent(&self) -> StopIntent {
        if self.shared.session_ending.load(Ordering::SeqCst) {
            StopIntent::PlannedRestart
        } else {
            StopIntent::User
        }
    }

    /// Writes `EndSession` for the session of this run, once. A failed
    /// write never blocks the exit; the next rotation closes the row
    /// `unknown` (spec 0031, *Akhir sesi*, step 5). Returns whether the row
    /// was written.
    pub async fn close_session(&self, reason: SessionEndReason) -> bool {
        let Some(id) = self.session() else {
            return false;
        };
        if self.shared.session_closed.swap(true, Ordering::SeqCst) {
            return false;
        }
        let deps = self.deps();
        let at = deps.platform.clock().now();
        let registry = deps.registry.clone();
        let batch = WriteBatch::new(vec![WriteOp::EndSession { id, at, reason }]);
        match blocking(move || registry.commit(batch)).await {
            Ok(Ok(_)) => true,
            Ok(Err(error)) => {
                tracing::warn!(%error, "the session row could not be closed");
                false
            }
            Err(error) => {
                tracing::warn!(%error, "the session row could not be closed");
                false
            }
        }
    }

    /// Step 2: asks every operation that is not final, except a stop, to
    /// cancel through the cancel path of spec 0007. A start that is
    /// cancelled after its spawn ends through the ordinary stop.
    async fn cancel_running_operations(&self) {
        let Some(engine) = self.operations() else {
            return;
        };
        let registry = self.deps().registry.clone();
        let Ok(Ok(open)) = blocking(move || registry.unfinished_operations()).await else {
            return;
        };
        for (operation, _) in open.value {
            if matches!(
                operation.kind(),
                OperationKind::StopInstance | OperationKind::ForceStopInstance
            ) {
                continue;
            }
            if let Err(error) = engine
                .cancel(operation.id(), OperationTrigger::background())
                .await
            {
                tracing::warn!(operation = %operation.id(), ?error, "an operation could not be cancelled for the end of the session");
            }
        }
    }

    /// The instances a session end stops: a process may run, that is
    /// `starting`, `running`, or `stopping`.
    async fn live_instances(&self) -> Vec<InstanceId> {
        let registry = self.deps().registry.clone();
        let read = blocking(move || {
            registry.instances(InstanceFilter {
                include_retained: false,
            })
        })
        .await;
        let Ok(Ok(instances)) = read else {
            tracing::warn!("the instances cannot be read for the end of the session");
            return Vec::new();
        };
        let mut live: Vec<_> = instances
            .value
            .iter()
            .filter(|instance| {
                matches!(
                    instance.lifecycle(),
                    InstanceLifecycle::Starting
                        | InstanceLifecycle::Running
                        | InstanceLifecycle::Stopping
                )
            })
            .map(|instance| (instance.name().clone(), instance.id()))
            .collect();
        live.sort();
        live.into_iter().map(|(_, id)| id).collect()
    }

    /// Step 4: waits until every stop is final or `deadline` passes.
    /// Returns how many succeeded and how many are final.
    async fn wait_for_stops(&self, stops: &[OperationId], deadline: Instant) -> (usize, usize) {
        let Some(engine) = self.operations() else {
            return (0, 0);
        };
        loop {
            let mut succeeded = 0;
            let mut finished = 0;
            for operation in stops {
                if let Ok(Some(detail)) = engine.get(*operation).await {
                    let state = detail.view.operation.state();
                    if state.is_final() {
                        finished += 1;
                        if state == OperationState::Succeeded {
                            succeeded += 1;
                        }
                    }
                }
            }
            if finished == stops.len() || Instant::now() >= deadline {
                return (succeeded, finished);
            }
            tokio::time::sleep(POLL).await;
        }
    }
}
