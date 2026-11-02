//! Wake from sleep (spec 0028 AC-9, *Wake dan masa tenggang*): a task that
//! notices the Mac slept, and the grace after it that the probe loops
//! apply.
//!
//! The wall clock keeps running while the Mac sleeps and tokio's monotonic
//! clock stops (proved in the spec 0028 spike), so the wall time that
//! passed between two ticks minus the monotonic time that passed is the
//! time spent asleep. A wall clock that moved back is ignored; one the user
//! moved forward reads as a wake, which costs one extra probe.

use std::future::pending;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use jiff::{SignedDuration, Timestamp};
use stackly_domain::ReasonCode;
use stackly_domain::{Health, HealthState, ObservationDetail, Readiness, ReadinessState};
use tokio::sync::watch;
use tokio::time::Instant;

use super::{Supervisor, WeakSupervisor};

/// Pause between two comparisons of the wall clock with the monotonic
/// clock; a wake is noticed at most this long after it.
pub const WAKE_CHECK_INTERVAL: Duration = Duration::from_secs(2);
/// How much more the wall clock must have moved than the monotonic clock
/// before the difference counts as a sleep.
pub const WAKE_GAP_THRESHOLD: Duration = Duration::from_secs(5);
/// How long after a wake a failed probe changes nothing and counts for
/// nothing, so a connection the OS closed during sleep is not read as a
/// failure of the instance.
pub const WAKE_GRACE: Duration = Duration::from_secs(30);

/// The reason of an observation made right after a wake, before the first
/// probe answers.
pub const WAKING: ReasonCode = ReasonCode::from_static("waking");

/// The detail of that observation.
const WAKING_DETAIL: &str = "rechecking after sleep";

/// One noticed sleep.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Wake {
    /// How long the Mac slept, as far as the two clocks tell.
    pub slept_for: Duration,
    /// The wall clock time at which the wake was noticed.
    pub detected_at: Timestamp,
}

/// The `unknown` readiness and health a wake leaves until a probe answers.
pub(crate) fn waking(wake: &Wake) -> (Readiness, Health) {
    let detail = || WAKING_DETAIL.parse::<ObservationDetail>().ok();
    (
        Readiness::new(ReadinessState::Unknown, wake.detected_at, WAKING, detail()),
        Health::new(HealthState::Unknown, wake.detected_at, WAKING, detail()),
    )
}

/// Where wakes are announced: one per daemon, shared by every probe loop.
pub(crate) struct WakeWatch {
    sender: watch::Sender<Option<Wake>>,
    started: AtomicBool,
}

impl WakeWatch {
    pub(crate) fn new() -> Self {
        Self {
            sender: watch::channel(None).0,
            started: AtomicBool::new(false),
        }
    }

    /// Announces `wake` to every receiver.
    pub(crate) fn announce(&self, wake: Wake) {
        self.sender.send_replace(Some(wake));
    }

    pub(crate) fn subscribe(&self) -> WakeReceiver {
        WakeReceiver(self.sender.subscribe())
    }
}

/// Hears the wakes announced after it was made.
pub(crate) struct WakeReceiver(watch::Receiver<Option<Wake>>);

impl WakeReceiver {
    /// Waits for the next wake. Never ends while the supervisor lives.
    pub(crate) async fn next(&mut self) -> Wake {
        loop {
            if self.0.changed().await.is_err() {
                // The sender lives in the supervisor the caller holds.
                pending::<()>().await;
            }
            if let Some(wake) = *self.0.borrow_and_update() {
                return wake;
            }
        }
    }
}

/// The time asleep between two ticks, if it is more than `threshold`.
fn slept(wall: SignedDuration, monotonic: Duration, threshold: Duration) -> Option<Duration> {
    let monotonic = SignedDuration::try_from(monotonic).unwrap_or(SignedDuration::MAX);
    let gap = wall.checked_sub(monotonic)?;
    let threshold = SignedDuration::try_from(threshold).unwrap_or(SignedDuration::MAX);
    if gap > threshold {
        Duration::try_from(gap).ok()
    } else {
        None
    }
}

impl Supervisor {
    /// Starts the wake watch if it does not run yet: one task for the
    /// whole daemon that ends at [`shutdown`](Self::shutdown). The probe
    /// loops call it, so a supervisor that never probes never watches.
    pub(crate) fn start_wake_watch(&self) {
        if self.shared.wake.started.swap(true, Ordering::SeqCst) {
            return;
        }
        tokio::spawn(watch_for_wake(self.downgrade()));
    }

    /// Returns a receiver of the wakes from now on.
    pub(crate) fn subscribe_wake(&self) -> WakeReceiver {
        self.shared.wake.subscribe()
    }
}

