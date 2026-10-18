//! The client's endpoint checks against real sockets: the path checks
//! before connecting, the peer check after, and not one byte written to an
//! endpoint that fails (AC-3).

use std::fs;
use std::io;
use std::os::unix::fs::symlink;
use std::path::Path;

use stackly_ipc::{ConnectPolicy, IpcError, connect_verified, verify_path};
use stackly_protocol::codes::{UntrustedReason, ipc};
use stackly_testkit::evidence::{Kind, evidence};
use tokio::io::AsyncReadExt;

use crate::common::{Site, accept_authorized, set_mode};

/// Asserts that `error` is `ipc.endpoint_untrusted` for `reason` at
/// `path`.
fn assert_untrusted(error: &IpcError, reason: UntrustedReason, path: &Path) {
    match error {
        IpcError::EndpointUntrusted {
            path: Some(found),
            reason: found_reason,
        } => {
            assert_eq!(*found_reason, reason, "{error:?}");
            assert_eq!(found, path);
        }
        other => panic!("expected {}, got {other:?}", reason.as_str()),
    }
    let body = error.to_error_body();
    assert!(body.is(&ipc::ENDPOINT_UNTRUSTED));
    assert_eq!(body.detail("reason"), Some(reason.as_str()));
    assert_eq!(body.detail("path"), Some(path.to_string_lossy().as_ref()));
}

#[tokio::test]
async fn a_private_socket_in_a_private_folder_passes_and_connects() {
    let site = Site::new();
    let listener = site.listen();

    verify_path(&site.socket, site.uid).unwrap();
    let mut client = connect_verified(&site.socket, ConnectPolicy::same_user(site.uid))
        .await
        .unwrap();
    client.write_frame(b"{}").await.unwrap();

    let (mut server, _) = accept_authorized(&listener).await;
    assert_eq!(
        server.read_frame(None).await.unwrap().as_deref(),
        Some(&b"{}"[..])
    );
}

#[tokio::test]
async fn fake_endpoints_are_refused_with_the_failed_check_as_reason() {
    let ev = evidence!(
        covers = ["AC-070", "REQ-047"],
        kind = Kind::Fixture,
        expected = "verify_path refuses a socket open to group or others, a socket in an open folder, a symlink to the real socket, a regular file, and a socket owned by another uid, each with its fixed reason, and connect_verified writes nothing",
    );
    let site = Site::new();
    let _listener = site.listen();

    set_mode(&site.socket, 0o666);
    let error = verify_path(&site.socket, site.uid).unwrap_err();
    assert_untrusted(&error, UntrustedReason::GroupOrOtherAccess, &site.socket);
    set_mode(&site.socket, 0o600);

    let error = verify_path(&site.socket, site.uid + 1).unwrap_err();
    assert_untrusted(&error, UntrustedReason::WrongOwner, &site.socket);

    set_mode(&site.dir, 0o755);
    let error = connect_verified(&site.socket, ConnectPolicy::same_user(site.uid))
        .await
        .unwrap_err();
    assert_untrusted(
        &error,
        UntrustedReason::ParentGroupOrOtherAccess,
        &site.socket,
    );
    set_mode(&site.dir, 0o700);

    let link = site.dir.join("link.sock");
    symlink(&site.socket, &link).unwrap();
    let error = verify_path(&link, site.uid).unwrap_err();
    assert_untrusted(&error, UntrustedReason::Symlink, &link);

    let file = site.dir.join("file.sock");
    fs::write(&file, b"").unwrap();
    set_mode(&file, 0o600);
    let error = verify_path(&file, site.uid).unwrap_err();
    assert_untrusted(&error, UntrustedReason::NotASocket, &file);

    let folder_link = site.dir.join("folder");
    let real_folder = site.dir.join("real");
    fs::create_dir(&real_folder).unwrap();
    set_mode(&real_folder, 0o700);
    drop(std::os::unix::net::UnixListener::bind(real_folder.join("s.sock")).unwrap());
    set_mode(&real_folder.join("s.sock"), 0o600);
    symlink(&real_folder, &folder_link).unwrap();
    let through_link = folder_link.join("s.sock");
    let error = verify_path(&through_link, site.uid).unwrap_err();
    assert_untrusted(&error, UntrustedReason::Symlink, &through_link);
    ev.pass();
}

#[tokio::test]
async fn a_server_of_another_user_is_refused_without_a_byte_written() {
    let ev = evidence!(
        covers = ["AC-070", "REQ-047"],
        kind = Kind::Fixture,
        expected = "a connect policy expecting another server uid makes verify_peer refuse a real listener with peer_uid_mismatch, and the listener reads zero bytes before the connection ends",
    );
    let site = Site::new();
    // A plain listener with no peer check of its own, so it counts every
    // byte even after the client already hung up.
    let listener = tokio::net::UnixListener::bind(&site.socket).unwrap();
    set_mode(&site.socket, 0o600);

    let error = connect_verified(&site.socket, ConnectPolicy::new(site.uid, site.uid + 1))
        .await
        .unwrap_err();

    assert_untrusted(&error, UntrustedReason::PeerUidMismatch, &site.socket);
    let (mut server, _) = listener.accept().await.unwrap();
    let mut received = Vec::new();
    server.read_to_end(&mut received).await.unwrap();
    assert!(received.is_empty(), "{} bytes were written", received.len());
    ev.pass();
}

#[tokio::test]
async fn nothing_at_the_path_is_endpoint_missing() {
    let site = Site::new();

    let error = connect_verified(&site.socket, ConnectPolicy::same_user(site.uid))
        .await
        .unwrap_err();

    assert!(
        matches!(&error, IpcError::EndpointMissing { path } if *path == site.socket),
        "{error:?}"
    );
    let body = error.to_error_body();
    assert!(body.is(&ipc::ENDPOINT_MISSING));
    assert!(body.retryable());
}

#[tokio::test]
async fn a_stale_socket_is_connection_refused() {
    let site = Site::new();
    drop(std::os::unix::net::UnixListener::bind(&site.socket).unwrap());
    set_mode(&site.socket, 0o600);

    let error = connect_verified(&site.socket, ConnectPolicy::same_user(site.uid))
        .await
        .unwrap_err();

    assert!(
        matches!(&error, IpcError::ConnectionRefused { path } if *path == site.socket),
        "{error:?}"
    );
    assert!(error.to_error_body().is(&ipc::CONNECTION_REFUSED));
}

/// Gives a folder its mode back when the test ends, pass or fail, so the
/// test root can be removed.
struct RestoreMode<'a>(&'a Path);

impl Drop for RestoreMode<'_> {
    fn drop(&mut self) {
        set_mode(self.0, 0o700);
    }
}

#[tokio::test]
async fn a_folder_the_client_cannot_search_is_an_io_error() {
    let site = Site::new();
    let _listener = site.listen();
    set_mode(&site.dir, 0o000);
    let _restore = RestoreMode(&site.dir);

    let error = connect_verified(&site.socket, ConnectPolicy::same_user(site.uid))
        .await
        .unwrap_err();

    assert!(
        matches!(&error, IpcError::Io { op: "stat", source } if source.kind() == io::ErrorKind::PermissionDenied),
        "{error:?}"
    );
    let body = error.to_error_body();
    assert!(body.is(&ipc::IO_ERROR));
    assert_eq!(body.detail("op"), Some("stat"));
    assert_eq!(body.detail("kind"), Some("permission_denied"));
}
