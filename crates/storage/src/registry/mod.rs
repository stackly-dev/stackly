//! `SqliteRegistry`: a cloneable handle to the one thread that owns the
//! registry's write connection, and its [`Registry`] implementation.
//!
//! Every call is one command on that thread, and one command is one
//! transaction: a read transaction for reads, `BEGIN IMMEDIATE` for a
//! commit. No transaction stays open between two commands (REQ-048).

mod column;
mod journal;
mod prune;
mod read;
mod secret;
mod supervision;
mod write;

use std::fmt;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::thread::JoinHandle;

use jiff::Timestamp;
use rusqlite::{Connection, Transaction};
use stackly_domain::{
    ArtifactRecord, AuditPage, AuditQuery, BatchError, Committed, ConfigRevisionRecord,
    Environment, EnvironmentId, IdempotencyKey, Instance, InstanceFilter, InstanceId, InstanceName,
    InstancePathEntry, JournalView, Lease, LeaseFilter, Operation, OperationId, OperationJournal,
    OperationPage, OperationQuery, ProcessObservation, ProviderRevisionKey, ProviderRevisionRecord,
    PruneReport, Registry, RegistryError, RetentionPolicy, Runtime, RuntimeId, RuntimeReferences,
    SecretRef, SecretReferenceFilter, SecretReferenceView, SecretUse, SecretUser, Sha256Digest,
    Versioned, WriteBatch,
};
use stackly_platform::Clock;

use crate::error::StorageError;
pub(crate) use column::sql_error;
use prune::{History, Limits};
use read::InstanceSelect;

/// Capacity of the queue in front of the registry thread. A caller that
/// finds it full waits (backpressure, NFR-004).
pub const REGISTRY_QUEUE_CAPACITY: usize = 64;
/// Name of the registry thread.
const THREAD_NAME: &str = "stackly-registry";

/// One unit of work for the registry thread: it runs one transaction at
/// most and replies through its own channel.
type Job = Box<dyn FnOnce(&mut Connection) + Send>;

enum Message {
    Run(Job),
    Shutdown,
}

/// A cloneable handle to the registry thread. Dropping the last clone stops
/// the thread: it runs `PRAGMA optimize`, closes the connection, and the
/// drop waits for it.
///
/// Instances read back with readiness and health `unknown`, reason
/// `daemon_restarted`, observed at [`OpenSummary::opened_at`]; commits are
/// stamped with one reading of the platform clock each.
///
/// [`OpenSummary::opened_at`]: crate::OpenSummary::opened_at
#[derive(Clone)]
pub struct SqliteRegistry {
    inner: Arc<Inner>,
}

struct Inner {
    sender: SyncSender<Message>,
    thread: Option<JoinHandle<()>>,
    clock: Arc<dyn Clock>,
    opened_at: Timestamp,
}

impl SqliteRegistry {
    /// Moves the write connection onto a new registry thread.
    pub(crate) fn start(
        conn: Connection,
        clock: Arc<dyn Clock>,
        opened_at: Timestamp,
    ) -> Result<Self, StorageError> {
        let (sender, receiver) = mpsc::sync_channel(REGISTRY_QUEUE_CAPACITY);
        let thread = std::thread::Builder::new()
            .name(THREAD_NAME.to_owned())
            .spawn(move || serve(conn, &receiver))
            .map_err(|err| StorageError::Io {
                op: "spawn",
                path: THREAD_NAME.into(),
                detail: err.to_string(),
            })?;
        Ok(Self {
            inner: Arc::new(Inner {
                sender,
                thread: Some(thread),
                clock,
                opened_at,
            }),
        })
    }

    /// Runs `job` on the registry thread and waits for what it returns.
    /// Blocks while the queue is full. Fails with `Unavailable` once the
    /// thread is gone.
    fn call<R, F>(&self, job: F) -> Result<R, RegistryError>
    where
        R: Send + 'static,
        F: FnOnce(&mut Connection) -> R + Send + 'static,
    {
        let (reply, result) = mpsc::sync_channel(1);
        let job: Job = Box::new(move |conn| {
            // The caller may have stopped waiting; nothing to do then.
            let _ = reply.send(job(conn));
        });
        self.inner
            .sender
            .send(Message::Run(job))
            .map_err(|_| RegistryError::Unavailable)?;
        result.recv().map_err(|_| RegistryError::Unavailable)
    }

