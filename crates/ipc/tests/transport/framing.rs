//! Frames on a live socket: limits checked at the header, the memory
//! bound, the end of the stream, and the stall and handshake timers on a
//! paused clock (AC-6).

use std::time::Duration;

use stackly_ipc::{FramedConnection, IpcError, READ_CHUNK};
use stackly_protocol::codes::ipc;
use stackly_protocol::frame::{FrameError, encode_frame};
use stackly_protocol::json::DecodedFrame;
use stackly_protocol::limits::CONTROL_FRAME_MAX;
use stackly_testkit::evidence::{Kind, evidence};
use tokio::io::AsyncWriteExt;
use tokio::net::UnixStream;
use tokio::time::Instant;

use crate::common::{Site, accept_authorized, raw_connect, read_raw_frame};

/// A server side connection and the raw client stream behind it.
async fn pair(site: &Site) -> (FramedConnection, UnixStream) {
    let listener = site.listen();
    let client = raw_connect(&site.socket).await;
    let (server, _) = accept_authorized(&listener).await;
    (server, client)
}

#[tokio::test]
async fn an_empty_frame_is_refused_and_answered_before_the_connection_closes() {
    let ev = evidence!(
        covers = ["VAL-017", "NFR-004"],
        kind = Kind::Fixture,
        expected = "a frame header announcing length 0 is refused as ipc.frame_empty, sent back as an error frame, and the connection is closed",
    );
    let site = Site::new();
    let (mut server, mut client) = pair(&site).await;
    client.write_all(&[0, 0, 0, 0]).await.unwrap();

    let error = server.read_frame(None).await.unwrap_err();

    assert!(
        matches!(error, IpcError::Frame(FrameError::Empty)),
        "{error:?}"
    );
    let body = error.to_error_body();
    assert!(body.is(&ipc::FRAME_EMPTY));
    server.close_with_error(None, body).await.unwrap();
    match read_raw_frame(&mut client).await {
        Some(DecodedFrame::Error(frame)) => {
            assert_eq!(frame.request_id, None);
            assert!(frame.error.is(&ipc::FRAME_EMPTY));
        }
        other => panic!("expected ipc.frame_empty, got {other:?}"),
    }
    assert!(read_raw_frame(&mut client).await.is_none());
    ev.pass();
}

#[tokio::test]
async fn a_header_above_one_mebibyte_is_refused_before_any_payload_buffer() {
    let ev = evidence!(
        covers = ["VAL-017", "NFR-004"],
        kind = Kind::Fixture,
        expected = "a header announcing 1 MiB plus 1 is refused as ipc.frame_too_large as soon as it is read, before a payload buffer of that size exists",
    );
    let site = Site::new();
    let (mut server, mut client) = pair(&site).await;
    client
        .write_all(&(CONTROL_FRAME_MAX + 1).to_be_bytes())
        .await
        .unwrap();

    let error = server.read_frame(None).await.unwrap_err();

    assert!(
        matches!(
            error,
            IpcError::Frame(FrameError::TooLarge {
                len: 1_048_577,
                max: 1_048_576
            })
        ),
        "{error:?}"
    );
    let body = error.to_error_body();
    assert!(body.is(&ipc::FRAME_TOO_LARGE));
    assert_eq!(body.detail("length"), Some("1048577"));
    assert_eq!(body.detail("max"), Some("1048576"));
    let (reader, _) = server.into_split();
    assert!(
        reader.buffer_capacity() < 1024,
        "{}",
        reader.buffer_capacity()
    );
    ev.pass();
}

#[tokio::test]
async fn a_frame_of_exactly_one_mebibyte_is_read_whole_within_the_memory_bound() {
    let site = Site::new();
    let (server, mut client) = pair(&site).await;
    let payload = vec![b'x'; CONTROL_FRAME_MAX as usize];
    let frame = encode_frame(&payload, CONTROL_FRAME_MAX).unwrap();
    let writer = tokio::spawn(async move {
        client.write_all(&frame).await.unwrap();
        client
    });
    let (mut reader, _writer) = server.into_split();

    let read = reader.read_frame(None).await.unwrap().unwrap();

    assert_eq!(read.len(), payload.len());
    assert!(read == payload);
    assert!(reader.buffer_capacity() <= CONTROL_FRAME_MAX as usize + 4 + READ_CHUNK);
    drop(writer.await.unwrap());
    assert_eq!(reader.read_frame(None).await.unwrap(), None);
}

