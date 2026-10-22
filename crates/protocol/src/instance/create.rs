//! The DTOs of `instance.get_create_form` and `instance.preflight` (spec
//! 0020, *Method IPC*): the form the daemon builds from the settings schema
//! and the resources manifest of a provider, the draft a client sends back
//! without any secret value, and the preflight rows both the GUI and
//! `stackly instance create --dry-run` show.
//!
//! `instance.create` keeps the params of spec 0010; this module adds none
//! for it. `stacklyd` maps the core types of `stackly-core` to these DTOs
//! and never sends a domain type.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use typeshare::typeshare;
use uuid::Uuid;

use super::{
    CREATE_ENDPOINTS_MAX, CREATE_SECRETS_MAX, EndpointRequest, EndpointTransport,
    INSTANCE_DISPLAY_NAME_MAX, INSTANCE_KEY_MAX, INSTANCE_NAME_MAX, PROVIDER_REVISION_MAX,
    ProviderSelector, SettingsDocument, length_code, settings_has_unsafe_integer,
};
use crate::catalog::{CapabilityStatus, ProviderCategory, provider_id_code};
use crate::error::{ErrorBody, FieldError};
use crate::json::{FieldCode, Validate};
use crate::runtime::RuntimeSummary;
use crate::secret::reference_field_code;
use crate::wire::{Count, SignedSafeInteger};

/// What an endpoint of the package is for. Open. The form lists endpoints
/// by purpose: `client`, `admin`, `monitoring`, then by name.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EndpointPurpose {
    /// Where applications connect.
    Client,
    /// Where an administrator connects.
    Admin,
    /// Where metrics and health are read.
    Monitoring,
    /// A purpose this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// The kind of control a settings field gets. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FieldKind {
    /// Text.
    Text,
    /// A whole number.
    Integer,
    /// True or false.
    Boolean,
    /// One of a fixed set of texts.
    Choice,
    /// A list of items, each with scalar entries.
    List,
    /// A kind this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// Whether a settings field is shown at first or under Advanced. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FieldVisibility {
    /// Shown with the other fields.
    Basic,
    /// Hidden in a closed group unless it has an error.
    Advanced,
    /// A visibility this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// How a change to a settings field takes effect. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FieldApply {
    /// A running instance reloads it.
    Reload,
    /// A running instance restarts to apply it.
    Restart,
    /// Only the first initialization reads it; the form marks it "Set once".
    ImmutableAfterInit,
    /// A way this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// Where the package of a form comes from. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CreateProviderSource {
    /// The catalog that ships with Stackly.
    BundledCatalog,
    /// A development catalog.
    DevelopmentCatalog,
    /// A package already pinned in the data folder.
    Pinned,
    /// A source this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// A scalar settings value, tagged `type` and `content`:
/// `{ "type": "integer", "content": 5 }`.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", content = "content", rename_all = "camelCase")]
pub enum ScalarSetting {
    /// Text.
    Text(String),
    /// A whole number.
    Integer(SignedSafeInteger),
    /// True or false.
    Boolean(bool),
}

/// One entry of a list item: a key and its scalar value.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SettingEntry {
    /// The key of the entry, one of the item fields of the schema.
    pub key: String,
    /// Its value.
    pub value: ScalarSetting,
}

/// One item of a list setting, as ordered entries.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SettingItem {
    /// The entries, in the order the schema declares its item fields.
    pub entries: Vec<SettingEntry>,
}

/// A settings value, tagged `type` and `content`.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", content = "content", rename_all = "camelCase")]
pub enum SettingValue {
    /// Text.
    Text(String),
    /// A whole number.
    Integer(SignedSafeInteger),
    /// True or false.
    Boolean(bool),
    /// A list of items.
    List(Vec<SettingItem>),
}

/// Params of `instance.get_create_form`.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GetCreateFormParams {
    /// The installed runtime the instance will run on.
    pub runtime_id: Uuid,
    /// The provider to use, such as `official/nats`. Absent means the
    /// catalog entry whose software is the software of the runtime.
    #[serde(default)]
    pub provider: Option<String>,
}

impl Validate for GetCreateFormParams {
    fn validate(&self) -> Vec<FieldError> {
        self.provider
            .as_deref()
            .and_then(provider_id_code)
            .map(|code| vec![FieldError::new("provider", code)])
            .unwrap_or_default()
    }
}

/// The provider a form was built from.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateProvider {
    /// The provider ID, such as `official/nats`.
    pub name: String,
    /// The revision of the loaded catalog entry.
    pub revision: String,
    /// SHA-256 of the package, in lowercase hex.
    pub manifest_sha256: String,
    /// The name to show.
    pub display_name: String,
    /// The category.
    pub category: ProviderCategory,
    /// Where the package comes from.
    pub source: CreateProviderSource,
}

