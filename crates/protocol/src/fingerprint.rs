//! Request fingerprints for idempotency keys (spec 0007, *Idempotency*).
//!
//! An idempotency key is bound to the fingerprint of the request that
//! first used it: the same key with another fingerprint is
//! `operation.idempotency_key_reused`. The fingerprint is SHA-256 over
//! `<method>\n<canonical call>`, where the canonical call is the [`Call`]
//! encoded as a JSON value with every object's keys sorted, recursively,
//! then written with `serde_json::to_vec`.
//!
//! While a fingerprint is computed, [`redacting`] is true on this thread and
//! every [`SecretText`](crate::secret::SecretText) encodes as the fixed
//! marker [`SECRET_MARKER`](crate::secret::SECRET_MARKER), so two requests
//! that differ only in a secret value have the same fingerprint, and a
//! fingerprint never depends on a secret.

use std::cell::Cell;

use serde::Serialize;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::message::Call;

thread_local! {
    /// Whether this thread is computing a fingerprint right now.
    static REDACTING: Cell<bool> = const { Cell::new(false) };
}

/// Whether a fingerprint is being computed on this thread, so a
/// [`SecretText`](crate::secret::SecretText) encodes as its marker instead
/// of its value.
#[must_use]
pub fn redacting() -> bool {
    REDACTING.with(Cell::get)
}

/// The fingerprint of `call`: SHA-256 over `<method>\n<canonical call>`.
/// Secret values never enter it.
#[must_use]
pub fn request_fingerprint(call: &Call) -> [u8; 32] {
    fingerprint_of(call.method(), call)
}

/// [`request_fingerprint`] over any value that stands for the call.
pub(crate) fn fingerprint_of(method: &str, value: &impl Serialize) -> [u8; 32] {
    let encoded = {
        let _redacting = Redacting::start();
        serde_json::to_value(value)
            .expect("a call always encodes: every map key in the contract is a string")
    };
    let canonical = serde_json::to_vec(&sorted(encoded))
        .expect("a json value always encodes: its map keys are strings");
    let mut hasher = Sha256::new();
    hasher.update(method.as_bytes());
    hasher.update(b"\n");
    hasher.update(&canonical);
    hasher.finalize().into()
}

/// `value` with the keys of every object in sorted order, at every depth.
fn sorted(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut entries: Vec<(String, Value)> = map.into_iter().collect();
            entries.sort_by(|(a, _), (b, _)| a.cmp(b));
            Value::Object(
                entries
                    .into_iter()
                    .map(|(key, value)| (key, sorted(value)))
                    .collect::<Map<_, _>>(),
            )
        }
        Value::Array(items) => Value::Array(items.into_iter().map(sorted).collect()),
        other => other,
    }
}

/// Sets [`redacting`] for its lifetime and restores the earlier value when
/// it drops, also during a panic.
struct Redacting {
    before: bool,
}

impl Redacting {
    fn start() -> Self {
        Self {
            before: REDACTING.with(|flag| flag.replace(true)),
        }
    }
}

impl Drop for Redacting {
    fn drop(&mut self) {
        REDACTING.with(|flag| flag.set(self.before));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{SubscribeParams, UnsubscribeParams};
    use crate::operation::{GetOperationParams, ListOperationsParams};
    use crate::secret::{SECRET_MARKER, SecretInput, SecretText};

    fn get(id: &str) -> Call {
        Call::OperationGet(GetOperationParams {
            operation_id: id.parse().unwrap(),
        })
    }

    const A: &str = "0192f3a0-7c1e-7b2a-9d4e-5f6a7b8c9d04";
    const B: &str = "0192f3a0-7c1e-7b2a-9d4e-5f6a7b8c9d05";

    #[test]
    fn the_same_call_always_has_the_same_fingerprint() {
        assert_eq!(request_fingerprint(&get(A)), request_fingerprint(&get(A)));
    }

    #[test]
    fn another_method_or_other_params_change_the_fingerprint() {
        assert_ne!(request_fingerprint(&get(A)), request_fingerprint(&get(B)));
        let cancel = Call::OperationCancel(crate::operation::CancelOperationParams {
            operation_id: A.parse().unwrap(),
        });
        assert_ne!(request_fingerprint(&get(A)), request_fingerprint(&cancel));
        assert_ne!(
            request_fingerprint(&Call::EventsUnsubscribe(UnsubscribeParams {
                subscription_id: 1
            })),
            request_fingerprint(&Call::EventsUnsubscribe(UnsubscribeParams {
                subscription_id: 2
            })),
        );
    }

    #[test]
    fn the_fingerprint_is_sha256_of_the_method_a_newline_and_the_sorted_call() {
        let call = Call::OperationList(ListOperationsParams {
            limit: Some(5),
            ..ListOperationsParams::default()
        });
        let expected = Sha256::digest(
            br#"operation.list
{"content":{"activeOnly":false,"before":null,"limit":5,"resource":null},"type":"operation.list"}"#,
        );
        assert_eq!(request_fingerprint(&call), <[u8; 32]>::from(expected));
    }

    #[test]
    fn keys_are_sorted_at_every_depth() {
        let value = serde_json::json!({ "b": [{ "z": 1, "a": 2 }], "a": { "y": 1, "x": 2 } });
        assert_eq!(
            serde_json::to_string(&sorted(value)).unwrap(),
            r#"{"a":{"x":2,"y":1},"b":[{"a":2,"z":1}]}"#
        );
    }

    #[test]
    fn two_values_that_differ_only_in_a_secret_have_the_same_fingerprint() {
        let one = SecretInput::Value(SecretText::new("fixture-secret-0001".to_owned()));
        let two = SecretInput::Value(SecretText::new("fixture-secret-0002".to_owned()));
        assert_eq!(
            fingerprint_of("secret.set", &one),
            fingerprint_of("secret.set", &two)
        );
        let reference = SecretInput::CopyFrom("local:erp/admin".to_owned());
        assert_ne!(
            fingerprint_of("secret.set", &one),
            fingerprint_of("secret.set", &reference)
        );
    }

    #[test]
    fn a_secret_encodes_as_the_marker_only_while_a_fingerprint_is_computed() {
        let text = SecretText::new("fixture-secret-0001".to_owned());
        assert!(!redacting());
        assert_eq!(
            serde_json::to_value(&text).unwrap(),
            serde_json::json!("fixture-secret-0001")
        );
        {
            let _redacting = Redacting::start();
            assert!(redacting());
            assert_eq!(
                serde_json::to_value(&text).unwrap(),
                serde_json::json!(SECRET_MARKER)
            );
        }
        assert!(!redacting());
    }

    #[test]
    fn the_flag_is_restored_after_a_panic_while_encoding() {
        struct Panics;
        impl Serialize for Panics {
            fn serialize<S: serde::Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
                panic!("encoding panicked");
            }
        }
        let result = std::panic::catch_unwind(|| fingerprint_of("x.y", &Panics));
        assert!(result.is_err());
        assert!(!redacting());
    }

    #[test]
    fn subscribe_params_fingerprint_with_their_lists_in_order() {
        let one = Call::EventsSubscribe(SubscribeParams {
            kinds: vec!["operation.".to_owned(), "events.".to_owned()],
            ..SubscribeParams::default()
        });
        let two = Call::EventsSubscribe(SubscribeParams {
            kinds: vec!["events.".to_owned(), "operation.".to_owned()],
            ..SubscribeParams::default()
        });
        assert_ne!(request_fingerprint(&one), request_fingerprint(&two));
    }
}
