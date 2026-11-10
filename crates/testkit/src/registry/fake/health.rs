//! The registry health of [`FakeRegistry`] (spec 0034 AC-8): what its
//! `quick_check` and `scan_unrecognized` answer, set by the test.
//!
//! The fake holds no SQLite file, so a test says what the checks find: by
//! default `quick_check` passes and the scan finds nothing. Like the SQLite
//! registry, a call whose stop flag is already set answers `Io` at once,
//! and a held call ([`FakeRegistry::hold_health_checks`]) waits until its
//! flag is set, as a check that runs past its time limit would.

use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use stackly_domain::{QuickCheck, RegistryError, UnrecognizedValue};

use super::{FakeRegistry, lock};

/// How often a held call looks at its stop flag.
const HOLD_POLL: Duration = Duration::from_millis(1);

/// What the health checks of a [`FakeRegistry`] answer.
#[derive(Debug, Default)]
pub(super) struct FakeHealth {
    /// The `quick_check` answer, or `None` for a pass.
    quick_check: Option<QuickCheck>,
    unrecognized: Vec<UnrecognizedValue>,
    fault: Option<RegistryError>,
    hold: bool,
    calls: usize,
}

impl FakeRegistry {
    /// Makes every later `quick_check` answer `answer`.
    pub fn set_quick_check(&self, answer: QuickCheck) {
        lock(&self.health).quick_check = Some(answer);
    }

    /// Makes every later `scan_unrecognized` answer `values`, in this
    /// order.
    pub fn set_unrecognized(&self, values: Vec<UnrecognizedValue>) {
        lock(&self.health).unrecognized = values;
    }

    /// Makes every later `quick_check` and `scan_unrecognized` fail with
    /// `error`, until a call with `None`.
    pub fn fail_health_checks(&self, error: Option<RegistryError>) {
        lock(&self.health).fault = error;
    }

    /// While `hold` is true, a `quick_check` or `scan_unrecognized` waits
    /// until its stop flag is set and then answers `Io`, like a check that
    /// runs past its time limit and is interrupted. Releasing the hold
    /// lets a waiting call answer as usual.
    pub fn hold_health_checks(&self, hold: bool) {
        lock(&self.health).hold = hold;
    }

    /// Returns how many `quick_check` and `scan_unrecognized` calls arrived.
    #[must_use]
    pub fn health_check_calls(&self) -> usize {
        lock(&self.health).calls
    }

    pub(super) fn answer_quick_check(
        &self,
        stop: &AtomicBool,
    ) -> Result<QuickCheck, RegistryError> {
        self.health_call(stop)?;
        Ok(lock(&self.health)
            .quick_check
            .clone()
            .unwrap_or(QuickCheck::Passed))
    }

    pub(super) fn answer_scan(
        &self,
        stop: &AtomicBool,
    ) -> Result<Vec<UnrecognizedValue>, RegistryError> {
        self.health_call(stop)?;
        Ok(lock(&self.health).unrecognized.clone())
    }

    /// Counts a call, waits while it is held, and answers its fault or
    /// interruption.
    fn health_call(&self, stop: &AtomicBool) -> Result<(), RegistryError> {
        lock(&self.health).calls += 1;
        loop {
            if stop.load(Ordering::Relaxed) {
                return Err(RegistryError::Io {
                    detail: "the registry check was interrupted by its stop flag".to_owned(),
                });
            }
            if !lock(&self.health).hold {
                break;
            }
            thread::sleep(HOLD_POLL);
        }
        match lock(&self.health).fault.clone() {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use stackly_domain::Registry;
    use stackly_platform::Clock;

    use super::*;
    use crate::FakeClock;

    fn registry() -> FakeRegistry {
        let clock = FakeClock::new();
        FakeRegistry::new(Arc::new(clock.clone()), clock.now())
    }

    fn running() -> Arc<AtomicBool> {
        Arc::new(AtomicBool::new(false))
    }

    #[test]
    fn by_default_quick_check_passes_and_the_scan_finds_nothing() {
        let registry = registry();

        assert_eq!(registry.quick_check(running()).unwrap(), QuickCheck::Passed);
        assert_eq!(registry.scan_unrecognized(running()).unwrap(), []);
        assert_eq!(registry.health_check_calls(), 2);
    }

    #[test]
    fn the_answers_a_test_sets_come_back() {
        let registry = registry();
        let failed = QuickCheck::from_messages(vec!["row 3 missing from index".to_owned()]);
        let value = UnrecognizedValue::new("operations", 4, "kind", "teleport");
        registry.set_quick_check(failed.clone());
        registry.set_unrecognized(vec![value.clone()]);

        assert_eq!(registry.quick_check(running()).unwrap(), failed);
        assert_eq!(registry.scan_unrecognized(running()).unwrap(), [value]);
    }

    #[test]
    fn a_fault_answers_every_call_until_it_is_cleared() {
        let registry = registry();
        registry.fail_health_checks(Some(RegistryError::Busy));

        assert_eq!(registry.quick_check(running()), Err(RegistryError::Busy));
        assert_eq!(
            registry.scan_unrecognized(running()),
            Err(RegistryError::Busy)
        );

        registry.fail_health_checks(None);
        assert!(registry.quick_check(running()).is_ok());
    }

    #[test]
    fn a_set_stop_flag_answers_io_at_once() {
        let registry = registry();

        let err = registry
            .quick_check(Arc::new(AtomicBool::new(true)))
            .unwrap_err();

        assert!(matches!(err, RegistryError::Io { .. }), "{err:?}");
    }

    #[test]
    fn a_held_call_waits_until_its_stop_flag_is_set() {
        let registry = Arc::new(registry());
        registry.hold_health_checks(true);
        let stop = running();
        let call = {
            let registry = Arc::clone(&registry);
            let stop = Arc::clone(&stop);
            thread::spawn(move || registry.scan_unrecognized(stop))
        };
        while registry.health_check_calls() == 0 {
            thread::sleep(HOLD_POLL);
        }
        assert!(!call.is_finished(), "a held call waits");

        stop.store(true, Ordering::Relaxed);

        let result = call.join().unwrap();
        assert!(
            matches!(result, Err(RegistryError::Io { .. })),
            "{result:?}"
        );
    }
}
