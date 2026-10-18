//! Frames and the method table: [`Request`] from clients, [`ServerFrame`]
//! from the daemon, and the [`Call`] and [`Reply`] enums with one variant
//! per method.
//!
//! The frozen parts never change across majors: the `Request` envelope
//! (`requestId`, `idempotencyKey`, `call.type`, `call.content`) and the
//! `error` variant of `ServerFrame`.
//!
//! To add a method: add a variant to [`Call`] and to [`Reply`] with the same
//! name and the same `#[serde(rename)]`, then add one line to the
//! `methods!` table below. A variant missing from the table does not
//! compile, and the tests check that each table name is the variant's serde
//! tag. typeshare cannot see items a macro expands to, so the two enums stay
//! written by hand.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
use typeshare::typeshare;

use crate::daemon::{Capabilities, DaemonHealth, DaemonInfo};
use crate::error::{ErrorBody, FieldError};
use crate::events::{Event, SubscribeParams, Subscribed, UnsubscribeParams};
use crate::handshake::{HandshakeAccepted, HandshakeParams};
use crate::json;

/// The only frame a client sends.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    /// 1 to 64 bytes of `[A-Za-z0-9._:-]`, chosen by the client and unique
    /// on the connection while the request runs. `stackly-ipc` uses a
    /// lowercase UUID v7.
    pub request_id: String,
    /// 1 to 128 bytes of printable ASCII without spaces. Spec 0007 decides
    /// what it means.
    #[serde(default)]
    pub idempotency_key: Option<String>,
    /// The method and its params.
    pub call: Call,
}

/// One variant per method, tagged by the method name. Methods without
/// params are unit variants, written without `content`.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", content = "content")]
pub enum Call {
    /// The first request on every connection.
    #[serde(rename = "daemon.handshake")]
    DaemonHandshake(HandshakeParams),
    /// Who the daemon is and where it keeps things.
    #[serde(rename = "daemon.get_info")]
    DaemonGetInfo,
    /// The daemon's health and counters.
    #[serde(rename = "daemon.get_health")]
    DaemonGetHealth,
    /// Platform capabilities, methods, and event kinds the daemon knows.
    #[serde(rename = "daemon.get_capabilities")]
    DaemonGetCapabilities,
    /// Starts an event subscription.
    #[serde(rename = "events.subscribe")]
    EventsSubscribe(SubscribeParams),
    /// Ends an event subscription.
    #[serde(rename = "events.unsubscribe")]
    EventsUnsubscribe(UnsubscribeParams),
}

/// One variant per method, with the same names and tags as [`Call`].
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", content = "content")]
pub enum Reply {
    /// The handshake was accepted.
    #[serde(rename = "daemon.handshake")]
    DaemonHandshake(HandshakeAccepted),
    /// Who the daemon is.
    #[serde(rename = "daemon.get_info")]
    DaemonGetInfo(DaemonInfo),
    /// The daemon's health.
    #[serde(rename = "daemon.get_health")]
    DaemonGetHealth(DaemonHealth),
    /// What the daemon supports.
    #[serde(rename = "daemon.get_capabilities")]
    DaemonGetCapabilities(Capabilities),
    /// The subscription started; its events follow.
    #[serde(rename = "events.subscribe")]
    EventsSubscribe(Subscribed),
    /// The subscription ended.
    #[serde(rename = "events.unsubscribe")]
    EventsUnsubscribe,
}

