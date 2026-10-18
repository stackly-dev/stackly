//! The daemon side: bind the socket so its final path only ever exists
//! with mode 0600, accept, and check every peer's uid before a frame is
//! read (AC-047, BR-012).

use std::ffi::OsString;
use std::fmt;
use std::fs::{self, FileType, Metadata, Permissions};
use std::io;
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use stackly_protocol::ErrorBody;
use stackly_protocol::codes::ipc;
use tokio::net::{UnixListener, UnixStream};

use super::connection::FramedConnection;
use crate::IpcError;

/// The socket's mode: only its owner may connect.
const SOCKET_MODE: u32 = 0o600;
/// Appended to the socket path to name the socket while it is prepared.
const TEMP_SUFFIX: &str = ".tmp";

/// The credentials of a connected peer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct PeerCred {
    /// The peer's user ID.
    pub uid: u32,
    /// The peer's process ID, when the OS reports it.
    pub pid: Option<u32>,
}

impl PeerCred {
    /// Credentials with this uid and pid.
    #[must_use]
    pub const fn new(uid: u32, pid: Option<u32>) -> Self {
        Self { uid, pid }
    }
}

/// Where a [`Listener`] reads the credentials of each peer.
///
/// The default is [`TokioPeerCred`]. Tests install a source that fails, to
/// prove that unreadable credentials reject the peer.
pub trait PeerCredSource: Send + Sync + 'static {
    /// Reads the credentials of the peer on `stream`.
    ///
    /// # Errors
    ///
    /// Returns the OS error when the credentials cannot be read.
    fn peer_cred(&self, stream: &UnixStream) -> io::Result<PeerCred>;
}

/// Reads peer credentials through tokio's `UnixStream::peer_cred`, which
/// uses `getpeereid` for the uid and, on macOS, `LOCAL_PEERPID` for the
/// pid.
#[derive(Debug, Clone, Copy, Default)]
pub struct TokioPeerCred;

impl PeerCredSource for TokioPeerCred {
    fn peer_cred(&self, stream: &UnixStream) -> io::Result<PeerCred> {
        let cred = stream.peer_cred()?;
        Ok(PeerCred::new(
            cred.uid(),
            cred.pid().and_then(|pid| u32::try_from(pid).ok()),
        ))
    }
}

/// What [`Listener::accept`] produced. Only an authorized peer comes with
/// a connection, so a frame can never be read from a peer that skipped the
/// uid check.
#[derive(Debug)]
#[non_exhaustive]
pub enum Accepted {
    /// The peer runs as the expected user.
    Authorized {
        /// The connection to read frames from.
        connection: FramedConnection,
        /// Who the peer is.
        peer: PeerCred,
    },
    /// The peer runs as another user, or its credentials could not be
    /// read. It was sent `ipc.peer_rejected` from a separate task and the
    /// connection is closed; these values are for the log.
    Rejected {
        /// The peer's uid, when it could be read.
        uid: Option<u32>,
        /// The peer's pid, when it could be read.
        pid: Option<u32>,
    },
}

/// Where the socket path stands compared with the socket this listener
/// bound, from [`Listener::check_bound`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindState {
    /// The path is still this listener's socket.
    Bound,
    /// Nothing is at the path, or it cannot be read.
    Missing,
    /// Something else now sits at the path.
    Replaced,
}

/// The device and inode of a path, which name the socket this listener
/// bound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileIdentity {
    dev: u64,
    ino: u64,
}

impl FileIdentity {
    fn of(path: &Path) -> io::Result<Self> {
        let meta = fs::symlink_metadata(path)?;
        Ok(Self {
            dev: meta.dev(),
            ino: meta.ino(),
        })
    }
}

/// The daemon's socket listener.
pub struct Listener {
    path: PathBuf,
    expected_uid: u32,
    inner: UnixListener,
    identity: FileIdentity,
    peer_source: Arc<dyn PeerCredSource>,
}

impl fmt::Debug for Listener {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Listener")
            .field("path", &self.path)
            .field("expected_uid", &self.expected_uid)
            .finish_non_exhaustive()
    }
}

