//! Limits every peer enforces, sent to clients in `HandshakeAccepted`, and
//! [`CONTRACT_LIMITS`], every limit of the contract by name.
//!
//! Feature 64 may retune these numbers through an update to spec 0006.
//! Every limit is part of the contract digest, so changing one needs a new
//! protocol version: raising a limit is a minor bump; lowering one is a
//! major bump.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use typeshare::typeshare;

/// Largest control frame payload, in bytes (1 MiB).
pub const CONTROL_FRAME_MAX: u32 = 1_048_576;
/// Largest log batch payload, in bytes (64 KiB). The log batch builder of
/// feature 27 keeps to it.
pub const LOG_BATCH_MAX: u32 = 65_536;
/// Deepest JSON nesting a request or a daemon frame may have.
pub const MAX_JSON_DEPTH: u32 = 32;
/// Most requests one connection may have in flight at once.
pub const MAX_IN_FLIGHT_REQUESTS: u32 = 32;
/// Most event subscriptions one connection may hold at once.
pub const MAX_SUBSCRIPTIONS: u32 = 8;
/// Events a subscriber may fall behind before it receives an overflow gap.
pub const EVENT_BUFFER_LEN: u32 = 1024;
/// Recent events the daemon keeps to replay after a cursor.
pub const EVENT_REPLAY_LEN: u32 = 4096;
/// Longest `requestId`, in bytes.
pub const REQUEST_ID_MAX: usize = 64;
/// Longest `idempotencyKey`, in bytes.
pub const IDEMPOTENCY_KEY_MAX: usize = 128;
/// Largest integer on the wire: 2^53 − 1, the largest one JavaScript and
/// `Double` readers hold exactly.
pub const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
/// Most `fields` one error carries; [`ErrorBody`](crate::ErrorBody) keeps
/// the first ones and counts the rest in its `omitted` detail.
pub const FIELD_ERRORS_MAX: usize = 32;
/// Longest `fields[].path`, in characters; longer paths are cut on a
/// character boundary.
pub const FIELD_PATH_MAX: usize = 256;

/// Every limit of the contract by its camelCase name, sorted by name: the
/// wire limits above, the lengths and counts each `Validate` rule checks,
/// and the caps on what an error echoes. `cargo xtask codegen` hashes this
/// list into the contract digest, so a changed limit needs a new protocol
/// version. A spec that adds a limit to `stackly-protocol` adds it here; a
/// test fails for a module level constant whose name holds `MAX` or `MIN`
/// or ends in `_LEN` or `_DEFAULT` that this list does not name.
pub const CONTRACT_LIMITS: &[(&str, u64)] = &[
    (
        "clientVersionMax",
        crate::handshake::CLIENT_VERSION_MAX as u64,
    ),
    (
        "contractDigestLen",
        crate::handshake::CONTRACT_DIGEST_LEN as u64,
    ),
    ("controlFrameMax", CONTROL_FRAME_MAX as u64),
    ("eventBufferLen", EVENT_BUFFER_LEN as u64),
    ("eventReplayLen", EVENT_REPLAY_LEN as u64),
    ("fieldErrorsMax", FIELD_ERRORS_MAX as u64),
    ("fieldPathMax", FIELD_PATH_MAX as u64),
    ("frameHeaderLen", crate::frame::HEADER_LEN as u64),
    ("idempotencyKeyMax", IDEMPOTENCY_KEY_MAX as u64),
    ("logBatchMax", LOG_BATCH_MAX as u64),
    ("maxInFlightRequests", MAX_IN_FLIGHT_REQUESTS as u64),
    ("maxJsonDepth", MAX_JSON_DEPTH as u64),
    ("maxSafeInteger", MAX_SAFE_INTEGER),
    ("maxSecretLen", crate::secret::MAX_SECRET_LEN as u64),
    ("maxSubscriptions", MAX_SUBSCRIPTIONS as u64),
    ("messageMax", crate::error::MESSAGE_MAX as u64),
    ("methodEchoMax", crate::json::METHOD_ECHO_MAX as u64),
    (
        "operationCursorMax",
        crate::operation::OPERATION_CURSOR_MAX as u64,
    ),
    (
        "operationListLimitDefault",
        crate::operation::OPERATION_LIST_LIMIT_DEFAULT as u64,
    ),
    (
        "operationListLimitMax",
        crate::operation::OPERATION_LIST_LIMIT_MAX as u64,
    ),
    (
        "operationListLimitMin",
        crate::operation::OPERATION_LIST_LIMIT_MIN as u64,
    ),
    (
        "operationPageBytesMax",
        crate::operation::OPERATION_PAGE_BYTES_MAX as u64,
    ),
    ("requestIdMax", REQUEST_ID_MAX as u64),
    (
        "secretReferenceMax",
        crate::secret::SECRET_REFERENCE_MAX as u64,
    ),
    ("secretSegmentMax", crate::secret::SEGMENT_MAX as u64),
    ("secretSegmentsMax", crate::secret::SEGMENTS_MAX as u64),
    ("secretSegmentsMin", crate::secret::SEGMENTS_MIN as u64),
    (
        "subscribeKindsMax",
        crate::events::SUBSCRIBE_KINDS_MAX as u64,
    ),
];

