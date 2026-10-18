//! Limits every peer enforces, sent to clients in `HandshakeAccepted`.
//!
//! Feature 64 may retune these numbers through an update to spec 0006.
//! Raising a limit is a minor bump; lowering one is a major bump.

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
}