impl Listener {
    /// Binds the socket at `path` and accepts only peers running as
    /// `expected_uid`.
    ///
    /// `path` and `<path>.tmp` may each be absent or a socket; anything
    /// else (a file, a folder, a symlink) is refused and nothing is
    /// removed. A socket at `<path>.tmp` is removed, a new socket is bound
    /// there, set to mode 0600, then renamed over `path`, which also
    /// replaces a stale socket there. The final path therefore never exists
    /// with another mode. The device and inode of `path` are recorded for
    /// [`Self::check_bound`] and [`Self::unlink_if_own`].
    ///
    /// The caller must already hold the daemon lock, because binding
    /// replaces any socket at `path`, and must call it inside a tokio
    /// runtime with IO enabled.
    ///
    /// # Errors
    ///
    /// Returns [`IpcError::EndpointOccupied`] when `path` or `<path>.tmp`
    /// holds something that is not a socket, and [`IpcError::Io`] with op
    /// `stat`, `unlink`, `bind`, `chmod`, or `rename` for an OS error.
    pub fn bind(path: impl Into<PathBuf>, expected_uid: u32) -> Result<Self, IpcError> {
        let path = path.into();
        let (inner, identity) = bind_private(&path)?;
        Ok(Self {
            path,
            expected_uid,
            inner,
            identity,
            peer_source: Arc::new(TokioPeerCred),
        })
    }

    /// Reads peer credentials from `source` instead of [`TokioPeerCred`].
    #[must_use]
    pub fn with_peer_cred_source(mut self, source: impl PeerCredSource) -> Self {
        self.peer_source = Arc::new(source);
        self
    }

    /// The socket path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The uid every peer must run as.
    #[must_use]
    pub const fn expected_uid(&self) -> u32 {
        self.expected_uid
    }

    /// Accepts the next connection and checks its peer's uid at once,
    /// before any frame is read.
    ///
    /// A peer running as another user, or whose credentials cannot be read,
    /// is sent `ipc.peer_rejected` from a separate task bounded by
    /// [`REJECT_WRITE_TIMEOUT`](crate::REJECT_WRITE_TIMEOUT), then closed;
    /// this call returns [`Accepted::Rejected`] without waiting on that
    /// write, so a peer that never reads cannot hold the accept loop.
    ///
    /// # Errors
    ///
    /// Returns [`IpcError::Io`] with op `accept` when accepting failed, for
    /// example with too many open files.
    pub async fn accept(&self) -> Result<Accepted, IpcError> {
        let (stream, _) = self.inner.accept().await.map_err(|source| IpcError::Io {
            op: "accept",
            source,
        })?;
        match self.peer_source.peer_cred(&stream) {
            Ok(peer) if peer.uid == self.expected_uid => Ok(Accepted::Authorized {
                connection: FramedConnection::new(stream),
                peer,
            }),
            Ok(peer) => {
                reject_in_background(stream);
                Ok(Accepted::Rejected {
                    uid: Some(peer.uid),
                    pid: peer.pid,
                })
            }
            Err(_) => {
                reject_in_background(stream);
                Ok(Accepted::Rejected {
                    uid: None,
                    pid: None,
                })
            }
        }
    }

    /// Compares the socket path with the socket this listener bound,
    /// without following symlinks. A path that cannot be read counts as
    /// [`BindState::Missing`], since clients cannot reach the socket either.
    #[must_use]
    pub fn check_bound(&self) -> BindState {
        match FileIdentity::of(&self.path) {
            Ok(identity) if identity == self.identity => BindState::Bound,
            Ok(_) => BindState::Replaced,
            Err(_) => BindState::Missing,
        }
    }

    /// Binds again at the same path with the same steps as [`Self::bind`],
    /// after the socket or its folder went missing. The old socket stops
    /// accepting once the new one is bound.
    ///
    /// # Errors
    ///
    /// The errors of [`Self::bind`]; the listener keeps its old socket.
    pub fn rebind(&mut self) -> Result<(), IpcError> {
        let (inner, identity) = bind_private(&self.path)?;
        self.inner = inner;
        self.identity = identity;
        Ok(())
    }