    /// Runs a fallible `job` on the registry thread.
    pub(crate) fn run<R, F>(&self, job: F) -> Result<R, RegistryError>
    where
        R: Send + 'static,
        F: FnOnce(&mut Connection) -> Result<R, RegistryError> + Send + 'static,
    {
        self.call(job)?
    }

    /// Runs `job` in one read transaction and pairs what it read with the
    /// generation read in the same transaction.
    fn read<R, F>(&self, job: F) -> Result<Versioned<R>, RegistryError>
    where
        R: Send + 'static,
        F: FnOnce(&Transaction<'_>) -> Result<R, RegistryError> + Send + 'static,
    {
        self.run(move |conn| {
            let tx = conn.transaction().map_err(sql_error)?;
            let generation = read::generation(&tx)?;
            let value = job(&tx)?;
            tx.commit().map_err(sql_error)?;
            Ok(Versioned::new(value, generation))
        })
    }
}

impl Registry for SqliteRegistry {
    fn provider_revision(
        &self,
        key: &ProviderRevisionKey,
    ) -> Result<Versioned<Option<ProviderRevisionRecord>>, RegistryError> {
        let key = key.clone();
        self.read(move |tx| read::provider_revision(tx, &key))
    }

    fn artifact(
        &self,
        sha256: &Sha256Digest,
    ) -> Result<Versioned<Option<ArtifactRecord>>, RegistryError> {
        let sha256 = *sha256;
        self.read(move |tx| read::artifact(tx, &sha256))
    }

    fn runtime(&self, id: RuntimeId) -> Result<Versioned<Option<Runtime>>, RegistryError> {
        self.read(move |tx| Ok(read::runtimes(tx, Some(id))?.into_iter().next()))
    }

    fn runtimes(&self) -> Result<Versioned<Vec<Runtime>>, RegistryError> {
        self.read(|tx| read::runtimes(tx, None))
    }

    fn runtime_references(
        &self,
        id: RuntimeId,
    ) -> Result<Versioned<RuntimeReferences>, RegistryError> {
        self.read(move |tx| read::runtime_references(tx, id))
    }

    fn instance(&self, id: InstanceId) -> Result<Versioned<Option<Instance>>, RegistryError> {
        let loaded_at = self.inner.opened_at;
        self.read(move |tx| {
            let select = InstanceSelect::Id(id);
            Ok(read::instances(tx, &select, loaded_at)?.into_iter().next())
        })
    }

    fn instance_by_name(
        &self,
        name: &InstanceName,
    ) -> Result<Versioned<Option<Instance>>, RegistryError> {
        let name = name.clone();
        let loaded_at = self.inner.opened_at;
        self.read(move |tx| {
            let select = InstanceSelect::Name(name.as_str());
            Ok(read::instances(tx, &select, loaded_at)?.into_iter().next())
        })
    }

    fn instances(&self, filter: InstanceFilter) -> Result<Versioned<Vec<Instance>>, RegistryError> {
        let loaded_at = self.inner.opened_at;
        self.read(move |tx| read::instances(tx, &InstanceSelect::List(filter), loaded_at))
    }

    fn known_instance_ids(&self) -> Result<Versioned<Vec<InstanceId>>, RegistryError> {
        self.read(|tx| read::known_instance_ids(tx))
    }

    fn latest_process_observation(
        &self,
        instance: InstanceId,
    ) -> Result<Versioned<Option<ProcessObservation>>, RegistryError> {
        self.read(move |tx| supervision::latest_observation(tx, instance))
    }

    fn active_config_revision(
        &self,
        instance: InstanceId,
    ) -> Result<Versioned<Option<ConfigRevisionRecord>>, RegistryError> {
        self.read(move |tx| supervision::active_config_revision(tx, instance))
    }

    fn instance_paths(
        &self,
        id: InstanceId,
    ) -> Result<Versioned<Option<Vec<InstancePathEntry>>>, RegistryError> {
        self.read(move |tx| read::instance_paths(tx, id))
    }

