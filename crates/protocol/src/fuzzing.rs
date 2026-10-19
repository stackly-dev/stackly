//! Fuzz harnesses (spec 0006, *Fuzzing*). The cargo-fuzz targets in `fuzz/`
//! and the stable regression tests in `tests/fuzz_regressions/` call these
//! same functions. Each one panics when an invariant breaks, which is what
//! the fuzzer reports as a crash.

use crate::codes::ipc;
use crate::frame::{FrameDecoder, FrameError, HEADER_LEN};
use crate::json::{DecodedFrame, decode_request, decode_server_frame, handshake_major, request_id};
use crate::limits::{CONTROL_FRAME_MAX, FIELD_ERRORS_MAX};
use crate::message::ServerFrame;

/// Largest chunk the frame harness pushes at once.
const FRAME_CHUNK_MAX: usize = 4096;

/// Target `ipc_frame_decode`: feeds random bytes to a [`FrameDecoder`] with
/// the 1 MiB limit, cut at random chunk boundaries taken from the first
/// byte. No frame is empty or above the limit, every pushed byte is either
/// handed out or still buffered, and the buffer stays within its bound.
///
/// # Panics
///
/// Panics when one of those invariants breaks.
pub fn frame_decode(data: &[u8]) {
    let Some((&seed, stream)) = data.split_first() else {
        return;
    };
    let max = CONTROL_FRAME_MAX as usize;
    let mut decoder = FrameDecoder::new(CONTROL_FRAME_MAX);
    let mut state = u32::from(seed) | 1;
    let (mut offset, mut pushed, mut handed_out) = (0, 0, 0);
    while offset < stream.len() {
        state = state.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        let len = 1 + (state >> 16) as usize % FRAME_CHUNK_MAX;
        let end = stream.len().min(offset + len);
        let chunk = &stream[offset..end];
        offset = end;
        let result = decoder.push(chunk);
        while let Some(frame) = decoder.next_frame() {
            assert!(
                !frame.is_empty() && frame.len() <= max,
                "frame of {} bytes",
                frame.len()
            );
            handed_out += HEADER_LEN + frame.len();
        }
        match result {
            Ok(()) => {
                pushed += chunk.len();
                assert_eq!(
                    handed_out + decoder.buffered(),
                    pushed,
                    "bytes went missing"
                );
                assert!(decoder.capacity() <= max + HEADER_LEN + FRAME_CHUNK_MAX);
            }
            Err(FrameError::TooLarge { len, .. }) => {
                assert!(len > CONTROL_FRAME_MAX.into());
                return;
            }
            Err(_) => return,
        }
    }
}

/// Target `ipc_request_decode`: feeds random bytes to
/// [`decode_request`] and the lenient readers. Nothing panics, every
/// accepted request encodes and decodes again to the same value, and every
/// rejection is a catalog error whose frame fits in a control frame.
///
/// # Panics
///
/// Panics when an accepted request does not survive the round trip, or a
/// rejection uses a code outside the catalog, carries more than
/// [`FIELD_ERRORS_MAX`] fields, or encodes above [`CONTROL_FRAME_MAX`].
pub fn request_decode(data: &[u8]) {
    let _ = request_id(data);
    let _ = handshake_major(data);
    match decode_request(data) {
        Ok(request) => {
            let encoded = serde_json::to_vec(&request).expect("a decoded request always encodes");
            let again = decode_request(&encoded).expect("an encoded request decodes again");
            assert_eq!(again, request);
        }
        Err(rejection) => {
            assert!(
                rejection.error.spec().is_some(),
                "every rejection uses a catalog code"
            );
            assert!(rejection.error.fields().len() <= FIELD_ERRORS_MAX);
            let frame = serde_json::to_vec(&ServerFrame::Error(rejection.into_frame()))
                .expect("an error frame always encodes");
            assert!(
                frame.len() <= CONTROL_FRAME_MAX as usize,
                "an error frame of {} bytes",
                frame.len()
            );
        }
    }
}

