//! The start queue of a launchd daemon (spec 0031, *Antrean start*): one
//! list per daemon start, built from the registry after rotation and
//! reconciliation, so a crash at any point heals to the same state.
//!
//! The queue holds every instance whose desired state is `running` and
//! that is `stopped` or `failed` for a reason a restart can fix, joined
//! with what reconciliation already decided to run again, once per
//! instance, with the trigger `login` after a new session and `reconcile`
//! otherwise. [`Supervisor::start_reconciled`] works through it by name,
//! at most `RECONCILE_START_CONCURRENCY` at a time, with a fresh restart
//! budget and no dialog.

use stackly_domain::{
    DesiredState, FailureCode, Instance, InstanceBlocker, InstanceFilter, InstanceLifecycle,
    PlanDecision,
};

use super::Supervisor;
use super::error::SupervisorError;
use super::pending::{self, PendingKind, PendingRun};
use super::persist::blocking;

/// The last failures of a `failed` instance that a start at login may try
/// again (spec 0031, *Antrean start*): a start that was interrupted, a
/// restart budget that ran out, a readiness timeout, a secret that was not
/// there, and a port that was taken. Every other failure, such as data of
/// an incompatible line, a runtime that is gone, a package that is not
/// pinned, or an init that failed, needs the user.
pub const LOGIN_ELIGIBLE_FAILURES: &[FailureCode] = &[
    FailureCode::INTERRUPTED,
    FailureCode::CRASH_LOOP,
    FailureCode::from_static("readiness_timeout"),
    FailureCode::from_static("needs_secret"),
    FailureCode::ENDPOINT_IN_USE,
];

/// What building the start queue found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct StartQueue {
    /// Instances the queue added.
    pub added: u32,
    /// Instances reconciliation had already queued to start, whose trigger
    /// is now `login`.
    pub upgraded: u32,
}

/// Whether the lifecycle and last failure of `instance` let a system start
/// run (not the blockers, which the actor holds).
fn eligible(instance: &Instance) -> bool {
    if instance.desired() != DesiredState::Running {
        return false;
    }
    match instance.lifecycle() {
        InstanceLifecycle::Stopped => true,
        InstanceLifecycle::Failed => instance
            .last_failure()
            .is_some_and(|code| LOGIN_ELIGIBLE_FAILURES.contains(code)),
        _ => false,
    }
}

impl Supervisor {
    /// Builds the start queue of a daemon that a launchd job started: every
    /// eligible instance joins the list [`start_reconciled`] works through,
    /// once, with the trigger `login` when `new_session` is true and
    /// `reconcile` otherwise. Call it after [`reconcile_in_session`] and
    /// before `start_reconciled`.
    ///
    /// An instance with a blocker `dataInUse` or `identityUnproven`, a
    /// retained or `unknown` one, and an `stop_restore` entry of a restart
    /// plan that is being restored (the restore starts it) stay out.
    ///
    /// # Errors
    ///
    /// Returns [`SupervisorError::Registry`] when the instances cannot be
    /// read.
    ///
    /// [`start_reconciled`]: Self::start_reconciled
    /// [`reconcile_in_session`]: Self::reconcile_in_session
    pub async fn queue_login_starts(
        &self,
        new_session: bool,
    ) -> Result<StartQueue, SupervisorError> {
        let registry = self.deps().registry.clone();
        let instances = blocking(move || {
            registry.instances(InstanceFilter {
                include_retained: false,
            })
        })
        .await?
        .map_err(SupervisorError::Registry)?
        .value;
        let trigger = if new_session {
            pending::LOGIN_INPUT
        } else {
            pending::TRIGGER_INPUT
        };
        let mut queue = StartQueue::default();
        for instance in instances.iter().filter(|instance| eligible(instance)) {
            let id = instance.id();
            let blocked = self.actor(id).read(|state| {
                matches!(
                    state.blocker.as_ref(),
                    Some(
                        InstanceBlocker::DataInUse { .. }
                            | InstanceBlocker::IdentityUnproven { .. }
                    )
                )
            });
            if blocked {
                continue;
            }
            if self
                .planned_entry(id)
                .await
                .is_some_and(|entry| entry.decision == PlanDecision::StopRestore)
            {
                continue;
            }
            let mut pending = self
                .shared
                .pending
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            match pending.iter_mut().find(|run| run.id == id) {
                Some(run) if run.kind == PendingKind::Start => {
                    if run.trigger != trigger {
                        run.trigger = trigger;
                        queue.upgraded += 1;
                    }
                }
                Some(_) => {}
                None => {
                    pending.push(PendingRun {
                        id,
                        name: instance.name().clone(),
                        kind: PendingKind::Start,
                        trigger,
                    });
                    queue.added += 1;
                }
            }
        }
        Ok(queue)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_eligible_failures_are_a_closed_list_of_valid_codes() {
        for code in LOGIN_ELIGIBLE_FAILURES {
            assert!(!code.as_str().is_empty());
        }
        assert!(!LOGIN_ELIGIBLE_FAILURES.contains(&FailureCode::DATA_MISSING));
        assert!(!LOGIN_ELIGIBLE_FAILURES.contains(&FailureCode::RUNTIME_UNAVAILABLE));
    }
}