    fn environment(
        &self,
        id: EnvironmentId,
    ) -> Result<Versioned<Option<Environment>>, RegistryError> {
        self.read(move |tx| Ok(read::environments(tx, Some(id))?.into_iter().next()))
    }

    fn environments(&self) -> Result<Versioned<Vec<Environment>>, RegistryError> {
        self.read(|tx| read::environments(tx, None))
    }

    fn leases(
        &self,
        instance: InstanceId,
        filter: LeaseFilter,
    ) -> Result<Versioned<Vec<Lease>>, RegistryError> {
        self.read(move |tx| read::leases(tx, instance, filter))
    }

    fn operation(&self, id: OperationId) -> Result<Versioned<Option<Operation>>, RegistryError> {
        self.read(move |tx| read::operation(tx, id))
    }

    fn operations(&self, query: OperationQuery) -> Result<Versioned<OperationPage>, RegistryError> {
        query.validate()?;
        self.read(move |tx| read::operations(tx, &query))
    }

    fn operation_journal(
        &self,
        id: OperationId,
    ) -> Result<Versioned<Option<JournalView>>, RegistryError> {
        self.read(move |tx| journal::journal_view(tx, id))
    }

    fn operation_journals(
        &self,
        ids: &[OperationId],
    ) -> Result<Versioned<Vec<OperationJournal>>, RegistryError> {
        let ids = ids.to_vec();
        self.read(move |tx| journal::journals(tx, &ids))
    }

    fn operation_by_idempotency_key(
        &self,
        key: &IdempotencyKey,
    ) -> Result<Versioned<Option<(Operation, OperationJournal)>>, RegistryError> {
        let key = key.clone();
        self.read(move |tx| journal::by_idempotency_key(tx, &key))
    }

    fn unfinished_operations(
        &self,
    ) -> Result<Versioned<Vec<(Operation, JournalView)>>, RegistryError> {
        self.read(|tx| journal::unfinished(tx))
    }

    fn secret_reference(
        &self,
        reference: &SecretRef,
    ) -> Result<Versioned<Option<SecretReferenceView>>, RegistryError> {
        let reference = reference.clone();
        self.read(move |tx| secret::reference(tx, &reference))
    }

    fn secret_references(
        &self,
        filter: SecretReferenceFilter,
    ) -> Result<Versioned<Vec<SecretReferenceView>>, RegistryError> {
        self.read(move |tx| secret::references(tx, filter))
    }

    fn secret_uses_by(&self, user: SecretUser) -> Result<Versioned<Vec<SecretUse>>, RegistryError> {
        self.read(move |tx| secret::uses_by(tx, user))
    }

    fn audit_events(&self, query: AuditQuery) -> Result<Versioned<AuditPage>, RegistryError> {
        query.validate()?;
        self.read(move |tx| secret::audit_events(tx, &query))
    }

    fn commit(&self, batch: WriteBatch) -> Result<Committed, BatchError> {
        let now = self.inner.clock.now();
        self.call(move |conn| write::commit(conn, &batch, now))
            .map_err(BatchError::batch)?
    }

    fn prune(
        &self,
        policy: &RetentionPolicy,
        now: Timestamp,
    ) -> Result<PruneReport, RegistryError> {
        let limits = Limits::new(policy, now);
        let mut report = PruneReport::default();
        for history in History::ALL {
            loop {
                let batch_limits = limits.clone();
                let deleted =
                    self.run(move |conn| prune::delete_batch(conn, &batch_limits, history))?;
                let count = match history {
                    History::Operations => &mut report.operations,
                    History::AuditEvents => &mut report.audit_events,
                    History::ReleasedLeases => &mut report.released_leases,
                    History::ProcessObservations => &mut report.process_observations,
                };
                *count += u64::try_from(deleted).unwrap_or(u64::MAX);
                if deleted < history.batch_rows() {
                    break;
                }
            }
        }
        Ok(report)
    }
}

impl fmt::Debug for SqliteRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SqliteRegistry")
            .field("opened_at", &self.inner.opened_at)
            .finish_non_exhaustive()
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        // The thread may be gone already, after a panic.
        let _ = self.sender.send(Message::Shutdown);
        if let Some(thread) = self.thread.take()
            && thread.join().is_err()
        {
            tracing::error!("the registry thread panicked");
        }
    }
}

