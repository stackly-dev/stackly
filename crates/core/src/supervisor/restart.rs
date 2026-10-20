//! The restart policy math (spec 0010, *Restart policy*): the pause before
//! a respawn, the random jitter, the crash budget in a sliding window, and
//! the reset after a healthy period.
//!
//! The history lives in the daemon's memory and runs on the monotonic tokio
//! clock, which stops while the Mac sleeps, so sleep never counts toward a
//! crash loop and tests can pause time.

use std::collections::VecDeque;
use std::num::NonZeroU32;
use std::time::Duration;

use tokio::time::Instant;

use super::SupervisorConfig;

/// Millionths in one whole jitter factor.
const MILLIONTHS: u32 = 1_000_000;
/// The smallest jitter factor, 0.5.
const JITTER_MIN: u32 = MILLIONTHS / 2;

/// The random factor, 0.5 to 1.0, a respawn pause is multiplied by
/// (`RESTART_JITTER`), held in millionths so it compares exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct JitterFactor(u32);

impl JitterFactor {
    /// The smallest factor, 0.5.
    pub const MIN: Self = Self(JITTER_MIN);
    /// The largest factor, 1.0: the full pause.
    pub const MAX: Self = Self(MILLIONTHS);

    /// Creates a factor of `millionths` / 1 000 000, or `None` outside
    /// 0.5 to 1.0.
    #[must_use]
    pub const fn from_millionths(millionths: u32) -> Option<Self> {
        if millionths >= JITTER_MIN && millionths <= MILLIONTHS {
            Some(Self(millionths))
        } else {
            None
        }
    }

    /// Draws a factor from the OS random source (`getrandom`). When the
    /// source fails, it returns [`MAX`](Self::MAX), the longest pause,
    /// and logs a warning.
    #[must_use]
    pub fn random() -> Self {
        let mut bytes = [0_u8; 4];
        match getrandom::fill(&mut bytes) {
            Ok(()) => Self(JITTER_MIN + u32::from_le_bytes(bytes) % (MILLIONTHS - JITTER_MIN + 1)),
            Err(error) => {
                tracing::warn!(%error, "random source failed, restart jitter uses 1.0");
                Self::MAX
            }
        }
    }

    /// Returns the factor in millionths.
    #[must_use]
    pub const fn millionths(self) -> u32 {
        self.0
    }

    /// Returns `duration` times this factor, rounded down to a nanosecond.
    #[must_use]
    pub fn apply(self, duration: Duration) -> Duration {
        let nanos = duration.as_nanos() * u128::from(self.0) / u128::from(MILLIONTHS);
        Duration::from_nanos(u64::try_from(nanos).unwrap_or(u64::MAX))
    }
}

/// Returns the pause before respawn number `attempt` in the window:
/// `min(max, base × 2^(attempt − 1)) × jitter`.
///
/// ```
/// use std::num::NonZeroU32;
/// use std::time::Duration;
///
/// use stackly_core::supervisor::{JitterFactor, SupervisorConfig, backoff_delay};
///
/// let config = SupervisorConfig::default();
/// let fifth = NonZeroU32::new(5).unwrap();
/// assert_eq!(backoff_delay(fifth, JitterFactor::MAX, &config), Duration::from_secs(16));
/// assert_eq!(backoff_delay(fifth, JitterFactor::MIN, &config), Duration::from_secs(8));
/// ```
#[must_use]
pub fn backoff_delay(
    attempt: NonZeroU32,
    jitter: JitterFactor,
    config: &SupervisorConfig,
) -> Duration {
    let full = 2_u32
        .checked_pow(attempt.get() - 1)
        .and_then(|factor| config.restart_base_delay.checked_mul(factor))
        .map_or(config.restart_max_delay, |delay| {
            delay.min(config.restart_max_delay)
        });
    jitter.apply(full)
}

