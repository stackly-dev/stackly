use crate::fs::{FileInfo, FileKind};
use crate::identity::UserId;

/// Why a path is not a private folder of the current user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, thiserror::Error)]
#[non_exhaustive]
pub enum PrivateDirViolation {
    /// The path is a symlink.
    #[error("a symlink")]
    Symlink,
    /// The path is a file, FIFO, socket, or device.
    #[error("not a directory")]
    NotADirectory,
    /// Another user owns the folder, or the OS reports no owner.
    #[error("owned by another user")]
    OwnedByAnotherUser,
    /// The group or others have some access, or the OS reports no mode.
    #[error("group or others have access")]
    OpenToOthers,
}

impl PrivateDirViolation {
    /// Returns the rule the folder broke, for example
    /// `owned by another user`.
    #[must_use]
    pub const fn reason(self) -> &'static str {
        match self {
            Self::Symlink => "a symlink",
            Self::NotADirectory => "not a directory",
            Self::OwnedByAnotherUser => "owned by another user",
            Self::OpenToOthers => "group or others have access",
        }
    }
}

/// Checks that metadata read without following a symlink describes a
/// private folder of `user`: a directory, not a symlink, owned by `user`,
/// with no group or other access (`mode & 0o077 == 0`).
///
/// It is the pure part of the folder privacy rule: core's
/// `ensure_private_dir` creates a missing folder and then checks it here,
/// and an adapter that owns a folder of its own, such as the Keychain
/// adapter's `keychain/`, checks it here without core. A folder that fails
/// is rejected, never repaired.
///
/// ```
/// use stackly_platform::{
///     FileIdentity, FileInfo, FileKind, PrivateDirViolation, UserId, check_private_dir,
/// };
///
/// let user = UserId::Unix(501);
/// let identity = FileIdentity { device: 1, inode: 2 };
/// let dir = |mode| FileInfo::new(FileKind::Dir, 0, identity, Some(user.clone()), Some(mode));
/// assert_eq!(check_private_dir(&dir(0o700), &user), Ok(()));
/// assert_eq!(
///     check_private_dir(&dir(0o750), &user),
///     Err(PrivateDirViolation::OpenToOthers)
/// );
/// ```
///
/// # Errors
///
/// Returns the [`PrivateDirViolation`] the metadata shows, checked in the
/// order kind, owner, then mode.
pub fn check_private_dir(info: &FileInfo, user: &UserId) -> Result<(), PrivateDirViolation> {
    match info.kind {
        FileKind::Symlink => Err(PrivateDirViolation::Symlink),
        FileKind::File | FileKind::Other => Err(PrivateDirViolation::NotADirectory),
        FileKind::Dir if info.owner.as_ref() != Some(user) => {
            Err(PrivateDirViolation::OwnedByAnotherUser)
        }
        FileKind::Dir if info.mode.is_none_or(|mode| mode & 0o077 != 0) => {
            Err(PrivateDirViolation::OpenToOthers)
        }
        FileKind::Dir => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::FileIdentity;

    const IDENTITY: FileIdentity = FileIdentity {
        device: 1,
        inode: 2,
    };

    fn info(kind: FileKind, owner: Option<UserId>, mode: Option<u32>) -> FileInfo {
        FileInfo::new(kind, 0, IDENTITY, owner, mode)
    }

    #[test]
    fn a_folder_of_the_user_with_no_group_or_other_access_is_private() {
        let user = UserId::Unix(501);
        for mode in [0o700, 0o500, 0o1700, 0o000] {
            let dir = info(FileKind::Dir, Some(user.clone()), Some(mode));
            assert_eq!(check_private_dir(&dir, &user), Ok(()), "{mode:o}");
        }
    }

    #[test]
    fn every_other_kind_owner_or_mode_names_the_broken_rule() {
        let user = UserId::Unix(501);
        let me = Some(user.clone());
        let cases = [
            (
                info(FileKind::Symlink, me.clone(), Some(0o700)),
                PrivateDirViolation::Symlink,
            ),
            (
                info(FileKind::File, me.clone(), Some(0o600)),
                PrivateDirViolation::NotADirectory,
            ),
            (
                info(FileKind::Other, me.clone(), Some(0o600)),
                PrivateDirViolation::NotADirectory,
            ),
            (
                info(FileKind::Dir, Some(UserId::Unix(0)), Some(0o700)),
                PrivateDirViolation::OwnedByAnotherUser,
            ),
            (
                info(FileKind::Dir, None, Some(0o700)),
                PrivateDirViolation::OwnedByAnotherUser,
            ),
            (
                info(FileKind::Dir, me.clone(), Some(0o710)),
                PrivateDirViolation::OpenToOthers,
            ),
            (
                info(FileKind::Dir, me.clone(), Some(0o701)),
                PrivateDirViolation::OpenToOthers,
            ),
            (
                info(FileKind::Dir, me, None),
                PrivateDirViolation::OpenToOthers,
            ),
        ];
        for (info, expected) in cases {
            assert_eq!(check_private_dir(&info, &user), Err(expected), "{info:?}");
            assert_eq!(expected.to_string(), expected.reason());
        }
    }
}
