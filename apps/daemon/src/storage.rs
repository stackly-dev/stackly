//! The storage method (spec 0022, *Kapasitas disk*): `storage.get_capacity`
//! reads the space left on the volume that holds the data root.
//!
//! The read runs on a blocking thread through one single flight slot: a
//! call that arrives while a read runs waits for the same result. A read
//! that passes [`CAPACITY_READ_TIMEOUT`] answers `storage.capacity_unavailable`
//! with reason `timeout`, and the thread that still waits on `statfs` is
//! left to finish and its result dropped, so at most one thread is held. The
//! method takes no registry lock, starts no operation, and sends no event.

use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use stackly_core::storage::{CapacityError, CapacityReading, capacity};
use stackly_domain::CapacityLevel as DomainLevel;
use stackly_platform::{Platform, RootKind};
use stackly_protocol::codes::storage;
use stackly_protocol::storage::{CapacityLevel, CapacityReport};
use stackly_protocol::{Count, ErrorBody, Timestamp};
use tokio::sync::watch;

use crate::dispatch::HandlerFuture;
use crate::state::DaemonCore;

/// How long one read may take before the call answers `timeout`.
pub const CAPACITY_READ_TIMEOUT: Duration = Duration::from_secs(5);

/// Why a read produced no report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Failure {
    /// The volume could not be read.
    Io(io::ErrorKind),
    /// The volume reports a size of 0.
    NoSize,
    /// The read took longer than the timeout.
    Timeout,
}

type Outcome = Result<CapacityReport, Failure>;
type Flight = watch::Receiver<Option<Outcome>>;

/// The single flight slot of `storage.get_capacity` and its timeout.
#[derive(Debug)]
pub struct Storage {
    flight: Arc<Mutex<Option<Flight>>>,
    timeout_millis: AtomicU64,
}

impl Default for Storage {
    fn default() -> Self {
        Self {
            flight: Arc::default(),
            timeout_millis: AtomicU64::new(millis(CAPACITY_READ_TIMEOUT)),
        }
    }
}

fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Storage {
    /// Changes how long a read may take; tests shorten it
    /// (`DaemonConfig.capacity_read_timeout`).
    pub fn set_read_timeout(&self, timeout: Duration) {
        self.timeout_millis
            .store(millis(timeout), Ordering::Relaxed);
    }

    fn read_timeout(&self) -> Duration {
        Duration::from_millis(self.timeout_millis.load(Ordering::Relaxed))
    }

    /// Reads the capacity of the data root's volume, or waits for the read
    /// already running.
    async fn read(&self, core: &DaemonCore) -> Outcome {
        let mut flight = {
            let mut slot = lock(&self.flight);
            match slot.as_ref() {
                Some(running) => running.clone(),
                None => {
                    let (sender, receiver) = watch::channel(None);
                    *slot = Some(receiver.clone());
                    let data_root = core.prepared().canonical_root(RootKind::Data).to_path_buf();
                    spawn_read(
                        core.platform().clone(),
                        data_root,
                        self.read_timeout(),
                        sender,
                        Arc::clone(&self.flight),
                    );
                    receiver
                }
            }
        };
        match flight.wait_for(Option::is_some).await {
            Ok(outcome) => outcome
                .clone()
                .unwrap_or(Err(Failure::Io(io::ErrorKind::Other))),
            // The reader task ended without an answer, which only happens
            // while the runtime shuts down.
            Err(_) => Err(Failure::Io(io::ErrorKind::Other)),
        }
    }
}

