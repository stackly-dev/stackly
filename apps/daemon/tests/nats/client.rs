//! A small NATS client for the native tests (spec 0039, *Test native*): the
//! raw client protocol over `tokio::net::TcpStream`, with one 5 second
//! deadline per request and no task in the background. It speaks just
//! enough of the protocol and the JetStream API to sign in, check
//! permissions, and create, fill, and read streams. Product code never uses
//! it.
#![allow(dead_code, reason = "each scenario module uses a part of this client")]

use std::collections::VecDeque;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::time::{Instant, timeout_at};

/// The deadline of one request, connect included.
pub const DEADLINE: Duration = Duration::from_secs(5);

/// One message the server delivered to a subscription.
#[derive(Debug, Clone)]
pub struct Message {
    /// The subject the message was published to.
    pub subject: String,
    /// The payload.
    pub payload: Vec<u8>,
}

/// One connection to a NATS server.
pub struct Nats {
    reader: BufReader<OwnedReadHalf>,
    writer: OwnedWriteHalf,
    next_sid: u64,
    /// `MSG` deliveries read while waiting for a `PONG`: the sid and the
    /// message.
    pending: VecDeque<(u64, Message)>,
}

/// The loopback address of `port`.
#[must_use]
pub fn loopback(port: u16) -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port)
}

impl Nats {
    /// Connects to `addr`, sends `CONNECT` with `credentials` (user and
    /// password) or none, then `PING`, and returns the connection with the
    /// first line after them that is not an `INFO`: `PONG` when the server
    /// took the connection, or the `-ERR` line when it refused it.
    pub async fn connect(addr: SocketAddr, credentials: Option<(&str, &str)>) -> (Self, String) {
        let deadline = Instant::now() + DEADLINE;
        let stream = timeout_at(deadline, TcpStream::connect(addr))
            .await
            .expect("the connect ends in time")
            .expect("the client port accepts");
        let (read, writer) = stream.into_split();
        let mut nats = Self {
            reader: BufReader::new(read),
            writer,
            next_sid: 1,
            pending: VecDeque::new(),
        };
        let info = nats.line(deadline).await;
        assert!(
            info.starts_with("INFO "),
            "the server greets with INFO: {info}"
        );
        let mut connect = json!({
            "verbose": false,
            "pedantic": false,
            "name": "stackly-native-test",
            "lang": "rust",
            "version": "0.0.0",
            "protocol": 1,
            "headers": false,
        });
        if let Some((user, pass)) = credentials {
            connect["user"] = json!(user);
            connect["pass"] = json!(pass);
        }
        nats.send(format!("CONNECT {connect}\r\nPING\r\n").as_bytes())
            .await;
        let first = nats.reply_line(deadline).await;
        (nats, first)
    }

    /// Connects with `credentials` and asserts that the server answered
    /// `PONG`.
    pub async fn signed_in(addr: SocketAddr, credentials: Option<(&str, &str)>) -> Self {
        let (nats, first) = Self::connect(addr, credentials).await;
        assert_eq!(first, "PONG", "the server refused the connection");
        nats
    }

    /// Writes raw protocol bytes.
    async fn send(&mut self, bytes: &[u8]) {
        self.writer
            .write_all(bytes)
            .await
            .expect("the protocol line is written");
        self.writer.flush().await.expect("the connection flushes");
    }

    /// Reads one line without its `\r\n`, answering a server `PING` with
    /// `PONG` on the way.
    async fn line(&mut self, deadline: Instant) -> String {
        loop {
            let mut line = String::new();
            let read = timeout_at(deadline, self.reader.read_line(&mut line))
                .await
                .expect("the server answers within the deadline")
                .expect("the connection reads");
            assert!(read > 0, "the server closed the connection");
            let line = line.trim_end_matches(['\r', '\n']).to_owned();
            if line == "PING" {
                self.send(b"PONG\r\n").await;
                continue;
            }
            return line;
        }
    }

    /// Reads the next line that is not an `INFO` (fact 1 of spec 0039: a
    /// second INFO can come after `CONNECT`).
    async fn reply_line(&mut self, deadline: Instant) -> String {
        loop {
            let line = self.line(deadline).await;
            if !line.starts_with("INFO ") {
                return line;
            }
        }
    }

