//! Staged decoding of frame payloads (AC-1, AC-7, AC-10).
//!
//! [`decode_request`] runs on the daemon: a depth check without a full
//! parse, the request head with `content` kept raw, the params of the named
//! method, [`Validate`], then the `requestId` and `idempotencyKey` formats.
//! [`decode_server_frame`] runs on Rust clients and skips event kinds this
//! build does not know. Neither ever passes serde's error text on, because
//! it can hold input values, secrets included.
//!
//! The request, its `call`, its params, a daemon frame, and its content
//! must each be a JSON object. serde derives also read a struct from a
//! positional array, so each stage checks the first byte before it decodes.

use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::value::RawValue;

use crate::codes::{ipc, request};
use crate::error::{ErrorBody, FieldError};
use crate::events::{Event, EventKind, EventProbe};
use crate::limits::{IDEMPOTENCY_KEY_MAX, MAX_JSON_DEPTH, REQUEST_ID_MAX};
use crate::message::{Call, ErrorFrame, Request, Response};
use crate::wire::Sequence;

/// The path of a field error about the params as a whole, as
/// `serde_path_to_error` writes it.
pub const PARAMS_ROOT: &str = ".";
/// Longest method name echoed back in `details.method`, in bytes.
pub(crate) const METHOD_ECHO_MAX: usize = 64;

/// The fixed set of field error codes. Each has a fixed English text, so a
/// field error never carries the input value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum FieldCode {
    /// The value has the wrong type or shape.
    InvalidValue,
    /// The value is missing.
    Required,
    /// The value is shorter than allowed.
    TooShort,
    /// The value is longer than allowed.
    TooLong,
    /// The value has the right type but a bad format.
    InvalidFormat,
    /// The number is outside the allowed range.
    OutOfRange,
    /// The list has more entries than allowed.
    TooMany,
    /// The value is not one of the known values.
    UnknownValue,
}

impl FieldCode {
    /// Every field code.
    pub const ALL: [Self; 8] = [
        Self::InvalidValue,
        Self::Required,
        Self::TooShort,
        Self::TooLong,
        Self::InvalidFormat,
        Self::OutOfRange,
        Self::TooMany,
        Self::UnknownValue,
    ];

    /// The wire value, such as `too_long`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidValue => "invalid_value",
            Self::Required => "required",
            Self::TooShort => "too_short",
            Self::TooLong => "too_long",
            Self::InvalidFormat => "invalid_format",
            Self::OutOfRange => "out_of_range",
            Self::TooMany => "too_many",
            Self::UnknownValue => "unknown_value",
        }
    }

    /// The fixed text sent as `fields[].message`.
    #[must_use]
    pub const fn message(self) -> &'static str {
        match self {
            Self::InvalidValue => "value does not match the expected type or format",
            Self::Required => "value is required",
            Self::TooShort => "value is too short",
            Self::TooLong => "value is too long",
            Self::InvalidFormat => "value does not have the expected format",
            Self::OutOfRange => "value is out of range",
            Self::TooMany => "list has too many entries",
            Self::UnknownValue => "value is not one of the known values",
        }
    }
}

/// Rules on params beyond what their types say, such as a length limit. Each
/// spec adds its rules here on its own params types, never in handlers.
///
/// The error keeps at most [`FIELD_ERRORS_MAX`] fields, so a rule over a
/// list checks at most as many entries as the list may hold, never the
/// whole input.
///
/// [`FIELD_ERRORS_MAX`]: crate::limits::FIELD_ERRORS_MAX
pub trait Validate {
    /// Every field that breaks a rule; empty means valid.
    fn validate(&self) -> Vec<FieldError> {
        Vec::new()
    }
}

/// Whether `bytes` hold a JSON object at the top: the first byte after
/// leading whitespace is `{`. It says nothing about the rest, which the
/// decoder checks.
#[must_use]
pub fn is_object(bytes: &[u8]) -> bool {
    bytes
        .iter()
        .find(|byte| !matches!(byte, b' ' | b'\t' | b'\n' | b'\r'))
        .is_some_and(|&byte| byte == b'{')
}