/// The method table: one line per method, `name => Variant` or
/// `name => Variant(Params)`. It gives `Call::METHODS`, `Call::method`,
/// `Reply::method`, and the per method params decoder.
macro_rules! methods {
    (@decode $content:ident, $variant:ident) => {
        json::decode_unit($content).map(|()| Self::$variant)
    };
    (@decode $content:ident, $variant:ident, $params:ty) => {
        json::decode_params::<$params>($content).map(Self::$variant)
    };
    ($($name:literal => $variant:ident $(($params:ty))?;)*) => {
        impl Call {
            /// Every method this build knows, in table order.
            pub const METHODS: &[&str] = &[$($name),*];

            /// The method name, such as `daemon.get_info`.
            #[must_use]
            pub const fn method(&self) -> &'static str {
                match self {
                    $(Self::$variant { .. } => $name,)*
                }
            }

            /// Decodes the params of `method` from the raw `content`, then
            /// runs [`Validate`](crate::json::Validate). `None` means the
            /// method is not in the table.
            pub(crate) fn decode(
                method: &str,
                content: Option<&RawValue>,
            ) -> Option<Result<Self, Vec<FieldError>>> {
                match method {
                    $($name => Some(methods!(@decode content, $variant $(, $params)?)),)*
                    _ => None,
                }
            }
        }

        impl Reply {
            /// The method this reply answers.
            #[must_use]
            pub const fn method(&self) -> &'static str {
                match self {
                    $(Self::$variant { .. } => $name,)*
                }
            }
        }
    };
}

methods! {
    "daemon.handshake" => DaemonHandshake(HandshakeParams);
    "daemon.get_info" => DaemonGetInfo;
    "daemon.get_health" => DaemonGetHealth;
    "daemon.get_capabilities" => DaemonGetCapabilities;
    "events.subscribe" => EventsSubscribe(SubscribeParams);
    "events.unsubscribe" => EventsUnsubscribe(UnsubscribeParams);
}

impl Reply {
    /// Whether this reply answers `call`. A reply with another tag is a
    /// protocol violation (`ipc.protocol_violation` on the client).
    #[must_use]
    pub fn answers(&self, call: &Call) -> bool {
        self.method() == call.method()
    }
}

/// Every frame the daemon sends. Adding a variant is a new major.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", content = "content", rename_all = "snake_case")]
pub enum ServerFrame {
    /// The reply to one request.
    Response(Response),
    /// The error for one request, or for a frame without a readable
    /// request.
    Error(ErrorFrame),
    /// One event for one subscription.
    Event(Event),
}

/// The reply to one request. Replies on one connection may arrive out of
/// order; clients match them by `requestId`.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Response {
    /// The request this answers.
    pub request_id: String,
    /// The reply, tagged like the request's call.
    pub reply: Reply,
}

/// The error for one request.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ErrorFrame {
    /// The request that failed, or null only when its `requestId` could not
    /// be read.
    #[serde(default)]
    pub request_id: Option<String>,
    /// The error.
    pub error: ErrorBody,
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn every_method_in_the_table_is_the_serde_tag_of_a_call_and_a_reply() {
        for method in Call::METHODS {
            let probe = serde_json::json!({ "type": method, "content": null });
            match serde_json::from_value::<Call>(probe.clone()) {
                Ok(call) => assert_eq!(call.method(), *method),
                Err(err) => assert!(
                    !err.to_string().contains("unknown variant"),
                    "{method}: {err}"
                ),
            }
            match serde_json::from_value::<Reply>(probe) {
                Ok(reply) => assert_eq!(reply.method(), *method),
                Err(err) => assert!(
                    !err.to_string().contains("unknown variant"),
                    "{method}: {err}"
                ),
            }
        }
    }

    #[test]
    fn method_names_are_unique_and_follow_the_area_dot_name_pattern() {
        let unique: HashSet<_> = Call::METHODS.iter().collect();
        assert_eq!(unique.len(), Call::METHODS.len());
        for method in Call::METHODS {
            assert!(
                crate::events::is_kind_filter(method) && !method.ends_with('.'),
                "{method}"
            );
        }
    }

    #[test]
    fn unit_calls_encode_without_content() {
        let json = serde_json::to_string(&Call::DaemonGetInfo).unwrap();
        assert_eq!(json, r#"{"type":"daemon.get_info"}"#);
    }

    #[test]
    fn a_reply_answers_only_the_call_with_its_tag() {
        assert!(
            Reply::EventsUnsubscribe.answers(&Call::EventsUnsubscribe(UnsubscribeParams {
                subscription_id: 1
            }))
        );
        assert!(!Reply::EventsUnsubscribe.answers(&Call::DaemonGetInfo));
    }
}
