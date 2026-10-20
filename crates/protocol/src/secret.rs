//! Secret values on the wire (spec 0011): [`SecretText`], the one DTO
//! field that may hold a secret value, and [`SecretInput`], where a new
//! secret value comes from.
//!
//! A [`SecretText`] travels as a plain JSON string, but its `Debug` prints
//! `SecretText(****)` and its memory is wiped when it drops. While
//! [`request_fingerprint`](crate::fingerprint::request_fingerprint) runs,
//! it encodes as the fixed marker `"[secret]"`, so a fingerprint never
//! depends on a secret value (spec 0007). The only way to read the value is
//! [`SecretText::expose`], which `cargo xtask check-deps` allows only in a
//! short list of modules (spec 0011, *Penjaga di kode*).
//!
//! The rules on a value (UTF-8, 1 to [`MAX_SECRET_LEN`] bytes, no NUL) and
//! on a reference (`local:<segment>/<segment>...`) are checked by
//! [`Validate`] on the params, so a bad input becomes
//! `request.invalid_params` before anything reads a store.

use std::borrow::Cow;
use std::fmt;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use typeshare::typeshare;
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::error::FieldError;
use crate::json::{FieldCode, Validate};
use crate::wire::{ResourceKind, Revision, Timestamp};

/// The longest secret value, in bytes: room for a PEM private key with its
/// certificate chain (spec 0011 AC-7).
pub const MAX_SECRET_LEN: usize = 65_536;
/// The longest secret reference, in bytes.
pub const SECRET_REFERENCE_MAX: usize = 255;
/// The text a [`SecretText`] encodes as while a fingerprint is computed.
pub const SECRET_MARKER: &str = "[secret]";

/// The scheme every secret reference starts with.
const REFERENCE_SCHEME: &str = "local:";
/// The fewest segments after the scheme.
pub(crate) const SEGMENTS_MIN: usize = 2;
/// The most segments after the scheme.
pub(crate) const SEGMENTS_MAX: usize = 8;
/// The longest segment, in bytes.
pub(crate) const SEGMENT_MAX: usize = 64;

/// A secret value as the wire carries it: in a request that sets a value,
/// and in the reply of `secret.reveal` (spec 0011 AC-14).
///
/// It encodes as a plain JSON string, its `Debug` prints
/// `SecretText(****)`, and its memory is wiped when it drops. Decoding
/// takes any string; the value rules are [`SecretText::field_code`], run
/// by the params' [`Validate`].
#[typeshare(serialized_as = "String")]
#[derive(Clone, PartialEq, Eq)]
pub struct SecretText(Zeroizing<String>);

impl SecretText {
    /// Takes ownership of a value. It is checked only by
    /// [`SecretText::field_code`], so a decoded value can always be
    /// refused through `request.invalid_params`.
    #[must_use]
    pub fn new(value: String) -> Self {
        Self(Zeroizing::new(value))
    }

    /// Returns the value. Only the modules on the `check-deps` allowlist
    /// may call it (spec 0011, *Penjaga di kode*).
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// Returns the length in bytes, without exposing the value.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns whether the value is empty, which no rule accepts.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Why the value breaks the rules of spec 0011 AC-7, or `None` when it
    /// is 1 to [`MAX_SECRET_LEN`] bytes without a NUL byte: `too_long`
    /// above the limit, `invalid_value` when empty or holding a NUL.
    #[must_use]
    pub fn field_code(&self) -> Option<FieldCode> {
        let value = self.expose();
        if value.len() > MAX_SECRET_LEN {
            Some(FieldCode::TooLong)
        } else if value.is_empty() || value.contains('\0') {
            Some(FieldCode::InvalidValue)
        } else {
            None
        }
    }
}

impl fmt::Debug for SecretText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretText(****)")
    }
}

