//! The frame codec: a `u32` big endian length, then that many payload
//! bytes (VAL-017, NFR-004). Pure byte work, no IO; `stackly-ipc` feeds it.
//!
//! The decoder checks every header as soon as it is complete, before any
//! payload buffer for that frame exists. A transport that reads at most
//! `READ_CHUNK` bytes at a time and drains every complete frame before the
//! next [`FrameDecoder::push`] keeps the buffer at or below
//! `max_len + 4 + READ_CHUNK` bytes.

use crate::codes::ipc;
use crate::error::ErrorBody;

/// Bytes in a frame header.
pub const HEADER_LEN: usize = 4;

/// A frame header announced a length the codec refuses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum FrameError {
    /// The header announced a length of 0.
    #[error("frame header announces a length of 0")]
    Empty,
    /// The header announced, or the payload has, more bytes than the limit.
    #[error("frame of {len} bytes is above the limit of {max} bytes")]
    TooLarge {
        /// The announced or actual length.
        len: u64,
        /// The limit.
        max: u32,
    },
}

impl FrameError {
    /// The wire error for this failure: `ipc.frame_empty` or
    /// `ipc.frame_too_large`.
    #[must_use]
    pub fn to_error_body(&self) -> ErrorBody {
        match *self {
            Self::Empty => {
                ErrorBody::new(&ipc::FRAME_EMPTY, "frame header announces a length of 0")
            }
            Self::TooLarge { len, max } => ErrorBody::new(
                &ipc::FRAME_TOO_LARGE,
                "frame is larger than the frame limit",
            )
            .with_detail("length", len.to_string())
            .with_detail("max", max.to_string()),
        }
    }
}

/// Splits a byte stream into frame payloads.
#[derive(Debug)]
pub struct FrameDecoder {
    max_len: u32,
    /// Bytes received and not yet handed out, from `start` on.
    buf: Vec<u8>,
    /// Offset in `buf` of the first byte not yet handed out.
    start: usize,
    /// Offset in `buf` of the next header to check. Every header before it
    /// passed; a frame is only handed out when its header sits before it.
    checked: usize,
    /// The first refused header, after which nothing more is accepted.
    failed: Option<FrameError>,
}

impl FrameDecoder {
    /// A decoder that refuses frames longer than `max_len` bytes.
    #[must_use]
    pub const fn new(max_len: u32) -> Self {
        Self {
            max_len,
            buf: Vec::new(),
            start: 0,
            checked: 0,
            failed: None,
        }
    }

    /// Adds one chunk of the stream.
    ///
    /// # Errors
    ///
    /// Returns [`FrameError`] as soon as a complete header announces a length
    /// of 0 or above the limit, and again on every later call. Frames before
    /// the refused header can still be taken with [`Self::next_frame`].
    pub fn push(&mut self, chunk: &[u8]) -> Result<(), FrameError> {
        if let Some(error) = self.failed {
            return Err(error);
        }
        if self.start > 0 {
            self.buf.drain(..self.start);
            self.checked -= self.start;
            self.start = 0;
        }
        self.buf.reserve_exact(chunk.len());
        self.buf.extend_from_slice(chunk);
        while self.checked + HEADER_LEN <= self.buf.len() {
            let len = read_len(&self.buf[self.checked..]);
            if let Err(error) = check_len(u64::from(len), self.max_len) {
                self.failed = Some(error);
                return Err(error);
            }
            self.checked += HEADER_LEN + len as usize;
        }
        Ok(())
    }

    /// Takes the next complete frame payload, or `None` when no frame is
    /// complete yet.
    pub fn next_frame(&mut self) -> Option<Vec<u8>> {
        if self.start >= self.checked {
            return None;
        }
        let available = &self.buf[self.start..];
        if available.len() < HEADER_LEN {
            return None;
        }
        let end = HEADER_LEN + read_len(available) as usize;
        let payload = available.get(HEADER_LEN..end)?.to_vec();
        self.start += end;
        Some(payload)
    }

    /// Bytes received and not yet handed out. Above 0 between frames means
    /// a frame has started, so its stall timer runs.
    #[must_use]
    pub fn buffered(&self) -> usize {
        self.buf.len() - self.start
    }

    /// The buffer's current capacity, for checks on the memory bound.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.buf.capacity()
    }

    /// The length limit.
    #[must_use]
    pub const fn max_len(&self) -> u32 {
        self.max_len
    }
}

