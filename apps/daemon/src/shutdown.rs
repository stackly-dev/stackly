//! Pieces of the stop order (spec 0006, *Berhenti*): the signals that
//! start it and the count of running requests it drains.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use tokio::sync::Notify;

/// How long the stop waits for running requests.
pub const SHUTDOWN_DRAIN: Duration = Duration::from_secs(5);
/// How long each connection's writer gets to send what it still holds.
pub const SHUTDOWN_FLUSH: Duration = Duration::from_secs(1);

/// The requests that run right now, daemon wide. Event forwarders are not
/// requests and are not counted.
#[derive(Debug, Default)]
pub struct Drain {
    inner: Arc<DrainInner>,
}

#[derive(Debug, Default)]
struct DrainInner {
    running: AtomicUsize,
    idle: Notify,
}

impl Drain {
    /// Counts one more running request until the guard drops.
    #[must_use]
    pub fn enter(&self) -> DrainGuard {
        self.inner.running.fetch_add(1, Ordering::SeqCst);
        DrainGuard {
            inner: Arc::clone(&self.inner),
        }
    }

    /// How many requests run right now.
    #[must_use]
    pub fn running(&self) -> usize {
        self.inner.running.load(Ordering::SeqCst)
    }

    /// Resolves once no request runs.
    pub async fn idle(&self) {
        loop {
            let notified = self.inner.idle.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.running() == 0 {
                return;
            }
            notified.await;
        }
    }
}

/// One running request; dropping it ends the count.
#[derive(Debug)]
pub struct DrainGuard {
    inner: Arc<DrainInner>,
}

impl Drop for DrainGuard {
    fn drop(&mut self) {
        if self.inner.running.fetch_sub(1, Ordering::SeqCst) == 1 {
            self.inner.idle.notify_waiters();
        }
    }
}

/// The two signals that stop the daemon.
#[cfg(unix)]
#[derive(Debug)]
pub struct StopSignals {
    term: tokio::signal::unix::Signal,
    interrupt: tokio::signal::unix::Signal,
}

#[cfg(unix)]
impl StopSignals {
    /// Installs the handlers. Call it inside a tokio runtime; from then on
    /// SIGTERM and SIGINT no longer end the process by themselves.
    ///
    /// # Errors
    ///
    /// Returns the OS error when a handler cannot be installed.
    pub fn install() -> std::io::Result<Self> {
        use tokio::signal::unix::{SignalKind, signal};

        Ok(Self {
            term: signal(SignalKind::terminate())?,
            interrupt: signal(SignalKind::interrupt())?,
        })
    }

    /// Waits for the next of the two and returns its name.
    pub async fn recv(&mut self) -> &'static str {
        tokio::select! {
            _ = self.term.recv() => "SIGTERM",
            _ = self.interrupt.recv() => "SIGINT",
        }
    }
}

#[cfg(test)]
mod tests {
    use tokio::time::timeout;

    use super::*;

    #[tokio::test]
    async fn idle_resolves_at_once_without_requests_and_after_the_last_one_ends() {
        let drain = Drain::default();
        timeout(Duration::from_secs(1), drain.idle()).await.unwrap();

        let first = drain.enter();
        let second = drain.enter();
        assert_eq!(drain.running(), 2);
        let waiting = tokio::spawn({
            let inner = Arc::clone(&drain.inner);
            async move { Drain { inner }.idle().await }
        });
        drop(first);
        tokio::task::yield_now().await;
        assert!(!waiting.is_finished());
        drop(second);

        timeout(Duration::from_secs(1), waiting)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(drain.running(), 0);
    }
}
