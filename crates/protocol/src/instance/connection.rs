//! Composing a connection string (spec 0021, *Blok connection di manifest*
//! and AC-3): the daemon sends a connection as text parts and secret parts
//! (`stackly-core`'s `render`), and a client joins them with the secret
//! values it fetched through `secret.reveal`. [`compose`] is that join, a
//! pure function the CLI calls and `ConnectionComposer` in `StacklyUI`
//! repeats in Swift; both give the same string for every case of
//! `protocols/fixtures/instance/connection-compose.json`.
//!
//! The part types below are the wire shape of
//! `InstanceDetail.connections[].parts` (spec 0021, *Method IPC*).

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use typeshare::typeshare;

/// What one [`ConnectionPart`] holds. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionPartKind {
    /// Text that is already percent encoded.
    Text,
    /// A secret value the client fills in.
    Secret,
    /// A kind this build does not know.
    #[serde(other)]
    Unrecognized,
}

/// One piece of a connection string: [`text`](Self::text) for a text part,
/// [`secret`](Self::secret) for a secret part.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionPart {
    /// What the part holds.
    pub kind: ConnectionPartKind,
    /// The text of a text part, already percent encoded, written as is.
    #[serde(default)]
    pub text: Option<String>,
    /// The secret of a secret part: its field and its reference, never its
    /// value.
    #[serde(default)]
    pub secret: Option<ConnectionSecretPart>,
}

impl ConnectionPart {
    /// A text part.
    #[must_use]
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            kind: ConnectionPartKind::Text,
            text: Some(text.into()),
            secret: None,
        }
    }

    /// A secret part.
    #[must_use]
    pub fn secret(field: impl Into<String>, reference: impl Into<String>) -> Self {
        Self {
            kind: ConnectionPartKind::Secret,
            text: None,
            secret: Some(ConnectionSecretPart {
                field: field.into(),
                reference: reference.into(),
            }),
        }
    }
}

/// The secret a secret part stands for.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionSecretPart {
    /// The settings field of the secret.
    pub field: String,
    /// The reference `secret.reveal` takes.
    pub reference: String,
}

/// Why [`compose`] could not build the string.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ComposeError {
    /// A secret part names a reference that `values` does not hold. The
    /// string is never completed with an empty value.
    #[error("no value for the secret reference {reference}")]
    MissingValue {
        /// The reference without a value.
        reference: String,
    },
    /// A part whose kind and fields do not match, or whose kind this build
    /// does not know.
    #[error("a connection part is malformed or of an unknown kind")]
    InvalidPart,
}

/// Joins `parts` into one string: text parts as they are, secret parts as
/// the percent encoded value of their reference in `values`.
///
/// A value is encoded as UTF-8 bytes in uppercase hex, except the
/// characters RFC 3986 calls unreserved (`A-Z a-z 0-9 - . _ ~`), so a value
/// can never change the structure of the URI.
///
/// # Errors
///
/// Returns [`ComposeError::MissingValue`] for a secret part whose reference
/// is not in `values`, and [`ComposeError::InvalidPart`] for a part of an
/// unknown kind or without the field its kind needs.
pub fn compose(
    parts: &[ConnectionPart],
    values: &BTreeMap<String, String>,
) -> Result<String, ComposeError> {
    let mut out = String::new();
    for part in parts {
        match (part.kind, &part.text, &part.secret) {
            (ConnectionPartKind::Text, Some(text), None) => out.push_str(text),
            (ConnectionPartKind::Secret, None, Some(secret)) => {
                let value =
                    values
                        .get(&secret.reference)
                        .ok_or_else(|| ComposeError::MissingValue {
                            reference: secret.reference.clone(),
                        })?;
                push_encoded(&mut out, value);
            }
            _ => return Err(ComposeError::InvalidPart),
        }
    }
    Ok(out)
}

/// Appends `value` percent encoded.
fn push_encoded(out: &mut String, value: &str) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(byte));
        } else {
            out.push('%');
            out.push(char::from(HEX[usize::from(byte >> 4)]));
            out.push(char::from(HEX[usize::from(byte & 0x0F)]));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    /// One case of `connection-compose.json`.
    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct ComposeCase {
        name: String,
        parts: Vec<ConnectionPart>,
        values: BTreeMap<String, String>,
        #[serde(default)]
        expected: Option<String>,
        #[serde(default)]
        missing_value: Option<String>,
    }

    #[derive(Debug, Deserialize)]
    struct Fixture {
        compose: Vec<ComposeCase>,
    }

    fn fixture() -> Fixture {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../protocols/fixtures/instance/connection-compose.json");
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("{}: {err}", path.display()));
        serde_json::from_str(&text).expect("the fixture is JSON of the expected shape")
    }

    #[test]
    fn every_fixture_case_composes_to_its_expected_string_or_error() {
        let fixture = fixture();
        assert!(fixture.compose.len() >= 8, "the fixture covers the cases");
        for case in fixture.compose {
            let result = compose(&case.parts, &case.values);
            match (case.expected, case.missing_value) {
                (Some(expected), None) => {
                    assert_eq!(result.as_deref(), Ok(expected.as_str()), "{}", case.name);
                }
                (None, Some(reference)) => {
                    assert_eq!(
                        result,
                        Err(ComposeError::MissingValue { reference }),
                        "{}",
                        case.name
                    );
                }
                other => panic!("{}: a case holds one expectation, got {other:?}", case.name),
            }
        }
    }

    #[test]
    fn a_text_part_without_text_or_an_unknown_kind_is_invalid() {
        let values = BTreeMap::new();
        let empty = ConnectionPart {
            kind: ConnectionPartKind::Text,
            text: None,
            secret: None,
        };
        let unknown = ConnectionPart {
            kind: ConnectionPartKind::Unrecognized,
            text: Some("x".to_owned()),
            secret: None,
        };
        let both = ConnectionPart {
            kind: ConnectionPartKind::Secret,
            text: Some("x".to_owned()),
            secret: Some(ConnectionSecretPart {
                field: "f".to_owned(),
                reference: "r".to_owned(),
            }),
        };

        for part in [empty, unknown, both] {
            assert_eq!(compose(&[part], &values), Err(ComposeError::InvalidPart));
        }
    }

    #[test]
    fn an_unknown_part_kind_reads_as_unrecognized() {
        let part: ConnectionPart =
            serde_json::from_str(r#"{"kind":"from_the_future","text":"x"}"#).expect("reads");

        assert_eq!(part.kind, ConnectionPartKind::Unrecognized);
    }
}