/// Prefixes `payload` with its length.
///
/// # Errors
///
/// Returns [`FrameError::Empty`] for an empty payload and
/// [`FrameError::TooLarge`] for one longer than `max_len` bytes.
pub fn encode_frame(payload: &[u8], max_len: u32) -> Result<Vec<u8>, FrameError> {
    let len = u64::try_from(payload.len()).unwrap_or(u64::MAX);
    check_len(len, max_len)?;
    let header = u32::try_from(len).expect("a length within a u32 limit fits in a u32");
    let mut frame = Vec::with_capacity(HEADER_LEN + payload.len());
    frame.extend_from_slice(&header.to_be_bytes());
    frame.extend_from_slice(payload);
    Ok(frame)
}

fn check_len(len: u64, max: u32) -> Result<(), FrameError> {
    if len == 0 {
        Err(FrameError::Empty)
    } else if len > u64::from(max) {
        Err(FrameError::TooLarge { len, max })
    } else {
        Ok(())
    }
}

/// The big endian length at the front of `bytes`, which holds at least
/// [`HEADER_LEN`] bytes.
fn read_len(bytes: &[u8]) -> u32 {
    u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frames(decoder: &mut FrameDecoder) -> Vec<Vec<u8>> {
        std::iter::from_fn(|| decoder.next_frame()).collect()
    }

    #[test]
    fn two_frames_in_one_chunk_come_out_in_order() {
        let mut stream = encode_frame(b"{}", 16).unwrap();
        stream.extend(encode_frame(b"[1]", 16).unwrap());
        let mut decoder = FrameDecoder::new(16);
        decoder.push(&stream).unwrap();
        assert_eq!(frames(&mut decoder), [b"{}".to_vec(), b"[1]".to_vec()]);
        assert_eq!(decoder.buffered(), 0);
    }

    #[test]
    fn a_frame_split_at_every_byte_reads_the_same() {
        let stream = encode_frame(b"hello", 16).unwrap();
        for split in 0..=stream.len() {
            let mut decoder = FrameDecoder::new(16);
            decoder.push(&stream[..split]).unwrap();
            let mut out = frames(&mut decoder);
            decoder.push(&stream[split..]).unwrap();
            out.extend(frames(&mut decoder));
            assert_eq!(out, [b"hello".to_vec()], "split at {split}");
        }
    }

    #[test]
    fn a_zero_length_header_fails_as_soon_as_it_is_complete() {
        let mut decoder = FrameDecoder::new(16);
        decoder.push(&[0, 0, 0]).unwrap();
        assert_eq!(decoder.push(&[0]), Err(FrameError::Empty));
        assert_eq!(decoder.push(&[1]), Err(FrameError::Empty));
    }

    #[test]
    fn a_header_above_the_limit_fails_before_the_payload_arrives() {
        let mut decoder = FrameDecoder::new(1_048_576);
        let header = 1_048_577_u32.to_be_bytes();
        assert_eq!(
            decoder.push(&header),
            Err(FrameError::TooLarge {
                len: 1_048_577,
                max: 1_048_576
            })
        );
        assert!(decoder.capacity() <= HEADER_LEN);
    }

    #[test]
    fn frames_before_a_refused_header_are_still_handed_out() {
        let mut stream = encode_frame(b"ok", 16).unwrap();
        stream.extend([0, 0, 0, 0]);
        let mut decoder = FrameDecoder::new(16);
        assert_eq!(decoder.push(&stream), Err(FrameError::Empty));
        assert_eq!(frames(&mut decoder), [b"ok".to_vec()]);
    }

    #[test]
    fn a_frame_of_exactly_the_limit_is_accepted_and_the_buffer_stays_bounded() {
        const MAX: u32 = 1_048_576;
        const CHUNK: usize = 65_536;
        let payload = vec![b'x'; MAX as usize];
        let stream = encode_frame(&payload, MAX).unwrap();
        let mut decoder = FrameDecoder::new(MAX);
        let mut out = Vec::new();
        for chunk in stream.chunks(CHUNK) {
            decoder.push(chunk).unwrap();
            out.extend(frames(&mut decoder));
            assert!(decoder.capacity() <= MAX as usize + HEADER_LEN + CHUNK);
        }
        assert_eq!(out, [payload]);
        assert_eq!(
            encode_frame(&vec![0; MAX as usize + 1], MAX).unwrap_err(),
            FrameError::TooLarge {
                len: u64::from(MAX) + 1,
                max: MAX
            }
        );
    }

    #[test]
    fn encoding_an_empty_payload_fails() {
        assert_eq!(encode_frame(b"", 16), Err(FrameError::Empty));
    }

    #[test]
    fn frame_errors_map_to_catalog_codes() {
        assert_eq!(FrameError::Empty.to_error_body().code(), "ipc.frame_empty");
        let body = FrameError::TooLarge { len: 9, max: 8 }.to_error_body();
        assert_eq!(body.code(), "ipc.frame_too_large");
        assert_eq!(body.detail("length"), Some("9"));
        assert_eq!(body.detail("max"), Some("8"));
    }
}
