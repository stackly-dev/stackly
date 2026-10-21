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
//! `methods!` table below, saying whether the daemon answers it while the
//! registry is in recovery mode (spec 0006 AC-18). A variant missing from the table does not
//! compile, and the tests check that each table name is the variant's serde
//! tag. typeshare cannot see items a macro expands to, so the two enums stay
//! written by hand.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
use typeshare::typeshare;

use crate::catalog::{GetProviderParams, ListProvidersReply, ProviderDetail};
use crate::daemon::{Capabilities, DaemonHealth, DaemonInfo};
use crate::error::{ErrorBody, FieldError};
use crate::events::{Event, SubscribeParams, Subscribed, UnsubscribeParams};
use crate::handshake::{HandshakeAccepted, HandshakeParams};
use crate::instance::{
    CreateInstanceParams, GetInstanceParams, InstanceActionAccepted, InstancePage, InstanceSummary,
    ListInstancesParams, StartInstanceParams, StopInstanceParams,
};
use crate::json;
use crate::operation::{
    AbandonOperationParams, AbandonReply, CancelOperationParams, CancelReply, GetOperationParams,
    ListOperationsParams, OperationDetail, OperationPage, OperationStats, OperationSummary,
    ResumeOperationParams,
};
use crate::secret::{
    DeleteSecretParams, DeleteSecretReply, ListSecretsParams, ListSecretsReply, RevealSecretParams,
    RevealSecretReply, SecretStatusParams, SecretStatusReply, SetSecretParams, SetSecretReply,
};
use crate::storage::CapacityReport;

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
    /// One operation with its steps and created items.
    #[serde(rename = "operation.get")]
    OperationGet(GetOperationParams),
    /// One page of operations, newest first.
    #[serde(rename = "operation.list")]
    OperationList(ListOperationsParams),
    /// Asks an operation to stop.
    #[serde(rename = "operation.cancel")]
    OperationCancel(CancelOperationParams),
    /// Queues an operation that needs recovery again.
    #[serde(rename = "operation.resume")]
    OperationResume(ResumeOperationParams),
    /// Cleans up what it can of an operation that needs recovery, then
    /// closes it.
    #[serde(rename = "operation.abandon")]
    OperationAbandon(AbandonOperationParams),
    /// The engine's counters and limits.
    #[serde(rename = "operation.get_stats")]
    OperationGetStats,
    /// Every catalog entry with the facts about the catalog.
    #[serde(rename = "catalog.list_providers")]
    CatalogListProviders,
    /// One catalog entry with its versions and artifacts.
    #[serde(rename = "catalog.get_provider")]
    CatalogGetProvider(GetProviderParams),
    /// The secret references, from the cached status.
    #[serde(rename = "secret.list")]
    SecretList(ListSecretsParams),
    /// One secret reference after a fresh probe.
    #[serde(rename = "secret.status")]
    SecretStatus(SecretStatusParams),
    /// Fills, generates, copies, or replaces the value behind a reference.
    #[serde(rename = "secret.set")]
    SecretSet(SetSecretParams),
    /// Reads the value behind a reference, recorded in the audit.
    #[serde(rename = "secret.reveal")]
    SecretReveal(RevealSecretParams),
    /// Deletes a reference and the value behind it.
    #[serde(rename = "secret.delete")]
    SecretDelete(DeleteSecretParams),
    /// Creates an instance and starts the operation that initializes it.
    #[serde(rename = "instance.create")]
    InstanceCreate(CreateInstanceParams),
    /// Starts an instance.
    #[serde(rename = "instance.start")]
    InstanceStart(StartInstanceParams),
    /// Stops an instance.
    #[serde(rename = "instance.stop")]
    InstanceStop(StopInstanceParams),
    /// One instance with its status.
    #[serde(rename = "instance.get")]
    InstanceGet(GetInstanceParams),
    /// One page of instances, retained ones left out.
    #[serde(rename = "instance.list")]
    InstanceList(ListInstancesParams),
    /// The space left on the volume that holds the data root.
    #[serde(rename = "storage.get_capacity")]
    StorageGetCapacity,
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
    /// The operation with its steps and created items.
    #[serde(rename = "operation.get")]
    OperationGet(Box<OperationDetail>),
    /// One page of operations.
    #[serde(rename = "operation.list")]
    OperationList(OperationPage),
    /// The operation after the cancel, and what the cancel did.
    #[serde(rename = "operation.cancel")]
    OperationCancel(Box<CancelReply>),
    /// The operation, queued again.
    #[serde(rename = "operation.resume")]
    OperationResume(Box<OperationSummary>),
    /// The closed operation and what was left over.
    #[serde(rename = "operation.abandon")]
    OperationAbandon(Box<AbandonReply>),
    /// The engine's counters and limits.
    #[serde(rename = "operation.get_stats")]
    OperationGetStats(OperationStats),
    /// Every catalog entry, in display order.
    #[serde(rename = "catalog.list_providers")]
    CatalogListProviders(ListProvidersReply),
    /// One catalog entry in detail.
    #[serde(rename = "catalog.get_provider")]
    CatalogGetProvider(Box<ProviderDetail>),
    /// The secret references and the store.
    #[serde(rename = "secret.list")]
    SecretList(ListSecretsReply),
    /// The reference after the probe, and the store.
    #[serde(rename = "secret.status")]
    SecretStatus(Box<SecretStatusReply>),
    /// The reference after the write, never the value.
    #[serde(rename = "secret.set")]
    SecretSet(Box<SetSecretReply>),
    /// The value; the one reply that carries one.
    #[serde(rename = "secret.reveal")]
    SecretReveal(RevealSecretReply),
    /// The reference that was deleted.
    #[serde(rename = "secret.delete")]
    SecretDelete(DeleteSecretReply),
    /// The operation that creates the instance.
    #[serde(rename = "instance.create")]
    InstanceCreate(Box<InstanceActionAccepted>),
    /// The operation that starts the instance, or that it already runs.
    #[serde(rename = "instance.start")]
    InstanceStart(Box<InstanceActionAccepted>),
    /// The operation that stops the instance, or that it already stopped.
    #[serde(rename = "instance.stop")]
    InstanceStop(Box<InstanceActionAccepted>),
    /// The instance.
    #[serde(rename = "instance.get")]
    InstanceGet(Box<InstanceSummary>),
    /// One page of instances.
    #[serde(rename = "instance.list")]
    InstanceList(Box<InstancePage>),
    /// The space left, read now.
    #[serde(rename = "storage.get_capacity")]
    StorageGetCapacity(CapacityReport),
}

