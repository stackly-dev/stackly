//! Defaults for reading readiness and health observations.

use std::time::Duration;

/// How old a readiness or health observation may be before
/// `effective(now, max_age)` reads it as `unknown` with reason `stale`:
/// three probe intervals of 10 seconds (PRD section 19.5).
// provisional: F28
pub const READINESS_MAX_AGE: Duration = Duration::from_secs(30);

#[cfg(test)]
mod tests {
    use stackly_domain::{Readiness, ReadinessState, ReasonCode};

    use super::*;

    #[test]
    fn a_ready_observation_goes_stale_after_the_default_max_age() {
        let ready = Readiness::new(
            ReadinessState::Ready,
            "2026-01-01T00:00:00Z".parse().unwrap(),
            "probe_ok".parse().unwrap(),
            None,
        );

        let fresh = ready.effective("2026-01-01T00:00:30Z".parse().unwrap(), READINESS_MAX_AGE);
        let stale = ready.effective("2026-01-01T00:00:31Z".parse().unwrap(), READINESS_MAX_AGE);

        assert_eq!(fresh, ready);
        assert_eq!(stale.state(), ReadinessState::Unknown);
        assert_eq!(stale.reason(), &ReasonCode::STALE);
    }
}