impl Serialize for SecretText {
    /// The value, or [`SECRET_MARKER`] while
    /// [`fingerprint::redacting`](crate::fingerprint::redacting) is true.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if crate::fingerprint::redacting() {
            serializer.serialize_str(SECRET_MARKER)
        } else {
            serializer.serialize_str(self.expose())
        }
    }
}

impl<'de> Deserialize<'de> for SecretText {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Zeroizing::<String>::deserialize(deserializer).map(Self)
    }
}

impl JsonSchema for SecretText {
    fn schema_name() -> Cow<'static, str> {
        "SecretText".into()
    }

    fn inline_schema() -> bool {
        true
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "writeOnly": true,
        })
    }
}

/// Where a new secret value comes from (spec 0011 AC-5): shared by
/// `secret.set` and by the requests that create an instance, bring an
/// environment up, or restore.
///
/// A reference is the text form `local:<segment>/<segment>...`; it is not a
/// secret and may appear anywhere.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", content = "content", rename_all = "camelCase")]
pub enum SecretInput {
    /// A value the person typed, pasted, or piped in.
    Value(SecretText),
    /// A new random value the daemon generates.
    Generate,
    /// A copy of the value stored under another reference.
    CopyFrom(String),
    /// Use an existing reference without writing a value. Only for a new
    /// resource; `secret.set` refuses it.
    Bind(String),
}

impl SecretInput {
    /// Every field that breaks a rule, for an input that sits at `at` in
    /// the params, such as `input`: the value rules of spec 0011 AC-7 on
    /// `<at>.content` for `value`, the reference format for `copyFrom` and
    /// `bind`. Empty means valid.
    #[must_use]
    pub fn field_errors(&self, at: &str) -> Vec<FieldError> {
        let path = if at.is_empty() {
            "content".to_owned()
        } else {
            format!("{at}.content")
        };
        let code = match self {
            Self::Value(text) => text.field_code(),
            Self::Generate => None,
            Self::CopyFrom(reference) | Self::Bind(reference) => reference_field_code(reference),
        };
        code.map(|code| vec![FieldError::new(path, code)])
            .unwrap_or_default()
    }
}

impl Validate for SecretInput {
    fn validate(&self) -> Vec<FieldError> {
        self.field_errors("")
    }
}

/// Why `text` is not a secret reference, or `None` when it is one:
/// `too_long` above [`SECRET_REFERENCE_MAX`] bytes, `invalid_format`
/// otherwise.
#[must_use]
pub fn reference_field_code(text: &str) -> Option<FieldCode> {
    if text.len() > SECRET_REFERENCE_MAX {
        Some(FieldCode::TooLong)
    } else if is_secret_reference(text) {
        None
    } else {
        Some(FieldCode::InvalidFormat)
    }
}

/// Whether `text` matches the reference grammar of spec 0011: `local:`,
/// then 2 to 8 segments separated by `/`, each 1 to 64 bytes matching
/// `[a-z0-9][a-z0-9._-]*`, at most [`SECRET_REFERENCE_MAX`] bytes in all.
/// The rules of `stackly_domain::SecretRef` that need state, such as the
/// reserved first segments, are not checked here.
#[must_use]
pub fn is_secret_reference(text: &str) -> bool {
    let Some(path) = text.strip_prefix(REFERENCE_SCHEME) else {
        return false;
    };
    text.len() <= SECRET_REFERENCE_MAX
        && (SEGMENTS_MIN..=SEGMENTS_MAX).contains(&path.split('/').count())
        && path.split('/').all(is_segment)
}

/// `[a-z0-9][a-z0-9._-]{0,63}`; such a segment is never `.` or `..`.
fn is_segment(segment: &str) -> bool {
    let mut bytes = segment.bytes();
    segment.len() <= SEGMENT_MAX
        && bytes
            .next()
            .is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        && bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-".contains(&b))
}

