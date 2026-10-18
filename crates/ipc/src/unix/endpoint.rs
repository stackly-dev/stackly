//! The client's trust checks (AC-070): the socket path before connecting,
//! the server's uid after, and not one byte written to an endpoint that
//! fails either.

use std::fs::{self, Metadata};
use std::io;
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::path::Path;

use stackly_protocol::codes::UntrustedReason;
use tokio::net::UnixStream;

use super::connection::FramedConnection;
use crate::IpcError;

/// Permission bits for the group and for others.
const GROUP_OR_OTHER: u32 = 0o077;

/// Whose socket a client accepts. In production both uids are the
/// client's own; tests give another `peer_uid` to exercise
/// [`verify_peer`] against a real listener.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct ConnectPolicy {
    /// The uid that must own the socket and the folder holding it.
    pub owner_uid: u32,
    /// The uid the server process must run as.
    pub peer_uid: u32,
}

impl ConnectPolicy {
    /// A policy with separate uids for the path checks and the peer check.
    #[must_use]
    pub const fn new(owner_uid: u32, peer_uid: u32) -> Self {
        Self {
            owner_uid,
            peer_uid,
        }
    }

    /// The production policy: the socket, its folder, and the server all
    /// belong to `uid`.
    #[must_use]
    pub const fn same_user(uid: u32) -> Self {
        Self::new(uid, uid)
    }
}

/// Checks the socket path without following symlinks: it must be a socket
/// owned by `owner_uid` with no access for group or others, in a folder
/// that is not a symlink, is owned by `owner_uid`, and has no access for
/// group or others either.
///
/// # Errors
///
/// Returns [`IpcError::EndpointMissing`] when nothing exists at `path`,
/// [`IpcError::EndpointUntrusted`] with the failed check as its reason, and
/// [`IpcError::Io`] with op `stat` for another OS error, such as a folder
/// the client may not search.
pub fn verify_path(path: &Path, owner_uid: u32) -> Result<(), IpcError> {
    let untrusted = |reason| IpcError::EndpointUntrusted {
        path: Some(path.to_path_buf()),
        reason,
    };
    let socket = lstat(path, path)?;
    let file_type = socket.file_type();
    if file_type.is_symlink() {
        return Err(untrusted(UntrustedReason::Symlink));
    }
    if !file_type.is_socket() {
        return Err(untrusted(UntrustedReason::NotASocket));
    }
    if socket.uid() != owner_uid {
        return Err(untrusted(UntrustedReason::WrongOwner));
    }
    if socket.mode() & GROUP_OR_OTHER != 0 {
        return Err(untrusted(UntrustedReason::GroupOrOtherAccess));
    }
    let folder = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    let folder = lstat(folder, path)?;
    if folder.file_type().is_symlink() {
        return Err(untrusted(UntrustedReason::Symlink));
    }
    if folder.uid() != owner_uid {
        return Err(untrusted(UntrustedReason::ParentWrongOwner));
    }
    if folder.mode() & GROUP_OR_OTHER != 0 {
        return Err(untrusted(UntrustedReason::ParentGroupOrOtherAccess));
    }
    Ok(())
}

/// Checks that the server process on `stream` runs as `peer_uid`.
///
/// # Errors
///
/// Returns [`IpcError::EndpointUntrusted`] (without a path) with reason
/// `peer_uid_mismatch`, or `peer_credentials_unavailable` when the
/// credentials cannot be read.
pub fn verify_peer(stream: &UnixStream, peer_uid: u32) -> Result<(), IpcError> {
    let reason = match stream.peer_cred() {
        Ok(cred) if cred.uid() == peer_uid => return Ok(()),
        Ok(_) => UntrustedReason::PeerUidMismatch,
        Err(_) => UntrustedReason::PeerCredentialsUnavailable,
    };
    Err(IpcError::EndpointUntrusted { path: None, reason })
}

/// Checks the socket path, connects, then checks the server's uid. Nothing
/// is written unless all three succeed.
///
/// # Errors
///
/// The errors of [`verify_path`] and [`verify_peer`] (with the path filled
/// in), [`IpcError::EndpointMissing`] when the socket vanished before the
/// connect, [`IpcError::ConnectionRefused`] for a stale socket, and
/// [`IpcError::Io`] with op `connect` for another OS error.
pub async fn connect_verified(
    path: &Path,
    policy: ConnectPolicy,
) -> Result<FramedConnection, IpcError> {
    verify_path(path, policy.owner_uid)?;
    let stream = UnixStream::connect(path)
        .await
        .map_err(|source| match source.kind() {
            io::ErrorKind::NotFound => IpcError::EndpointMissing {
                path: path.to_path_buf(),
            },
            io::ErrorKind::ConnectionRefused => IpcError::ConnectionRefused {
                path: path.to_path_buf(),
            },
            _ => IpcError::Io {
                op: "connect",
                source,
            },
        })?;
    verify_peer(&stream, policy.peer_uid).map_err(|error| error.at_path(path))?;
    Ok(FramedConnection::new(stream))
}

/// `symlink_metadata` with its errors mapped against the socket path.
fn lstat(target: &Path, socket: &Path) -> Result<Metadata, IpcError> {
    fs::symlink_metadata(target).map_err(|source| {
        if source.kind() == io::ErrorKind::NotFound {
            IpcError::EndpointMissing {
                path: socket.to_path_buf(),
            }
        } else {
            IpcError::Io { op: "stat", source }
        }
    })
}