/// Whether `bytes` nest objects and arrays deeper than `max`, counted
/// without a full parse. Brackets inside strings, escaped quotes included,
/// do not count.
#[must_use]
pub fn exceeds_depth(bytes: &[u8], max: u32) -> bool {
    let mut depth: u32 = 0;
    let mut in_string = false;
    let mut escaped = false;
    for &byte in bytes {
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            continue;
        }
        match byte {
            b'"' => in_string = true,
            b'{' | b'[' => {
                depth += 1;
                if depth > max {
                    return true;
                }
            }
            b'}' | b']' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    false
}

/// Whether `text` is a valid `requestId`: 1 to 64 bytes of
/// `[A-Za-z0-9._:-]`.
#[must_use]
pub fn is_request_id(text: &str) -> bool {
    request_id_field(text).is_none()
}

/// Whether `text` is a valid `idempotencyKey`: 1 to 128 bytes of printable
/// ASCII without spaces (0x21 to 0x7e).
#[must_use]
pub fn is_idempotency_key(text: &str) -> bool {
    idempotency_key_field(text).is_none()
}

fn request_id_field(text: &str) -> Option<FieldCode> {
    let allowed = |b: u8| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b':' | b'-');
    if text.is_empty() {
        Some(FieldCode::TooShort)
    } else if text.len() > REQUEST_ID_MAX {
        Some(FieldCode::TooLong)
    } else if !text.bytes().all(allowed) {
        Some(FieldCode::InvalidFormat)
    } else {
        None
    }
}

fn idempotency_key_field(text: &str) -> Option<FieldCode> {
    if text.is_empty() {
        Some(FieldCode::TooShort)
    } else if text.len() > IDEMPOTENCY_KEY_MAX {
        Some(FieldCode::TooLong)
    } else if !text.bytes().all(|b| (0x21..=0x7e).contains(&b)) {
        Some(FieldCode::InvalidFormat)
    } else {
        None
    }
}

/// The `requestId` of a payload, read leniently: `Some` only when the
/// payload is a JSON object whose `requestId` is a string with a valid
/// format. Other fields are ignored, whatever they hold.
#[must_use]
pub fn request_id(bytes: &[u8]) -> Option<String> {
    #[derive(Deserialize)]
    struct IdOnly {
        #[serde(rename = "requestId", default)]
        request_id: Option<String>,
    }
    if !is_object(bytes) {
        return None;
    }
    serde_json::from_slice::<IdOnly>(bytes)
        .ok()?
        .request_id
        .filter(|id| is_request_id(id))
}

/// The `protocol.major` of a `daemon.handshake` request, read leniently:
/// `Some` when the request is a JSON object, `call.type` is
/// `daemon.handshake`, and `call.content.protocol.major` is a `u32`,
/// whatever the other fields hold.
/// The daemon checks it before decoding the params, so a client of another
/// major always receives `protocol.major_mismatch`.
#[must_use]
pub fn handshake_major(bytes: &[u8]) -> Option<u32> {
    #[derive(Deserialize)]
    struct Head {
        call: CallProbe,
    }
    #[derive(Deserialize)]
    struct CallProbe {
        #[serde(rename = "type")]
        method: String,
        content: ContentProbe,
    }
    #[derive(Deserialize)]
    struct ContentProbe {
        protocol: MajorProbe,
    }
    #[derive(Deserialize)]
    struct MajorProbe {
        major: u32,
    }
    if !is_object(bytes) {
        return None;
    }
    let head: Head = serde_json::from_slice(bytes).ok()?;
    (head.call.method == "daemon.handshake").then_some(head.call.content.protocol.major)
}

/// A request the daemon refuses, with the `requestId` to answer it under.
/// The error is boxed because an [`ErrorBody`] is about 200 bytes, too
/// large to return by value (`clippy::result_large_err`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestRejection {
    /// The request's `requestId` when it was readable and valid.
    pub request_id: Option<String>,
    /// The error to send.
    pub error: Box<ErrorBody>,
}

impl RequestRejection {
    /// The error frame to send.
    #[must_use]
    pub fn into_frame(self) -> ErrorFrame {
        ErrorFrame {
            request_id: self.request_id,
            error: *self.error,
        }
    }
}

