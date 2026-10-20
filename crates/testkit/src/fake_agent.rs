use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use stackly_platform::{
    AgentOp, AgentProbe, AgentRegistration, BackgroundAgent, BackgroundAgentError,
};

/// The bundle path a [`FakeBackgroundAgent`] reports until a test sets
/// another one.
pub const FAKE_AGENT_BUNDLE: &str = "/Applications/Stackly.app";

/// A background agent in memory that the test scripts, and that logs every
/// call (spec 0015).
///
/// It starts [`AgentRegistration::NotRegistered`] inside
/// [`FAKE_AGENT_BUNDLE`]. Each change call moves the registration to the
/// state the test set for it, so a test decides whether `register` ends
/// `Enabled` (the default) or `RequiresApproval`. A queued failure answers
/// the next call of that kind and leaves the state as it was. Clones share
/// the state, so a test keeps one handle while the code under test holds the
/// port.
///
/// ```
/// use stackly_platform::{AgentOp, AgentProbe, AgentRegistration, BackgroundAgent};
/// use stackly_testkit::FakeBackgroundAgent;
///
/// let agent = FakeBackgroundAgent::new();
/// agent.on_register(AgentRegistration::RequiresApproval);
///
/// let probe = agent.register()?;
/// assert!(matches!(
///     probe,
///     AgentProbe::Registration { registration: AgentRegistration::RequiresApproval, .. }
/// ));
/// assert_eq!(agent.calls(), vec![AgentOp::Register]);
/// # Ok::<(), stackly_platform::BackgroundAgentError>(())
/// ```
#[derive(Debug, Clone, Default)]
pub struct FakeBackgroundAgent {
    state: Arc<Mutex<State>>,
}

#[derive(Debug)]
struct State {
    probe: Probe,
    bundle: PathBuf,
    after_register: AgentRegistration,
    after_unregister: AgentRegistration,
    after_relaunch: AgentRegistration,
    failures: VecDeque<(AgentOp, BackgroundAgentError)>,
    calls: Vec<AgentOp>,
}

/// What the fake reports, apart from the bundle path.
#[derive(Debug, Clone)]
enum Probe {
    Registration(AgentRegistration),
    NotBundled(PathBuf),
    HelperMissing,
}

impl Default for State {
    fn default() -> Self {
        Self {
            probe: Probe::Registration(AgentRegistration::NotRegistered),
            bundle: PathBuf::from(FAKE_AGENT_BUNDLE),
            after_register: AgentRegistration::Enabled,
            after_unregister: AgentRegistration::NotRegistered,
            after_relaunch: AgentRegistration::Enabled,
            failures: VecDeque::new(),
            calls: Vec::new(),
        }
    }
}

impl FakeBackgroundAgent {
    /// Creates an agent that is not registered.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates an agent that already reports `registration`.
    #[must_use]
    pub fn with_registration(registration: AgentRegistration) -> Self {
        let agent = Self::new();
        agent.set_registration(registration);
        agent
    }

    /// Sets the registration the next call reports.
    pub fn set_registration(&self, registration: AgentRegistration) {
        self.lock().probe = Probe::Registration(registration);
    }

    /// Sets the bundle path the OS reports with a registration.
    pub fn set_bundle(&self, bundle: impl Into<PathBuf>) {
        self.lock().bundle = bundle.into();
    }

    /// Makes every call answer [`AgentProbe::NotBundled`] for `executable`,
    /// as for a CLI built by cargo.
    pub fn set_not_bundled(&self, executable: impl Into<PathBuf>) {
        self.lock().probe = Probe::NotBundled(executable.into());
    }

    /// Makes every call answer [`AgentProbe::HelperMissing`] for the
    /// bundle, as for a build from the Xcode IDE.
    pub fn set_helper_missing(&self) {
        self.lock().probe = Probe::HelperMissing;
    }

    /// Sets the registration that [`register`](BackgroundAgent::register)
    /// leaves.
    pub fn on_register(&self, registration: AgentRegistration) {
        self.lock().after_register = registration;
    }