async fn watch_for_wake(weak: WeakSupervisor) {
    let Some(sup) = weak.upgrade() else {
        return;
    };
    let config = sup.deps().config.clone();
    let clock = sup.deps().platform.clone();
    drop(sup);
    let mut wall = clock.clock().now();
    let mut mono = Instant::now();
    loop {
        tokio::time::sleep(config.wake_check_interval).await;
        let Some(sup) = weak.upgrade() else {
            return;
        };
        if sup.is_shutdown() {
            return;
        }
        let (now_wall, now_mono) = (clock.clock().now(), Instant::now());
        let gap = slept(
            now_wall.duration_since(wall),
            now_mono.duration_since(mono),
            config.wake_gap_threshold,
        );
        (wall, mono) = (now_wall, now_mono);
        if let Some(slept_for) = gap {
            tracing::info!(
                slept_seconds = slept_for.as_secs(),
                "the Mac woke from sleep"
            );
            sup.shared.wake.announce(Wake {
                slept_for,
                detected_at: now_wall,
            });
        }
    }
}

/// Consecutive failures and the end of the grace of one observation.
#[derive(Debug, Default)]
pub(crate) struct Track {
    /// Failed probes in a row that counted.
    pub(crate) failures: u32,
    grace_until: Option<Instant>,
}

impl Track {
    /// Starts the grace of a wake: the count is cleared, and failures
    /// before `until` do not count.
    pub(crate) fn wake(&mut self, until: Instant) {
        self.failures = 0;
        self.grace_until = Some(until);
    }

    /// A probe succeeded: the count and the grace of this observation go.
    pub(crate) fn succeeded(&mut self) {
        self.failures = 0;
        self.grace_until = None;
    }

    /// A probe failed at `now`. Returns false while the grace lasts: the
    /// failure changes nothing and is not counted. Otherwise it counts and
    /// returns true.
    pub(crate) fn failed(&mut self, now: Instant) -> bool {
        if self.grace_until.is_some_and(|until| now < until) {
            return false;
        }
        self.grace_until = None;
        self.failures = self.failures.saturating_add(1);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const THRESHOLD: Duration = Duration::from_secs(5);

    fn secs(seconds: i64) -> SignedDuration {
        SignedDuration::from_secs(seconds)
    }

    #[test]
    fn the_constants_are_the_values_of_spec_0028() {
        assert_eq!(WAKE_CHECK_INTERVAL, Duration::from_secs(2));
        assert_eq!(WAKE_GAP_THRESHOLD, Duration::from_secs(5));
        assert_eq!(WAKE_GRACE, Duration::from_secs(30));
        assert_eq!(WAKING.as_str(), "waking");
    }

    #[test]
    fn equal_clocks_are_no_sleep() {
        assert_eq!(slept(secs(2), Duration::from_secs(2), THRESHOLD), None);
    }

    #[test]
    fn a_gap_of_exactly_the_threshold_is_no_sleep() {
        assert_eq!(slept(secs(7), Duration::from_secs(2), THRESHOLD), None);
    }

    #[test]
    fn a_larger_gap_is_the_time_asleep() {
        assert_eq!(
            slept(secs(3602), Duration::from_secs(2), THRESHOLD),
            Some(Duration::from_secs(3600))
        );
    }

    #[test]
    fn a_wall_clock_that_moved_back_is_ignored() {
        assert_eq!(slept(secs(-3600), Duration::from_secs(2), THRESHOLD), None);
    }

    #[test]
    fn a_failure_inside_the_grace_counts_for_nothing() {
        let start = Instant::now();
        let mut track = Track {
            failures: 1,
            ..Track::default()
        };
        track.wake(start + WAKE_GRACE);

        assert!(!track.failed(start + Duration::from_secs(29)));
        assert_eq!(track.failures, 0);
    }

    #[test]
    fn a_failure_after_the_grace_counts_and_the_grace_is_gone() {
        let start = Instant::now();
        let mut track = Track::default();
        track.wake(start + WAKE_GRACE);

        assert!(track.failed(start + WAKE_GRACE));
        assert_eq!(track.failures, 1);
        assert!(track.failed(start + Duration::from_secs(31)));
        assert_eq!(track.failures, 2);
    }

    #[test]
    fn a_success_ends_the_grace_and_clears_the_count() {
        let start = Instant::now();
        let mut track = Track::default();
        track.wake(start + WAKE_GRACE);

        track.succeeded();

        assert!(track.failed(start + Duration::from_secs(1)));
        assert_eq!(track.failures, 1);
    }

    #[test]
    fn a_wake_reads_unknown_with_the_reason_waking() {
        let wake = Wake {
            slept_for: Duration::from_secs(60),
            detected_at: "2026-01-01T00:00:00Z".parse().unwrap(),
        };

        let (readiness, health) = waking(&wake);

        assert_eq!(readiness.state(), ReadinessState::Unknown);
        assert_eq!(readiness.reason(), &WAKING);
        assert_eq!(readiness.observed_at(), wake.detected_at);
        assert_eq!(readiness.latency(), None);
        assert_eq!(health.state(), HealthState::Unknown);
        assert_eq!(
            health.detail().map(ObservationDetail::as_str),
            Some("rechecking after sleep")
        );
    }
}