/// How `secret.set` treats a value that exists already: `create` fails on
/// an existing item, `replace` on a missing one, and `upsert` takes either
/// (spec 0011 AC-5).
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SecretMode {
    /// The item must not exist yet.
    Create,
    /// The item must exist.
    Replace,
    /// Either; it is written whichever way.
    Upsert,
    /// A mode this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// Why a client reveals a value (spec 0011 AC-9).
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RevealPurpose {
    /// To show it on screen.
    Show,
    /// To copy it.
    Copy,
    /// A purpose this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// Who owns a secret reference.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SecretOwnerKind {
    /// An instance.
    Instance,
    /// An environment.
    Environment,
    /// The user, who registered the reference with `secret.set`.
    User,
    /// A kind this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// What the store held when the daemon last looked, never a value.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SecretStatus {
    /// The store holds a value.
    Present,
    /// The store holds none.
    Missing,
    /// The store could not be checked.
    Unknown,
    /// A status this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// The status in `secret.changed`: a [`SecretStatus`] or `deleted`.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SecretChangeStatus {
    /// The store holds a value.
    Present,
    /// The store holds none.
    Missing,
    /// The store could not be checked.
    Unknown,
    /// The reference row is gone.
    Deleted,
    /// A status this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// How the current value came to be.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SecretValueOrigin {
    /// Typed, pasted, or piped in.
    Entered,
    /// Generated by the daemon.
    Generated,
    /// Copied from another reference.
    Copied,
    /// An origin this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// Which secret store the daemon uses.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SecretStoreBackend {
    /// The macOS Keychain.
    MacosKeychain,
    /// A fake store, only in tests.
    Fake,
    /// No store on this OS.
    Unsupported,
    /// A backend this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// The state of the secret store, from the daemon's cache; asking never
/// waits on the store.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SecretStoreState {
    /// The store answers.
    Available,
    /// The keychain is locked.
    Locked,
    /// The OS has no store.
    Unsupported,
    /// Nothing was observed yet.
    Unknown,
    /// A state this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// Which keychain the daemon chose at start.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SecretStoreTarget {
    /// The user's login Keychain, for the default root.
    Login,
    /// A keychain file of its own, for a `STACKLY_ROOT` root.
    File,
    /// A target this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// The secret store as `secret.list`, `secret.status`, and
/// `daemon.get_info` report it.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SecretStoreStatus {
    /// Which store it is.
    pub backend: SecretStoreBackend,
    /// Its state now.
    pub state: SecretStoreState,
    /// The keychain chosen at start, or null for a store without one.
    #[serde(default)]
    pub target: Option<SecretStoreTarget>,
}

/// One resource that uses a reference.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SecretUseInfo {
    /// The kind of resource: `instance` or `environment`.
    pub resource_kind: ResourceKind,
    /// The resource ID.
    pub resource_id: String,
    /// The secret field of the manifest, or the `secretRefs` key.
    pub field: String,
}

/// One reference row, without any value.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SecretRefSummary {
    /// The reference, `local:<segment>/<segment>...`.
    pub reference: String,
    /// Who owns it.
    pub owner_kind: SecretOwnerKind,
    /// The owner's UUID, or null for the user.
    #[serde(default)]
    pub owner_id: Option<Uuid>,
    /// The purpose of its field, or `user`.
    pub purpose: String,
    /// What the store held at the last look.
    pub status: SecretStatus,
    /// When the status was observed.
    #[serde(default)]
    pub status_observed_at: Option<Timestamp>,
    /// How the value came to be, or null before the first value.
    #[serde(default)]
    pub value_origin: Option<SecretValueOrigin>,
    /// When the value was last filled through Stackly.
    #[serde(default)]
    pub value_updated_at: Option<Timestamp>,
    /// Every resource that uses the reference.
    #[serde(default)]
    pub used_by: Vec<SecretUseInfo>,
    /// The row's revision.
    pub revision: Revision,
}