/// The request head with every part kept raw, so a broken part never hides
/// the others and key order does not matter.
#[derive(Deserialize)]
struct RawHead<'a> {
    #[serde(rename = "requestId", default, borrow)]
    request_id: Option<&'a RawValue>,
    #[serde(rename = "idempotencyKey", default, borrow)]
    idempotency_key: Option<&'a RawValue>,
    #[serde(default, borrow)]
    call: Option<&'a RawValue>,
}

/// `call` with `content` kept raw, so the params decode against the method
/// named in `type` whatever the key order.
#[derive(Deserialize)]
struct RawCall<'a> {
    #[serde(rename = "type")]
    method: String,
    #[serde(default, borrow)]
    content: Option<&'a RawValue>,
}

/// Decodes one request payload in stages.
///
/// # Errors
///
/// Returns a [`RequestRejection`] with `request.too_deep`,
/// `request.malformed`, `request.unknown_method`, or
/// `request.invalid_params`, checked in that order of stages. The rejection
/// carries the `requestId` whenever it was readable and valid.
pub fn decode_request(bytes: &[u8]) -> Result<Request, RequestRejection> {
    if exceeds_depth(bytes, MAX_JSON_DEPTH) {
        let error = ErrorBody::new(
            &request::TOO_DEEP,
            "request nests deeper than the json limit",
        )
        .with_detail("max", MAX_JSON_DEPTH.to_string());
        return Err(reject(request_id(bytes), error));
    }

    let head: RawHead<'_> = match serde_json::from_slice(bytes) {
        Ok(head) if is_object(bytes) => head,
        Ok(_) => return Err(reject(None, malformed("request is not a json object"))),
        Err(err) if err.is_syntax() || err.is_eof() => {
            return Err(reject(None, malformed("request is not valid json")));
        }
        Err(_) => return Err(reject(None, malformed("request is not a json object"))),
    };
    let request_id: String = match head.request_id.map(decode_raw) {
        Some(Some(id)) => id,
        _ => return Err(reject(None, malformed("request has no requestId string"))),
    };
    let reply_id = is_request_id(&request_id).then(|| request_id.clone());
    let idempotency_key: Option<String> = match head.idempotency_key.map(decode_raw) {
        None => None,
        Some(Some(key)) => Some(key),
        Some(None) => {
            return Err(reject(
                reply_id,
                malformed("idempotencyKey is not a string"),
            ));
        }
    };
    let Some(call) = head.call.and_then(decode_object::<RawCall<'_>>) else {
        return Err(reject(
            reply_id,
            malformed("request has no call object with a type string"),
        ));
    };

    let call = match Call::decode(&call.method, call.content) {
        Some(Ok(call)) => call,
        Some(Err(fields)) => {
            let error = ErrorBody::new(&request::INVALID_PARAMS, "params do not match the method")
                .with_detail("method", call.method)
                .with_fields(fields);
            return Err(reject(reply_id, error));
        }
        None => {
            let mut error = ErrorBody::new(
                &request::UNKNOWN_METHOD,
                "the daemon does not know this method",
            );
            if is_echoable(&call.method) {
                error = error.with_detail("method", call.method);
            }
            return Err(reject(reply_id, error));
        }
    };

    let mut fields = Vec::new();
    if let Some(code) = request_id_field(&request_id) {
        fields.push(FieldError::new("requestId", code));
    }
    if let Some(code) = idempotency_key.as_deref().and_then(idempotency_key_field) {
        fields.push(FieldError::new("idempotencyKey", code));
    }
    if !fields.is_empty() {
        let error = malformed("requestId or idempotencyKey has a bad format").with_fields(fields);
        return Err(reject(reply_id, error));
    }

    Ok(Request {
        request_id,
        idempotency_key,
        call,
    })
}

fn reject(request_id: Option<String>, error: ErrorBody) -> RequestRejection {
    RequestRejection {
        request_id,
        error: Box::new(error),
    }
}

fn malformed(message: &str) -> ErrorBody {
    ErrorBody::new(&request::MALFORMED, message)
}

