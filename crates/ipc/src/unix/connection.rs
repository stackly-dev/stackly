//! Frames over one socket: a reader that feeds the codec in bounded chunks
//! and enforces the stall timer, and a writer that prefixes each payload
//! with its length (VAL-017, NFR-004).

use std::io;

use stackly_protocol::frame::{FrameDecoder, FrameError, encode_frame};
use stackly_protocol::limits::CONTROL_FRAME_MAX;
use stackly_protocol::{ErrorBody, ErrorFrame, ServerFrame};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};
use tokio::time::{Instant, timeout, timeout_at};

use crate::{FRAME_STALL_TIMEOUT, IpcError, READ_CHUNK, REJECT_WRITE_TIMEOUT};

/// One connection that reads and writes whole frames of at most
/// [`CONTROL_FRAME_MAX`] bytes.
///
/// The listener hands one out only for a peer that passed the uid check,
/// and [`connect_verified`](crate::connect_verified) only for an endpoint
/// that passed both trust checks. Split it with [`Self::into_split`] to
/// read and write from separate tasks.
#[derive(Debug)]
pub struct FramedConnection {
    reader: FrameReader,
    writer: FrameWriter,
}

impl FramedConnection {
    pub(crate) fn new(stream: UnixStream) -> Self {
        let (read, write) = stream.into_split();
        Self {
            reader: FrameReader::new(read),
            writer: FrameWriter { half: write },
        }
    }

    /// Reads the next frame payload. See [`FrameReader::read_frame`].
    ///
    /// # Errors
    ///
    /// The errors of [`FrameReader::read_frame`].
    pub async fn read_frame(
        &mut self,
        deadline: Option<Instant>,
    ) -> Result<Option<Vec<u8>>, IpcError> {
        self.reader.read_frame(deadline).await
    }

    /// Writes one frame. See [`FrameWriter::write_frame`].
    ///
    /// # Errors
    ///
    /// The errors of [`FrameWriter::write_frame`].
    pub async fn write_frame(&mut self, payload: &[u8]) -> Result<(), IpcError> {
        self.writer.write_frame(payload).await
    }

    /// Writes one daemon frame. See [`FrameWriter::write_server_frame`].
    ///
    /// # Errors
    ///
    /// The errors of [`FrameWriter::write_server_frame`].
    pub async fn write_server_frame(&mut self, frame: &ServerFrame) -> Result<(), IpcError> {
        self.writer.write_server_frame(frame).await
    }

    /// Sends one error frame and closes the connection, the way the daemon
    /// answers a framing error, a refused handshake, or a refused peer.
    ///
    /// The write gets at most [`REJECT_WRITE_TIMEOUT`], so a peer that does
    /// not read never holds the caller. Whatever happens, the connection is
    /// closed when this returns.
    ///
    /// # Errors
    ///
    /// Returns [`IpcError::Io`] with op `write` when writing failed or did
    /// not finish in time (kind `timed_out`); the frame may not have
    /// arrived.
    pub async fn close_with_error(
        self,
        request_id: Option<String>,
        error: ErrorBody,
    ) -> Result<(), IpcError> {
        let mut writer = self.writer;
        let frame = ServerFrame::Error(ErrorFrame { request_id, error });
        let sent = timeout(REJECT_WRITE_TIMEOUT, async {
            writer.write_server_frame(&frame).await?;
            writer.shutdown().await
        })
        .await;
        match sent {
            Ok(result) => result,
            Err(_) => Err(IpcError::Io {
                op: "write",
                source: io::Error::from(io::ErrorKind::TimedOut),
            }),
        }
    }

    /// Splits the connection so one task reads while another writes.
    #[must_use]
    pub fn into_split(self) -> (FrameReader, FrameWriter) {
        (self.reader, self.writer)
    }
}

/// The reading half of a [`FramedConnection`].
#[derive(Debug)]
pub struct FrameReader {
    half: OwnedReadHalf,
    decoder: FrameDecoder,
    /// The read buffer, [`READ_CHUNK`] bytes once the first read happens.
    chunk: Vec<u8>,
    /// When the first byte of the incomplete frame in the decoder arrived.
    frame_started: Option<Instant>,
    /// When the last chunk arrived.
    last_chunk_at: Option<Instant>,
    /// A refused header met while earlier frames were still to hand out.
    failed: Option<FrameError>,
}

impl FrameReader {
    fn new(half: OwnedReadHalf) -> Self {
        Self {
            half,
            decoder: FrameDecoder::new(CONTROL_FRAME_MAX),
            chunk: Vec::new(),
            frame_started: None,
            last_chunk_at: None,
            failed: None,
        }
    }

