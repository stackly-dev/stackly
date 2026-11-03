//! The login session of this daemon run (spec 0031, *Urutan daemon start*,
//! step 3): read once when a launchd daemon starts, rotated in the registry
//! before reconciliation, and kept in memory for the whole run.
//!
//! Only a daemon that launchd started, with a ready registry, knows
//! sessions. Any other daemon (a terminal, `SpawnedDaemon` in a test) and a
//! registry in recovery mode skip rotation and write nothing.

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};

use stackly_core::session::{RotationKind, RotationOutcome, rotate_session};
use stackly_domain::{
    InstanceFilter, InstanceId, LeaseFilter, LeaseHolder, LeaseState, Registry, SessionId,
};
use stackly_platform::{LoginSession, LoginSessionError};
use stackly_protocol::Timestamp;
use stackly_protocol::daemon::{
    DaemonSessionInfo, LaunchContextKind, SessionEndKind, SessionRotationKind,
};
use stackly_protocol::instance::AutostartSession;

use crate::state::DaemonCore;

/// What the start slot found out about the session of this run.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SessionStatus {
    /// The daemon is not a launchd job, or the registry is in recovery mode:
    /// rotation was skipped and nothing was written.
    Skipped,
    /// The session could not be read or the rotation could not be written;
    /// the subsystem `session` is degraded with `platform.session_unavailable`
    /// and only the "run again" list of reconciliation starts.
    Unavailable(String),
    /// The rotation ran.
    Rotated {
        /// The session of this run.
        login: LoginSession,
        /// What the rotation left in the registry.
        outcome: RotationOutcome,
    },
}

impl SessionStatus {
    /// Whether the rotation was `new`: the reconciliation then reads a gone
    /// process as ended by the logout, not as a crash (AC-6).
    #[must_use]
    pub fn session_changed(&self) -> bool {
        matches!(
            self,
            Self::Rotated { outcome, .. } if outcome.kind == RotationKind::New
        )
    }

    /// The session of this run, when the rotation ran.
    #[must_use]
    pub const fn login(&self) -> Option<&LoginSession> {
        match self {
            Self::Rotated { login, .. } => Some(login),
            _ => None,
        }
    }

    /// The rotation outcome, when it ran.
    #[must_use]
    pub const fn outcome(&self) -> Option<&RotationOutcome> {
        match self {
            Self::Rotated { outcome, .. } => Some(outcome),
            _ => None,
        }
    }
}

/// Whether this daemon was started by launchd.
#[must_use]
pub fn is_launchd(core: &DaemonCore) -> bool {
    crate::ipc::restart::launch_context(core) == LaunchContextKind::Launchd
}

/// Whether `signal` starts *Akhir sesi* (spec 0031, *Sinyal per konteks*):
/// a `SIGTERM` at a launchd daemon with no `daemon.shutdown` accepted
/// before it and no restart plan committed in `stopping`. Every other signal, and every signal at a manual daemon, is
/// the ordinary stop of spec 0006 and 0010.
#[must_use]
pub fn ends_session(core: &DaemonCore, signal: &str) -> bool {
    signal == "SIGTERM" && is_launchd(core) && !core.shutdown_pending() && !core.plan_stopping()
}

/// Steps 1 to 5 of *Akhir sesi*, counted from `t0`, the instant the
/// `SIGTERM` arrived: new requests are refused from here, the operations
/// that run are cancelled, every instance with a live process stops, and the
/// session row is closed `clean` or `interrupted`. The stop order of spec
/// 0006 (step 6) is the caller's next step.
pub async fn end(core: &DaemonCore, t0: tokio::time::Instant) {
    // Step 1: every request from here gets `daemon.shutting_down`.
    core.mark_stopping();
    let Ok(supervisor) = core.supervision().supervisor() else {
        tracing::warn!("the session ends without a supervisor: no instance is stopped");
        return;
    };
    let deadline = t0 + supervisor.session_end_budget();
    let report = supervisor.end_session(deadline).await;
    tracing::info!(
        reason = report.reason.as_code(),
        stops = report.stops,
        unfinished = report.unfinished,
        "the login session ends",
    );
    supervisor.close_session(report.reason).await;
}

/// Closes the session row `clean` when the daemon stops by a request, a
/// planned restart, or a signal that is not a logout (spec 0031, *Sinyal
/// per konteks*). Only a launchd daemon that knows its session writes it.
pub async fn close_clean(core: &DaemonCore) {
    if !is_launchd(core) {
        return;
    }
    if let Ok(supervisor) = core.supervision().supervisor() {
        supervisor
            .close_session(stackly_domain::SessionEndReason::Clean)
            .await;
    }
}