/// Decodes one raw part, or `None` when it has the wrong shape.
fn decode_raw<'a, T: Deserialize<'a>>(raw: &'a RawValue) -> Option<T> {
    serde_json::from_str(raw.get()).ok()
}

/// [`decode_raw`] for a part that must be a JSON object.
fn decode_object<'a, T: Deserialize<'a>>(raw: &'a RawValue) -> Option<T> {
    if is_object(raw.get().as_bytes()) {
        decode_raw(raw)
    } else {
        None
    }
}

/// Whether an unknown method name is safe to echo in `details.method`.
fn is_echoable(method: &str) -> bool {
    !method.is_empty()
        && method.len() <= METHOD_ECHO_MAX
        && method.bytes().all(|b| (0x21..=0x7e).contains(&b))
}

/// Params of a method without params: `content` must be absent or null.
pub(crate) fn decode_unit(content: Option<&RawValue>) -> Result<(), Vec<FieldError>> {
    match content {
        None => Ok(()),
        Some(_) => Err(vec![FieldError::new(PARAMS_ROOT, FieldCode::InvalidValue)]),
    }
}

/// Params of type `P` from the raw `content`, which must be a JSON object,
/// then [`Validate`].
pub(crate) fn decode_params<P: DeserializeOwned + Validate>(
    content: Option<&RawValue>,
) -> Result<P, Vec<FieldError>> {
    let Some(content) = content else {
        return Err(vec![FieldError::new(PARAMS_ROOT, FieldCode::Required)]);
    };
    if !is_object(content.get().as_bytes()) {
        return Err(vec![FieldError::new(PARAMS_ROOT, FieldCode::InvalidValue)]);
    }
    let mut deserializer = serde_json::Deserializer::from_str(content.get());
    match serde_path_to_error::deserialize::<_, P>(&mut deserializer) {
        Ok(params) => {
            let fields = params.validate();
            if fields.is_empty() {
                Ok(params)
            } else {
                Err(fields)
            }
        }
        Err(err) => Err(vec![FieldError::new(
            err.path().to_string(),
            FieldCode::InvalidValue,
        )]),
    }
}

/// One daemon frame as a Rust client reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodedFrame {
    /// The reply to one request.
    Response(Response),
    /// The error for one request.
    Error(ErrorFrame),
    /// An event of a kind this build knows.
    Event(Event),
    /// An event of a kind this build does not know. Its sequence still
    /// advances the cursor.
    SkippedEvent {
        /// The subscription the event was delivered for.
        subscription_id: u32,
        /// The event's sequence.
        sequence: Sequence,
        /// The unknown kind, such as `instance.paused`.
        kind: String,
    },
}

/// The frame head with `content` kept raw.
#[derive(Deserialize)]
struct RawFrame<'a> {
    #[serde(rename = "type")]
    frame_type: String,
    #[serde(borrow)]
    content: &'a RawValue,
}

/// Decodes one daemon frame in three stages: the frame type, then for an
/// event its [`EventProbe`], then the full value for a kind this build
/// knows.
///
/// # Errors
///
/// Returns `ipc.protocol_violation` when the frame nests too deep, the frame
/// or its content is not a JSON object, the frame is not a daemon frame, or
/// its content does not decode. The error is boxed like every `Result`
/// that carries an [`ErrorBody`].
pub fn decode_server_frame(bytes: &[u8]) -> Result<DecodedFrame, Box<ErrorBody>> {
    if exceeds_depth(bytes, MAX_JSON_DEPTH) {
        return Err(violation("daemon frame nests deeper than the json limit"));
    }
    let frame: RawFrame<'_> = serde_json::from_slice(bytes)
        .ok()
        .filter(|_| is_object(bytes))
        .ok_or_else(|| violation("daemon frame has no type and content"))?;
    let content = frame.content.get();
    if !is_object(content.as_bytes()) {
        return Err(violation("daemon frame content is not a json object"));
    }
    match frame.frame_type.as_str() {
        "response" => serde_json::from_str(content)
            .map(DecodedFrame::Response)
            .map_err(|_| violation("daemon response does not decode")),
        "error" => serde_json::from_str(content)
            .map(DecodedFrame::Error)
            .map_err(|_| violation("daemon error frame does not decode")),
        "event" => {
            let probe: EventProbe = serde_json::from_str(content)
                .map_err(|_| violation("daemon event has no subscription, sequence, or kind"))?;
            if !EventKind::KINDS.contains(&probe.kind.r#type.as_str()) {
                return Ok(DecodedFrame::SkippedEvent {
                    subscription_id: probe.subscription_id,
                    sequence: probe.sequence,
                    kind: probe.kind.r#type,
                });
            }
            serde_json::from_str(content)
                .map(DecodedFrame::Event)
                .map_err(|_| violation("daemon event does not decode"))
        }
        _ => Err(violation("daemon frame type is not known")),
    }
}