    /// Sends `command`, then `PING`, and returns `Ok` when the server
    /// answers `PONG`, or the `-ERR` line it sent first. A permission
    /// violation of the command comes before the `PONG`.
    async fn checked(&mut self, command: &[u8]) -> Result<(), String> {
        let deadline = Instant::now() + DEADLINE;
        let mut bytes = command.to_vec();
        bytes.extend_from_slice(b"PING\r\n");
        self.send(&bytes).await;
        let mut error = None;
        loop {
            let line = self.reply_line(deadline).await;
            if line == "PONG" {
                return error.map_or(Ok(()), Err);
            }
            if line.starts_with("-ERR") {
                // A connection error closes the connection without a PONG.
                if !line.contains("Permissions Violation") {
                    return Err(line);
                }
                error.get_or_insert(line);
                continue;
            }
            if line.starts_with("MSG ") {
                let delivery = self.delivery(&line, deadline).await;
                self.pending.push_back(delivery);
            }
        }
    }

    /// Reads the payload of a `MSG` line and returns its sid and message.
    async fn delivery(&mut self, line: &str, deadline: Instant) -> (u64, Message) {
        let fields: Vec<&str> = line.split_whitespace().collect();
        let sid = fields
            .get(2)
            .and_then(|sid| sid.parse().ok())
            .unwrap_or_else(|| panic!("a MSG line names its sid: {line}"));
        let payload = self.payload(line, deadline).await;
        (
            sid,
            Message {
                subject: fields[1].to_owned(),
                payload,
            },
        )
    }

    /// Reads the payload a `MSG` line announces.
    async fn payload(&mut self, line: &str, deadline: Instant) -> Vec<u8> {
        let size: usize = line
            .split_whitespace()
            .last()
            .and_then(|size| size.parse().ok())
            .unwrap_or_else(|| panic!("a MSG line ends with its size: {line}"));
        let mut payload = vec![0; size + 2];
        timeout_at(deadline, self.reader.read_exact(&mut payload))
            .await
            .expect("the payload arrives within the deadline")
            .expect("the payload reads");
        payload.truncate(size);
        payload
    }

    /// Publishes `payload` to `subject` with `reply` as the reply subject,
    /// raw, and returns whether the server allowed it.
    pub async fn publish_with_reply(
        &mut self,
        subject: &str,
        reply: Option<&str>,
        payload: &[u8],
    ) -> Result<(), String> {
        let mut command = match reply {
            Some(reply) => format!("PUB {subject} {reply} {}\r\n", payload.len()),
            None => format!("PUB {subject} {}\r\n", payload.len()),
        }
        .into_bytes();
        command.extend_from_slice(payload);
        command.extend_from_slice(b"\r\n");
        self.checked(&command).await
    }

    /// Subscribes to `subject` and returns whether the server allowed it.
    pub async fn subscribe(&mut self, subject: &str) -> Result<u64, String> {
        let sid = self.next_sid;
        self.next_sid += 1;
        self.checked(format!("SUB {subject} {sid}\r\n").as_bytes())
            .await
            .map(|()| sid)
    }

    /// Every message the server delivered to the subscription `sid` up to
    /// a `PING` round trip now, taken out of the pending list.
    pub async fn deliveries(&mut self, sid: u64) -> Vec<Message> {
        self.checked(b"")
            .await
            .expect("a PING round trip needs no permission");
        let mut out = Vec::new();
        self.pending.retain(|(got, message)| {
            if *got == sid {
                out.push(message.clone());
                false
            } else {
                true
            }
        });
        out
    }

    /// Publishes `payload` to `subject` with a fresh `_INBOX.test.<hex>`
    /// reply subject, outside the inbox space of the readiness probe, and
    /// returns the first reply, or the `-ERR` line of a refusal.
    pub async fn request(&mut self, subject: &str, payload: &[u8]) -> Result<Message, String> {
        self.request_in("_INBOX.test", subject, payload).await
    }

    /// [`Nats::request`] with the reply subject under `prefix`, for a user
    /// that may only subscribe there, such as `_INBOX.stackly` for the
    /// readiness user.
    pub async fn request_in(
        &mut self,
        prefix: &str,
        subject: &str,
        payload: &[u8],
    ) -> Result<Message, String> {
        let deadline = Instant::now() + DEADLINE;
        let inbox = format!("{prefix}.{}", uuid::Uuid::now_v7().simple());
        let sid = self.subscribe(&inbox).await?;
        self.publish_with_reply(subject, Some(&inbox), payload)
            .await?;
        let message = loop {
            if let Some(index) = self.pending.iter().position(|(got, _)| *got == sid) {
                let (_, message) = self.pending.remove(index).expect("the index exists");
                break message;
            }
            let line = self.reply_line(deadline).await;
            if line.starts_with("-ERR") {
                return Err(line);
            }
            if line.starts_with("MSG ") {
                let delivery = self.delivery(&line, deadline).await;
                self.pending.push_back(delivery);
            }
        };
        self.send(format!("UNSUB {sid}\r\n").as_bytes()).await;
        Ok(message)
    }