#[tokio::test]
async fn frames_written_one_byte_at_a_time_read_the_same() {
    let site = Site::new();
    let (mut server, mut client) = pair(&site).await;
    let mut stream = encode_frame(b"{}", CONTROL_FRAME_MAX).unwrap();
    stream.extend(encode_frame(b"[1]", CONTROL_FRAME_MAX).unwrap());
    let writer = tokio::spawn(async move {
        for byte in stream {
            client.write_all(&[byte]).await.unwrap();
            tokio::task::yield_now().await;
        }
    });

    let first = server.read_frame(None).await.unwrap();
    let second = server.read_frame(None).await.unwrap();
    writer.await.unwrap();

    assert_eq!(first.as_deref(), Some(&b"{}"[..]));
    assert_eq!(second.as_deref(), Some(&b"[1]"[..]));
    assert_eq!(server.read_frame(None).await.unwrap(), None);
}

#[tokio::test]
async fn the_end_of_the_stream_inside_a_frame_is_connection_closed() {
    let site = Site::new();
    let (mut server, mut client) = pair(&site).await;
    client.write_all(&[0, 0, 0, 10, b'{', b'"']).await.unwrap();
    drop(client);

    let error = server.read_frame(None).await.unwrap_err();

    assert!(matches!(error, IpcError::ConnectionClosed), "{error:?}");
}

#[tokio::test]
async fn a_valid_frame_before_a_refused_header_is_still_handed_out() {
    let site = Site::new();
    let (mut server, mut client) = pair(&site).await;
    let mut stream = encode_frame(b"{}", CONTROL_FRAME_MAX).unwrap();
    stream.extend([0, 0, 0, 0]);
    client.write_all(&stream).await.unwrap();

    assert_eq!(
        server.read_frame(None).await.unwrap().as_deref(),
        Some(&b"{}"[..])
    );
    let error = server.read_frame(None).await.unwrap_err();
    assert!(
        matches!(error, IpcError::Frame(FrameError::Empty)),
        "{error:?}"
    );
}

#[tokio::test(start_paused = true)]
async fn a_started_frame_that_stalls_is_refused_after_30_seconds() {
    let ev = evidence!(
        covers = ["VAL-017", "NFR-004"],
        kind = Kind::Fixture,
        expected = "a truncated header, or a header with part of its payload, followed by silence is refused as ipc.frame_stalled 30 seconds after its first byte on a paused clock",
    );
    let site = Site::new();
    for partial in [&[0, 0][..], &[0, 0, 0, 8, b'{'][..]] {
        let (mut server, mut client) = pair(&site).await;
        client.write_all(partial).await.unwrap();
        let started = Instant::now();

        let error = server.read_frame(None).await.unwrap_err();

        assert!(matches!(error, IpcError::FrameStalled), "{error:?}");
        assert!(error.to_error_body().is(&ipc::FRAME_STALLED));
        let waited = started.elapsed();
        assert!(
            waited >= Duration::from_secs(30) && waited < Duration::from_secs(31),
            "{waited:?}"
        );
        drop(client);
    }
    ev.pass();
}

#[tokio::test(start_paused = true)]
async fn a_silent_connection_between_frames_never_stalls() {
    let site = Site::new();
    let (mut server, mut client) = pair(&site).await;
    client
        .write_all(&encode_frame(b"{}", CONTROL_FRAME_MAX).unwrap())
        .await
        .unwrap();
    assert!(server.read_frame(None).await.unwrap().is_some());

    let waited = tokio::time::timeout(Duration::from_secs(300), server.read_frame(None)).await;

    assert!(
        waited.is_err(),
        "no frame had started, so no timer ran: {waited:?}"
    );
}

#[tokio::test(start_paused = true)]
async fn the_handshake_deadline_holds_until_the_frame_is_complete() {
    let site = Site::new();
    for partial in [&[][..], &[0, 0, 0, 40][..]] {
        let (mut server, mut client) = pair(&site).await;
        client.write_all(partial).await.unwrap();
        let started = Instant::now();

        let error = server
            .read_frame(Some(started + Duration::from_secs(5)))
            .await
            .unwrap_err();

        assert!(matches!(error, IpcError::HandshakeTimeout), "{error:?}");
        assert!(error.to_error_body().is(&ipc::HANDSHAKE_TIMEOUT));
        assert_eq!(started.elapsed(), Duration::from_secs(5));
        drop(client);
    }
}

#[tokio::test]
async fn write_frame_refuses_an_empty_or_oversized_payload_before_writing() {
    let site = Site::new();
    let (mut server, mut client) = pair(&site).await;

    let empty = server.write_frame(&[]).await.unwrap_err();
    let large = server
        .write_frame(&vec![b'x'; CONTROL_FRAME_MAX as usize + 1])
        .await
        .unwrap_err();

    assert!(
        matches!(empty, IpcError::Frame(FrameError::Empty)),
        "{empty:?}"
    );
    assert!(
        matches!(large, IpcError::Frame(FrameError::TooLarge { .. })),
        "{large:?}"
    );
    drop(server);
    assert!(
        read_raw_frame(&mut client).await.is_none(),
        "nothing was written"
    );
}