/// The registry thread: one command at a time until shutdown, then
/// `PRAGMA optimize` and close.
fn serve(mut conn: Connection, receiver: &Receiver<Message>) {
    while let Ok(Message::Run(job)) = receiver.recv() {
        job(&mut conn);
    }
    if let Err(err) = conn.execute_batch("PRAGMA optimize") {
        tracing::warn!(error = %err, "PRAGMA optimize failed while closing the registry");
    }
    if let Err((_, err)) = conn.close() {
        tracing::error!(error = %err, "closing the registry failed");
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Barrier;
    use std::time::Duration;

    use stackly_platform::SystemClock;

    use super::*;

    fn registry() -> SqliteRegistry {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE t (x INTEGER NOT NULL CHECK (x > 0)) STRICT;")
            .unwrap();
        SqliteRegistry::start(conn, Arc::new(SystemClock), Timestamp::UNIX_EPOCH).unwrap()
    }

    #[test]
    fn commands_run_on_the_named_registry_thread_one_at_a_time() {
        let registry = registry();

        let name = registry
            .run(|_| Ok(std::thread::current().name().map(ToOwned::to_owned)))
            .unwrap();
        let clone = registry.clone();
        let inserted = std::thread::spawn(move || {
            clone.run(|conn| {
                conn.execute("INSERT INTO t VALUES (1)", [])
                    .map_err(sql_error)
            })
        })
        .join()
        .unwrap()
        .unwrap();
        let count: i64 = registry
            .run(|conn| {
                conn.query_row("SELECT count(*) FROM t", [], |row| row.get(0))
                    .map_err(sql_error)
            })
            .unwrap();

        assert_eq!(name.as_deref(), Some(THREAD_NAME));
        assert_eq!((inserted, count), (1, 1));
    }

    #[test]
    fn a_constraint_maps_to_integrity_and_other_errors_to_io() {
        let registry = registry();

        let err = registry
            .run(|conn| {
                conn.execute("INSERT INTO t VALUES (0)", [])
                    .map_err(sql_error)
            })
            .unwrap_err();
        assert!(matches!(err, RegistryError::Integrity { .. }), "{err:?}");
        let err = registry
            .run(|conn| {
                conn.execute("INSERT INTO missing VALUES (1)", [])
                    .map_err(sql_error)
            })
            .unwrap_err();
        assert!(matches!(err, RegistryError::Io { .. }), "{err:?}");
    }

    #[test]
    fn a_panicking_command_makes_every_later_call_unavailable() {
        let registry = registry();

        let err = registry
            .run::<(), _>(|_| panic!("simulated registry thread panic"))
            .unwrap_err();
        assert_eq!(err, RegistryError::Unavailable);
        assert_eq!(registry.run(|_| Ok(())), Err(RegistryError::Unavailable));
        let err = registry.commit(WriteBatch::default()).unwrap_err();
        assert_eq!(err, BatchError::batch(RegistryError::Unavailable));
    }

    #[test]
    fn a_full_queue_blocks_the_sender_until_the_thread_catches_up() {
        let registry = registry();
        let gate = Arc::new(Barrier::new(2));
        let held = Arc::clone(&gate);
        let first = registry.clone();
        let blocker = std::thread::spawn(move || {
            first.run(move |_| {
                held.wait();
                Ok(())
            })
        });
        std::thread::sleep(Duration::from_millis(50));
        // The thread is busy with the first command, so these fill the queue.
        let senders: Vec<_> = (0..REGISTRY_QUEUE_CAPACITY + 4)
            .map(|_| {
                let registry = registry.clone();
                std::thread::spawn(move || registry.run(|_| Ok(())))
            })
            .collect();
        gate.wait();

        blocker.join().unwrap().unwrap();
        for sender in senders {
            sender.join().unwrap().unwrap();
        }
    }

    #[test]
    fn dropping_the_last_handle_closes_the_connection_and_joins_the_thread() {
        let registry = registry();
        let clone = registry.clone();
        drop(registry);

        assert_eq!(clone.run(|_| Ok(7)), Ok(7));
        drop(clone);
    }
}
