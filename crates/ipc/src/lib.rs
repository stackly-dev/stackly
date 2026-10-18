//! The Stackly IPC transport (spec 0006): the daemon's private socket
//! listener, framed connections, and the client that verifies the endpoint
//! before it sends a byte.
//!
//! Every wire rule lives in `stackly-protocol`; this crate only moves bytes
//! over a Unix domain socket with tokio. It holds no `unsafe` and never
//! opens a TCP or UDP socket (REQ-047). The daemon side is [`Listener`],
//! which binds through a temporary name so the final path only ever exists
//! with mode 0600, and checks the uid of every peer before a frame is read.
//! The client side is [`connect_verified`] and [`Client`], which check the
//! socket path and the server's uid first (AC-070), then run the handshake.
//!
//! The Unix transport sits in a private `unix` module and is re-exported
//! here; a Windows named pipe transport would be a second module with the
//! same API. Time uses `tokio::time::Instant`, so tests can pause the clock.

use std::time::Duration;

mod error;
#[cfg(unix)]
mod unix;

pub use error::IpcError;
#[cfg(unix)]
pub use unix::{
    Accepted, BindState, Client, ConnectPolicy, EventStream, FrameReader, FrameWriter,
    FramedConnection, Listener, PeerCred, PeerCredSource, TokioPeerCred, connect_verified,
    verify_path, verify_peer,
};

/// The most bytes read from a socket at once. With every complete frame
/// taken before the next read, a frame buffer never holds more than
/// `CONTROL_FRAME_MAX + 4 + READ_CHUNK` bytes.
pub const READ_CHUNK: usize = 65_536;

/// How long a frame may take to arrive once its first byte did.
pub const FRAME_STALL_TIMEOUT: Duration = Duration::from_secs(30);

/// How long writing a rejection frame may take before the connection is
/// closed without it, so a peer that does not read never holds anything.
pub const REJECT_WRITE_TIMEOUT: Duration = Duration::from_secs(1);

/// How long a client keeps reading after a write failed, to report the
/// daemon's error frame instead of `ipc.connection_closed`.
pub const PENDING_READ_TIMEOUT: Duration = Duration::from_secs(1);
