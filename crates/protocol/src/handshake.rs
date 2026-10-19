//! The `daemon.handshake` method: the first request on every connection.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use typeshare::typeshare;
use uuid::Uuid;

use crate::error::FieldError;
use crate::events::EventCursor;
use crate::json::{FieldCode, Validate};
use crate::limits::Limits;
use crate::version::ProtocolVersion;

/// Longest `client.version`, in bytes.
pub const CLIENT_VERSION_MAX: usize = 64;
/// Exact length of a contract digest: SHA-256 as lowercase hex.
pub const CONTRACT_DIGEST_LEN: usize = 64;

/// Params of `daemon.handshake`.
///
/// `protocol` is frozen across majors: a daemon of any major reads
/// `protocol.major` before anything else, so a client of another major
/// always receives `protocol.major_mismatch`.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct HandshakeParams {
    /// The protocol version the client speaks.
    pub protocol: ProtocolVersion,
    /// Who the client is.
    pub client: ClientInfo,
    /// The client's contract digest, 64 lowercase hex characters.
    pub contract_digest: String,
}

impl Validate for HandshakeParams {
    fn validate(&self) -> Vec<FieldError> {
        let mut fields = Vec::new();
        let version = &self.client.version;
        if version.is_empty() {
            fields.push(FieldError::new("client.version", FieldCode::TooShort));
        } else if version.len() > CLIENT_VERSION_MAX {
            fields.push(FieldError::new("client.version", FieldCode::TooLong));
        } else if !version.bytes().all(|b| (0x21..=0x7e).contains(&b)) {
            fields.push(FieldError::new("client.version", FieldCode::InvalidFormat));
        }
        if !is_contract_digest(&self.contract_digest) {
            fields.push(FieldError::new("contractDigest", FieldCode::InvalidFormat));
        }
        fields
    }
}

/// Whether `text` is exactly 64 lowercase hex characters.
#[must_use]
pub fn is_contract_digest(text: &str) -> bool {
    text.len() == CONTRACT_DIGEST_LEN
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Who a client is: for the daemon log, and for handlers that pick how to
/// interact with the person behind it (spec 0011).
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClientInfo {
    /// The kind of client.
    pub kind: ClientKind,
    /// The client's own version, 1 to 64 bytes of printable ASCII without
    /// spaces.
    pub version: String,
    /// The client's process ID, when it shares it.
    #[serde(default)]
    pub pid: Option<u32>,
    /// True when the client can show a dialog the person sees right now:
    /// always for the GUI, for the CLI only when stdin and stderr are both
    /// terminals. Absent means false, so an older client never triggers a
    /// dialog.
    #[serde(default)]
    pub interactive: Option<bool>,
}

impl ClientInfo {
    /// Whether the client said it can show a dialog; absent means false.
    #[must_use]
    pub fn is_interactive(&self) -> bool {
        self.interactive.unwrap_or(false)
    }
}

/// The kinds of client. Open: a newer client may send a value an older
/// daemon reads as `unrecognized`.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ClientKind {
    /// The `stackly` CLI.
    Cli,
    /// The SwiftUI app.
    Gui,
    /// A kind this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// Reply of `daemon.handshake` when the majors match and the params are
/// valid.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct HandshakeAccepted {
    /// The protocol version the daemon speaks.
    pub protocol: ProtocolVersion,
    /// A fresh UUID v7 for each daemon run; event cursors belong to it.
    pub server_instance_id: Uuid,
    /// The daemon's version and process.
    pub daemon: DaemonIdentity,
    /// The daemon's contract digest.
    pub contract_digest: String,
    /// True when the versions match but the digests differ: the connection
    /// works, and the daemon logs one warning.
    pub contract_drift: bool,
    /// The limits the daemon enforces.
    pub limits: Limits,
    /// The newest event cursor at handshake time; sequence 0 means no event
    /// yet.
    pub head: EventCursor,
}

/// The daemon's version and process ID.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DaemonIdentity {
    /// The daemon's version.
    pub version: String,
    /// The daemon's process ID.
    pub pid: u32,
}

/// Whether a client and a daemon with the same version still disagree on
/// the contract: same major and minor, different digests.
#[must_use]
pub fn contract_drift(
    client: ProtocolVersion,
    client_digest: &str,
    daemon: ProtocolVersion,
    daemon_digest: &str,
) -> bool {
    client == daemon && client_digest != daemon_digest
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(version: &str, digest: &str) -> HandshakeParams {
        HandshakeParams {
            protocol: ProtocolVersion::CURRENT,
            client: ClientInfo {
                kind: ClientKind::Cli,
                version: version.to_owned(),
                pid: Some(42),
                interactive: None,
            },
            contract_digest: digest.to_owned(),
        }
    }

    fn codes(fields: &[FieldError]) -> Vec<(&str, &str)> {
        fields.iter().map(|f| (f.path(), f.code())).collect()
    }

    #[test]
    fn valid_handshake_params_pass_validation() {
        assert!(params("0.1.0", &"a".repeat(64)).validate().is_empty());
    }

    #[test]
    fn client_version_must_be_1_to_64_printable_ascii_bytes() {
        let digest = "0".repeat(64);
        assert_eq!(
            codes(&params("", &digest).validate()),
            [("client.version", "too_short")]
        );
        assert_eq!(
            codes(&params(&"1".repeat(65), &digest).validate()),
            [("client.version", "too_long")]
        );
        assert_eq!(
            codes(&params("0.1 beta", &digest).validate()),
            [("client.version", "invalid_format")]
        );
    }

    #[test]
    fn contract_digest_must_be_64_lowercase_hex_characters() {
        assert_eq!(
            codes(&params("0.1.0", &"A".repeat(64)).validate()),
            [("contractDigest", "invalid_format")]
        );
        assert_eq!(
            codes(&params("0.1.0", &"a".repeat(63)).validate()),
            [("contractDigest", "invalid_format")]
        );
    }

    #[test]
    fn drift_means_same_version_with_another_digest() {
        let v = ProtocolVersion::CURRENT;
        let newer = ProtocolVersion { major: 1, minor: 7 };
        assert!(contract_drift(v, "a", v, "b"));
        assert!(!contract_drift(v, "a", v, "a"));
        assert!(!contract_drift(v, "a", newer, "b"));
    }

    #[test]
    fn an_absent_interactive_flag_means_not_interactive() {
        let client: ClientInfo =
            serde_json::from_str(r#"{"kind":"cli","version":"0.1.0","pid":null}"#).unwrap();
        assert_eq!(client.interactive, None);
        assert!(!client.is_interactive());

        let client: ClientInfo =
            serde_json::from_str(r#"{"kind":"gui","version":"0.1.0","interactive":true}"#).unwrap();
        assert!(client.is_interactive());
    }

    #[test]
    fn foreign_client_kinds_decode_as_unrecognized() {
        let kind: ClientKind = serde_json::from_str("\"watch_app\"").unwrap();
        assert_eq!(kind, ClientKind::Unrecognized);
        assert_eq!(serde_json::to_string(&ClientKind::Gui).unwrap(), "\"gui\"");
    }
}
