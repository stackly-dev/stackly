//! The daemon side listener: private bind, what it refuses to replace,
//! the socket watch helpers, and the peer uid check (AC-2, AC-11).

use std::fs;
use std::io;
use std::os::unix::fs::{FileTypeExt, symlink};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use stackly_ipc::{Accepted, BindState, IpcError, Listener, PeerCred, PeerCredSource};
use stackly_protocol::codes::ipc;
use stackly_protocol::json::DecodedFrame;
use stackly_testkit::evidence::{Kind, evidence};
use tokio::net::UnixStream;

use crate::common::{
    Site, accept_authorized, mode_of, raw_connect, read_raw_frame, write_raw_frame,
};

const HANDSHAKE: &[u8] = br#"{"requestId":"r-1","call":{"type":"daemon.handshake","content":{"protocol":{"major":1,"minor":0},"client":{"kind":"cli","version":"0.1.0","pid":1},"contractDigest":"0000000000000000000000000000000000000000000000000000000000000000"}}}"#;

/// A credential source that never reads anything.
struct Unreadable;

impl PeerCredSource for Unreadable {
    fn peer_cred(&self, _stream: &UnixStream) -> io::Result<PeerCred> {
        Err(io::Error::other("credentials are hidden in this test"))
    }
}

#[tokio::test]
async fn bind_publishes_a_0600_socket_and_leaves_no_temporary_name() {
    let site = Site::new();

    let listener = site.listen();

    let meta = fs::symlink_metadata(&site.socket).unwrap();
    assert!(meta.file_type().is_socket());
    assert_eq!(mode_of(&site.socket), 0o600);
    assert!(!site.dir.join("stacklyd.sock.tmp").exists());
    assert_eq!(listener.check_bound(), BindState::Bound);
    assert_eq!(listener.path(), site.socket);
    assert_eq!(listener.expected_uid(), site.uid);
}

#[tokio::test]
async fn the_final_path_never_shows_another_mode_while_binding() {
    let site = Site::new();
    let stop = Arc::new(AtomicBool::new(false));
    let watcher = {
        let stop = Arc::clone(&stop);
        let socket = site.socket.clone();
        std::thread::spawn(move || {
            let mut seen = Vec::new();
            while !stop.load(Ordering::Relaxed) {
                if let Ok(meta) = fs::symlink_metadata(&socket) {
                    use std::os::unix::fs::MetadataExt;
                    seen.push(meta.mode() & 0o7777);
                }
            }
            seen
        })
    };

    let mut listener = site.listen();
    for _ in 0..200 {
        listener.rebind().unwrap();
    }
    stop.store(true, Ordering::Relaxed);
    let seen = watcher.join().unwrap();

    assert!(!seen.is_empty(), "the watcher saw the socket at least once");
    assert!(
        seen.iter().all(|mode| *mode == 0o600),
        "modes seen: {seen:?}"
    );
}

#[tokio::test]
async fn a_file_folder_or_symlink_at_the_socket_path_is_left_alone() {
    let site = Site::new();
    let real = site.dir.join("real.sock");
    let _real = Listener::bind(&real, site.uid).unwrap();

    for kind in ["file", "directory", "symlink"] {
        for target in [site.socket.clone(), site.dir.join("stacklyd.sock.tmp")] {
            match kind {
                "file" => fs::write(&target, b"keep me").unwrap(),
                "directory" => fs::create_dir(&target).unwrap(),
                _ => symlink(&real, &target).unwrap(),
            }

            let error = Listener::bind(&site.socket, site.uid).unwrap_err();

            match &error {
                IpcError::EndpointOccupied { path, kind: found } => {
                    assert_eq!(path, &target);
                    assert_eq!(*found, kind);
                }
                other => panic!("{kind} at {}: {other:?}", target.display()),
            }
            let body = error.to_error_body();
            assert!(body.is(&ipc::ENDPOINT_OCCUPIED));
            assert_eq!(body.detail("kind"), Some(kind));
            assert!(fs::symlink_metadata(&target).is_ok(), "{kind} was removed");
            if kind == "file" {
                assert_eq!(fs::read(&target).unwrap(), b"keep me");
            }
            if kind == "directory" {
                fs::remove_dir(&target).unwrap();
            } else {
                fs::remove_file(&target).unwrap();
            }
        }
    }
    assert!(fs::symlink_metadata(&real).unwrap().file_type().is_socket());
}

#[tokio::test]
async fn stale_sockets_at_the_path_and_the_temporary_name_are_replaced() {
    let site = Site::new();
    drop(std::os::unix::net::UnixListener::bind(&site.socket).unwrap());
    drop(std::os::unix::net::UnixListener::bind(site.dir.join("stacklyd.sock.tmp")).unwrap());

    let listener = site.listen();

    assert!(!site.dir.join("stacklyd.sock.tmp").exists());
    let _client = raw_connect(&site.socket).await;
    let (_connection, peer) = accept_authorized(&listener).await;
    assert_eq!(peer.uid, site.uid);
    assert_eq!(peer.pid, Some(std::process::id()));
}