/// One endpoint of the package, as the form shows it.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateEndpoint {
    /// The endpoint name from the manifest.
    pub name: String,
    /// `tcp`, `udp`, or `unix`.
    pub transport: EndpointTransport,
    /// What it is for.
    pub purpose: EndpointPurpose,
    /// The loopback host, for `tcp` and `udp`.
    #[serde(default)]
    pub host: Option<String>,
    /// The port the form starts with, for `tcp` and `udp`.
    #[serde(default)]
    pub default_port: Option<u16>,
    /// The `socketName` template as the manifest writes it, for `unix`.
    #[serde(default)]
    pub socket_name: Option<String>,
}

/// One scalar field of a list item.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FormItemField {
    /// The key inside the item.
    pub key: String,
    /// The title to show.
    pub title: String,
    /// A sentence under the control.
    #[serde(default)]
    pub description: Option<String>,
    /// The kind of control: `text`, `integer`, `boolean`, or `choice`.
    pub kind: FieldKind,
    /// Whether every item must set it.
    pub required: bool,
    /// The value a new item starts with.
    #[serde(default)]
    pub default: Option<ScalarSetting>,
    /// The fewest characters of a text.
    #[serde(default)]
    pub min_length: Option<Count>,
    /// The most characters of a text.
    #[serde(default)]
    pub max_length: Option<Count>,
    /// The smallest integer.
    #[serde(default)]
    pub minimum: Option<SignedSafeInteger>,
    /// The largest integer.
    #[serde(default)]
    pub maximum: Option<SignedSafeInteger>,
    /// The values of a `choice`; empty otherwise.
    pub choices: Vec<String>,
}

/// One settings field, typed from the settings schema in the order the
/// schema declares it (VAL-012).
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FormField {
    /// The key in the settings document.
    pub key: String,
    /// The title to show.
    pub title: String,
    /// A sentence under the control.
    #[serde(default)]
    pub description: Option<String>,
    /// The kind of control.
    pub kind: FieldKind,
    /// Whether the settings must set it.
    pub required: bool,
    /// Whether the value is a secret, sent in `secrets` and never in
    /// `settings`.
    pub secret: bool,
    /// Whether a secret refuses control characters.
    pub printable_only: bool,
    /// Whether Stackly sets it; the form locks the control.
    pub managed: bool,
    /// Whether the field is shown at first.
    pub visibility: FieldVisibility,
    /// How a later change takes effect.
    #[serde(default)]
    pub apply: Option<FieldApply>,
    /// The value the form starts with.
    #[serde(default)]
    pub default: Option<SettingValue>,
    /// The fewest characters of a text, or of a secret.
    #[serde(default)]
    pub min_length: Option<Count>,
    /// The most characters of a text, or of a secret.
    #[serde(default)]
    pub max_length: Option<Count>,
    /// The smallest integer.
    #[serde(default)]
    pub minimum: Option<SignedSafeInteger>,
    /// The largest integer.
    #[serde(default)]
    pub maximum: Option<SignedSafeInteger>,
    /// The values of a `choice`; empty otherwise.
    pub choices: Vec<String>,
    /// The fewest items of a `list`.
    #[serde(default)]
    pub min_items: Option<Count>,
    /// The most items of a `list`.
    #[serde(default)]
    pub max_items: Option<Count>,
    /// The item field whose value must be unique across the items.
    #[serde(default)]
    pub item_key: Option<String>,
    /// The fields of one item of a `list`; empty otherwise.
    pub items: Vec<FormItemField>,
}

/// Reply of `instance.get_create_form`.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateForm {
    /// The provider the form was built from.
    pub provider: CreateProvider,
    /// The runtime the instance will run on.
    pub runtime: RuntimeSummary,
    /// The name the form starts with; absent when every candidate is taken.
    #[serde(default)]
    pub suggested_name: Option<String>,
    /// Where the data lives, relative to the data root:
    /// `instances/<new instance id>/data`.
    pub data_path: String,
    /// The endpoints of the package: purpose `client`, `admin`,
    /// `monitoring`, then name.
    pub endpoints: Vec<CreateEndpoint>,
    /// The settings fields, in the order the schema declares them.
    pub fields: Vec<FormField>,
    /// The capabilities of the package, in the order of spec 0017.
    pub capabilities: Vec<CapabilityStatus>,
}

/// Where a secret field gets its value, without the value. Tagged `type`
/// and `content`.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", content = "content", rename_all = "camelCase")]
pub enum SecretSource {
    /// The daemon generates a value.
    Generate,
    /// The client sends a value with `instance.create`.
    Value,
    /// A copy of the value stored under this reference.
    CopyFrom(String),
    /// The instance uses this reference.
    Bind(String),
}

/// The source of one secret field in a preflight.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SecretPlan {
    /// The settings field the secret fills.
    pub field: String,
    /// Where its value comes from.
    pub source: SecretSource,
}

/// Params of `instance.preflight`: the draft of `instance.create` without
/// any secret value.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PreflightCreateParams {
    /// The unique instance name.
    pub name: String,
    /// The name to show.
    #[serde(default)]
    pub display_name: Option<String>,
    /// The provider to create it from.
    pub provider: ProviderSelector,
    /// The installed runtime to run it on.
    pub runtime_id: Uuid,
    /// One request per manifest endpoint that asks for a port.
    #[serde(default)]
    pub endpoints: Vec<EndpointRequest>,
    /// The settings, without secret values.
    pub settings: SettingsDocument,
    /// The source of each secret field.
    #[serde(default)]
    pub secrets: Vec<SecretPlan>,
    /// Whether the equivalent command ends with `--start`.
    #[serde(default)]
    pub start: bool,
}