/// Target `ipc_server_frame_decode`: feeds random bytes to
/// [`decode_server_frame`], the reader of every Rust client. Nothing
/// panics, every refused frame is `ipc.protocol_violation`, and every
/// decoded response, error, or event encodes and decodes again to the same
/// value.
///
/// # Panics
///
/// Panics when one of those invariants breaks.
pub fn server_frame_decode(data: &[u8]) {
    let frame = match decode_server_frame(data) {
        Ok(DecodedFrame::Response(response)) => ServerFrame::Response(response),
        Ok(DecodedFrame::Error(error)) => ServerFrame::Error(error),
        Ok(DecodedFrame::Event(event)) => ServerFrame::Event(event),
        Ok(DecodedFrame::SkippedEvent { .. }) => return,
        Err(error) => {
            assert!(
                error.is(&ipc::PROTOCOL_VIOLATION),
                "a refused daemon frame is {}",
                error.code()
            );
            return;
        }
    };
    let encoded = serde_json::to_vec(&frame).expect("a decoded daemon frame always encodes");
    let again = decode_server_frame(&encoded).expect("an encoded daemon frame decodes again");
    let expected = match frame {
        ServerFrame::Response(response) => DecodedFrame::Response(response),
        ServerFrame::Error(error) => DecodedFrame::Error(error),
        ServerFrame::Event(event) => DecodedFrame::Event(event),
    };
    assert_eq!(again, expected);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn harnesses_accept_empty_and_tiny_inputs() {
        frame_decode(&[]);
        frame_decode(&[3]);
        request_decode(&[]);
        request_decode(b"{}");
        server_frame_decode(&[]);
        server_frame_decode(b"{}");
    }

    #[test]
    fn the_frame_harness_handles_valid_and_refused_streams() {
        let mut stream = vec![7];
        stream.extend(crate::frame::encode_frame(b"{}", 16).unwrap());
        stream.extend([0, 0, 0, 0]);
        frame_decode(&stream);
        frame_decode(&[1, 0xff, 0xff, 0xff, 0xff]);
    }

    /// A small xorshift generator, so the corpus is the same on every run.
    fn corpus(seed: u64, count: usize, max_len: usize) -> Vec<Vec<u8>> {
        let mut state = seed;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        (0..count)
            .map(|_| {
                let len = (next() % max_len as u64) as usize;
                (0..len).map(|_| next() as u8).collect()
            })
            .collect()
    }

    #[test]
    fn every_harness_holds_on_a_fixed_random_corpus() {
        for input in corpus(0x5eed, 2000, 256) {
            frame_decode(&input);
            request_decode(&input);
            server_frame_decode(&input);
        }
    }

    #[test]
    fn the_server_frame_harness_holds_on_byte_flips_of_valid_frames() {
        let frames: [&[u8]; 3] = [
            br#"{"type":"event","content":{"subscriptionId":2,"sequence":9,"at":"9999-12-30T22:00:00.999Z","resource":{"kind":"instance","id":"a"},"revision":3,"kind":{"type":"events.gap","content":{"reason":"overflow"}}}}"#,
            br#"{"type":"error","content":{"requestId":"r-1","error":{"code":"request.invalid_params","category":"invalid_input","message":"m","retryable":false,"fields":[{"path":"kinds[0]","code":"invalid_format","message":"x"}],"details":[{"key":"omitted","value":"2"}]}}}"#,
            br#"{"type":"response","content":{"requestId":"r-1","reply":{"type":"events.subscribe","content":{"subscriptionId":1,"head":{"serverInstanceId":"0192f3a0-7c1e-7b2a-9d4e-5f6a7b8c9d0e","sequence":0}}}}}"#,
        ];
        for valid in frames {
            server_frame_decode(valid);
            for (index, flip) in corpus(0xf7a3, valid.len() * 4, 2).into_iter().enumerate() {
                let mut input = valid.to_vec();
                let at = index % input.len();
                input[at] ^= flip.first().copied().unwrap_or(1).max(1);
                server_frame_decode(&input);
            }
        }
    }

    #[test]
    fn the_request_harness_bounds_the_error_frame_of_a_long_bad_kinds_list() {
        let kinds = vec![r#""""#; 20_000].join(",");
        let payload = format!(
            r#"{{"requestId":"a","call":{{"type":"events.subscribe","content":{{"kinds":[{kinds}]}}}}}}"#
        );
        request_decode(payload.as_bytes());
    }

    #[test]
    fn the_request_harness_holds_on_byte_flips_of_a_valid_request() {
        let valid = br#"{"requestId":"r-1","idempotencyKey":"k","call":{"type":"events.subscribe","content":{"since":{"serverInstanceId":"0192f3a0-7c1e-7b2a-9d4e-5f6a7b8c9d0e","sequence":4},"kinds":["daemon."],"resource":{"kind":"instance","id":"a"}}}}"#;
        request_decode(valid);
        for (index, flip) in corpus(0xf11b, valid.len() * 4, 2).into_iter().enumerate() {
            let mut input = valid.to_vec();
            let at = index % input.len();
            input[at] ^= flip.first().copied().unwrap_or(1).max(1);
            request_decode(&input);
        }
    }

    #[test]
    fn the_request_harness_round_trips_a_valid_request() {
        request_decode(br#"{"requestId":"a","call":{"type":"daemon.get_info"}}"#);
    }
}