/// Params of `secret.list`.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListSecretsParams {
    /// Only references of this owner kind.
    #[serde(default)]
    pub owner_kind: Option<SecretOwnerKind>,
    /// Only references with this status.
    #[serde(default)]
    pub status: Option<SecretStatus>,
}

impl Validate for ListSecretsParams {
    fn validate(&self) -> Vec<FieldError> {
        let mut fields = Vec::new();
        if self.owner_kind == Some(SecretOwnerKind::Unrecognized) {
            fields.push(FieldError::new("ownerKind", FieldCode::UnknownValue));
        }
        if self.status == Some(SecretStatus::Unrecognized) {
            fields.push(FieldError::new("status", FieldCode::UnknownValue));
        }
        fields
    }
}

/// Reply of `secret.list`: the cached rows, ordered by reference.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListSecretsReply {
    /// The references.
    #[serde(default)]
    pub items: Vec<SecretRefSummary>,
    /// The store.
    pub store: SecretStoreStatus,
}

/// Params of `secret.status` and the other methods that name only a
/// reference.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SecretStatusParams {
    /// The reference.
    pub reference: String,
}

impl Validate for SecretStatusParams {
    fn validate(&self) -> Vec<FieldError> {
        reference_errors("reference", &self.reference)
    }
}

/// Reply of `secret.status`: the row after a fresh probe.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SecretStatusReply {
    /// The reference row.
    pub summary: SecretRefSummary,
    /// The store.
    pub store: SecretStoreStatus,
}

/// Params of `secret.set`.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SetSecretParams {
    /// The reference to fill.
    pub reference: String,
    /// Where the value comes from; `bind` is refused here.
    pub input: SecretInput,
    /// How to treat a value that exists.
    pub mode: SecretMode,
    /// The revision of the row the client read, if it names one.
    #[serde(default)]
    pub expected_revision: Option<Revision>,
    /// Confirms that a replace changes only the stored copy.
    #[serde(default)]
    pub confirm_stored_only: bool,
}

impl Validate for SetSecretParams {
    fn validate(&self) -> Vec<FieldError> {
        let mut fields = reference_errors("reference", &self.reference);
        fields.extend(self.input.field_errors("input"));
        if matches!(self.input, SecretInput::Bind(_)) {
            fields.push(FieldError::new("input", FieldCode::InvalidValue));
        }
        if self.mode == SecretMode::Unrecognized {
            fields.push(FieldError::new("mode", FieldCode::UnknownValue));
        }
        fields
    }
}

/// A warning a stored value comes with. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SecretWarning {
    /// The value is shorter than 8 bytes, so redaction never matches it in
    /// text.
    ShortValueNotScrubbed,
    /// A warning this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// What a new value changes for one use. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SecretEffectKind {
    /// Only the stored copy changes; the value inside the data stays.
    StoredOnly,
    /// The next reload uses it.
    NextReload,
    /// The next start uses it.
    NextStart,
    /// An effect this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// The effect of a new value on one use.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SecretEffectInfo {
    /// The kind of resource that uses the reference.
    pub resource_kind: ResourceKind,
    /// The resource ID.
    pub resource_id: String,
    /// The secret field.
    pub field: String,
    /// What changes.
    pub effect: SecretEffectKind,
}

/// Reply of `secret.set`: never the value.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SetSecretReply {
    /// The reference row after the write.
    pub summary: SecretRefSummary,
    /// Warnings about the stored value.
    #[serde(default)]
    pub warnings: Vec<SecretWarning>,
    /// The effect on every use.
    #[serde(default)]
    pub effects: Vec<SecretEffectInfo>,
}

/// Params of `secret.reveal`.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RevealSecretParams {
    /// The reference.
    pub reference: String,
    /// Why the client asks; recorded in the audit row.
    pub purpose: RevealPurpose,
}

impl Validate for RevealSecretParams {
    fn validate(&self) -> Vec<FieldError> {
        let mut fields = reference_errors("reference", &self.reference);
        if self.purpose == RevealPurpose::Unrecognized {
            fields.push(FieldError::new("purpose", FieldCode::UnknownValue));
        }
        fields
    }
}

