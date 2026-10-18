//! `SqliteRegistry`: a cloneable handle to the one thread that owns the
//! registry's write connection.
//!
//! The `Registry` methods come with spec 0005 step 10; this module already
//! has the thread, the bounded queue, shutdown, and the mapping of SQLite
//! errors they will use.

use std::fmt;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::thread::JoinHandle;

use rusqlite::{Connection, ErrorCode};
use stackly_platform::Clock;

use crate::error::StorageError;

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

/// A failure of one command sent to the registry thread, before step 10
/// maps it onto `RegistryError`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "provisional: the Registry methods of spec 0005 step 10 use it"
    )
)]
pub(crate) enum CommandError {
    /// The disk is full (`SQLITE_FULL`).
    #[error("the disk holding the registry is full")]
    NoSpace,
    /// The database stayed locked past the busy timeout (`SQLITE_BUSY`).
    #[error("the registry is busy")]
    Busy,
    /// A SQL constraint refused the write: a check before it was missed.
    #[error("a registry constraint failed: {detail}")]
    Integrity {
        /// SQLite's message.
        detail: String,
    },
    /// The registry thread is gone, for example after a panic.
    #[error("the registry thread is not running")]
    Unavailable,
    /// Any other SQLite or I/O failure.
    #[error("registry I/O failed: {detail}")]
    Io {
        /// SQLite's message.
        detail: String,
    },
}

#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "provisional: the Registry methods of spec 0005 step 10 use it"
    )
)]
impl CommandError {
    /// Maps a SQLite error: `SQLITE_FULL` to `NoSpace`, `SQLITE_BUSY` to
    /// `Busy`, a constraint to `Integrity`, and anything else to `Io`.
    pub(crate) fn from_sqlite(err: &rusqlite::Error) -> Self {
        match err.sqlite_error_code() {
            Some(ErrorCode::DiskFull) => Self::NoSpace,
            Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked) => Self::Busy,
            Some(ErrorCode::ConstraintViolation) => Self::Integrity {
                detail: err.to_string(),
            },
            _ => Self::Io {
                detail: err.to_string(),
            },
        }
    }
}

/// A cloneable handle to the registry thread. Dropping the last clone stops
/// the thread: it runs `PRAGMA optimize`, closes the connection, and the
/// drop waits for it.
#[derive(Clone)]
pub struct SqliteRegistry {
    inner: Arc<Inner>,
}

struct Inner {
    sender: SyncSender<Message>,
    thread: Option<JoinHandle<()>>,
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "provisional: spec 0005 step 10 stamps commits with it"
        )
    )]
    clock: Arc<dyn Clock>,
}

impl SqliteRegistry {
    /// Moves the write connection onto a new registry thread.
    pub(crate) fn start(conn: Connection, clock: Arc<dyn Clock>) -> Result<Self, StorageError> {
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
            }),
        })
    }

    /// Runs `job` on the registry thread and waits for its result. Blocks
    /// while the queue is full.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "provisional: the Registry methods of spec 0005 step 10 use it"
        )
    )]
    pub(crate) fn run<R, F>(&self, job: F) -> Result<R, CommandError>
    where
        R: Send + 'static,
        F: FnOnce(&mut Connection) -> Result<R, CommandError> + Send + 'static,
    {
        let (reply, result) = mpsc::sync_channel(1);
        let job: Job = Box::new(move |conn| {
            // The caller may have stopped waiting; nothing to do then.
            let _ = reply.send(job(conn));
        });
        self.inner
            .sender
            .send(Message::Run(job))
            .map_err(|_| CommandError::Unavailable)?;
        result.recv().map_err(|_| CommandError::Unavailable)?
    }

    /// Returns the clock commits are stamped with.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "provisional: spec 0005 step 10 stamps commits with it"
        )
    )]
    pub(crate) fn clock(&self) -> &dyn Clock {
        &*self.inner.clock
    }
}

impl fmt::Debug for SqliteRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SqliteRegistry").finish_non_exhaustive()
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
        SqliteRegistry::start(conn, Arc::new(SystemClock)).unwrap()
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
                    .map_err(|err| CommandError::from_sqlite(&err))
            })
        })
        .join()
        .unwrap()
        .unwrap();
        let count: i64 = registry
            .run(|conn| {
                conn.query_row("SELECT count(*) FROM t", [], |row| row.get(0))
                    .map_err(|err| CommandError::from_sqlite(&err))
            })
            .unwrap();

        assert_eq!(name.as_deref(), Some(THREAD_NAME));
        assert_eq!((inserted, count), (1, 1));
        assert!(registry.clock().now() > jiff::Timestamp::UNIX_EPOCH);
    }

    #[test]
    fn a_constraint_maps_to_integrity_and_other_errors_to_io() {
        let registry = registry();

        let err = registry
            .run(|conn| {
                conn.execute("INSERT INTO t VALUES (0)", [])
                    .map_err(|err| CommandError::from_sqlite(&err))
            })
            .unwrap_err();
        assert!(matches!(err, CommandError::Integrity { .. }), "{err:?}");
        let err = registry
            .run(|conn| {
                conn.execute("INSERT INTO missing VALUES (1)", [])
                    .map_err(|err| CommandError::from_sqlite(&err))
            })
            .unwrap_err();
        assert!(matches!(err, CommandError::Io { .. }), "{err:?}");
    }

    #[test]
    fn a_panicking_command_makes_every_later_call_unavailable() {
        let registry = registry();

        let err = registry
            .run::<(), _>(|_| panic!("simulated registry thread panic"))
            .unwrap_err();
        assert_eq!(err, CommandError::Unavailable);
        assert_eq!(registry.run(|_| Ok(())), Err(CommandError::Unavailable));
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
