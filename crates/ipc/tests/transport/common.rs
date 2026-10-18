//! Shared helpers: a private socket folder per test, the current uid, and
//! raw frame IO for tests that play one side by hand.

use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

use stackly_ipc::{Accepted, FramedConnection, Listener, PeerCred};
use stackly_protocol::json::{DecodedFrame, decode_request, decode_server_frame};
use stackly_protocol::{Reply, Request, Response, ServerFrame};
use stackly_testkit::TestRoot;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

/// One test's socket site: a 0700 folder inside the test's run folder.
pub struct Site {
    /// Kept so the folders live as long as the test.
    _root: TestRoot,
    /// The folder holding the socket, mode 0700.
    pub dir: PathBuf,
    /// The socket path.
    pub socket: PathBuf,
    /// The uid this test runs as.
    pub uid: u32,
}

impl Site {
    /// A fresh site with nothing at the socket path yet.
    pub fn new() -> Self {
        let root = TestRoot::new().expect("a test root can be created");
        let dir = root.run_dir().join("rt");
        fs::create_dir(&dir).expect("the run folder is writable");
        set_mode(&dir, 0o700);
        let uid = fs::metadata(&dir)
            .expect("the folder just created can be read")
            .uid();
        Self {
            socket: dir.join("stacklyd.sock"),
            dir,
            uid,
            _root: root,
        }
    }

    /// A listener for this site's socket that accepts this test's uid.
    pub fn listen(&self) -> Listener {
        Listener::bind(&self.socket, self.uid).expect("the site's socket path is free")
    }
}

/// Sets the permission bits of `path`.
pub fn set_mode(path: &Path, mode: u32) {
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .expect("the test owns the path it changes");
}

/// The permission bits of `path`, without following symlinks.
pub fn mode_of(path: &Path) -> u32 {
    fs::symlink_metadata(path).expect("the path exists").mode() & 0o7777
}

/// Accepts one peer and expects it to be authorized.
pub async fn accept_authorized(listener: &Listener) -> (FramedConnection, PeerCred) {
    match listener.accept().await.expect("accepting works") {
        Accepted::Authorized { connection, peer } => (connection, peer),
        other => panic!("expected an authorized peer, got {other:?}"),
    }
}

/// Connects without any trust check, the way a hostile or hand written
/// client would.
pub async fn raw_connect(path: &Path) -> UnixStream {
    UnixStream::connect(path)
        .await
        .expect("the socket accepts connections")
}

/// Writes one frame by hand.
pub async fn write_raw_frame(stream: &mut UnixStream, payload: &[u8]) {
    let len = u32::try_from(payload.len()).expect("test payloads fit a frame header");
    stream
        .write_all(&len.to_be_bytes())
        .await
        .expect("the peer is connected");
    stream
        .write_all(payload)
        .await
        .expect("the peer is connected");
}

/// Reads one daemon frame by hand, or `None` at the end of the stream.
pub async fn read_raw_frame(stream: &mut UnixStream) -> Option<DecodedFrame> {
    let mut header = [0; 4];
    match stream.read_exact(&mut header).await {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => return None,
        Err(error) => panic!("reading a frame header failed: {error}"),
    }
    let mut payload = vec![0; u32::from_be_bytes(header) as usize];
    stream
        .read_exact(&mut payload)
        .await
        .expect("the daemon sends whole frames");
    Some(decode_server_frame(&payload).expect("the daemon frame decodes"))
}

/// Reads one request on the daemon side.
pub async fn read_request(connection: &mut FramedConnection) -> Request {
    let payload = connection
        .read_frame(None)
        .await
        .expect("the client sends whole frames")
        .expect("the client sends a request before closing");
    decode_request(&payload).expect("the client sends valid requests")
}

/// Answers one request on the daemon side.
pub async fn respond(connection: &mut FramedConnection, request_id: &str, reply: Reply) {
    connection
        .write_server_frame(&ServerFrame::Response(Response {
            request_id: request_id.to_owned(),
            reply,
        }))
        .await
        .expect("the client is connected");
}
