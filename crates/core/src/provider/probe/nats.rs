//! `nats-readiness` (spec 0023, *Algoritma satu percobaan*): one readiness
//! attempt over the NATS client protocol, as a state machine without IO.
//!
//! The attempt reads the server's `INFO` line and checks the server name,
//! TLS, auth, lame duck, and JetStream mode, then sends `CONNECT` and
//! `PING` and waits for `PONG`. When the instance asks for JetStream, it
//! also sends one `$JS.API.INFO` request to its own inbox and checks the
//! account info reply. The caller (the driver of spec 0010) connects,
//! feeds every byte it reads to [`Exchange::feed`], writes every byte the
//! exchange returns, and closes the connection once a result is known.
//! The caller also owns the one deadline of the attempt and reports a
//! timeout, a closed connection, or a reset through [`Exchange::timed_out`],
//! [`Exchange::closed`], and [`Exchange::reset`].

use std::time::Duration;

use serde_json::{Map, Value};
use stackly_domain::ReasonCode;

use super::NativeProbeFailure;

/// Longest line the server may send before its `\r\n`, in bytes.
pub const NATS_LINE_MAX: usize = 16 * 1024;
/// Longest `$JS.API.INFO` reply payload, in bytes.
pub const NATS_REPLY_MAX: usize = 64 * 1024;
/// Most bytes one attempt reads from the server.
pub const NATS_READ_MAX: usize = 128 * 1024;
/// Most characters of one value from the server kept in a detail; longer
/// values keep their first 61 characters and end in `...`.
pub const NATS_DETAIL_VALUE_MAX: usize = 64;

/// The client name in `CONNECT`, which the server shows in its monitoring.
pub const CLIENT_NAME: &str = "stackly-readiness";
/// The subject of the JetStream account info request.
pub const JETSTREAM_INFO_SUBJECT: &str = "$JS.API.INFO";
/// The `type` of a successful account info reply.
pub const ACCOUNT_INFO_TYPE: &str = "io.nats.jetstream.api.v1.account_info_response";
/// The start of the probe's inbox subject; 32 lowercase hex digits follow.
pub const INBOX_PREFIX: &str = "_INBOX.stackly.";

/// The TCP connect was refused or failed.
pub const CONNECT_FAILED: ReasonCode = ReasonCode::from_static("connect_failed");
/// The attempt ran out of time.
pub const TIMEOUT: ReasonCode = ReasonCode::from_static("timeout");
/// The server sent something the protocol does not allow here, an `-ERR`,
/// or closed or reset the connection.
pub const PROTOCOL_ERROR: ReasonCode = ReasonCode::from_static("protocol_error");
/// The server is not the instance's server.
pub const IDENTITY_MISMATCH: ReasonCode = ReasonCode::from_static("identity_mismatch");
/// The server requires TLS, which the package never configures.
pub const TLS_REQUIRED: ReasonCode = ReasonCode::from_static("tls_required");
/// The server requires authentication, which the package never configures.
pub const AUTH_REQUIRED: ReasonCode = ReasonCode::from_static("auth_required");
/// The server is in lame duck mode, on its way to stop.
pub const LAME_DUCK: ReasonCode = ReasonCode::from_static("lame_duck");
/// JetStream is off, but the instance asks for it.
pub const JETSTREAM_DISABLED: ReasonCode = ReasonCode::from_static("jetstream_disabled");
/// JetStream is on, but the instance asks for it off.
pub const JETSTREAM_UNEXPECTED: ReasonCode = ReasonCode::from_static("jetstream_unexpected");
/// The JetStream API answered with an error or another reply type.
pub const JETSTREAM_UNAVAILABLE: ReasonCode = ReasonCode::from_static("jetstream_unavailable");

/// What the instance asks of its server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expectation {
    /// The `server_name` the instance's server must report.
    pub server_name: String,
    /// Whether JetStream must be on (`true`) or off (`false`).
    pub jetstream: bool,
}