/// Reply of `secret.reveal`: the one place a value leaves the daemon.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RevealSecretReply {
    /// The reference.
    pub reference: String,
    /// The value.
    pub value: SecretText,
}

/// Params of `secret.delete`.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeleteSecretParams {
    /// The reference.
    pub reference: String,
    /// The revision of the row the client read, if it names one.
    #[serde(default)]
    pub expected_revision: Option<Revision>,
}

impl Validate for DeleteSecretParams {
    fn validate(&self) -> Vec<FieldError> {
        reference_errors("reference", &self.reference)
    }
}

/// Reply of `secret.delete`.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeleteSecretReply {
    /// The reference that was deleted.
    pub reference: String,
}

/// Content of the `secret.changed` event. The event's resource is
/// `{kind: secret, id: <reference>}` and its revision the row's; a change
/// of status seen by a probe keeps the revision.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SecretChanged {
    /// The status now.
    pub status: SecretChangeStatus,
    /// When the status was observed.
    #[serde(default)]
    pub status_observed_at: Option<Timestamp>,
}

/// Why a reference needs a value. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum NeedsSecretReason {
    /// The store holds no value.
    Missing,
    /// The keychain is locked.
    StoreLocked,
    /// The user or the access list refused the read.
    AccessDenied,
    /// The store call did not start in time.
    StoreBusy,
    /// The OS has no store.
    StoreUnsupported,
    /// A reason this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// One reference a resource needs and cannot read.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NeedsSecretItem {
    /// The reference.
    pub reference: String,
    /// Why it cannot be read.
    pub reason: NeedsSecretReason,
    /// Whether a generated value would work for it.
    pub generate_allowed: bool,
}

/// The content of the `needsSecret` blocker of an instance (spec 0011,
/// *Needs secret*): the instance DTO of feature 10 carries it in its
/// `blocker` slot as `{type: needsSecret, content: ...}`.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NeedsSecretBlocker {
    /// Every reference that cannot be read, ordered by reference text.
    #[serde(default)]
    pub references: Vec<NeedsSecretItem>,
    /// When the daemon saw it.
    pub observed_at: Timestamp,
}

