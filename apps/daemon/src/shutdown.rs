//! Pieces of the stop order (spec 0006, *Berhenti*): the signals that
//! start it and the count of running requests it drains.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use stackly_core::operation::limits::OPERATION_SHUTDOWN_GRACE;
use tokio::sync::Notify;

use crate::log_sink::LOG_CONTROL_TIMEOUT;

/// How long the stop waits for running requests.
pub const SHUTDOWN_DRAIN: Duration = Duration::from_secs(5);
/// How long each connection's writer gets to send what it still holds.
pub const SHUTDOWN_FLUSH: Duration = Duration::from_secs(1);

/// The `ExitTimeOut` of the launchd job: after it launchd sends `SIGKILL`.
/// The plist of the app (`xtask/src/build_app/plist.rs`) writes the same
/// number.
pub const LAUNCHD_EXIT_TIMEOUT: Duration = Duration::from_secs(30);
/// What the end of a login session keeps free in `ExitTimeOut` for what the
/// stop order does not count: the runtime's own shutdown and the exit
/// (spec 0031, *Akhir sesi*).
pub const SESSION_END_MARGIN: Duration = Duration::from_secs(3);

/// The longest the stop order of spec 0006 takes: step 4 waits for the
/// longer of the drain and the engine's grace, step 5 flushes the
/// connections, and step 7 drains the log queue.
#[must_use]
pub const fn stop_order_bound() -> Duration {
    let wait = if SHUTDOWN_DRAIN.as_millis() > OPERATION_SHUTDOWN_GRACE.as_millis() {
        SHUTDOWN_DRAIN
    } else {
        OPERATION_SHUTDOWN_GRACE
    };
    wait.saturating_add(SHUTDOWN_FLUSH)
        .saturating_add(LOG_CONTROL_TIMEOUT)
}

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

/// The signals that stop the daemon: `SIGTERM`, `SIGINT`, and `SIGHUP`
/// (spec 0031, *Sinyal per konteks*). Only `SIGTERM` at a launchd daemon
/// can mean a logout.
#[cfg(unix)]
#[derive(Debug)]
pub struct StopSignals {
    term: tokio::signal::unix::Signal,
    interrupt: tokio::signal::unix::Signal,
    hangup: tokio::signal::unix::Signal,
}

#[cfg(unix)]
impl StopSignals {
    /// Installs the handlers. Call it inside a tokio runtime; from then on
    /// SIGTERM, SIGINT, and SIGHUP no longer end the process by themselves.
    ///
    /// # Errors
    ///
    /// Returns the OS error when a handler cannot be installed.
    pub fn install() -> std::io::Result<Self> {
        use tokio::signal::unix::{SignalKind, signal};

        Ok(Self {
            term: signal(SignalKind::terminate())?,
            interrupt: signal(SignalKind::interrupt())?,
            hangup: signal(SignalKind::hangup())?,
        })
    }

    /// Waits for the next of the three and returns its name.
    pub async fn recv(&mut self) -> &'static str {
        tokio::select! {
            _ = self.term.recv() => "SIGTERM",
            _ = self.interrupt.recv() => "SIGINT",
            _ = self.hangup.recv() => "SIGHUP",
        }
    }
}

#[cfg(test)]
mod tests {
    use stackly_core::supervisor::SESSION_END_STOP_BUDGET;
    use tokio::time::timeout;

    use super::*;

    #[test]
    fn the_session_end_budget_the_stop_order_and_the_margin_fit_in_the_exit_timeout() {
        assert_eq!(stop_order_bound(), Duration::from_secs(12));
        assert!(
            SESSION_END_STOP_BUDGET + stop_order_bound() + SESSION_END_MARGIN
                <= LAUNCHD_EXIT_TIMEOUT,
            "launchd would send SIGKILL before the daemon exits"
        );
    }

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