/// The limits a daemon enforces, as sent in `HandshakeAccepted`.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Limits {
    /// Largest control frame payload, in bytes.
    pub max_frame_bytes: u32,
    /// Largest log batch payload, in bytes.
    pub max_log_batch_bytes: u32,
    /// Deepest JSON nesting accepted.
    pub max_json_depth: u32,
    /// Most requests in flight per connection.
    pub max_in_flight_requests: u32,
    /// Most subscriptions per connection.
    pub max_subscriptions: u32,
    /// Events a subscriber may fall behind before an overflow gap.
    pub event_buffer_len: u32,
    /// Recent events kept for replay.
    pub event_replay_len: u32,
}

impl Limits {
    /// The limits of this build, taken from the constants in this module.
    pub const CURRENT: Self = Self {
        max_frame_bytes: CONTROL_FRAME_MAX,
        max_log_batch_bytes: LOG_BATCH_MAX,
        max_json_depth: MAX_JSON_DEPTH,
        max_in_flight_requests: MAX_IN_FLIGHT_REQUESTS,
        max_subscriptions: MAX_SUBSCRIPTIONS,
        event_buffer_len: EVENT_BUFFER_LEN,
        event_replay_len: EVENT_REPLAY_LEN,
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn max_safe_integer_is_two_to_the_fifty_third_minus_one() {
        assert_eq!(MAX_SAFE_INTEGER, (1_u64 << 53) - 1);
    }

    #[test]
    fn a_log_batch_always_fits_in_a_control_frame() {
        const { assert!(LOG_BATCH_MAX <= CONTROL_FRAME_MAX) };
    }

    #[test]
    fn the_replay_ring_holds_more_events_than_a_subscriber_may_lag() {
        const { assert!(EVENT_REPLAY_LEN >= EVENT_BUFFER_LEN) };
    }

    #[test]
    fn contract_limits_have_unique_camel_case_names_in_sorted_order() {
        let names: Vec<&str> = CONTRACT_LIMITS.iter().map(|(name, _)| *name).collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(names, sorted, "names are unique and sorted");
        for name in names {
            assert!(
                name.starts_with(|c: char| c.is_ascii_lowercase())
                    && name.chars().all(|c| c.is_ascii_alphanumeric()),
                "{name} is not camelCase"
            );
        }
    }

    /// Every module of the crate whose constants can be contract limits:
    /// all but the fuzz harnesses and the generated digest.
    const SOURCES: [(&str, &str); 14] = [
        ("codes", include_str!("codes.rs")),
        ("daemon", include_str!("daemon.rs")),
        ("error", include_str!("error.rs")),
        ("events", include_str!("events.rs")),
        ("fingerprint", include_str!("fingerprint.rs")),
        ("frame", include_str!("frame.rs")),
        ("handshake", include_str!("handshake.rs")),
        ("json", include_str!("json.rs")),
        ("limits", include_str!("limits.rs")),
        ("message", include_str!("message.rs")),
        ("operation", include_str!("operation.rs")),
        ("secret", include_str!("secret.rs")),
        ("version", include_str!("version.rs")),
        ("wire", include_str!("wire.rs")),
    ];
    /// Modules [`SOURCES`] leaves out on purpose.
    const NOT_SCANNED: [&str; 2] = ["contract_digest", "fuzzing"];

    /// Whether `text` holds `word` with no identifier character around it.
    fn contains_word(text: &str, word: &str) -> bool {
        let ident = |c: char| c.is_ascii_alphanumeric() || c == '_';
        text.match_indices(word).any(|(at, _)| {
            !text[..at].ends_with(ident) && !text[at + word.len()..].starts_with(ident)
        })
    }

    #[test]
    fn every_module_of_the_crate_is_scanned_for_limits() {
        let mut modules: Vec<&str> = include_str!("lib.rs")
            .lines()
            .filter_map(|line| {
                let line = line.strip_prefix("pub ").unwrap_or(line);
                line.strip_prefix("mod ")?.strip_suffix(';')
            })
            .collect();
        modules.sort_unstable();
        let mut known: Vec<&str> = SOURCES.iter().map(|(name, _)| *name).collect();
        known.extend(NOT_SCANNED);
        known.sort_unstable();
        assert_eq!(
            modules, known,
            "add the new module to SOURCES or NOT_SCANNED"
        );
    }

    #[test]
    fn every_limit_constant_of_the_crate_is_a_contract_limit() {
        let limits = include_str!("limits.rs");
        let start = limits.find("pub const CONTRACT_LIMITS").unwrap();
        let table = &limits[start..start + limits[start..].find("];").unwrap()];
        let mut checked = 0;
        for (module, source) in SOURCES {
            for line in source.lines() {
                let Some(rest) = ["pub const ", "pub(crate) const ", "const "]
                    .iter()
                    .find_map(|prefix| line.strip_prefix(prefix))
                else {
                    continue;
                };
                let Some((name, _)) = rest.split_once(':') else {
                    continue;
                };
                let is_limit = name.contains("MAX")
                    || name.contains("MIN")
                    || name.ends_with("_LEN")
                    || name.ends_with("_DEFAULT");
                if is_limit && name != "CONTRACT_LIMITS" {
                    assert!(
                        contains_word(table, name),
                        "{module}::{name} looks like a limit but CONTRACT_LIMITS does not name it"
                    );
                    checked += 1;
                }
            }
        }
        assert_eq!(
            checked,
            CONTRACT_LIMITS.len(),
            "every entry names one constant"
        );
    }

    #[test]
    fn contract_limits_hold_every_handshake_limit() {
        let value = |name: &str| {
            CONTRACT_LIMITS
                .iter()
                .find(|(limit, _)| *limit == name)
                .map(|(_, value)| *value)
                .unwrap()
        };
        let limits = Limits::CURRENT;
        assert_eq!(value("controlFrameMax"), u64::from(limits.max_frame_bytes));
        assert_eq!(value("logBatchMax"), u64::from(limits.max_log_batch_bytes));
        assert_eq!(value("maxJsonDepth"), u64::from(limits.max_json_depth));
        assert_eq!(
            value("maxInFlightRequests"),
            u64::from(limits.max_in_flight_requests)
        );
        assert_eq!(
            value("maxSubscriptions"),
            u64::from(limits.max_subscriptions)
        );
        assert_eq!(value("eventBufferLen"), u64::from(limits.event_buffer_len));
        assert_eq!(value("eventReplayLen"), u64::from(limits.event_replay_len));
    }
}
