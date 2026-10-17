use std::fmt;

use crate::error::PlatformError;

/// Tells which OS user the process runs as.
pub trait UserIdentity: Send + Sync {
    /// Returns the user the process runs as.
    ///
    /// # Errors
    ///
    /// Returns [`PlatformError::NotSupported`] on an OS without an adapter.
    fn current_user(&self) -> Result<UserId, PlatformError>;
}

/// An OS user.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum UserId {
    /// A Unix user ID.
    Unix(u32),
    /// A Windows security identifier in its text form.
    Windows(String),
}

impl fmt::Display for UserId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unix(uid) => write!(f, "uid {uid}"),
            Self::Windows(sid) => write!(f, "sid {sid}"),
        }
    }
}