/// What the exchange is waiting for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Phase {
    /// The server's first line, `INFO`.
    Info,
    /// The `PONG` that answers the probe's `PING`.
    Pong,
    /// The reply to the `$JS.API.INFO` request.
    JetStreamReply,
}

impl Phase {
    /// The phase as the end of a "while waiting for ..." sentence.
    fn awaited(self) -> String {
        match self {
            Self::Info => "INFO".to_owned(),
            Self::Pong => "PONG".to_owned(),
            Self::JetStreamReply => format!("the {JETSTREAM_INFO_SUBJECT} reply"),
        }
    }
}

/// What the caller does after one [`Exchange::feed`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Step {
    /// Bytes to write to the server, in order; often empty.
    pub write: Vec<u8>,
    /// The attempt's result once it is known; the caller then closes the
    /// connection. `Ok` means ready.
    pub done: Option<Result<(), NativeProbeFailure>>,
}

/// One `nats-readiness` attempt.
#[derive(Debug, Clone)]
pub struct Exchange {
    expectation: Expectation,
    inbox: String,
    phase: Phase,
    /// Bytes read and not yet consumed.
    buffer: Vec<u8>,
    /// How many leading bytes of `buffer` are known to hold no `\r\n`.
    scanned: usize,
    /// Every byte read so far, for [`NATS_READ_MAX`].
    received: usize,
    /// The size of a `MSG` payload whose bytes are still due.
    payload: Option<usize>,
    result: Option<Result<(), NativeProbeFailure>>,
}

impl Exchange {
    /// Starts an attempt. `inbox_token` is 16 random bytes from the caller
    /// (spec 0023 uses `getrandom`); they become the hex tail of the inbox.
    #[must_use]
    pub fn new(expectation: Expectation, inbox_token: [u8; 16]) -> Self {
        let mut inbox = String::with_capacity(INBOX_PREFIX.len() + 32);
        inbox.push_str(INBOX_PREFIX);
        for byte in inbox_token {
            inbox.push(hex_digit(byte >> 4));
            inbox.push(hex_digit(byte & 0x0f));
        }
        Self {
            expectation,
            inbox,
            phase: Phase::Info,
            buffer: Vec::new(),
            scanned: 0,
            received: 0,
            payload: None,
            result: None,
        }
    }

    /// The inbox subject the `$JS.API.INFO` reply goes to.
    #[must_use]
    pub fn inbox(&self) -> &str {
        &self.inbox
    }

    /// What the exchange waits for; after a result, what it waited for
    /// last.
    #[must_use]
    pub const fn phase(&self) -> Phase {
        self.phase
    }

    /// The result, once known.
    #[must_use]
    pub const fn result(&self) -> Option<&Result<(), NativeProbeFailure>> {
        self.result.as_ref()
    }

    /// Takes the bytes the server sent, in any chunk sizes, and returns
    /// what to write and, once known, the result. How the bytes are split
    /// never changes the result or the bytes to write. After a result,
    /// further bytes are ignored and the same result is returned.
    pub fn feed(&mut self, bytes: &[u8]) -> Step {
        let mut write = Vec::new();
        if self.result.is_none() {
            // Only the first NATS_READ_MAX bytes of the attempt are read, so
            // a result they settle wins over the limit in any chunking.
            let allowed = NATS_READ_MAX - self.received;
            let (read, beyond) = bytes.split_at(bytes.len().min(allowed));
            self.received += read.len();
            self.buffer.extend_from_slice(read);
            self.consume(&mut write);
            if self.result.is_none() && !beyond.is_empty() {
                self.finish(Err(protocol_error(format!(
                    "the server sent more than {NATS_READ_MAX} bytes"
                ))));
            }
        }
        Step {
            write,
            done: self.result.clone(),
        }
    }