/// Reads the login session and rotates it. Run it after `recover` and
/// before reconciliation, and only with a ready registry.
pub async fn rotate(core: &DaemonCore) -> SessionStatus {
    if !is_launchd(core) {
        return SessionStatus::Skipped;
    }
    let Some(registry) = core.registry() else {
        return SessionStatus::Skipped;
    };
    let platform = core.platform().clone();
    let result = tokio::task::spawn_blocking(move || {
        let login = platform
            .login_session()
            .current()
            .map_err(|error: LoginSessionError| error.to_string())?;
        let now = platform.clock().now();
        let outcome = rotate_session(&registry, &login, now).map_err(|error| error.to_string())?;
        let book = read_book(&registry, &login.id).map_err(|error| error.to_string())?;
        Ok::<_, String>((login, outcome, book))
    })
    .await;
    match result {
        Ok(Ok((login, outcome, book))) => {
            core.supervision().autostart().load(book);
            tracing::info!(
                session = %login.id,
                rotation = outcome.kind.as_code(),
                started = outcome.started.len(),
                "the login session rotated",
            );
            SessionStatus::Rotated { login, outcome }
        }
        Ok(Err(error)) => {
            tracing::warn!(
                code = "platform.session_unavailable",
                %error,
                "the login session cannot be rotated; only the run again list starts",
            );
            SessionStatus::Unavailable(error)
        }
        Err(error) => {
            tracing::warn!(%error, "the session rotation did not run");
            SessionStatus::Unavailable(error.to_string())
        }
    }
}

/// The startup lease state of every instance in this session, in memory
/// (spec 0031, `InstanceSummary.autostart.session`). It is loaded once after
/// the rotation and changes only with a lease: a stop that suppresses one, a
/// policy that releases one. A summary and the events read it without the
/// registry.
#[derive(Debug, Default)]
pub struct AutostartBook {
    sessions: Mutex<HashMap<InstanceId, AutostartSession>>,
}

impl AutostartBook {
    /// The state of `id` in this session: `none` when it holds no lease.
    #[must_use]
    pub fn get(&self, id: InstanceId) -> AutostartSession {
        self.sessions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&id)
            .copied()
            .unwrap_or(AutostartSession::None)
    }

    /// Records the state of `id`; `none` forgets it.
    pub fn set(&self, id: InstanceId, session: AutostartSession) {
        let mut sessions = self.sessions.lock().unwrap_or_else(PoisonError::into_inner);
        if session == AutostartSession::None {
            sessions.remove(&id);
        } else {
            sessions.insert(id, session);
        }
    }

    /// Replaces every state with `loaded`.
    pub fn load(&self, loaded: HashMap<InstanceId, AutostartSession>) {
        *self.sessions.lock().unwrap_or_else(PoisonError::into_inner) = loaded;
    }
}

/// The wire state of a startup lease state.
#[must_use]
pub const fn autostart_of(state: stackly_core::session::StartupLeaseState) -> AutostartSession {
    match state {
        stackly_core::session::StartupLeaseState::Active => AutostartSession::Active,
        stackly_core::session::StartupLeaseState::Suppressed => AutostartSession::Suppressed,
        _ => AutostartSession::None,
    }
}

/// Reads the startup leases of `session` for every instance. A blocking
/// call.
fn read_book(
    registry: &dyn Registry,
    session: &SessionId,
) -> Result<HashMap<InstanceId, AutostartSession>, stackly_domain::RegistryError> {
    let mut loaded = HashMap::new();
    let instances = registry
        .instances(InstanceFilter {
            include_retained: false,
        })?
        .value;
    for instance in instances {
        let leases = registry
            .leases(instance.id(), LeaseFilter::Unreleased)?
            .value;
        let state = leases
            .iter()
            .filter(|lease| {
                lease.holder() == LeaseHolder::Startup && lease.session() == Some(session)
            })
            .map(|lease| match lease.state() {
                LeaseState::Suppressed { .. } => AutostartSession::Suppressed,
                _ => AutostartSession::Active,
            })
            .next();
        if let Some(state) = state {
            loaded.insert(instance.id(), state);
        }
    }
    Ok(loaded)
}

/// `DaemonInfo.session` for the status of this run.
#[must_use]
pub fn session_info(status: &SessionStatus) -> Option<DaemonSessionInfo> {
    let SessionStatus::Rotated { login, outcome } = status else {
        return None;
    };
    Some(DaemonSessionInfo {
        id: login.id.to_string(),
        started_at: Timestamp::from_jiff(outcome.started_at),
        rotation: match outcome.kind {
            RotationKind::First => SessionRotationKind::First,
            RotationKind::New => SessionRotationKind::New,
            RotationKind::Continued => SessionRotationKind::Continued,
            _ => SessionRotationKind::Unrecognized,
        },
        previous_end: outcome.previous_end.map(|reason| match reason {
            stackly_domain::SessionEndReason::Clean => SessionEndKind::Clean,
            stackly_domain::SessionEndReason::Interrupted => SessionEndKind::Interrupted,
            stackly_domain::SessionEndReason::Unknown => SessionEndKind::Unknown,
        }),
    })
}
