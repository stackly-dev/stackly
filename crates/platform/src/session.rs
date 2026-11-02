//! The login session port (spec 0031, AC-1): which login session this
//! daemon belongs to, as the `SessionId` that rotation and suppression
//! compare. The ID is the boot UUID plus the audit session of the login.

use stackly_domain::{BootId, Os, SessionId};

use crate::capability::PlatformCapability;

/// The login session a process runs in: the boot it started in and the
/// audit session of the login, joined into one [`SessionId`].
///
/// ```
/// use stackly_platform::LoginSession;
///
/// let boot = "6F9619FF-8B86-D011-B42D-00C04FC964FF".parse()?;
/// let session = LoginSession::new(boot, 100014)?;
/// assert_eq!(session.id.as_str(), "6f9619ff-8b86-d011-b42d-00c04fc964ff/100014");
/// assert!(LoginSession::new(boot, 0).is_err());
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct LoginSession {
    /// `<boot UUID, lowercase>/<audit session, decimal>`, at most 47 bytes.
    pub id: SessionId,
    /// The boot the session started in (`kern.bootsessionuuid`).
    pub boot_id: BootId,
    /// The audit session ID of the login, from `getaudit_addr`.
    pub audit_session: u32,
}

impl LoginSession {
    /// Joins a boot and an audit session into a session.
    ///
    /// # Errors
    ///
    /// Returns [`LoginSessionError::Unavailable`] for audit session `0`,
    /// which macOS reports for a process outside any login session
    /// (`AU_DEFAUDITSID`).
    pub fn new(boot_id: BootId, audit_session: u32) -> Result<Self, LoginSessionError> {
        if audit_session == 0 {
            return Err(LoginSessionError::Unavailable {
                reason: "the process is not in a login session (audit session 0)",
            });
        }
        let id = format!("{boot_id}/{audit_session}")
            .parse::<SessionId>()
            .map_err(|_| LoginSessionError::Unavailable {
                reason: "the session ID is not a valid session ID",
            })?;
        Ok(Self {
            id,
            boot_id,
            audit_session,
        })
    }
}

/// Why the login session could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum LoginSessionError {
    /// The OS has no adapter for this port.
    #[error("{capability} is not supported on {os}")]
    NotSupported {
        /// The capability that was asked for.
        capability: PlatformCapability,
        /// The OS that lacks it.
        os: Os,
    },
    /// The OS did not give a usable session. The daemon reports it as
    /// `platform.session_unavailable`.
    #[error("cannot read the login session: {reason}")]
    Unavailable {
        /// What was wrong, without values.
        reason: &'static str,
    },
}

/// Reads the login session of the current process.
pub trait LoginSessionInfo: Send + Sync {
    /// Returns the login session of this process.
    ///
    /// # Errors
    ///
    /// Returns a [`LoginSessionError`] when the OS gives no session, such as
    /// a process outside every login, or has no adapter.
    fn current(&self) -> Result<LoginSession, LoginSessionError>;
}

/// The login session port of an OS without an adapter: it answers
/// [`LoginSessionError::NotSupported`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnsupportedLoginSessionInfo {
    os: Os,
}

impl UnsupportedLoginSessionInfo {
    /// Creates the port for `os`.
    #[must_use]
    pub const fn new(os: Os) -> Self {
        Self { os }
    }
}

impl LoginSessionInfo for UnsupportedLoginSessionInfo {
    fn current(&self) -> Result<LoginSession, LoginSessionError> {
        Err(LoginSessionError::NotSupported {
            capability: PlatformCapability::LoginSession,
            os: self.os,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOOT: &str = "6F9619FF-8B86-D011-B42D-00C04FC964FF";

    #[test]
    fn the_session_id_joins_the_lowercase_boot_and_the_decimal_audit_session() {
        let session = LoginSession::new(BOOT.parse().unwrap(), 100_014).unwrap();

        assert_eq!(
            session.id.as_str(),
            "6f9619ff-8b86-d011-b42d-00c04fc964ff/100014"
        );
        assert_eq!(session.audit_session, 100_014);
    }

    #[test]
    fn the_longest_session_id_is_47_bytes() {
        let session = LoginSession::new(BOOT.parse().unwrap(), u32::MAX).unwrap();

        assert_eq!(session.id.as_str().len(), 47);
    }

    #[test]
    fn audit_session_zero_is_unavailable() {
        let error = LoginSession::new(BOOT.parse().unwrap(), 0).unwrap_err();

        assert!(matches!(error, LoginSessionError::Unavailable { .. }));
    }

    #[test]
    fn an_unsupported_port_names_the_capability_and_the_os() {
        let error = UnsupportedLoginSessionInfo::new(Os::Windows)
            .current()
            .unwrap_err();

        assert_eq!(
            error.to_string(),
            "login_session is not supported on windows"
        );
    }
}