    /// The failure when the attempt's deadline passes in the current phase.
    #[must_use]
    pub fn timed_out(&self, attempt: Duration) -> NativeProbeFailure {
        let ms = attempt.as_millis();
        let detail = match self.phase {
            Phase::Info => format!("no INFO from the server within {ms} ms"),
            Phase::Pong => format!("no PONG from the server within {ms} ms"),
            Phase::JetStreamReply => {
                format!("no reply to {JETSTREAM_INFO_SUBJECT} within {ms} ms")
            }
        };
        NativeProbeFailure::new(TIMEOUT, detail)
    }

    /// The failure when the server closes the connection (end of file)
    /// before a result.
    #[must_use]
    pub fn closed(&self) -> NativeProbeFailure {
        protocol_error(format!(
            "connection closed while waiting for {}",
            self.phase.awaited()
        ))
    }

    /// The failure when a read or write fails with a reset or a broken
    /// pipe before a result.
    #[must_use]
    pub fn reset(&self) -> NativeProbeFailure {
        protocol_error(format!(
            "connection reset while waiting for {}",
            self.phase.awaited()
        ))
    }

    /// Records the result once; later results are ignored.
    fn finish(&mut self, result: Result<(), NativeProbeFailure>) {
        if self.result.is_none() {
            self.result = Some(result);
            self.buffer.clear();
        }
    }

    /// Consumes every complete line and payload in the buffer, then drops
    /// the consumed bytes once, so many small lines cost linear time.
    fn consume(&mut self, write: &mut Vec<u8>) {
        let mut buffer = std::mem::take(&mut self.buffer);
        let mut at = 0;
        while self.result.is_none() {
            let pending = &buffer[at..];
            if let Some(len) = self.payload {
                let Some(frame) = pending.get(..len + 2) else {
                    break;
                };
                at += len + 2;
                self.payload = None;
                self.scanned = 0;
                let result = match frame.split_at(len) {
                    (payload, b"\r\n") => reply(payload),
                    _ => Err(protocol_error(format!(
                        "the {JETSTREAM_INFO_SUBJECT} reply does not end with CRLF"
                    ))),
                };
                self.finish(result);
                break;
            }
            let start = self.scanned.saturating_sub(1);
            let Some(end) = find_crlf(&pending[start..]).map(|found| start + found) else {
                self.scanned = pending.len();
                // One byte more than the limit may be the `\r` of a line
                // that ends right at the limit.
                if pending.len() > NATS_LINE_MAX + 1 {
                    self.finish(Err(line_too_long()));
                }
                break;
            };
            if end > NATS_LINE_MAX {
                self.finish(Err(line_too_long()));
                break;
            }
            at += end + 2;
            self.scanned = 0;
            self.line(&pending[..end], write);
        }
        if self.result.is_none() {
            buffer.drain(..at);
            self.buffer = buffer;
        }
    }

    /// Handles one line without its `\r\n`.
    fn line(&mut self, line: &[u8], write: &mut Vec<u8>) {
        let (op, rest) = split_op(line);
        if self.phase == Phase::Info {
            if !op.eq_ignore_ascii_case(b"INFO") {
                self.finish(Err(protocol_error(format!(
                    "expected INFO, got {}",
                    quoted_bytes(line)
                ))));
                return;
            }
            match self.check_info(rest) {
                Ok(()) => {
                    write.extend_from_slice(&connect_bytes());
                    self.phase = Phase::Pong;
                }
                Err(failure) => self.finish(Err(failure)),
            }
            return;
        }
        if op.eq_ignore_ascii_case(b"PING") && rest.is_empty() {
            write.extend_from_slice(b"PONG\r\n");
        } else if op.eq_ignore_ascii_case(b"PONG") && rest.is_empty() && self.phase == Phase::Pong {
            if self.expectation.jetstream {
                write.extend_from_slice(&self.jetstream_request());
                self.phase = Phase::JetStreamReply;
            } else {
                self.finish(Ok(()));
            }
        } else if op.eq_ignore_ascii_case(b"INFO")
            || (op.eq_ignore_ascii_case(b"+OK") && rest.is_empty())
        {
            // A later INFO (the server sends one after CONNECT) and +OK
            // carry nothing the attempt needs.
        } else if op.eq_ignore_ascii_case(b"-ERR") {
            self.finish(Err(protocol_error(format!(
                "server error: {}",
                sanitize(&String::from_utf8_lossy(rest))
            ))));
        } else if op.eq_ignore_ascii_case(b"MSG") && self.phase == Phase::JetStreamReply {
            self.msg(rest);
        } else {
            self.finish(Err(protocol_error(format!(
                "unexpected line {}",
                quoted_bytes(line)
            ))));
        }
    }