/// The field errors of a secret reference at `path`.
fn reference_errors(path: &str, text: &str) -> Vec<FieldError> {
    reference_field_code(text)
        .map(|code| FieldError::new(path, code))
        .into_iter()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALUE: &str = "fixture-secret-0001";

    fn codes(input: &SecretInput, at: &str) -> Vec<(String, String)> {
        input
            .field_errors(at)
            .iter()
            .map(|field| (field.path().to_owned(), field.code().to_owned()))
            .collect()
    }

    #[test]
    fn secret_text_encodes_as_a_plain_string_and_decodes_back() {
        let text = SecretText::new(VALUE.to_owned());
        let json = serde_json::to_string(&text).unwrap();
        assert_eq!(json, format!("\"{VALUE}\""));
        let back: SecretText = serde_json::from_str(&json).unwrap();
        assert_eq!(back, text);
        assert_eq!(back.expose(), VALUE);
        assert_eq!(back.len(), VALUE.len());
    }

    #[test]
    fn debug_never_prints_the_value_even_inside_an_input() {
        let input = SecretInput::Value(SecretText::new(VALUE.to_owned()));
        let debug = format!("{input:?} {:?}", SecretText::new(VALUE.to_owned()));
        assert!(!debug.contains(VALUE), "{debug}");
        assert!(debug.contains("SecretText(****)"), "{debug}");
    }

    #[test]
    fn secret_inputs_use_the_type_and_content_shape_of_spec_0011() {
        let cases = [
            (
                SecretInput::Value(SecretText::new(VALUE.to_owned())),
                serde_json::json!({ "type": "value", "content": VALUE }),
            ),
            (
                SecretInput::Generate,
                serde_json::json!({ "type": "generate" }),
            ),
            (
                SecretInput::CopyFrom("local:erp/postgres/admin".to_owned()),
                serde_json::json!({ "type": "copyFrom", "content": "local:erp/postgres/admin" }),
            ),
            (
                SecretInput::Bind("local:erp/postgres/admin".to_owned()),
                serde_json::json!({ "type": "bind", "content": "local:erp/postgres/admin" }),
            ),
        ];
        for (input, json) in cases {
            assert_eq!(serde_json::to_value(&input).unwrap(), json);
            assert_eq!(serde_json::from_value::<SecretInput>(json).unwrap(), input);
        }
    }

    #[test]
    fn a_value_of_one_to_65536_bytes_without_nul_is_valid() {
        for value in ["x".to_owned(), "é".repeat(MAX_SECRET_LEN / 2)] {
            let input = SecretInput::Value(SecretText::new(value));
            assert!(input.validate().is_empty());
        }
    }

    #[test]
    fn a_long_empty_or_nul_value_fails_on_its_content_path() {
        let too_long = SecretInput::Value(SecretText::new("x".repeat(MAX_SECRET_LEN + 1)));
        assert_eq!(
            codes(&too_long, "input"),
            [("input.content".to_owned(), "too_long".to_owned())]
        );
        for value in ["", "a\0b"] {
            let input = SecretInput::Value(SecretText::new(value.to_owned()));
            assert_eq!(
                codes(&input, ""),
                [("content".to_owned(), "invalid_value".to_owned())]
            );
        }
    }

    #[test]
    fn a_field_error_never_carries_the_value() {
        let input = SecretInput::Value(SecretText::new(format!("{VALUE}\0")));
        let json = serde_json::to_string(&input.validate()).unwrap();
        assert!(!json.contains(VALUE), "{json}");
    }

    #[test]
    fn references_follow_the_local_grammar() {
        let longest = format!("local:a/{}", "b".repeat(SEGMENT_MAX));
        for text in [
            "local:erp/postgres/admin",
            "local:instance/0192f3a0-7c1e-7b2a-9d4e-5f6a7b8c9d0e/admin-password",
            "local:a/b",
            "local:a.b/c_d-e",
            "local:1/2/3/4/5/6/7/8",
            longest.as_str(),
        ] {
            assert!(is_secret_reference(text), "{text}");
            assert_eq!(reference_field_code(text), None, "{text}");
        }
        let segment_too_long = format!("local:a/{}", "b".repeat(SEGMENT_MAX + 1));
        for text in [
            "",
            "local:",
            "local:one",
            "local:a//b",
            "local:a/",
            "local:A/b",
            "local:.a/b",
            "local:a/../b",
            "local:-a/b",
            "local:a/b c",
            "local:1/2/3/4/5/6/7/8/9",
            "remote:a/b",
            "LOCAL:a/b",
            segment_too_long.as_str(),
        ] {
            assert!(!is_secret_reference(text), "{text:?}");
            assert_eq!(
                reference_field_code(text),
                Some(FieldCode::InvalidFormat),
                "{text:?}"
            );
        }
        let too_long = format!("local:{}", ["a"; 130].join("/"));
        assert_eq!(reference_field_code(&too_long), Some(FieldCode::TooLong));
    }

    #[test]
    fn copy_from_and_bind_check_their_reference_and_generate_has_nothing_to_check() {
        assert!(SecretInput::Generate.validate().is_empty());
        let bad = SecretInput::CopyFrom("local:Admin".to_owned());
        assert_eq!(
            codes(&bad, "secrets[0].input"),
            [(
                "secrets[0].input.content".to_owned(),
                "invalid_format".to_owned()
            )]
        );
        assert!(
            SecretInput::Bind("local:erp/admin".to_owned())
                .validate()
                .is_empty()
        );
    }
}