#[tokio::test]
async fn check_bound_rebind_and_unlink_if_own_follow_the_socket_inode() {
    let site = Site::new();
    let mut listener = site.listen();

    fs::remove_file(&site.socket).unwrap();
    assert_eq!(listener.check_bound(), BindState::Missing);
    assert!(!listener.unlink_if_own().unwrap());

    listener.rebind().unwrap();
    assert_eq!(listener.check_bound(), BindState::Bound);
    let _client = raw_connect(&site.socket).await;
    accept_authorized(&listener).await;

    let other = site.dir.join("other.sock");
    let _other = std::os::unix::net::UnixListener::bind(&other).unwrap();
    fs::rename(&other, &site.socket).unwrap();
    assert_eq!(listener.check_bound(), BindState::Replaced);
    assert!(!listener.unlink_if_own().unwrap());
    assert!(site.socket.exists(), "another socket is never removed");

    listener.rebind().unwrap();
    assert_eq!(listener.check_bound(), BindState::Bound);
    assert!(listener.unlink_if_own().unwrap());
    assert!(fs::symlink_metadata(&site.socket).is_err());
    assert!(!listener.unlink_if_own().unwrap());
}

#[tokio::test]
async fn rebind_after_the_folder_vanished_fails_until_the_folder_is_back() {
    let site = Site::new();
    let mut listener = site.listen();

    fs::remove_dir_all(&site.dir).unwrap();
    assert_eq!(listener.check_bound(), BindState::Missing);
    let error = listener.rebind().unwrap_err();
    assert!(
        matches!(&error, IpcError::Io { op: "bind", source } if source.kind() == io::ErrorKind::NotFound),
        "{error:?}"
    );

    fs::create_dir(&site.dir).unwrap();
    crate::common::set_mode(&site.dir, 0o700);
    listener.rebind().unwrap();
    assert_eq!(listener.check_bound(), BindState::Bound);
    assert_eq!(mode_of(&site.socket), 0o600);
}

#[tokio::test]
async fn a_peer_of_another_user_is_rejected_before_any_frame_is_read() {
    let ev = evidence!(
        covers = ["REQ-047", "AC-047", "BR-012"],
        kind = Kind::Fixture,
        expected = "a listener expecting another uid rejects a real client with ipc.peer_rejected before reading its handshake, reports the peer uid and pid, and closes the connection",
    );
    let site = Site::new();
    let listener = Listener::bind(&site.socket, site.uid + 1).unwrap();
    let mut client = raw_connect(&site.socket).await;
    write_raw_frame(&mut client, HANDSHAKE).await;

    let accepted = listener.accept().await.unwrap();

    match accepted {
        Accepted::Rejected { uid, pid } => {
            assert_eq!(uid, Some(site.uid));
            assert_eq!(pid, Some(std::process::id()));
        }
        other => panic!("expected a rejection, got {other:?}"),
    }
    match read_raw_frame(&mut client).await {
        Some(DecodedFrame::Error(frame)) => {
            assert_eq!(frame.request_id, None, "the handshake was never read");
            assert!(frame.error.is(&ipc::PEER_REJECTED));
            assert_eq!(frame.error.category(), ipc::PEER_REJECTED.category);
        }
        other => panic!("expected ipc.peer_rejected, got {other:?}"),
    }
    assert!(read_raw_frame(&mut client).await.is_none());
    ev.pass();
}

#[tokio::test]
async fn unreadable_peer_credentials_reject_the_peer() {
    let ev = evidence!(
        covers = ["REQ-047", "AC-047"],
        kind = Kind::Fixture,
        expected = "a peer whose credentials cannot be read is rejected with ipc.peer_rejected, failing closed",
    );
    let site = Site::new();
    let listener = site.listen().with_peer_cred_source(Unreadable);
    let mut client = raw_connect(&site.socket).await;

    let accepted = listener.accept().await.unwrap();

    assert!(
        matches!(
            accepted,
            Accepted::Rejected {
                uid: None,
                pid: None
            }
        ),
        "{accepted:?}"
    );
    match read_raw_frame(&mut client).await {
        Some(DecodedFrame::Error(frame)) => assert!(frame.error.is(&ipc::PEER_REJECTED)),
        other => panic!("expected ipc.peer_rejected, got {other:?}"),
    }
    ev.pass();
}

#[tokio::test]
async fn rejected_peers_that_never_read_do_not_hold_the_accept_loop() {
    let site = Site::new();
    let listener = Listener::bind(&site.socket, site.uid + 1).unwrap();
    let mut silent = Vec::new();
    for _ in 0..32 {
        silent.push(raw_connect(&site.socket).await);
    }

    let accepted = tokio::time::timeout(Duration::from_secs(5), async {
        let mut rejected = 0;
        for _ in 0..32 {
            if let Accepted::Rejected { .. } = listener.accept().await.unwrap() {
                rejected += 1;
            }
        }
        rejected
    })
    .await
    .expect("accepting never waits on a peer that does not read");

    assert_eq!(accepted, 32);
    drop(silent);
}