fn violation(message: &str) -> Box<ErrorBody> {
    Box::new(ErrorBody::new(&ipc::PROTOCOL_VIOLATION, message))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codes::{ErrorSpec, request};
    use crate::error::OMITTED_DETAIL;
    use crate::events::{EventCursor, SUBSCRIBE_KINDS_MAX, SubscribeParams};
    use crate::handshake::ClientKind;
    use crate::limits::{CONTROL_FRAME_MAX, FIELD_ERRORS_MAX};
    use crate::message::ServerFrame;

    const HANDSHAKE: &str = r#"{"requestId":"r-1","idempotencyKey":null,"call":{"type":"daemon.handshake","content":{"protocol":{"major":1,"minor":0},"client":{"kind":"cli","version":"0.1.0","pid":7},"contractDigest":"0000000000000000000000000000000000000000000000000000000000000000"}}}"#;

    fn rejected(payload: &str) -> RequestRejection {
        decode_request(payload.as_bytes()).unwrap_err()
    }

    fn assert_code(rejection: &RequestRejection, spec: &ErrorSpec) {
        assert_eq!(rejection.error.code(), spec.code, "{rejection:?}");
    }

    #[test]
    fn a_valid_handshake_decodes() {
        let request = decode_request(HANDSHAKE.as_bytes()).unwrap();
        assert_eq!(request.request_id, "r-1");
        let Call::DaemonHandshake(params) = request.call else {
            panic!("expected a handshake");
        };
        assert_eq!(params.client.kind, ClientKind::Cli);
    }

    #[test]
    fn content_before_type_decodes_the_same() {
        let request = decode_request(
            br#"{"call":{"content":{"subscriptionId":3},"type":"events.unsubscribe"},"requestId":"a"}"#,
        )
        .unwrap();
        assert_eq!(request.call.method(), "events.unsubscribe");
    }

    #[test]
    fn depth_33_is_too_deep_and_keeps_the_request_id() {
        let deep = format!(
            r#"{{"requestId":"r-9","call":{{"type":"events.subscribe","content":{}{}}}}}"#,
            "[".repeat(31),
            "]".repeat(31)
        );
        let rejection = rejected(&deep);
        assert_code(&rejection, &request::TOO_DEEP);
        assert_eq!(rejection.request_id.as_deref(), Some("r-9"));
        assert_eq!(rejection.error.detail("max"), Some("32"));
    }

    #[test]
    fn brackets_inside_strings_do_not_count_toward_depth() {
        let text = format!(r#"{{"a":"{}\"{}"}}"#, "[".repeat(40), "{".repeat(40));
        assert!(!exceeds_depth(text.as_bytes(), 2));
        assert!(exceeds_depth(b"[[[", 2));
        assert!(!exceeds_depth(b"[[]]", 2));
    }

    #[test]
    fn broken_json_and_wrong_shapes_are_malformed() {
        for payload in [
            "{",
            "[]",
            "\"x\"",
            r#"{"call":{"type":"daemon.get_info"}}"#,
            r#"{"requestId":5,"call":{"type":"daemon.get_info"}}"#,
            r#"{"requestId":"a"}"#,
            r#"{"requestId":"a","call":{"content":null}}"#,
            r#"{"requestId":"a","call":"daemon.get_info"}"#,
            r#"{"requestId":"a","idempotencyKey":7,"call":{"type":"daemon.get_info"}}"#,
        ] {
            assert_code(&rejected(payload), &request::MALFORMED);
        }
    }

    #[test]
    fn a_malformed_request_keeps_a_valid_request_id() {
        let rejection = rejected(r#"{"requestId":"r-2","call":7}"#);
        assert_eq!(rejection.request_id.as_deref(), Some("r-2"));
        let rejection = rejected(r#"{"requestId":"bad id","call":7}"#);
        assert_eq!(rejection.request_id, None);
    }

    #[test]
    fn bad_request_id_and_idempotency_key_formats_are_malformed_with_fields() {
        let rejection = rejected(
            r#"{"requestId":"has space","idempotencyKey":"","call":{"type":"daemon.get_info"}}"#,
        );
        assert_code(&rejection, &request::MALFORMED);
        assert_eq!(rejection.request_id, None);
        let fields: Vec<_> = rejection
            .error
            .fields()
            .iter()
            .map(|f| (f.path(), f.code()))
            .collect();
        assert_eq!(
            fields,
            [
                ("requestId", "invalid_format"),
                ("idempotencyKey", "too_short")
            ]
        );
        let long = format!(
            r#"{{"requestId":"{}","call":{{"type":"daemon.get_info"}}}}"#,
            "a".repeat(65)
        );
        assert_eq!(rejected(&long).error.fields()[0].code(), "too_long");
    }

    #[test]
    fn an_unknown_method_is_unsupported_and_echoes_a_safe_name() {
        let rejection = rejected(r#"{"requestId":"a","call":{"type":"instance.explode"}}"#);
        assert_code(&rejection, &request::UNKNOWN_METHOD);
        assert_eq!(rejection.error.detail("method"), Some("instance.explode"));
        let long = format!(
            r#"{{"requestId":"a","call":{{"type":"{}"}}}}"#,
            "x".repeat(65)
        );
        assert_eq!(rejected(&long).error.detail("method"), None);
    }

    #[test]
    fn a_type_mismatch_names_the_field_path_but_never_the_value() {
        let payload = r#"{"requestId":"a","call":{"content":{"since":{"serverInstanceId":"secret-sentinel","sequence":1}},"type":"events.subscribe"}}"#;
        let rejection = rejected(payload);
        assert_code(&rejection, &request::INVALID_PARAMS);
        assert_eq!(rejection.error.detail("method"), Some("events.subscribe"));
        let field = &rejection.error.fields()[0];
        assert_eq!(
            (field.path(), field.code()),
            ("since.serverInstanceId", "invalid_value")
        );
        let text = serde_json::to_string(&rejection.into_frame()).unwrap();
        assert!(!text.contains("secret-sentinel"), "{text}");
    }

    #[test]
    fn validate_failures_are_invalid_params_with_every_field() {
        let payload = r#"{"requestId":"a","call":{"type":"events.subscribe","content":{"kinds":["ok.kind","NOPE","x"]}}}"#;
        let rejection = rejected(payload);
        assert_code(&rejection, &request::INVALID_PARAMS);
        let paths: Vec<_> = rejection
            .error
            .fields()
            .iter()
            .map(FieldError::path)
            .collect();
        assert_eq!(paths, ["kinds[1]", "kinds[2]"]);
    }

    #[test]
    fn unit_methods_take_no_content_and_params_methods_require_it() {
        assert!(
            decode_request(
                br#"{"requestId":"a","call":{"type":"daemon.get_info","content":null}}"#
            )
            .is_ok()
        );
        let rejection =
            rejected(r#"{"requestId":"a","call":{"type":"daemon.get_info","content":{}}}"#);
        assert_code(&rejection, &request::INVALID_PARAMS);
        assert_eq!(rejection.error.fields()[0].path(), PARAMS_ROOT);
        let rejection = rejected(r#"{"requestId":"a","call":{"type":"events.unsubscribe"}}"#);
        assert_eq!(rejection.error.fields()[0].code(), "required");
    }

    #[test]
    fn handshake_major_reads_a_major_even_when_other_params_are_broken() {
        let payload = br#"{"requestId":7,"call":{"type":"daemon.handshake","content":{"protocol":{"major":2,"minor":"x"},"client":12}}}"#;
        assert_eq!(handshake_major(payload), Some(2));
        assert_eq!(handshake_major(HANDSHAKE.as_bytes()), Some(1));
        assert_eq!(
            handshake_major(
                br#"{"call":{"type":"daemon.get_info","content":{"protocol":{"major":2}}}}"#
            ),
            None
        );
        assert_eq!(handshake_major(b"{"), None);
    }

    #[test]
    fn a_decoded_request_encodes_and_decodes_to_the_same_value() {
        let request = decode_request(HANDSHAKE.as_bytes()).unwrap();
        let again = decode_request(&serde_json::to_vec(&request).unwrap()).unwrap();
        assert_eq!(again, request);
        let params = SubscribeParams {
            since: Some(EventCursor {
                server_instance_id: uuid::Uuid::nil(),
                sequence: Sequence::new(4).unwrap(),
            }),
            ..SubscribeParams::default()
        };
        let request = Request {
            request_id: "b".to_owned(),
            idempotency_key: Some("key-1".to_owned()),
            call: Call::EventsSubscribe(params),
        };
        let again = decode_request(&serde_json::to_vec(&request).unwrap()).unwrap();
        assert_eq!(again, request);
    }

    #[test]
    fn unknown_event_kinds_become_skipped_events() {
        let frame = br#"{"type":"event","content":{"subscriptionId":2,"sequence":9,"at":"2026-10-18T03:04:05.123Z","resource":null,"revision":null,"kind":{"type":"instance.paused","content":{"why":1}}}}"#;
        assert_eq!(
            decode_server_frame(frame).unwrap(),
            DecodedFrame::SkippedEvent {
                subscription_id: 2,
                sequence: Sequence::new(9).unwrap(),
                kind: "instance.paused".to_owned(),
            }
        );
    }

    #[test]
    fn bad_daemon_frames_are_protocol_violations() {
        for frame in [
            &b"{"[..],
            br#"{"type":"hello","content":{}}"#,
            br#"{"type":"response","content":{"requestId":"a"}}"#,
            br#"{"type":"event","content":{"subscriptionId":1,"sequence":1,"at":"x","kind":{"type":"events.gap","content":{"reason":"overflow"}}}}"#,
        ] {
            assert_eq!(decode_server_frame(frame).unwrap_err().code(), "ipc.protocol_violation");
        }
    }

    #[test]
    fn a_huge_kinds_list_still_yields_a_small_error_frame() {
        // 349 000 empty kinds fill a request just under the 1 MiB frame
        // limit; each one is a bad kind filter.
        let kinds = vec![r#""""#; 349_000].join(",");
        let payload = format!(
            r#"{{"requestId":"r-1","call":{{"type":"events.subscribe","content":{{"kinds":[{kinds}]}}}}}}"#
        );
        assert!(
            payload.len() < CONTROL_FRAME_MAX as usize,
            "{}",
            payload.len()
        );
        let rejection = rejected(&payload);
        assert_code(&rejection, &request::INVALID_PARAMS);
        let fields = rejection.error.fields();
        assert_eq!(fields.len(), FIELD_ERRORS_MAX);
        assert_eq!((fields[0].path(), fields[0].code()), ("kinds", "too_many"));
        assert_eq!(fields[1].path(), "kinds[0]");
        assert_eq!(fields[FIELD_ERRORS_MAX - 1].path(), "kinds[30]");
        assert_eq!(rejection.error.detail(OMITTED_DETAIL), Some("1"));
        let frame = serde_json::to_vec(&ServerFrame::Error(rejection.into_frame())).unwrap();
        assert!(frame.len() < CONTROL_FRAME_MAX as usize, "{}", frame.len());
        assert!(frame.len() < 8 * 1024, "{}", frame.len());
    }

    #[test]
    fn a_kinds_list_just_over_the_limit_reports_too_many_and_each_bad_kind() {
        let kinds = vec![r#""NOPE""#; SUBSCRIBE_KINDS_MAX + 3].join(",");
        let payload = format!(
            r#"{{"requestId":"r-1","call":{{"type":"events.subscribe","content":{{"kinds":[{kinds}]}}}}}}"#
        );
        let rejection = rejected(&payload);
        assert_eq!(rejection.error.fields().len(), FIELD_ERRORS_MAX);
        assert_eq!(rejection.error.detail(OMITTED_DETAIL), Some("1"));
        assert_eq!(rejection.error.detail("method"), Some("events.subscribe"));
    }

    #[test]
    fn a_positional_array_is_malformed_wherever_an_object_is_expected() {
        for payload in [
            r#"["r-1",null,{"type":"daemon.get_info"}]"#,
            r#"  ["r-1"]"#,
            r#"{"requestId":"r-1","call":["daemon.get_info",null]}"#,
            r#"{"requestId":"r-1","call":[["daemon.get_info"]]}"#,
        ] {
            assert_code(&rejected(payload), &request::MALFORMED);
        }
        let rejection = rejected(r#"{"requestId":"r-1","call":["daemon.get_info"]}"#);
        assert_eq!(rejection.request_id.as_deref(), Some("r-1"));
        assert_eq!(
            rejection.error.message(),
            "request has no call object with a type string"
        );
        let rejection = rejected(r#"["r-1",null,{"type":"daemon.get_info"}]"#);
        assert_eq!(rejection.request_id, None);
        assert_eq!(rejection.error.message(), "request is not a json object");
    }

    #[test]
    fn positional_params_are_invalid_params_at_the_root() {
        for content in [r#"[3]"#, r#"[null,[],null]"#, r#""x""#, "7"] {
            let payload = format!(
                r#"{{"requestId":"a","call":{{"type":"events.subscribe","content":{content}}}}}"#
            );
            let rejection = rejected(&payload);
            assert_code(&rejection, &request::INVALID_PARAMS);
            let field = &rejection.error.fields()[0];
            assert_eq!((field.path(), field.code()), (PARAMS_ROOT, "invalid_value"));
        }
        let payload = r#"{"requestId":"a","call":{"type":"events.unsubscribe","content":[3]}}"#;
        assert_code(&rejected(payload), &request::INVALID_PARAMS);
        assert!(
            decode_request(
                br#"{"requestId":"a","call":{"type":"events.unsubscribe","content": {"subscriptionId":3}}}"#
            )
            .is_ok()
        );
    }

    #[test]
    fn lenient_readers_read_nothing_from_a_positional_array() {
        assert_eq!(request_id(br#"["r-1"]"#), None);
        assert_eq!(
            request_id(br#" {"requestId":"r-1"}"#).as_deref(),
            Some("r-1")
        );
        assert_eq!(
            handshake_major(br#"[{"type":"daemon.handshake","content":{"protocol":{"major":2}}}]"#),
            None
        );
        assert!(is_object(b" \n\t{"));
        assert!(!is_object(b"[{}]"));
        assert!(!is_object(b""));
    }

    #[test]
    fn positional_daemon_frames_and_contents_are_protocol_violations() {
        for frame in [
            &br#"["response",{"requestId":"a","reply":{"type":"events.unsubscribe"}}]"#[..],
            br#"{"type":"response","content":["a",{"type":"events.unsubscribe"}]}"#,
            br#"{"type":"error","content":[null,{"code":"x.y","category":"failed","message":"m","retryable":false}]}"#,
            br#"{"type":"event","content":[1,1,"2026-10-18T03:04:05.123Z",null,null,{"type":"x.y"}]}"#,
        ] {
            assert_eq!(
                decode_server_frame(frame).unwrap_err().code(),
                "ipc.protocol_violation"
            );
        }
        assert!(
            decode_server_frame(
                br#" {"type":"response","content":{"requestId":"a","reply":{"type":"events.unsubscribe"}}}"#
            )
            .is_ok()
        );
    }

    #[test]
    fn field_codes_have_distinct_snake_case_values_and_fixed_texts() {
        let values: std::collections::HashSet<_> =
            FieldCode::ALL.iter().map(|c| c.as_str()).collect();
        assert_eq!(values.len(), FieldCode::ALL.len());
        assert_eq!(
            FieldCode::InvalidValue.message(),
            "value does not match the expected type or format"
        );
    }
}