/// Starts the one read of a flight: a blocking thread under a timeout.
/// The answer reaches every waiter through `sender`; the slot is cleared
/// only after the thread ended, so a stuck `statfs` never gets company.
fn spawn_read(
    platform: Platform,
    data_root: PathBuf,
    timeout: Duration,
    sender: watch::Sender<Option<Outcome>>,
    slot: Arc<Mutex<Option<Flight>>>,
) {
    tokio::spawn(async move {
        let mut work = tokio::task::spawn_blocking(move || read_now(&platform, &data_root));
        let (outcome, still_running) = match tokio::time::timeout(timeout, &mut work).await {
            Ok(Ok(outcome)) => (outcome, false),
            Ok(Err(_panicked)) => (Err(Failure::Io(io::ErrorKind::Other)), false),
            Err(_elapsed) => (Err(Failure::Timeout), true),
        };
        sender.send_replace(Some(outcome));
        if still_running {
            let _ = work.await;
        }
        *lock(&slot) = None;
    });
}

fn read_now(platform: &Platform, data_root: &std::path::Path) -> Outcome {
    let now = platform.clock().now();
    capacity(platform.fs(), data_root, now)
        .map(|reading| report(&reading))
        .map_err(|error| match error {
            CapacityError::Io { kind } => Failure::Io(kind),
            _ => Failure::NoSize,
        })
}

fn report(reading: &CapacityReading) -> CapacityReport {
    CapacityReport {
        checked_at: Timestamp::from_jiff(reading.checked_at),
        total_bytes: Count::saturating(reading.total_bytes),
        available_bytes: Count::saturating(reading.available_bytes),
        used_percent: reading.used_percent,
        level: match reading.level {
            DomainLevel::Ok => CapacityLevel::Ok,
            DomainLevel::Filling => CapacityLevel::Filling,
            DomainLevel::Low => CapacityLevel::Low,
            DomainLevel::Critical => CapacityLevel::Critical,
            _ => CapacityLevel::Unrecognized,
        },
    }
}

/// The wire name of an I/O error kind: its variant name in snake case, such
/// as `permission_denied`.
fn kind_code(kind: io::ErrorKind) -> String {
    let mut code = String::new();
    for (index, character) in format!("{kind:?}").chars().enumerate() {
        if character.is_ascii_uppercase() {
            if index > 0 {
                code.push('_');
            }
            code.push(character.to_ascii_lowercase());
        } else {
            code.push(character);
        }
    }
    code
}

/// `storage.capacity_unavailable` for a failed read.
fn unavailable(failure: Failure) -> ErrorBody {
    let body = ErrorBody::new(
        &storage::CAPACITY_UNAVAILABLE,
        "disk space for the data folder is unavailable",
    );
    let body = match failure {
        Failure::Io(kind) => body
            .with_detail("reason", "io")
            .with_detail("kind", kind_code(kind)),
        Failure::NoSize => body.with_detail("reason", "no_size"),
        Failure::Timeout => body.with_detail("reason", "timeout"),
    };
    match storage::hint(body.code(), body.details()) {
        Some(hint) => body.with_hint(hint),
        None => body,
    }
}

/// The real `storage.get_capacity`.
pub fn get_capacity(core: &DaemonCore) -> HandlerFuture<'_, CapacityReport> {
    Box::pin(async move {
        core.storage()
            .read(core)
            .await
            .map_err(|failure| Box::new(unavailable(failure)))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_io_error_kind_is_written_in_snake_case() {
        assert_eq!(
            kind_code(io::ErrorKind::PermissionDenied),
            "permission_denied"
        );
        assert_eq!(kind_code(io::ErrorKind::NotFound), "not_found");
        assert_eq!(kind_code(io::ErrorKind::Other), "other");
    }

    #[test]
    fn every_failure_maps_to_the_catalog_code_with_its_reason() {
        let io = unavailable(Failure::Io(io::ErrorKind::PermissionDenied));
        let no_size = unavailable(Failure::NoSize);
        let timeout = unavailable(Failure::Timeout);

        assert_eq!(io.code(), "storage.capacity_unavailable");
        assert!(io.retryable());
        assert_eq!(io.detail("reason"), Some("io"));
        assert_eq!(io.detail("kind"), Some("permission_denied"));
        assert_eq!(io.hint(), None);
        assert_eq!(no_size.detail("reason"), Some("no_size"));
        assert_eq!(timeout.detail("reason"), Some("timeout"));
        assert!(timeout.hint().is_some());
    }
}
