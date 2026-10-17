use stackly_platform::{PlatformError, UserId, UserIdentity};

/// The macOS user identity: the real user ID of this process.
#[derive(Debug, Default, Clone, Copy)]
pub struct MacosIdentity;

impl UserIdentity for MacosIdentity {
    fn current_user(&self) -> Result<UserId, PlatformError> {
        Ok(UserId::Unix(current_uid()))
    }
}

/// Returns the real user ID of this process.
pub(crate) fn current_uid() -> libc::uid_t {
    // SAFETY: getuid has no preconditions, touches no memory of ours, and
    // cannot fail.
    unsafe { libc::getuid() }
}