    /// Checks the first `INFO` against the expectation. Fields the probe
    /// does not read are ignored; a read field of the wrong type (`null`
    /// included) fails the attempt, and a missing flag reads as `false`.
    fn check_info(&self, json: &[u8]) -> Result<(), NativeProbeFailure> {
        let value: Value = serde_json::from_slice(json)
            .map_err(|_| protocol_error("INFO is not one valid JSON value"))?;
        let Some(info) = value.as_object() else {
            return Err(protocol_error("INFO is not a JSON object"));
        };
        let server_name = text_field(info, "server_name")?;
        text_field(info, "server_id")?;
        text_field(info, "version")?;
        let jetstream = flag_field(info, "jetstream")?;
        let auth_required = flag_field(info, "auth_required")?;
        let tls_required = flag_field(info, "tls_required")?;
        let lame_duck = flag_field(info, "ldm")?;
        let expected = &self.expectation.server_name;
        match server_name {
            Some(name) if name == expected => {}
            Some(name) => {
                return Err(NativeProbeFailure::new(
                    IDENTITY_MISMATCH,
                    format!(
                        "server_name is {}, expected {}",
                        quoted(name),
                        quoted(expected)
                    ),
                ));
            }
            None => {
                return Err(NativeProbeFailure::new(
                    IDENTITY_MISMATCH,
                    format!("server_name is missing, expected {}", quoted(expected)),
                ));
            }
        }
        if tls_required {
            return Err(NativeProbeFailure::new(
                TLS_REQUIRED,
                "the server requires TLS",
            ));
        }
        if auth_required {
            return Err(NativeProbeFailure::new(
                AUTH_REQUIRED,
                "the server requires authentication",
            ));
        }
        if lame_duck {
            return Err(NativeProbeFailure::new(
                LAME_DUCK,
                "the server is in lame duck mode",
            ));
        }
        match (jetstream, self.expectation.jetstream) {
            (false, true) => Err(NativeProbeFailure::new(
                JETSTREAM_DISABLED,
                "JetStream is off, the instance asks for it",
            )),
            (true, false) => Err(NativeProbeFailure::new(
                JETSTREAM_UNEXPECTED,
                "JetStream is on, the instance asks for it off",
            )),
            _ => Ok(()),
        }
    }

    /// Reads the arguments of a `MSG` line and waits for its payload.
    fn msg(&mut self, args: &[u8]) {
        let parts: Vec<&[u8]> = args
            .split(|byte| matches!(byte, b' ' | b'\t'))
            .filter(|part| !part.is_empty())
            .collect();
        let (subject, sid, size) = match parts.as_slice() {
            [subject, sid, size] | [subject, sid, _, size] => (*subject, *sid, *size),
            _ => {
                self.finish(Err(protocol_error(
                    "a MSG line does not have 3 or 4 arguments",
                )));
                return;
            }
        };
        if subject != self.inbox.as_bytes() || sid != b"1" {
            self.finish(Err(protocol_error(
                "a MSG arrived for another subscription",
            )));
            return;
        }
        match parse_size(size) {
            Some(len) if len <= NATS_REPLY_MAX => self.payload = Some(len),
            Some(_) => self.finish(Err(protocol_error(format!(
                "the {JETSTREAM_INFO_SUBJECT} reply is larger than {NATS_REPLY_MAX} bytes"
            )))),
            None => self.finish(Err(protocol_error("a MSG line has an invalid size"))),
        }
    }