    /// Sets the registration that
    /// [`unregister`](BackgroundAgent::unregister) leaves.
    pub fn on_unregister(&self, registration: AgentRegistration) {
        self.lock().after_unregister = registration;
    }

    /// Sets the registration that [`relaunch`](BackgroundAgent::relaunch)
    /// leaves.
    pub fn on_relaunch(&self, registration: AgentRegistration) {
        self.lock().after_relaunch = registration;
    }

    /// Queues `error` for the next call of `op`; queued failures for the
    /// same call answer in order.
    pub fn fail_next(&self, op: AgentOp, error: BackgroundAgentError) {
        self.lock().failures.push_back((op, error));
    }

    /// Returns every call so far, in the order they ran.
    #[must_use]
    pub fn calls(&self) -> Vec<AgentOp> {
        self.lock().calls.clone()
    }

    /// Returns how many times `op` was called.
    #[must_use]
    pub fn count(&self, op: AgentOp) -> usize {
        self.lock().calls.iter().filter(|call| **call == op).count()
    }

    /// Returns whether a call that changes the registration ran, which a
    /// test of a read only path asserts never happens.
    #[must_use]
    pub fn changed_registration(&self) -> bool {
        self.lock()
            .calls
            .iter()
            .any(|call| !matches!(call, AgentOp::Status))
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn answer(&self, op: AgentOp) -> Result<AgentProbe, BackgroundAgentError> {
        let mut state = self.lock();
        state.calls.push(op);
        if let Some(index) = state.failures.iter().position(|(queued, _)| *queued == op)
            && let Some((_, error)) = state.failures.remove(index)
        {
            return Err(error);
        }
        if let Probe::Registration(_) = state.probe {
            let next = match op {
                AgentOp::Status => None,
                AgentOp::Register => Some(state.after_register),
                AgentOp::Unregister => Some(state.after_unregister),
                AgentOp::Relaunch => Some(state.after_relaunch),
            };
            if let Some(next) = next {
                state.probe = Probe::Registration(next);
            }
        }
        Ok(match &state.probe {
            Probe::Registration(registration) => AgentProbe::Registration {
                registration: *registration,
                bundle: state.bundle.clone(),
            },
            Probe::NotBundled(executable) => AgentProbe::NotBundled {
                executable: executable.clone(),
            },
            Probe::HelperMissing => AgentProbe::HelperMissing {
                bundle: state.bundle.clone(),
            },
        })
    }
}

impl BackgroundAgent for FakeBackgroundAgent {
    fn status(&self) -> Result<AgentProbe, BackgroundAgentError> {
        self.answer(AgentOp::Status)
    }

    fn register(&self) -> Result<AgentProbe, BackgroundAgentError> {
        self.answer(AgentOp::Register)
    }

    fn unregister(&self) -> Result<AgentProbe, BackgroundAgentError> {
        self.answer(AgentOp::Unregister)
    }

    fn relaunch(&self) -> Result<AgentProbe, BackgroundAgentError> {
        self.answer(AgentOp::Relaunch)
    }
}

#[cfg(test)]
mod tests {
    use stackly_platform::AgentOsErrorKind;

    use super::*;

    fn registration_of(probe: Result<AgentProbe, BackgroundAgentError>) -> AgentRegistration {
        match probe {
            Ok(AgentProbe::Registration { registration, .. }) => registration,
            other => panic!("expected a registration, got {other:?}"),
        }
    }

    #[test]
    fn a_new_agent_is_not_registered_in_the_default_bundle() {
        let agent = FakeBackgroundAgent::new();
        assert_eq!(
            agent.status(),
            Ok(AgentProbe::Registration {
                registration: AgentRegistration::NotRegistered,
                bundle: PathBuf::from(FAKE_AGENT_BUNDLE),
            })
        );
    }

    #[test]
    fn register_leaves_enabled_by_default() {
        let agent = FakeBackgroundAgent::new();
        assert_eq!(
            registration_of(agent.register()),
            AgentRegistration::Enabled
        );
        assert_eq!(registration_of(agent.status()), AgentRegistration::Enabled);
    }

