//! Login sessions of the daemon (spec 0031): the rows `daemon_sessions`
//! keeps, one per login the daemon ran in, and how a row ended.

use jiff::Timestamp;

use crate::code::code_enum;
use crate::error::{InvariantDetail, InvariantError};
use crate::process::BootId;
use crate::value::SessionId;

/// How many session rows the registry keeps (spec 0031, AC-14).
pub const SESSIONS_KEPT: u32 = 50;

code_enum! {
    /// How a session row ended.
    pub enum SessionEndReason: SessionEndReason {
        /// Every instance stopped cleanly before the daemon exited.
        Clean => "clean",
        /// The daemon exited while an instance was still stopping.
        Interrupted => "interrupted",
        /// The row was still open when the next session began, so the
        /// daemon did not see it end.
        Unknown => "unknown",
    }
}

/// Every field of a [`DaemonSession`] as the registry stores it, for
/// [`DaemonSession::restore`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredSession {
    /// The session ID.
    pub id: SessionId,
    /// The boot the session started in.
    pub boot_id: BootId,
    /// The audit session ID of the login.
    pub audit_session: u32,
    /// When the session was first seen.
    pub started_at: Timestamp,
    /// When it ended, with the reason.
    pub ended: Option<(Timestamp, SessionEndReason)>,
}

/// One login session the daemon ran in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonSession {
    id: SessionId,
    boot_id: BootId,
    audit_session: u32,
    started_at: Timestamp,
    ended: Option<(Timestamp, SessionEndReason)>,
}

impl DaemonSession {
    /// Creates an open session first seen at `started_at`.
    ///
    /// # Errors
    ///
    /// Returns an [`InvariantError`] with
    /// [`InvariantDetail::InconsistentRecord`] for audit session `0`.
    pub fn new(
        id: SessionId,
        boot_id: BootId,
        audit_session: u32,
        started_at: Timestamp,
    ) -> Result<Self, InvariantError> {
        Self::restore(StoredSession {
            id,
            boot_id,
            audit_session,
            started_at,
            ended: None,
        })
    }

    /// Rebuilds a session the registry stored. Only the registry and test
    /// fakes call it.
    ///
    /// # Errors
    ///
    /// Returns an [`InvariantError`] with
    /// [`InvariantDetail::InconsistentRecord`] for audit session `0`, and
    /// for a session that ended before it started.
    pub fn restore(stored: StoredSession) -> Result<Self, InvariantError> {
        if stored.audit_session == 0 {
            return Err(InvariantDetail::InconsistentRecord {
                resource: "session",
                reason: "the audit session is at least 1",
            }
            .into());
        }
        if let Some((ended_at, _)) = &stored.ended
            && *ended_at < stored.started_at
        {
            return Err(InvariantDetail::InconsistentRecord {
                resource: "session",
                reason: "a session does not end before it starts",
            }
            .into());
        }
        Ok(Self {
            id: stored.id,
            boot_id: stored.boot_id,
            audit_session: stored.audit_session,
            started_at: stored.started_at,
            ended: stored.ended,
        })
    }

    /// Returns the session ID.
    #[must_use]
    pub const fn id(&self) -> &SessionId {
        &self.id
    }

    /// Returns the boot the session started in.
    #[must_use]
    pub const fn boot_id(&self) -> BootId {
        self.boot_id
    }

    /// Returns the audit session ID of the login.
    #[must_use]
    pub const fn audit_session(&self) -> u32 {
        self.audit_session
    }

    /// Returns when the session was first seen.
    #[must_use]
    pub const fn started_at(&self) -> Timestamp {
        self.started_at
    }

    /// Returns when and why the session ended, or `None` while it is open.
    #[must_use]
    pub const fn ended(&self) -> Option<(Timestamp, SessionEndReason)> {
        self.ended
    }

    /// Returns whether the session is open.
    #[must_use]
    pub const fn is_open(&self) -> bool {
        self.ended.is_none()
    }

    /// Returns the reason the session ended, or `None` while it is open.
    #[must_use]
    pub fn end_reason(&self) -> Option<SessionEndReason> {
        self.ended.map(|(_, reason)| reason)
    }

    /// Closes the session at `at` with `reason`.
    pub fn end(&mut self, at: Timestamp, reason: SessionEndReason) {
        self.ended = Some((at.max(self.started_at), reason));
    }

    /// Opens a closed session again, as a daemon that comes back in the
    /// same session does (spec 0031, AC-13).
    pub fn reopen(&mut self) {
        self.ended = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> DaemonSession {
        DaemonSession::new(
            "6f9619ff-8b86-d011-b42d-00c04fc964ff/100014"
                .parse()
                .unwrap(),
            "6f9619ff-8b86-d011-b42d-00c04fc964ff".parse().unwrap(),
            100_014,
            "2026-11-02T08:00:00Z".parse().unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn a_new_session_is_open() {
        let session = session();

        assert!(session.is_open());
        assert_eq!(session.end_reason(), None);
    }

    #[test]
    fn ending_and_reopening_a_session_clears_the_end() {
        let mut session = session();
        session.end(
            "2026-11-02T09:00:00Z".parse().unwrap(),
            SessionEndReason::Clean,
        );
        assert_eq!(session.end_reason(), Some(SessionEndReason::Clean));

        session.reopen();

        assert!(session.is_open());
    }

    #[test]
    fn audit_session_zero_is_refused() {
        let error = DaemonSession::new(
            "boot/0".parse().unwrap(),
            "6f9619ff-8b86-d011-b42d-00c04fc964ff".parse().unwrap(),
            0,
            "2026-11-02T08:00:00Z".parse().unwrap(),
        );

        assert!(error.is_err());
    }

    #[test]
    fn end_reasons_round_trip_through_their_codes() {
        for reason in SessionEndReason::ALL {
            assert_eq!(
                reason.as_code().parse::<SessionEndReason>().unwrap(),
                *reason
            );
        }
    }
}