/// The method table: one line per method, `name => Variant` or
/// `name => Variant(Params)`, then `recovery allowed` or `recovery refused`.
/// It gives `Call::METHODS`, `Call::method`, `Call::allowed_in_recovery`,
/// `Reply::method`, and the per method params decoder.
macro_rules! methods {
    (@recovery allowed) => {
        true
    };
    (@recovery refused) => {
        false
    };
    (@decode $content:ident, $variant:ident) => {
        json::decode_unit($content).map(|()| Self::$variant)
    };
    (@decode $content:ident, $variant:ident, $params:ty) => {
        json::decode_params::<$params>($content).map(Self::$variant)
    };
    ($($name:literal => $variant:ident $(($params:ty))?, recovery $recovery:ident;)*) => {
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

            /// Whether the daemon answers this method while the registry is
            /// in recovery mode. Every other method gets
            /// `registry.recovery_mode` before its handler runs (spec 0006
            /// AC-18, spec 0005).
            #[must_use]
            pub const fn allowed_in_recovery(&self) -> bool {
                match self {
                    $(Self::$variant { .. } => methods!(@recovery $recovery),)*
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
    "daemon.handshake" => DaemonHandshake(HandshakeParams), recovery allowed;
    "daemon.get_info" => DaemonGetInfo, recovery allowed;
    "daemon.get_health" => DaemonGetHealth, recovery allowed;
    "daemon.get_capabilities" => DaemonGetCapabilities, recovery allowed;
    "events.subscribe" => EventsSubscribe(SubscribeParams), recovery allowed;
    "events.unsubscribe" => EventsUnsubscribe(UnsubscribeParams), recovery allowed;
    "operation.get" => OperationGet(GetOperationParams), recovery refused;
    "operation.list" => OperationList(ListOperationsParams), recovery refused;
    "operation.cancel" => OperationCancel(CancelOperationParams), recovery refused;
    "operation.resume" => OperationResume(ResumeOperationParams), recovery refused;
    "operation.abandon" => OperationAbandon(AbandonOperationParams), recovery refused;
    "operation.get_stats" => OperationGetStats, recovery refused;
    "catalog.list_providers" => CatalogListProviders, recovery allowed;
    "catalog.get_provider" => CatalogGetProvider(GetProviderParams), recovery allowed;
    "secret.list" => SecretList(ListSecretsParams), recovery refused;
    "secret.status" => SecretStatus(SecretStatusParams), recovery refused;
    "secret.set" => SecretSet(SetSecretParams), recovery refused;
    "secret.reveal" => SecretReveal(RevealSecretParams), recovery refused;
    "secret.delete" => SecretDelete(DeleteSecretParams), recovery refused;
    "instance.create" => InstanceCreate(CreateInstanceParams), recovery refused;
    "instance.start" => InstanceStart(StartInstanceParams), recovery refused;
    "instance.stop" => InstanceStop(StopInstanceParams), recovery refused;
    "instance.get" => InstanceGet(GetInstanceParams), recovery refused;
    "instance.list" => InstanceList(ListInstancesParams), recovery refused;
    "storage.get_capacity" => StorageGetCapacity, recovery allowed;
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

    /// The methods the table marks `recovery allowed`.
    fn methods_allowed_in_recovery() -> Vec<&'static str> {
        include_str!("message.rs")
            .lines()
            .filter(|line| line.contains("recovery allowed;"))
            .filter_map(|line| line.trim().split('"').nth(1))
            .collect()
    }

    #[test]
    fn every_method_of_spec_0006_and_the_catalog_is_allowed_in_recovery_and_no_operation_method_is()
    {
        let allowed = [
            Call::DaemonGetInfo,
            Call::DaemonGetHealth,
            Call::DaemonGetCapabilities,
            Call::EventsSubscribe(SubscribeParams::default()),
            Call::EventsUnsubscribe(UnsubscribeParams { subscription_id: 1 }),
            Call::CatalogListProviders,
            Call::CatalogGetProvider(GetProviderParams {
                provider: "official/nats".to_owned(),
            }),
            Call::StorageGetCapacity,
        ];
        assert!(allowed.iter().all(Call::allowed_in_recovery));
        let operation_id = uuid::Uuid::nil();
        let refused = [
            Call::OperationGet(GetOperationParams { operation_id }),
            Call::OperationList(ListOperationsParams::default()),
            Call::OperationCancel(CancelOperationParams { operation_id }),
            Call::OperationResume(ResumeOperationParams {
                operation_id,
                expected_revision: None,
            }),
            Call::OperationAbandon(AbandonOperationParams {
                operation_id,
                expected_revision: crate::wire::Revision::ZERO,
            }),
            Call::OperationGetStats,
        ];
        assert!(!refused.iter().any(Call::allowed_in_recovery));
        assert!(
            Call::METHODS
                .iter()
                .filter(|method| method.starts_with("secret."))
                .count()
                == 5,
            "every secret method needs the registry, so none is allowed in recovery"
        );
        assert!(
            Call::METHODS
                .iter()
                .filter(|method| method.starts_with("instance."))
                .count()
                == 5
                && Call::METHODS
                    .iter()
                    .filter(|method| method.starts_with("instance."))
                    .all(|method| {
                        !matches!(
                            *method,
                            "instance.create"
                                | "instance.start"
                                | "instance.stop"
                                | "instance.get"
                                | "instance.list"
                        ) || !methods_allowed_in_recovery().contains(method)
                    }),
            "every instance method needs the registry, so none is allowed in recovery"
        );
        assert_eq!(
            allowed.len() + refused.len() + 1 + 5 + 5,
            Call::METHODS.len(),
            "a new method needs a line here"
        );
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
