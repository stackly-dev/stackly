//! The login session of this daemon run (spec 0031, *Urutan daemon start*,
//! step 3): read once when a launchd daemon starts, rotated in the registry
//! before reconciliation, and kept in memory for the whole run.
//!
//! Only a daemon that launchd started, with a ready registry, knows
//! sessions. Any other daemon (a terminal, `SpawnedDaemon` in a test) and a
//! registry in recovery mode skip rotation and write nothing.

use stackly_core::session::{RotationKind, RotationOutcome, rotate_session};
use stackly_platform::{LoginSession, LoginSessionError};
use stackly_protocol::daemon::LaunchContextKind;

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
        Ok::<_, String>((login, outcome))
    })
    .await;
    match result {
        Ok(Ok((login, outcome))) => {
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
