//! Defaults and bounds of the manifest's `timeouts` block, probe timing,
//! and exit codes (spec 0009, "Timeout dan reload").

use std::ops::RangeInclusive;

/// One bounded setting with its default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Bound {
    pub(crate) default: u64,
    pub(crate) min: u64,
    pub(crate) max: u64,
}

impl Bound {
    const fn new(default: u64, min: u64, max: u64) -> Self {
        Self { default, min, max }
    }

    pub(crate) const fn range(self) -> RangeInclusive<u64> {
        self.min..=self.max
    }
}

/// `timeouts.initSeconds`: the whole initialize action.
pub(crate) const INIT_SECONDS: Bound = Bound::new(120, 1, 1800);
/// `timeouts.readinessSeconds`: from spawn to ready on start.
pub(crate) const READINESS_SECONDS: Bound = Bound::new(60, 1, 600);
/// `timeouts.recoverySeconds`: from a respawn after a crash to ready. Its
/// default is the readiness timeout.
pub(crate) const RECOVERY_SECONDS: Bound = Bound::new(60, 1, 1800);
/// `timeouts.stopSeconds`: from the stop action to the process exit.
pub(crate) const STOP_SECONDS: Bound = Bound::new(30, 1, 600);
/// `timeouts.reloadSeconds`: the reload action through verification.
pub(crate) const RELOAD_SECONDS: Bound = Bound::new(30, 1, 300);
/// `readiness.intervalMilliseconds`: the pause after one attempt.
pub(crate) const READINESS_INTERVAL_MS: Bound = Bound::new(500, 100, 10_000);
/// `attemptTimeoutMilliseconds` of readiness and health.
pub(crate) const ATTEMPT_TIMEOUT_MS: Bound = Bound::new(2000, 100, 30_000);
/// `health.intervalSeconds`.
pub(crate) const HEALTH_INTERVAL_SECONDS: Bound = Bound::new(10, 1, 300);
/// `health.failureThreshold`.
pub(crate) const FAILURE_THRESHOLD: Bound = Bound::new(3, 1, 10);
/// Most values in `successExitCodes`.
pub(crate) const SUCCESS_EXIT_CODES_MAX: usize = 8;
/// The default `successExitCodes`.
pub(crate) const DEFAULT_SUCCESS_EXIT_CODES: [u8; 1] = [0];

/// The checked `timeouts` block, in seconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Timeouts {
    pub(crate) init: u64,
    pub(crate) readiness: u64,
    pub(crate) recovery: u64,
    pub(crate) stop: u64,
    pub(crate) reload: u64,
}