impl Validate for PreflightCreateParams {
    /// The same sizes and counts as [`CreateInstanceParams`](super::CreateInstanceParams),
    /// and the reference format of `copyFrom` and `bind`, so a draft that
    /// preflight accepts is never refused by `instance.create` for its
    /// shape.
    fn validate(&self) -> Vec<FieldError> {
        let mut fields = Vec::new();
        if let Some(code) = length_code(&self.name, INSTANCE_NAME_MAX) {
            fields.push(FieldError::new("name", code));
        }
        if let Some(display_name) = &self.display_name
            && let Some(code) = length_code(display_name, INSTANCE_DISPLAY_NAME_MAX)
        {
            fields.push(FieldError::new("displayName", code));
        }
        if let Some(code) = provider_id_code(&self.provider.name) {
            fields.push(FieldError::new("provider.name", code));
        }
        if let Some(revision) = &self.provider.revision
            && let Some(code) = length_code(revision, PROVIDER_REVISION_MAX)
        {
            fields.push(FieldError::new("provider.revision", code));
        }
        if settings_has_unsafe_integer(self.settings.as_map()) {
            fields.push(FieldError::new("settings", FieldCode::OutOfRange));
        }
        if self.endpoints.len() > CREATE_ENDPOINTS_MAX {
            fields.push(FieldError::new("endpoints", FieldCode::TooMany));
        }
        for (index, endpoint) in self.endpoints.iter().enumerate().take(CREATE_ENDPOINTS_MAX) {
            if let Some(code) = length_code(&endpoint.name, INSTANCE_KEY_MAX) {
                fields.push(FieldError::new(format!("endpoints[{index}].name"), code));
            }
        }
        if self.secrets.len() > CREATE_SECRETS_MAX {
            fields.push(FieldError::new("secrets", FieldCode::TooMany));
        }
        for (index, secret) in self.secrets.iter().enumerate().take(CREATE_SECRETS_MAX) {
            if let Some(code) = length_code(&secret.field, INSTANCE_KEY_MAX) {
                fields.push(FieldError::new(format!("secrets[{index}].field"), code));
            }
            if let SecretSource::CopyFrom(reference) | SecretSource::Bind(reference) =
                &secret.source
                && let Some(code) = reference_field_code(reference)
            {
                fields.push(FieldError::new(
                    format!("secrets[{index}].source.content"),
                    code,
                ));
            }
        }
        fields
    }
}

/// What kind of row a preflight check is. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PreflightCheckKind {
    /// The provider package.
    Provider,
    /// The runtime.
    Runtime,
    /// The instance name.
    Name,
    /// One endpoint.
    Endpoint,
    /// The settings.
    Settings,
    /// The data folder.
    Data,
    /// The free disk space.
    Disk,
    /// One secret field.
    Secret,
    /// A kind this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// How a preflight row went. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PreflightStatus {
    /// Fine.
    Ok,
    /// Fine as far as Stackly can tell, with a note.
    Caution,
    /// Create would be refused.
    Error,
    /// A status this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// The package a preflight resolved.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedProvider {
    /// The provider ID.
    pub name: String,
    /// The revision.
    pub revision: String,
    /// SHA-256 of the package, in lowercase hex.
    pub manifest_sha256: String,
}

/// One row of a preflight. The English `title` and `detail` are the daemon's
/// text, the same in the sheet and on the command line.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PreflightCheck {
    /// `provider`, `runtime`, `name`, `endpoint.<name>`,
    /// `endpoint.request.<i>`, `settings`, `data`, `disk`, or
    /// `secret.<field>`.
    pub id: String,
    /// What the row is about.
    pub kind: PreflightCheckKind,
    /// How it went.
    pub status: PreflightStatus,
    /// The short title, at most 200 characters.
    pub title: String,
    /// The sentence under the title, at most 200 characters.
    pub detail: String,
    /// The endpoint name or the field key the row is about.
    #[serde(default)]
    pub subject: Option<String>,
    /// A free port offered for a fixed port that is taken. The form uses it
    /// only when the person asks.
    #[serde(default)]
    pub suggested_port: Option<u16>,
    /// The error `instance.create` answers with for this row; only for the
    /// status `error`.
    #[serde(default)]
    pub error: Option<ErrorBody>,
}

/// Reply of `instance.preflight`.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreatePreflight {
    /// True exactly when no row is an error.
    pub ready: bool,
    /// The package the draft resolved to, when one could be determined.
    #[serde(default)]
    pub provider: Option<ResolvedProvider>,
    /// The rows, in the order of spec 0020.
    pub checks: Vec<PreflightCheck>,
    /// Every field issue of the draft, sorted by path.
    pub fields: Vec<FieldError>,
    /// The `stackly instance create` command that does the same.
    pub command: String,
}