    /// Reads the next frame payload, or `None` for a clean end of stream
    /// between frames.
    ///
    /// Reads at most [`READ_CHUNK`] bytes at a time and only when no
    /// complete frame is buffered, so the buffer stays within
    /// `CONTROL_FRAME_MAX + 4 + READ_CHUNK` bytes. `deadline`, when given,
    /// holds until the frame is complete; the daemon passes its handshake
    /// deadline. Once the first byte of a frame arrived, the frame must be
    /// complete within [`FRAME_STALL_TIMEOUT`] whether a deadline was given
    /// or not; the earlier of the two applies.
    ///
    /// # Errors
    ///
    /// - [`IpcError::Frame`] when a header announces a length of 0 or above
    ///   the limit, as soon as the header is complete; frames before it are
    ///   still handed out first.
    /// - [`IpcError::HandshakeTimeout`] when `deadline` passes first.
    /// - [`IpcError::FrameStalled`] when the stall timer runs out first.
    /// - [`IpcError::ConnectionClosed`] when the stream ends inside a frame.
    /// - [`IpcError::Io`] with op `read` for an OS error.
    pub async fn read_frame(
        &mut self,
        deadline: Option<Instant>,
    ) -> Result<Option<Vec<u8>>, IpcError> {
        loop {
            if let Some(frame) = self.decoder.next_frame() {
                self.frame_started = if self.decoder.buffered() > 0 {
                    self.last_chunk_at
                } else {
                    None
                };
                return Ok(Some(frame));
            }
            if let Some(error) = self.failed {
                return Err(IpcError::Frame(error));
            }
            if self.chunk.is_empty() {
                self.chunk = vec![0; READ_CHUNK];
            }
            let stall_at = self
                .frame_started
                .map(|started| started + FRAME_STALL_TIMEOUT);
            let read = self.half.read(&mut self.chunk);
            let read = match earliest(deadline, stall_at) {
                None => read.await,
                Some((at, stalled)) => match timeout_at(at, read).await {
                    Ok(read) => read,
                    Err(_) if stalled => return Err(IpcError::FrameStalled),
                    Err(_) => return Err(IpcError::HandshakeTimeout),
                },
            };
            let len = read.map_err(|source| IpcError::Io { op: "read", source })?;
            if len == 0 {
                return if self.decoder.buffered() == 0 {
                    Ok(None)
                } else {
                    Err(IpcError::ConnectionClosed)
                };
            }
            let now = Instant::now();
            if self.decoder.buffered() == 0 {
                self.frame_started = Some(now);
            }
            self.last_chunk_at = Some(now);
            if let Err(error) = self.decoder.push(&self.chunk[..len]) {
                self.failed = Some(error);
            }
        }
    }

    /// The capacity of the frame buffer, for checks on the memory bound.
    #[must_use]
    pub fn buffer_capacity(&self) -> usize {
        self.decoder.capacity()
    }
}

/// The earlier of the caller's deadline and the stall deadline, and
/// whether it is the stall one.
fn earliest(deadline: Option<Instant>, stall_at: Option<Instant>) -> Option<(Instant, bool)> {
    match (deadline, stall_at) {
        (Some(deadline), Some(stall_at)) if stall_at < deadline => Some((stall_at, true)),
        (Some(deadline), _) => Some((deadline, false)),
        (None, Some(stall_at)) => Some((stall_at, true)),
        (None, None) => None,
    }
}

/// The writing half of a [`FramedConnection`].
#[derive(Debug)]
pub struct FrameWriter {
    half: OwnedWriteHalf,
}

impl FrameWriter {
    /// Writes `payload` as one frame.
    ///
    /// # Errors
    ///
    /// Returns [`IpcError::Frame`] when the payload is empty or above
    /// [`CONTROL_FRAME_MAX`], before anything is written, and
    /// [`IpcError::Io`] with op `write` for an OS error.
    pub async fn write_frame(&mut self, payload: &[u8]) -> Result<(), IpcError> {
        let frame = encode_frame(payload, CONTROL_FRAME_MAX)?;
        self.half
            .write_all(&frame)
            .await
            .map_err(|source| IpcError::Io {
                op: "write",
                source,
            })
    }

    /// Writes one daemon frame as JSON.
    ///
    /// # Errors
    ///
    /// The errors of [`Self::write_frame`]; a reply above the limit is
    /// [`IpcError::Frame`], which the daemon answers with
    /// `ipc.reply_too_large`.
    pub async fn write_server_frame(&mut self, frame: &ServerFrame) -> Result<(), IpcError> {
        let payload = serde_json::to_vec(frame)
            .expect("a server frame always serializes: every map key in it is a string");
        self.write_frame(&payload).await
    }

    /// Shuts the writing direction down, so the peer reads the end of the
    /// stream after the frames already written.
    ///
    /// # Errors
    ///
    /// Returns [`IpcError::Io`] with op `write` for an OS error.
    pub async fn shutdown(&mut self) -> Result<(), IpcError> {
        self.half.shutdown().await.map_err(|source| IpcError::Io {
            op: "write",
            source,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn the_earlier_deadline_wins_and_says_which_it_is() {
        let now = Instant::now();
        let soon = now + Duration::from_secs(5);
        let late = now + Duration::from_secs(30);
        assert_eq!(earliest(None, None), None);
        assert_eq!(earliest(Some(soon), None), Some((soon, false)));
        assert_eq!(earliest(None, Some(late)), Some((late, true)));
        assert_eq!(earliest(Some(soon), Some(late)), Some((soon, false)));
        assert_eq!(earliest(Some(late), Some(soon)), Some((soon, true)));
    }
}
