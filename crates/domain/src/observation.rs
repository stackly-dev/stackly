//! Readiness and health: what probes observe about a running instance, kept
//! apart from its lifecycle.

use std::time::Duration;

use jiff::{SignedDuration, Timestamp};

use crate::code::code_enum;
use crate::value::{ObservationDetail, ReasonCode};

code_enum! {
    /// Whether an instance accepts work, as its readiness probe reports.
    pub enum ReadinessState: ReadinessState {
        /// Nothing is known, or the last observation is too old.
        Unknown => "unknown",
        /// The process runs but does not accept work yet.
        NotReady => "not_ready",
        /// The instance accepts work. Only a `running` instance can be
        /// ready (BR-010).
        Ready => "ready",
    }
}

code_enum! {
    /// How well an instance works, as its health probe reports.
    pub enum HealthState: HealthState {
        /// Nothing is known, or the last observation is too old.
        Unknown => "unknown",
        /// Everything works.
        Healthy => "healthy",
        /// It works with problems.
        Degraded => "degraded",
        /// It does not work.
        Unhealthy => "unhealthy",
    }
}

mod sealed {
    pub trait Sealed {}

    impl Sealed for super::ReadinessState {}
    impl Sealed for super::HealthState {}
}

/// A state that a probe observes: [`ReadinessState`] or [`HealthState`].
pub trait ObservedState: Copy + Eq + sealed::Sealed {
    /// The state that says nothing is known.
    const UNKNOWN: Self;
}

impl ObservedState for ReadinessState {
    const UNKNOWN: Self = Self::Unknown;
}

impl ObservedState for HealthState {
    const UNKNOWN: Self = Self::Unknown;
}

/// One observation: the state, when it was observed, why, and an optional
/// detail.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Observation<S> {
    state: S,
    observed_at: Timestamp,
    reason: ReasonCode,
    detail: Option<ObservationDetail>,
}

/// The readiness of an instance.
pub type Readiness = Observation<ReadinessState>;

/// The health of an instance.
pub type Health = Observation<HealthState>;

impl<S: ObservedState> Observation<S> {
    /// Creates an observation of `state` made at `observed_at`.
    #[must_use]
    pub const fn new(
        state: S,
        observed_at: Timestamp,
        reason: ReasonCode,
        detail: Option<ObservationDetail>,
    ) -> Self {
        Self {
            state,
            observed_at,
            reason,
            detail,
        }
    }

    /// Creates an `unknown` observation with no detail.
    pub(crate) const fn unknown(observed_at: Timestamp, reason: ReasonCode) -> Self {
        Self::new(S::UNKNOWN, observed_at, reason, None)
    }

    /// Returns the observed state, as stored.
    #[must_use]
    pub const fn state(&self) -> S {
        self.state
    }

    /// Returns when it was observed.
    #[must_use]
    pub const fn observed_at(&self) -> Timestamp {
        self.observed_at
    }

    /// Returns why the state is what it is.
    #[must_use]
    pub const fn reason(&self) -> &ReasonCode {
        &self.reason
    }

    /// Returns the detail, if any.
    #[must_use]
    pub const fn detail(&self) -> Option<&ObservationDetail> {
        self.detail.as_ref()
    }

    /// Returns the observation as read at `now`: a state other than
    /// `unknown` that is older than `max_age` reads as `unknown` with reason
    /// `stale`, keeping its `observed_at`. The result is computed when read
    /// and never stored (BR-010).
    #[must_use]
    pub fn effective(&self, now: Timestamp, max_age: Duration) -> Self {
        let max_age = SignedDuration::try_from(max_age).unwrap_or(SignedDuration::MAX);
        if self.state != S::UNKNOWN && now.duration_since(self.observed_at) > max_age {
            Self::unknown(self.observed_at, ReasonCode::STALE)
        } else {
            self.clone()
        }
    }

    /// Returns whether `other` was made before this observation, so it must
    /// be ignored.
    pub(crate) fn is_newer_than(&self, other: &Self) -> bool {
        other.observed_at < self.observed_at
    }
}

/// What happened to an observation handed to an instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ObservationOutcome {
    /// The observation replaced the stored one.
    Applied,
    /// The observation was dropped.
    Ignored(Ignored),
}

/// Why an observation was dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Ignored {
    /// It was made before the stored observation.
    Stale,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str) -> Timestamp {
        text.parse().unwrap()
    }

    #[test]
    fn effective_reads_old_known_values_as_stale_unknown() {
        let ready = Readiness::new(
            ReadinessState::Ready,
            at("2026-01-01T00:00:00Z"),
            "probe_ok".parse().unwrap(),
            None,
        );
        let max_age = Duration::from_secs(30);

        assert_eq!(ready.effective(at("2026-01-01T00:00:30Z"), max_age), ready);
        let stale = ready.effective(at("2026-01-01T00:00:31Z"), max_age);
        assert_eq!(stale.state(), ReadinessState::Unknown);
        assert_eq!(stale.reason(), &ReasonCode::STALE);
        assert_eq!(stale.observed_at(), ready.observed_at());
    }

    #[test]
    fn effective_keeps_unknown_values_and_their_reason() {
        let unknown = Health::unknown(at("2026-01-01T00:00:00Z"), ReasonCode::NOT_RUNNING);

        assert_eq!(
            unknown.effective(at("2027-01-01T00:00:00Z"), Duration::ZERO),
            unknown
        );
    }

    #[test]
    fn effective_with_a_huge_max_age_never_goes_stale() {
        let healthy = Health::new(
            HealthState::Healthy,
            at("2026-01-01T00:00:00Z"),
            "probe_ok".parse().unwrap(),
            None,
        );

        assert_eq!(
            healthy.effective(at("2126-01-01T00:00:00Z"), Duration::MAX),
            healthy
        );
    }

    #[test]
    fn observed_states_round_trip_through_their_codes() {
        for state in ReadinessState::ALL {
            assert_eq!(state.as_code().parse::<ReadinessState>().unwrap(), *state);
        }
        for state in HealthState::ALL {
            assert_eq!(state.as_code().parse::<HealthState>().unwrap(), *state);
        }
    }
}