    /// A JetStream API request whose reply is JSON; a reply with an
    /// `error` member is returned as `Err` with that member.
    pub async fn api(&mut self, subject: &str, body: &Value) -> Result<Value, Value> {
        let reply = self
            .request(subject, body.to_string().as_bytes())
            .await
            .map_err(|line| json!({ "protocol": line }))?;
        let value: Value = serde_json::from_slice(&reply.payload)
            .unwrap_or_else(|error| panic!("{subject} answers JSON: {error}"));
        match value.get("error") {
            Some(error) => Err(error.clone()),
            None => Ok(value),
        }
    }

    /// Creates the stream `name` on `subject` with `storage` (`file` or
    /// `memory`).
    pub async fn stream_create(
        &mut self,
        name: &str,
        subject: &str,
        storage: &str,
    ) -> Result<Value, Value> {
        self.api(
            &format!("$JS.API.STREAM.CREATE.{name}"),
            &json!({ "name": name, "subjects": [subject], "storage": storage }),
        )
        .await
    }

    /// Publishes `payload` to `subject` and waits for the JetStream ack.
    pub async fn publish(&mut self, subject: &str, payload: &[u8]) -> Result<Value, Value> {
        let reply = self
            .request(subject, payload)
            .await
            .map_err(|line| json!({ "protocol": line }))?;
        let value: Value = serde_json::from_slice(&reply.payload)
            .unwrap_or_else(|error| panic!("the ack of {subject} is JSON: {error}"));
        match value.get("error") {
            Some(error) => Err(error.clone()),
            None => Ok(value),
        }
    }

    /// The `$JS.API.STREAM.INFO` reply of the stream `name`.
    pub async fn stream_info(&mut self, name: &str) -> Result<Value, Value> {
        self.api(&format!("$JS.API.STREAM.INFO.{name}"), &json!({}))
            .await
    }

    /// The number of messages in the stream `name`.
    pub async fn stream_messages(&mut self, name: &str) -> u64 {
        let info = self
            .stream_info(name)
            .await
            .unwrap_or_else(|error| panic!("stream {name}: {error}"));
        info["state"]["messages"]
            .as_u64()
            .unwrap_or_else(|| panic!("stream {name} reports its messages: {info}"))
    }

    /// The payload of the message `seq` of the stream `name`.
    pub async fn stream_get(&mut self, name: &str, seq: u64) -> Vec<u8> {
        let reply = self
            .api(
                &format!("$JS.API.STREAM.MSG.GET.{name}"),
                &json!({ "seq": seq }),
            )
            .await
            .unwrap_or_else(|error| panic!("message {seq} of {name}: {error}"));
        let data = reply["message"]["data"].as_str().unwrap_or("");
        base64_decode(data)
    }
}

/// Decodes standard base64 with padding, as the JetStream API writes it.
#[must_use]
pub fn base64_decode(text: &str) -> Vec<u8> {
    fn value(byte: u8) -> u32 {
        match byte {
            b'A'..=b'Z' => u32::from(byte - b'A'),
            b'a'..=b'z' => u32::from(byte - b'a') + 26,
            b'0'..=b'9' => u32::from(byte - b'0') + 52,
            b'+' => 62,
            b'/' => 63,
            other => panic!("not a base64 character: {other}"),
        }
    }
    let bytes: Vec<u8> = text.bytes().filter(|byte| *byte != b'=').collect();
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    for chunk in bytes.chunks(4) {
        let mut acc = 0u32;
        for (index, byte) in chunk.iter().enumerate() {
            acc |= value(*byte) << (18 - 6 * index);
        }
        let produced = chunk.len() * 6 / 8;
        for index in 0..produced {
            out.push(((acc >> (16 - 8 * index)) & 0xFF) as u8);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::base64_decode;

    #[test]
    fn base64_decodes_with_and_without_padding() {
        assert_eq!(base64_decode("YS0x"), b"a-1");
        assert_eq!(base64_decode("Yi01"), b"b-5");
        assert_eq!(base64_decode("YQ=="), b"a");
        assert_eq!(base64_decode("YWI="), b"ab");
        assert_eq!(base64_decode(""), b"");
    }
}