/// What the policy decides after a crash.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestartDecision {
    /// Respawn after `delay` (`crashed { will_retry: true }`, lifecycle
    /// `recovering`).
    Retry {
        /// The number of this respawn in the window, from 1.
        attempt: NonZeroU32,
        /// The pause before it.
        delay: Duration,
    },
    /// The budget is spent: no respawn (`crashed { will_retry: false,
    /// reason: crash_loop }`, lifecycle `failed`).
    Exhausted {
        /// Crashes in the window, this one included: the budget plus 1.
        crashes: u32,
    },
}

/// The crash history of one instance: crash times in the sliding window,
/// and since when the instance has been `ready` without a break.
///
/// ```
/// use stackly_core::supervisor::{JitterFactor, RestartDecision, RestartHistory, SupervisorConfig};
/// use tokio::time::Instant;
///
/// let config = SupervisorConfig::default();
/// let mut history = RestartHistory::new();
/// let now = Instant::now();
/// for _ in 0..config.restart_budget {
///     let decision = history.record_crash(now, JitterFactor::MAX, &config);
///     assert!(matches!(decision, RestartDecision::Retry { .. }));
/// }
/// let sixth = history.record_crash(now, JitterFactor::MAX, &config);
/// assert_eq!(sixth, RestartDecision::Exhausted { crashes: 6 });
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RestartHistory {
    crashes: VecDeque<Instant>,
    ready_since: Option<Instant>,
}

impl RestartHistory {
    /// Creates an empty history, as at daemon start: every daemon run
    /// starts a new budget.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            crashes: VecDeque::new(),
            ready_since: None,
        }
    }

    /// Records a crash at `now` and decides: drops the crashes outside the
    /// window, then retries while the crashes in the window, this one
    /// included, are at most the budget. A healthy period that reached
    /// [`restart_healthy_reset`](SupervisorConfig::restart_healthy_reset)
    /// before the crash clears the history first, even when no ready
    /// observation reported it.
    pub fn record_crash(
        &mut self,
        now: Instant,
        jitter: JitterFactor,
        config: &SupervisorConfig,
    ) -> RestartDecision {
        if self.healthy_long_enough(now, config) {
            self.crashes.clear();
        }
        self.ready_since = None;
        self.prune(now, config);
        self.crashes.push_back(now);
        let crashes = u32::try_from(self.crashes.len()).unwrap_or(u32::MAX);
        match NonZeroU32::new(crashes) {
            Some(attempt) if crashes <= config.restart_budget => RestartDecision::Retry {
                attempt,
                delay: backoff_delay(attempt, jitter, config),
            },
            _ => RestartDecision::Exhausted { crashes },
        }
    }

    /// Notes a successful readiness observation at `now`. Returns true when
    /// this cleared the history, because the instance has been `ready`
    /// without a break for the healthy reset period.
    pub fn note_ready(&mut self, now: Instant, config: &SupervisorConfig) -> bool {
        if self.ready_since.is_none() {
            self.ready_since = Some(now);
        }
        if self.healthy_long_enough(now, config) && !self.crashes.is_empty() {
            self.crashes.clear();
            return true;
        }
        false
    }

    /// Notes that readiness left `ready` (or the process exited): the
    /// healthy period starts over.
    pub const fn note_not_ready(&mut self) {
        self.ready_since = None;
    }

    /// Clears the history, as a user start from `failed` does.
    pub fn clear(&mut self) {
        self.crashes.clear();
        self.ready_since = None;
    }

    /// Returns the crashes inside the window at `now`, for the `restart`
    /// status.
    #[must_use]
    pub fn crashes_in_window(&self, now: Instant, config: &SupervisorConfig) -> u32 {
        let count = self
            .crashes
            .iter()
            .filter(|crash| in_window(**crash, now, config))
            .count();
        u32::try_from(count).unwrap_or(u32::MAX)
    }

    fn healthy_long_enough(&self, now: Instant, config: &SupervisorConfig) -> bool {
        self.ready_since.is_some_and(|since| {
            now.saturating_duration_since(since) >= config.restart_healthy_reset
        })
    }

    fn prune(&mut self, now: Instant, config: &SupervisorConfig) {
        while self
            .crashes
            .front()
            .is_some_and(|crash| !in_window(*crash, now, config))
        {
            self.crashes.pop_front();
        }
    }
}