    /// `SUB <inbox> 1` and `PUB $JS.API.INFO <inbox> 0` with its empty
    /// payload.
    pub(crate) fn jetstream_request(&self) -> Vec<u8> {
        format!(
            "SUB {inbox} 1\r\nPUB {JETSTREAM_INFO_SUBJECT} {inbox} 0\r\n\r\n",
            inbox = self.inbox
        )
        .into_bytes()
    }
}

/// The exact `CONNECT` and `PING` the probe sends after a good `INFO`:
/// no credentials, no echo, no headers, and no `+OK` per command.
#[must_use]
pub fn connect_bytes() -> Vec<u8> {
    format!(
        "CONNECT {{\"verbose\":false,\"pedantic\":false,\"tls_required\":false,\"name\":\"{CLIENT_NAME}\",\"lang\":\"rust\",\"version\":\"{}\",\"protocol\":1,\"echo\":false,\"headers\":false,\"no_responders\":false}}\r\nPING\r\n",
        env!("CARGO_PKG_VERSION")
    )
    .into_bytes()
}

/// The failure when the TCP connect to `address` (`host:port`) fails with
/// `error`, a short description of the IO error kind.
#[must_use]
pub fn connect_failed(address: &str, error: &str) -> NativeProbeFailure {
    NativeProbeFailure::new(
        CONNECT_FAILED,
        format!(
            "connect to {} failed: {}",
            sanitize(address),
            sanitize(error)
        ),
    )
}

/// The failure when the TCP connect to `address` does not finish within
/// the attempt.
#[must_use]
pub fn connect_timed_out(address: &str, attempt: Duration) -> NativeProbeFailure {
    NativeProbeFailure::new(
        TIMEOUT,
        format!(
            "connect to {} timed out after {} ms",
            sanitize(address),
            attempt.as_millis()
        ),
    )
}

/// The failure for a Unix socket target, which validation already refuses
/// for `nats-readiness`; the attempt never connects.
#[must_use]
pub fn unix_target() -> NativeProbeFailure {
    protocol_error("nats-readiness supports tcp endpoints only")
}

/// Checks the payload of the `$JS.API.INFO` reply.
fn reply(payload: &[u8]) -> Result<(), NativeProbeFailure> {
    let value: Value = serde_json::from_slice(payload).map_err(|_| {
        protocol_error(format!(
            "the {JETSTREAM_INFO_SUBJECT} reply is not one valid JSON value"
        ))
    })?;
    let Some(object) = value.as_object() else {
        return Err(protocol_error(format!(
            "the {JETSTREAM_INFO_SUBJECT} reply is not a JSON object"
        )));
    };
    if let Some(error) = object.get("error") {
        let code = error.get("code").map_or_else(String::new, value_text);
        let description = error
            .get("description")
            .map_or_else(String::new, value_text);
        return Err(NativeProbeFailure::new(
            JETSTREAM_UNAVAILABLE,
            format!(
                "{JETSTREAM_INFO_SUBJECT} error {}: {}",
                sanitize(&code),
                sanitize(&description)
            ),
        ));
    }
    match object.get("type").and_then(Value::as_str) {
        Some(ACCOUNT_INFO_TYPE) => Ok(()),
        Some(other) => Err(NativeProbeFailure::new(
            JETSTREAM_UNAVAILABLE,
            format!(
                "{JETSTREAM_INFO_SUBJECT} replied with type {}",
                quoted(other)
            ),
        )),
        None => Err(NativeProbeFailure::new(
            JETSTREAM_UNAVAILABLE,
            format!("{JETSTREAM_INFO_SUBJECT} replied without a type"),
        )),
    }
}

