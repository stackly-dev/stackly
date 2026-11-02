//! A scripted login session port for tests (spec 0031, AC-1): the session
//! the daemon sees, which a test changes to simulate a login, a logout, or
//! a reboot.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use stackly_domain::BootId;
use stackly_platform::{LoginSession, LoginSessionError, LoginSessionInfo};

/// The boot a fresh fake reports.
const DEFAULT_BOOT: &str = "00000000-0000-4000-8000-000000000001";

/// The audit session a fresh fake reports.
const DEFAULT_AUDIT_SESSION: u32 = 100_001;

/// A [`LoginSessionInfo`] the test scripts. Clones share one state.
///
/// ```
/// use stackly_platform::LoginSessionInfo;
/// use stackly_testkit::FakeLoginSessionInfo;
///
/// let fake = FakeLoginSessionInfo::new();
/// let first = fake.current()?;
/// fake.set_audit_session(100_002);
/// assert_ne!(fake.current()?.id, first.id);
/// # Ok::<(), stackly_platform::LoginSessionError>(())
/// ```
#[derive(Debug, Clone)]
pub struct FakeLoginSessionInfo {
    state: Arc<Mutex<State>>,
}

#[derive(Debug)]
struct State {
    boot: BootId,
    audit_session: u32,
    failure: Option<LoginSessionError>,
    reads: usize,
}

impl Default for FakeLoginSessionInfo {
    fn default() -> Self {
        Self::new()
    }
}

impl FakeLoginSessionInfo {
    /// Creates a fake in boot `00000000-0000-4000-8000-000000000001` and
    /// audit session 100001.
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(State {
                boot: DEFAULT_BOOT
                    .parse()
                    .expect("the default fake boot ID is a valid UUID"),
                audit_session: DEFAULT_AUDIT_SESSION,
                failure: None,
                reads: 0,
            })),
        }
    }

    /// Makes the next reads answer a session in `audit_session`.
    pub fn set_audit_session(&self, audit_session: u32) {
        self.lock().audit_session = audit_session;
    }

    /// Makes the next reads answer a session of `boot`.
    pub fn set_boot(&self, boot: BootId) {
        self.lock().boot = boot;
    }

    /// Makes the next reads fail with `error`.
    pub fn fail_with(&self, error: LoginSessionError) {
        self.lock().failure = Some(error);
    }

    /// Stops failing reads.
    pub fn recover(&self) {
        self.lock().failure = None;
    }

    /// Returns how many times `current` was called.
    #[must_use]
    pub fn reads(&self) -> usize {
        self.lock().reads
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl LoginSessionInfo for FakeLoginSessionInfo {
    fn current(&self) -> Result<LoginSession, LoginSessionError> {
        let mut state = self.lock();
        state.reads += 1;
        if let Some(error) = &state.failure {
            return Err(error.clone());
        }
        LoginSession::new(state.boot, state.audit_session)
    }
}
