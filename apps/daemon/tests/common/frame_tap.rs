//! A recording relay between a client and the daemon socket: every frame
//! that crosses it, in both directions, is kept for the test to scan. The
//! relay forwards the bytes untouched, so the daemon and the client behave
//! as they do without it (spec 0011 AC-14, AC-16).

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::task::JoinHandle;

/// Which way a frame went.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Way {
    /// From the client to the daemon.
    Request,
    /// From the daemon to the client.
    Answer,
}

/// One frame the relay saw.
#[derive(Debug, Clone)]
pub struct Seen {
    /// The direction.
    pub way: Way,
    /// The payload after the 4 byte length header.
    pub payload: Vec<u8>,
}

impl Seen {
    /// The payload as JSON; every frame the daemon protocol carries is.
    pub fn json(&self) -> Value {
        serde_json::from_slice(&self.payload).expect("a frame payload is JSON")
    }

    /// The payload as text.
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.payload).into_owned()
    }

    /// The call name of a request, or the reply, error, or event kind of
    /// an answer.
    pub fn name(&self) -> String {
        let frame = self.json();
        match self.way {
            Way::Request => frame["call"]["type"].as_str().unwrap_or("?").to_owned(),
            Way::Answer => match frame["type"].as_str() {
                Some("response") => frame["content"]["reply"]["type"]
                    .as_str()
                    .unwrap_or("?")
                    .to_owned(),
                Some("error") => frame["content"]["error"]["code"]
                    .as_str()
                    .unwrap_or("?")
                    .to_owned(),
                Some("event") => frame["content"]["kind"]["type"]
                    .as_str()
                    .unwrap_or("?")
                    .to_owned(),
                _ => "?".to_owned(),
            },
        }
    }
}

type Log = Arc<Mutex<Vec<Seen>>>;

/// The relay: a socket of its own in a private folder, in front of the
/// daemon socket.
pub struct FrameTap {
    socket: PathBuf,
    seen: Log,
    accept: JoinHandle<()>,
    _dir: PathBuf,
}

impl FrameTap {
    /// Listens on a socket inside `folder` and relays every connection to
    /// `daemon`. The folder is made private, as the client's trust rules
    /// need.
    pub fn start(folder: &Path, daemon: &Path) -> Self {
        std::fs::create_dir_all(folder).expect("the relay folder is made");
        std::fs::set_permissions(folder, std::fs::Permissions::from_mode(0o700))
            .expect("the relay folder is private");
        let socket = folder.join("tap.sock");
        let listener = UnixListener::bind(&socket).expect("the relay socket binds");
        std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600))
            .expect("the relay socket is private");
        let seen: Log = Arc::default();
        let daemon = daemon.to_path_buf();
        let log = Arc::clone(&seen);
        let accept = tokio::spawn(async move {
            while let Ok((client, _)) = listener.accept().await {
                let Ok(upstream) = UnixStream::connect(&daemon).await else {
                    continue;
                };
                let (client_read, client_write) = client.into_split();
                let (daemon_read, daemon_write) = upstream.into_split();
                tokio::spawn(relay(client_read, daemon_write, Way::Request, log.clone()));
                tokio::spawn(relay(daemon_read, client_write, Way::Answer, log.clone()));
            }
        });
        Self {
            socket,
            seen,
            accept,
            _dir: folder.to_path_buf(),
        }
    }

    /// The socket clients connect to.
    pub fn socket(&self) -> &Path {
        &self.socket
    }

    /// Every frame seen so far, in the order they were completed.
    pub fn frames(&self) -> Vec<Seen> {
        self.seen
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl Drop for FrameTap {
    fn drop(&mut self) {
        self.accept.abort();
    }
}

/// Copies bytes from `from` to `to` and records each complete frame.
async fn relay(
    mut from: tokio::net::unix::OwnedReadHalf,
    mut to: tokio::net::unix::OwnedWriteHalf,
    way: Way,
    log: Log,
) {
    let mut pending: Vec<u8> = Vec::new();
    let mut chunk = vec![0_u8; 16 * 1024];
    while let Ok(read) = from.read(&mut chunk).await {
        if read == 0 {
            break;
        }
        // Record before forwarding, so a frame is in the log by the time
        // its other side can react to it.
        pending.extend_from_slice(&chunk[..read]);
        while pending.len() >= 4 {
            let len = u32::from_be_bytes([pending[0], pending[1], pending[2], pending[3]]) as usize;
            if pending.len() < 4 + len {
                break;
            }
            let payload = pending[4..4 + len].to_vec();
            pending.drain(..4 + len);
            log.lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(Seen { way, payload });
        }
        if to.write_all(&chunk[..read]).await.is_err() {
            break;
        }
    }
    let _ = to.shutdown().await;
}
