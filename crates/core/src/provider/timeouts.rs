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

/// The timing of a checked manifest that a caller outside the crate may
/// read, such as the conformance harness of spec 0036: the `timeouts`
/// block and the health cadence, with the spec 0009 defaults already
/// filled in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct ManifestTimings {
    /// `timeouts.initSeconds`.
    pub init_seconds: u64,
    /// `timeouts.readinessSeconds`.
    pub readiness_seconds: u64,
    /// `timeouts.stopSeconds`.
    pub stop_seconds: u64,
    /// `timeouts.reloadSeconds`.
    pub reload_seconds: u64,
    /// `health.intervalSeconds`.
    pub health_interval_seconds: u64,
    /// `health.failureThreshold`.
    pub health_failure_threshold: u8,
    /// `health.attemptTimeoutMilliseconds`.
    pub health_attempt_milliseconds: u64,
}

impl super::Manifest {
    /// The timing of this manifest, whatever YAML style it was written in.
    #[must_use]
    pub const fn timings(&self) -> ManifestTimings {
        ManifestTimings {
            init_seconds: self.timeouts.init,
            readiness_seconds: self.timeouts.readiness,
            stop_seconds: self.timeouts.stop,
            reload_seconds: self.timeouts.reload,
            health_interval_seconds: self.health.interval_seconds,
            health_failure_threshold: self.health.failure_threshold,
            health_attempt_milliseconds: self.health.attempt_ms,
        }
    }
}