/// A string field of `INFO`, or `None` when it is missing.
fn text_field<'a>(
    info: &'a Map<String, Value>,
    name: &str,
) -> Result<Option<&'a str>, NativeProbeFailure> {
    match info.get(name) {
        None => Ok(None),
        Some(Value::String(text)) => Ok(Some(text)),
        Some(_) => Err(protocol_error(format!("INFO field {name} is not a string"))),
    }
}

/// A boolean field of `INFO`; a missing one reads as `false`.
fn flag_field(info: &Map<String, Value>, name: &str) -> Result<bool, NativeProbeFailure> {
    match info.get(name) {
        None => Ok(false),
        Some(Value::Bool(flag)) => Ok(*flag),
        Some(_) => Err(protocol_error(format!(
            "INFO field {name} is not a boolean"
        ))),
    }
}

/// A protocol error with `detail`.
fn protocol_error(detail: impl Into<String>) -> NativeProbeFailure {
    NativeProbeFailure::new(PROTOCOL_ERROR, detail)
}

fn line_too_long() -> NativeProbeFailure {
    protocol_error(format!(
        "a line from the server is longer than {NATS_LINE_MAX} bytes"
    ))
}

/// Splits a line into its operation and the rest without the spaces and
/// tabs around it.
fn split_op(line: &[u8]) -> (&[u8], &[u8]) {
    let is_space = |byte: &u8| matches!(byte, b' ' | b'\t');
    match line.iter().position(is_space) {
        Some(at) => {
            let (op, rest) = line.split_at(at);
            (op, trim(rest))
        }
        None => (line, &[]),
    }
}

/// `bytes` without leading and trailing spaces and tabs.
fn trim(bytes: &[u8]) -> &[u8] {
    let is_space = |byte: &u8| matches!(byte, b' ' | b'\t');
    let start = bytes.iter().position(|byte| !is_space(byte));
    let end = bytes.iter().rposition(|byte| !is_space(byte));
    match (start, end) {
        (Some(start), Some(end)) => &bytes[start..=end],
        _ => &[],
    }
}

/// The offset of the first `\r\n` in `bytes`.
fn find_crlf(bytes: &[u8]) -> Option<usize> {
    bytes.windows(2).position(|pair| pair == b"\r\n")
}

/// A `MSG` size: 1 to 10 ASCII digits.
fn parse_size(text: &[u8]) -> Option<usize> {
    if text.is_empty() || text.len() > 10 || !text.iter().all(u8::is_ascii_digit) {
        return None;
    }
    std::str::from_utf8(text).ok()?.parse().ok()
}

/// A JSON value as detail text: a string as is, anything else as JSON.
fn value_text(value: &Value) -> String {
    value
        .as_str()
        .map_or_else(|| value.to_string(), str::to_owned)
}

/// A value from the server, safe for a detail: control characters (C0,
/// DEL, C1) become `?`, and a value longer than
/// [`NATS_DETAIL_VALUE_MAX`] characters keeps its first 61 and ends in
/// `...`.
fn sanitize(text: &str) -> String {
    let cleaned: String = text
        .chars()
        .map(|c| if is_control(c) { '?' } else { c })
        .collect();
    if cleaned.chars().count() <= NATS_DETAIL_VALUE_MAX {
        return cleaned;
    }
    let mut cut: String = cleaned.chars().take(NATS_DETAIL_VALUE_MAX - 3).collect();
    cut.push_str("...");
    cut
}

/// A sanitized value in double quotes.
fn quoted(text: &str) -> String {
    format!("\"{}\"", sanitize(text))
}

/// Bytes from the server, read as lossy UTF-8, sanitized, in double
/// quotes.
fn quoted_bytes(bytes: &[u8]) -> String {
    quoted(&String::from_utf8_lossy(bytes))
}

const fn is_control(c: char) -> bool {
    matches!(c, '\u{0}'..='\u{1f}' | '\u{7f}'..='\u{9f}')
}

const fn hex_digit(nibble: u8) -> char {
    match nibble {
        0..=9 => (b'0' + nibble) as char,
        _ => (b'a' + nibble - 10) as char,
    }
}
