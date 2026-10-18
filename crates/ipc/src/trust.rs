//! The trust rules of [`verify_path`](crate::verify_path) on facts already
//! read from the file system, so the rules and their order can be tested
//! with data. The Swift client runs the same rules, and the shared fixture
//! `protocols/fixtures/endpoint/checks.json` keeps both in step (spec 0013).

use stackly_protocol::codes::UntrustedReason;

/// Permission bits for the group and for others.
const GROUP_OR_OTHER: u32 = 0o077;

/// What a path is, as `lstat` sees it, without following a symlink.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PathKind {
    /// A Unix domain socket.
    Socket,
    /// A directory.
    Directory,
    /// A symbolic link.
    Symlink,
    /// Anything else: a regular file, a fifo, a device.
    Other,
}

/// What [`verify_path`](crate::verify_path) reads about the socket path or
/// its folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct PathFacts {
    /// What the path is.
    pub kind: PathKind,
    /// The owner's uid.
    pub uid: u32,
    /// The mode bits; only the permission bits are checked.
    pub mode: u32,
}

impl PathFacts {
    /// Facts about one path.
    #[must_use]
    pub const fn new(kind: PathKind, uid: u32, mode: u32) -> Self {
        Self { kind, uid, mode }
    }
}

/// The first check the socket path fails, in order: a symlink, not a
/// socket, another owner, access for group or others. `None` when it passes.
#[must_use]
pub fn socket_reason(socket: PathFacts, owner_uid: u32) -> Option<UntrustedReason> {
    match socket.kind {
        PathKind::Symlink => Some(UntrustedReason::Symlink),
        PathKind::Directory | PathKind::Other => Some(UntrustedReason::NotASocket),
        PathKind::Socket if socket.uid != owner_uid => Some(UntrustedReason::WrongOwner),
        PathKind::Socket if socket.mode & GROUP_OR_OTHER != 0 => {
            Some(UntrustedReason::GroupOrOtherAccess)
        }
        PathKind::Socket => None,
    }
}

/// The first check the socket's folder fails, in order: a symlink, not a
/// directory or another owner, access for group or others. `None` when it
/// passes.
#[must_use]
pub fn folder_reason(folder: PathFacts, owner_uid: u32) -> Option<UntrustedReason> {
    match folder.kind {
        PathKind::Symlink => Some(UntrustedReason::Symlink),
        PathKind::Socket | PathKind::Other => Some(UntrustedReason::ParentWrongOwner),
        PathKind::Directory if folder.uid != owner_uid => Some(UntrustedReason::ParentWrongOwner),
        PathKind::Directory if folder.mode & GROUP_OR_OTHER != 0 => {
            Some(UntrustedReason::ParentGroupOrOtherAccess)
        }
        PathKind::Directory => None,
    }
}

/// The first check of [`verify_path`](crate::verify_path) that fails: the
/// socket's checks, then its folder's. `None` when both pass.
#[must_use]
pub fn evaluate_path(
    socket: PathFacts,
    folder: PathFacts,
    owner_uid: u32,
) -> Option<UntrustedReason> {
    socket_reason(socket, owner_uid).or_else(|| folder_reason(folder, owner_uid))
}

#[cfg(test)]
mod tests {
    use super::*;

    const OWN: u32 = 501;

    #[test]
    fn a_private_socket_in_a_private_folder_passes() {
        let socket = PathFacts::new(PathKind::Socket, OWN, 0o140_600);
        let folder = PathFacts::new(PathKind::Directory, OWN, 0o040_700);
        assert_eq!(evaluate_path(socket, folder, OWN), None);
    }

    #[test]
    fn the_socket_is_checked_before_its_folder() {
        let socket = PathFacts::new(PathKind::Socket, OWN, 0o666);
        let folder = PathFacts::new(PathKind::Directory, 502, 0o777);
        assert_eq!(
            evaluate_path(socket, folder, OWN),
            Some(UntrustedReason::GroupOrOtherAccess)
        );
    }
}
