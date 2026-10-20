//! The commit rules of the supervisor (spec 0010, *Commit*): one
//! `UpdateInstance` per domain mutation with consecutive expected
//! revisions, and how an actor commit reacts to each registry failure.

use std::time::Duration;

use stackly_domain::{BatchError, Instance, RegistryError, ResourceRevision, WriteOp};

use super::SupervisorConfig;

/// The changes to one instance aggregate in one batch: every mutation that
/// raises the revision becomes its own `UpdateInstance`, expecting the
/// revision the one before it left, because the registry takes exactly one
/// revision step per op (spec 0005).
///
/// ```
/// use stackly_core::supervisor::InstanceChanges;
/// use stackly_domain::{DesiredState, Instance};
///
/// fn stop_ops(instance: Instance) -> Vec<stackly_domain::WriteOp> {
///     let mut changes = InstanceChanges::new(instance);
///     changes.apply(|instance| {
///         instance.set_desired(DesiredState::Stopped);
///         Ok::<_, std::convert::Infallible>(())
///     });
///     changes.into_ops()
/// }
/// ```
#[derive(Debug, Clone)]
pub struct InstanceChanges {
    instance: Instance,
    read: ResourceRevision,
    ops: Vec<WriteOp>,
}

impl InstanceChanges {
    /// Starts from `instance` as read, at the revision it carries.
    #[must_use]
    pub fn new(instance: Instance) -> Self {
        let read = instance.revision();
        Self {
            instance,
            read,
            ops: Vec::new(),
        }
    }

    /// Applies one domain mutation. When it raises the revision, the
    /// changed aggregate becomes the next `UpdateInstance`, expecting the
    /// revision before it; a mutation that leaves the revision alone (a
    /// readiness or health observation) adds no op.
    ///
    /// # Errors
    ///
    /// Returns the mutation's error and keeps the aggregate as it was
    /// before the call.
    pub fn apply<E>(
        &mut self,
        change: impl FnOnce(&mut Instance) -> Result<(), E>,
    ) -> Result<(), E> {
        let mut changed = self.instance.clone();
        change(&mut changed)?;
        let expected = self.instance.revision();
        if changed.revision() != expected {
            self.ops.push(WriteOp::UpdateInstance {
                instance: changed.clone(),
                expected,
            });
        }
        self.instance = changed;
        Ok(())
    }

    /// Returns the aggregate after every mutation applied so far.
    #[must_use]
    pub const fn instance(&self) -> &Instance {
        &self.instance
    }

    /// Returns the revision the changes started from, the one read.
    #[must_use]
    pub const fn read_revision(&self) -> ResourceRevision {
        self.read
    }

    /// Returns true when no mutation raised the revision.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    /// Returns the ops, in mutation order, for the caller's batch.
    #[must_use]
    pub fn into_ops(self) -> Vec<WriteOp> {
        self.ops
    }
}

/// What an actor commit (exit, crash, `exited`, `adopted`,
/// `confirmed_stopped`, reconciliation, `holds_file`) does after the
/// registry refused its batch (spec 0010 AC-22).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommitRetry {
    /// Read the instance again, apply the event again if it still holds,
    /// and commit the new batch.
    Reread,
    /// Wait, then commit the same batch again: the registry was busy.
    Wait(Duration),
    /// Report the `supervisor` subsystem `degraded`, wait, then commit the
    /// same batch again: the registry is full or not available.
    Degraded(Duration),
    /// Stop retrying: the error is final, or the conflicts went past the
    /// limit.
    GiveUp,
}

/// The retry state of one actor commit. While it retries, the actor
/// neither spawns the instance nor signals its main process, and shows its
/// in-memory state with the detail `pending_commit`.
///
/// - A revision or generation conflict rereads, at most
///   [`commit_retry_limit`](SupervisorConfig::commit_retry_limit) times.
/// - `Busy` waits 100 ms, doubling up to 5 seconds, with no limit.
/// - `NoSpace`, `Unavailable`, and `Io` degrade and wait
///   [`registry_retry_interval`](SupervisorConfig::registry_retry_interval),
///   with no limit.
/// - Anything else gives up.
#[derive(Debug, Clone)]
pub struct ActorCommitRetry {
    conflicts: u32,
    next_busy: Duration,
    limit: u32,
    busy_max: Duration,
    degraded_interval: Duration,
}

impl ActorCommitRetry {
    /// Starts the retry state of one commit.
    #[must_use]
    pub const fn new(config: &SupervisorConfig) -> Self {
        Self {
            conflicts: 0,
            next_busy: config.commit_busy_first_delay,
            limit: config.commit_retry_limit,
            busy_max: config.commit_busy_max_delay,
            degraded_interval: config.registry_retry_interval,
        }
    }

    /// Decides what to do after the registry refused the batch with
    /// `error`.
    pub fn next(&mut self, error: &BatchError) -> CommitRetry {
        match &error.error {
            RegistryError::RevisionConflict { .. } | RegistryError::GenerationConflict { .. } => {
                if self.conflicts < self.limit {
                    self.conflicts += 1;
                    CommitRetry::Reread
                } else {
                    CommitRetry::GiveUp
                }
            }
            RegistryError::Busy => {
                let delay = self.next_busy;
                self.next_busy = self.next_busy.saturating_mul(2).min(self.busy_max);
                CommitRetry::Wait(delay)
            }
            RegistryError::NoSpace | RegistryError::Unavailable | RegistryError::Io { .. } => {
                CommitRetry::Degraded(self.degraded_interval)
            }
            _ => CommitRetry::GiveUp,
        }
    }