fn in_window(crash: Instant, now: Instant, config: &SupervisorConfig) -> bool {
    now.saturating_duration_since(crash) < config.restart_window
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECOND: Duration = Duration::from_secs(1);

    fn retry_delay(decision: RestartDecision) -> Duration {
        match decision {
            RestartDecision::Retry { delay, .. } => delay,
            RestartDecision::Exhausted { .. } => panic!("expected a retry, got {decision:?}"),
        }
    }

    #[test]
    fn the_full_pauses_are_one_two_four_eight_and_sixteen_seconds() {
        let config = SupervisorConfig::default();
        let mut history = RestartHistory::new();
        let now = Instant::now();

        let delays: Vec<_> = (0..5)
            .map(|_| retry_delay(history.record_crash(now, JitterFactor::MAX, &config)))
            .collect();

        assert_eq!(delays, [1, 2, 4, 8, 16].map(Duration::from_secs));
    }

    #[test]
    fn the_smallest_jitter_halves_every_pause() {
        let config = SupervisorConfig::default();
        let mut history = RestartHistory::new();
        let now = Instant::now();

        let delays: Vec<_> = (0..5)
            .map(|_| retry_delay(history.record_crash(now, JitterFactor::MIN, &config)))
            .collect();

        assert_eq!(
            delays,
            [500, 1000, 2000, 4000, 8000].map(Duration::from_millis)
        );
    }

    #[test]
    fn the_sixth_crash_in_the_window_is_not_retried_so_a_loop_spawns_six_times() {
        let config = SupervisorConfig::default();
        let mut history = RestartHistory::new();
        let start = Instant::now();
        let mut spawns = 1;
        let mut now = start;

        loop {
            now += SECOND;
            match history.record_crash(now, JitterFactor::MAX, &config) {
                RestartDecision::Retry { delay, .. } => {
                    now += delay;
                    spawns += 1;
                }
                RestartDecision::Exhausted { crashes } => {
                    assert_eq!(crashes, 6);
                    break;
                }
            }
        }

        assert_eq!(spawns, 6);
        assert!(now.duration_since(start) < config.restart_window);
    }

    #[test]
    fn crashes_older_than_the_window_leave_the_budget() {
        let config = SupervisorConfig::default();
        let mut history = RestartHistory::new();
        let start = Instant::now();
        for _ in 0..5 {
            history.record_crash(start, JitterFactor::MAX, &config);
        }

        let later = start + config.restart_window;
        let decision = history.record_crash(later, JitterFactor::MAX, &config);

        assert_eq!(
            decision,
            RestartDecision::Retry {
                attempt: NonZeroU32::MIN,
                delay: SECOND
            }
        );
        assert_eq!(history.crashes_in_window(later, &config), 1);
    }

    #[test]
    fn the_window_slides_one_crash_at_a_time() {
        let config = SupervisorConfig::default();
        let mut history = RestartHistory::new();
        let start = Instant::now();
        for offset in 0..5 {
            history.record_crash(start + SECOND * offset, JitterFactor::MAX, &config);
        }

        // Only the first crash has left the window.
        let now = start + config.restart_window + Duration::from_millis(500);
        let decision = history.record_crash(now, JitterFactor::MAX, &config);

        assert!(
            matches!(decision, RestartDecision::Retry { attempt, .. } if attempt.get() == 5),
            "{decision:?}"
        );
        assert_eq!(history.crashes_in_window(now, &config), 5);
    }

    #[test]
    fn sixty_seconds_of_continuous_ready_clears_the_history() {
        let config = SupervisorConfig::default();
        let mut history = RestartHistory::new();
        let start = Instant::now();
        for _ in 0..4 {
            history.record_crash(start, JitterFactor::MAX, &config);
        }

        assert!(!history.note_ready(start + SECOND, &config));
        assert!(!history.note_ready(start + SECOND * 60, &config));
        assert!(history.note_ready(start + SECOND * 61, &config));
        assert_eq!(history.crashes_in_window(start + SECOND * 61, &config), 0);

        let decision = history.record_crash(start + SECOND * 62, JitterFactor::MAX, &config);
        assert!(
            matches!(decision, RestartDecision::Retry { attempt, .. } if attempt.get() == 1),
            "{decision:?}"
        );
    }

    #[test]
    fn a_break_in_readiness_restarts_the_healthy_period() {
        let config = SupervisorConfig::default();
        let mut history = RestartHistory::new();
        let start = Instant::now();
        history.record_crash(start, JitterFactor::MAX, &config);

        history.note_ready(start + SECOND, &config);
        history.note_not_ready();
        assert!(!history.note_ready(start + SECOND * 40, &config));
        assert!(!history.note_ready(start + SECOND * 70, &config));

        assert_eq!(history.crashes_in_window(start + SECOND * 70, &config), 1);
        assert!(history.note_ready(start + SECOND * 100, &config));
    }

    #[test]
    fn a_crash_after_a_long_unreported_healthy_period_starts_a_new_budget() {
        let config = SupervisorConfig::default();
        let mut history = RestartHistory::new();
        let start = Instant::now();
        for _ in 0..5 {
            history.record_crash(start, JitterFactor::MAX, &config);
        }
        history.note_ready(start + SECOND, &config);

        let decision = history.record_crash(start + SECOND * 90, JitterFactor::MAX, &config);

        assert!(
            matches!(decision, RestartDecision::Retry { attempt, .. } if attempt.get() == 1),
            "{decision:?}"
        );
    }

    #[test]
    fn a_crash_ends_the_healthy_period() {
        let config = SupervisorConfig::default();
        let mut history = RestartHistory::new();
        let start = Instant::now();
        history.note_ready(start, &config);
        history.record_crash(start + SECOND * 10, JitterFactor::MAX, &config);

        assert!(!history.note_ready(start + SECOND * 61, &config));
        assert_eq!(history.crashes_in_window(start + SECOND * 61, &config), 1);
    }

    #[test]
    fn clear_forgets_every_crash() {
        let config = SupervisorConfig::default();
        let mut history = RestartHistory::new();
        let now = Instant::now();
        for _ in 0..6 {
            history.record_crash(now, JitterFactor::MAX, &config);
        }

        history.clear();

        assert_eq!(history, RestartHistory::new());
    }

    #[test]
    fn the_pause_never_exceeds_the_maximum() {
        let config = SupervisorConfig {
            restart_budget: 40,
            ..SupervisorConfig::default()
        };

        let delays: Vec<_> = [6, 7, 32, 33, 40]
            .map(|attempt| {
                backoff_delay(
                    NonZeroU32::new(attempt).unwrap(),
                    JitterFactor::MAX,
                    &config,
                )
            })
            .into();

        assert_eq!(delays, [Duration::from_secs(30); 5]);
    }

    #[test]
    fn random_jitter_stays_between_one_half_and_one() {
        for _ in 0..1000 {
            let jitter = JitterFactor::random();
            assert!((JitterFactor::MIN..=JitterFactor::MAX).contains(&jitter));
        }
    }

    #[test]
    fn jitter_outside_one_half_to_one_is_refused() {
        assert_eq!(JitterFactor::from_millionths(499_999), None);
        assert_eq!(JitterFactor::from_millionths(1_000_001), None);
        assert_eq!(
            JitterFactor::from_millionths(750_000).map(|jitter| jitter.apply(SECOND * 4)),
            Some(SECOND * 3)
        );
    }
}