    #[test]
    fn register_leaves_the_state_the_test_set() {
        let agent = FakeBackgroundAgent::new();
        agent.on_register(AgentRegistration::RequiresApproval);
        assert_eq!(
            registration_of(agent.register()),
            AgentRegistration::RequiresApproval
        );
    }

    #[test]
    fn unregister_and_relaunch_leave_the_states_the_test_set() {
        let agent = FakeBackgroundAgent::with_registration(AgentRegistration::Enabled);
        assert_eq!(
            registration_of(agent.unregister()),
            AgentRegistration::NotRegistered
        );
        agent.on_relaunch(AgentRegistration::RequiresApproval);
        assert_eq!(
            registration_of(agent.relaunch()),
            AgentRegistration::RequiresApproval
        );
    }

    #[test]
    fn a_queued_failure_answers_once_and_keeps_the_state() {
        let agent = FakeBackgroundAgent::new();
        let error = BackgroundAgentError::Os {
            op: AgentOp::Register,
            kind: AgentOsErrorKind::InvalidSignature,
            domain: "SMAppServiceErrorDomain".to_owned(),
            code: 3,
            message: "invalid signature".to_owned(),
        };
        agent.fail_next(AgentOp::Register, error.clone());
        assert_eq!(agent.register(), Err(error));
        assert_eq!(
            registration_of(agent.status()),
            AgentRegistration::NotRegistered
        );
        assert_eq!(
            registration_of(agent.register()),
            AgentRegistration::Enabled
        );
    }

    #[test]
    fn a_failure_queued_for_another_call_does_not_answer_status() {
        let agent = FakeBackgroundAgent::new();
        agent.fail_next(
            AgentOp::Relaunch,
            BackgroundAgentError::Os {
                op: AgentOp::Relaunch,
                kind: AgentOsErrorKind::Other,
                domain: "d".to_owned(),
                code: 1,
                message: "m".to_owned(),
            },
        );
        assert!(agent.status().is_ok());
    }

    #[test]
    fn a_not_bundled_agent_answers_the_executable_and_never_changes() {
        let agent = FakeBackgroundAgent::new();
        agent.set_not_bundled("/work/target/debug/stackly");
        let expected = AgentProbe::NotBundled {
            executable: PathBuf::from("/work/target/debug/stackly"),
        };
        assert_eq!(agent.register(), Ok(expected.clone()));
        assert_eq!(agent.status(), Ok(expected));
    }

    #[test]
    fn a_helper_missing_agent_answers_its_bundle() {
        let agent = FakeBackgroundAgent::new();
        agent.set_bundle("/DerivedData/Stackly.app");
        agent.set_helper_missing();
        assert_eq!(
            agent.status(),
            Ok(AgentProbe::HelperMissing {
                bundle: PathBuf::from("/DerivedData/Stackly.app"),
            })
        );
    }

    #[test]
    fn the_call_log_records_every_call_in_order() {
        let agent = FakeBackgroundAgent::new();
        let _ = agent.status();
        let _ = agent.relaunch();
        let _ = agent.status();
        assert_eq!(
            agent.calls(),
            vec![AgentOp::Status, AgentOp::Relaunch, AgentOp::Status]
        );
        assert_eq!(agent.count(AgentOp::Status), 2);
        assert!(agent.changed_registration());
    }

    #[test]
    fn status_alone_never_counts_as_a_change() {
        let agent = FakeBackgroundAgent::new();
        let _ = agent.status();
        let _ = agent.status();
        assert!(!agent.changed_registration());
    }

    #[test]
    fn clones_share_the_state() {
        let agent = FakeBackgroundAgent::new();
        let port: Arc<dyn BackgroundAgent> = Arc::new(agent.clone());
        let _ = port.register();
        assert_eq!(agent.calls(), vec![AgentOp::Register]);
        assert_eq!(registration_of(agent.status()), AgentRegistration::Enabled);
    }
}