    /// Returns how many conflicts were reread so far.
    #[must_use]
    pub const fn conflicts(&self) -> u32 {
        self.conflicts
    }
}

#[cfg(test)]
mod tests {
    use stackly_domain::{
        DesiredState, Generation, InstanceId, InstanceLifecycle, LifecycleEvent, ResourceRef,
    };
    use stackly_testkit::{InstanceBuilder, ProviderRevisionBuilder, RuntimeBuilder};

    use super::*;

    fn stopped_instance() -> Instance {
        let runtime = RuntimeBuilder::new().build();
        let provider = ProviderRevisionBuilder::new();
        InstanceBuilder::new(&runtime, &provider.key())
            .lifecycle(InstanceLifecycle::Stopped)
            .build()
    }

    fn conflict() -> BatchError {
        BatchError::at(
            0,
            RegistryError::RevisionConflict {
                resource: ResourceRef::Instance(InstanceId::generate()),
                expected: ResourceRevision::new(1).unwrap(),
                actual: ResourceRevision::new(2).unwrap(),
            },
        )
    }

    #[test]
    fn each_mutation_that_raises_the_revision_becomes_its_own_update() {
        let instance = stopped_instance();
        let read = instance.revision().get();
        let mut changes = InstanceChanges::new(instance);

        changes
            .apply(|instance| {
                instance.set_desired(DesiredState::Running);
                Ok::<_, std::convert::Infallible>(())
            })
            .unwrap();
        changes
            .apply(|instance| {
                instance
                    .apply_lifecycle(LifecycleEvent::StartBegin, at())
                    .map(drop)
            })
            .unwrap();

        assert_eq!(changes.instance().lifecycle(), InstanceLifecycle::Starting);
        let expected: Vec<_> = changes
            .clone()
            .into_ops()
            .iter()
            .map(|op| match op {
                WriteOp::UpdateInstance { instance, expected } => {
                    (expected.get(), instance.revision().get())
                }
                other => panic!("unexpected op {other:?}"),
            })
            .collect();
        assert_eq!(expected, [(read, read + 1), (read + 1, read + 2)]);
        assert_eq!(changes.read_revision().get(), read);
    }

    #[test]
    fn a_failed_mutation_adds_no_op_and_keeps_the_aggregate() {
        let instance = stopped_instance();
        let before = instance.clone();
        let mut changes = InstanceChanges::new(instance);

        let result = changes.apply(|instance| {
            instance.set_desired(DesiredState::Running);
            Err("refused")
        });

        assert_eq!(result, Err("refused"));
        assert!(changes.is_empty());
        assert_eq!(changes.instance(), &before);
    }

    #[test]
    fn a_mutation_that_keeps_the_revision_adds_no_op() {
        let mut changes = InstanceChanges::new(stopped_instance());

        changes
            .apply(|_| Ok::<_, std::convert::Infallible>(()))
            .unwrap();

        assert!(changes.is_empty());
    }

    #[test]
    fn revision_conflicts_reread_five_times_then_give_up() {
        let mut retry = ActorCommitRetry::new(&SupervisorConfig::default());

        let steps: Vec<_> = (0..6).map(|_| retry.next(&conflict())).collect();

        assert_eq!(steps[..5], [CommitRetry::Reread; 5]);
        assert_eq!(steps[5], CommitRetry::GiveUp);
        assert_eq!(retry.conflicts(), 5);
    }

    #[test]
    fn a_generation_conflict_rereads_like_a_revision_conflict() {
        let mut retry = ActorCommitRetry::new(&SupervisorConfig::default());
        let error = BatchError::batch(RegistryError::GenerationConflict {
            expected: Generation::new(1),
            actual: Generation::new(2),
        });

        assert_eq!(retry.next(&error), CommitRetry::Reread);
    }

    #[test]
    fn busy_waits_from_100_ms_doubling_up_to_5_seconds() {
        let mut retry = ActorCommitRetry::new(&SupervisorConfig::default());
        let busy = BatchError::batch(RegistryError::Busy);

        let delays: Vec<_> = (0..9).map(|_| retry.next(&busy)).collect();

        let expected = [100, 200, 400, 800, 1600, 3200, 5000, 5000, 5000]
            .map(|ms| CommitRetry::Wait(Duration::from_millis(ms)));
        assert_eq!(delays, expected);
    }

    #[test]
    fn a_full_or_unavailable_registry_degrades_and_retries_every_5_seconds() {
        let mut retry = ActorCommitRetry::new(&SupervisorConfig::default());
        let errors = [
            RegistryError::NoSpace,
            RegistryError::Unavailable,
            RegistryError::Io {
                detail: "disk I/O error".to_owned(),
            },
        ];

        for error in errors {
            for _ in 0..3 {
                assert_eq!(
                    retry.next(&BatchError::batch(error.clone())),
                    CommitRetry::Degraded(Duration::from_secs(5))
                );
            }
        }
    }

    #[test]
    fn any_other_error_gives_up_at_once() {
        let mut retry = ActorCommitRetry::new(&SupervisorConfig::default());
        let error = BatchError::at(
            0,
            RegistryError::NotFound {
                resource: ResourceRef::Instance(InstanceId::generate()),
            },
        );

        assert_eq!(retry.next(&error), CommitRetry::GiveUp);
    }

    fn at() -> jiff::Timestamp {
        "2026-10-20T08:00:00Z".parse().unwrap()
    }
}