    /// Removes the socket path, but only while its device and inode are
    /// still the ones this listener bound. Returns whether it removed it.
    ///
    /// # Errors
    ///
    /// Returns [`IpcError::Io`] with op `stat` or `unlink` for an OS error
    /// other than the path being gone.
    pub fn unlink_if_own(&self) -> Result<bool, IpcError> {
        match FileIdentity::of(&self.path) {
            Ok(identity) if identity == self.identity => match fs::remove_file(&self.path) {
                Ok(()) => Ok(true),
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
                Err(source) => Err(IpcError::Io {
                    op: "unlink",
                    source,
                }),
            },
            Ok(_) => Ok(false),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(source) => Err(IpcError::Io { op: "stat", source }),
        }
    }
}

/// Sends `ipc.peer_rejected` on `stream` from a new task, then closes it.
fn reject_in_background(stream: UnixStream) {
    let error = ErrorBody::new(
        &ipc::PEER_REJECTED,
        "the peer runs as another user or its credentials cannot be read",
    );
    tokio::spawn(async move {
        if let Err(failure) = FramedConnection::new(stream)
            .close_with_error(None, error)
            .await
        {
            tracing::debug!(error = %failure, "the peer rejection frame was not sent");
        }
    });
}

/// The bind steps of [`Listener::bind`].
fn bind_private(path: &Path) -> Result<(UnixListener, FileIdentity), IpcError> {
    let temp = temp_path(path);
    let stale_temp = check_replaceable(&temp)?;
    check_replaceable(path)?;
    if stale_temp {
        match fs::remove_file(&temp) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(IpcError::Io {
                    op: "unlink",
                    source,
                });
            }
        }
    }
    let listener = std::os::unix::net::UnixListener::bind(&temp)
        .map_err(|source| IpcError::Io { op: "bind", source })?;
    if let Err(error) = publish(&temp, path) {
        // The temporary socket is the one just bound, so removing it is safe.
        let _ = fs::remove_file(&temp);
        return Err(error);
    }
    let identity = FileIdentity::of(path).map_err(|source| IpcError::Io { op: "stat", source })?;
    listener
        .set_nonblocking(true)
        .map_err(|source| IpcError::Io { op: "bind", source })?;
    let listener =
        UnixListener::from_std(listener).map_err(|source| IpcError::Io { op: "bind", source })?;
    Ok((listener, identity))
}

/// Sets the temporary socket to 0600, then renames it to its final path.
fn publish(temp: &Path, path: &Path) -> Result<(), IpcError> {
    fs::set_permissions(temp, Permissions::from_mode(SOCKET_MODE)).map_err(|source| {
        IpcError::Io {
            op: "chmod",
            source,
        }
    })?;
    fs::rename(temp, path).map_err(|source| IpcError::Io {
        op: "rename",
        source,
    })
}

/// `<path>.tmp`.
fn temp_path(path: &Path) -> PathBuf {
    let mut name = OsString::from(path.as_os_str());
    name.push(TEMP_SUFFIX);
    PathBuf::from(name)
}

/// Whether a socket sits at `path` (to be replaced), or nothing does.
/// Anything else is [`IpcError::EndpointOccupied`].
fn check_replaceable(path: &Path) -> Result<bool, IpcError> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_socket() => Ok(true),
        Ok(meta) => Err(IpcError::EndpointOccupied {
            path: path.to_path_buf(),
            kind: kind_of(&meta),
        }),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(source) => Err(IpcError::Io { op: "stat", source }),
    }
}

/// The `kind` detail of `ipc.endpoint_occupied`.
fn kind_of(meta: &Metadata) -> &'static str {
    let file_type: FileType = meta.file_type();
    if file_type.is_symlink() {
        "symlink"
    } else if file_type.is_dir() {
        "directory"
    } else if file_type.is_file() {
        "file"
    } else if file_type.is_fifo() {
        "fifo"
    } else if file_type.is_block_device() {
        "block_device"
    } else if file_type.is_char_device() {
        "char_device"
    } else {
        "other"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_temporary_name_appends_tmp_to_the_whole_file_name() {
        assert_eq!(
            temp_path(Path::new("/tmp/skt-a/stackly-0011/stacklyd.sock")),
            Path::new("/tmp/skt-a/stackly-0011/stacklyd.sock.tmp")
        );
    }
}
