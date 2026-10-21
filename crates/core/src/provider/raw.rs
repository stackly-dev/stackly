//! Raw decode types for `stackly/v1alpha1`, and the strict YAML and JSON
//! decoders that fill them.
//!
//! Every object denies unknown fields. Names, versions, enums, and tags stay
//! strings or plain numbers here; the semantic pass in `validate` turns them
//! into domain types and closed enums, so a wrong value gets a specific code
//! and its field path. Variants share one flat struct with optional fields,
//! and the semantic pass rejects a field that does not belong to the chosen
//! variant.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::fmt;

use serde::de::{
    self, DeserializeOwned, DeserializeSeed, EnumAccess, MapAccess, SeqAccess, VariantAccess,
    Visitor,
};
use serde::{Deserialize, Deserializer};

use super::issue::{Issue, IssueCode, Location};

/// The only `apiVersion` this engine reads.
pub(crate) const API_VERSION: &str = "stackly/v1alpha1";
/// The only `kind` this engine reads.
pub(crate) const KIND: &str = "Provider";

/// `provider.yaml`.
#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[cfg_attr(
    feature = "json-schema",
    schemars(
        rename = "ProviderManifest",
        title = "Stackly provider manifest (stackly/v1alpha1)"
    )
)]
pub(crate) struct RawManifest {
    #[expect(dead_code, reason = "checked by the header stage before this decode")]
    #[cfg_attr(feature = "json-schema", schemars(extend("const" = API_VERSION)))]
    pub(crate) api_version: String,
    #[expect(dead_code, reason = "checked by the header stage before this decode")]
    #[cfg_attr(feature = "json-schema", schemars(extend("const" = KIND)))]
    pub(crate) kind: String,
    pub(crate) metadata: RawMetadata,
    pub(crate) compatibility: RawCompatibility,
    pub(crate) runtime: RawRuntime,
    #[serde(default)]
    pub(crate) capabilities: RawCapabilities,
    pub(crate) resources: RawResources,
    #[serde(default)]
    #[cfg_attr(feature = "json-schema", schemars(extend("propertyNames" = { "pattern": super::json_schema::MANIFEST_NAME }, "maxProperties" = 16)))]
    pub(crate) config_files: BTreeMap<String, RawConfigFile>,
    pub(crate) lifecycle: RawLifecycle,
    pub(crate) readiness: RawReadiness,
    pub(crate) health: Option<RawHealth>,
    #[serde(default)]
    pub(crate) timeouts: RawTimeouts,
    pub(crate) logs: Option<RawLogs>,
    pub(crate) backup: Option<RawBackup>,
    pub(crate) permissions: RawPermissions,
    #[cfg_attr(feature = "json-schema", schemars(length(min = 1, max = 8)))]
    pub(crate) connection: Option<Vec<RawConnection>>,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[cfg_attr(feature = "json-schema", schemars(rename = "Metadata"))]
pub(crate) struct RawMetadata {
    #[cfg_attr(
        feature = "json-schema",
        schemars(pattern(super::json_schema::PROVIDER_NAME), length(max = 127))
    )]
    pub(crate) id: String,
    #[cfg_attr(
        feature = "json-schema",
        schemars(pattern(super::json_schema::NAME), length(max = 63))
    )]
    pub(crate) software: String,
    #[cfg_attr(feature = "json-schema", schemars(length(min = 1, max = 64)))]
    pub(crate) display_name: String,
    #[cfg_attr(
        feature = "json-schema",
        schemars(pattern(super::json_schema::REVISION_LABEL), length(max = 64))
    )]
    pub(crate) revision: String,
    pub(crate) category: String,
    pub(crate) description: Option<String>,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[cfg_attr(feature = "json-schema", schemars(rename = "Compatibility"))]
pub(crate) struct RawCompatibility {
    pub(crate) platforms: Vec<RawPlatform>,
    #[serde(default)]
    #[cfg_attr(feature = "json-schema", schemars(extend("propertyNames" = { "pattern": super::json_schema::RUNTIME_LINE }, "additionalProperties" = { "type": "array", "items": { "type": "string", "pattern": super::json_schema::RUNTIME_LINE } })))]
    pub(crate) line_compatibility: BTreeMap<String, Vec<String>>,
    pub(crate) data_marker: Option<RawDataMarker>,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[cfg_attr(feature = "json-schema", schemars(rename = "Platform"))]
pub(crate) struct RawPlatform {
    pub(crate) os: String,
    pub(crate) arch: String,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[cfg_attr(feature = "json-schema", schemars(rename = "DataMarker"))]
pub(crate) struct RawDataMarker {
    pub(crate) file: String,
    pub(crate) pattern: String,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[cfg_attr(feature = "json-schema", schemars(rename = "Runtime"))]
pub(crate) struct RawRuntime {
    pub(crate) executable: String,
    pub(crate) execution_mode: String,
    pub(crate) version_check: Option<RawVersionCheck>,
}

/// `runtime.versionCheck`: how the installer reads the version of a
/// runtime once (spec 0018, *Validasi binary* step 8).
#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[cfg_attr(feature = "json-schema", schemars(rename = "VersionCheck"))]
pub(crate) struct RawVersionCheck {
    #[cfg_attr(feature = "json-schema", schemars(length(max = 8)))]
    pub(crate) args: Vec<String>,
    #[cfg_attr(feature = "json-schema", schemars(length(min = 1, max = 512)))]
    pub(crate) pattern: String,
    #[cfg_attr(feature = "json-schema", schemars(range(min = 1, max = 60)))]
    pub(crate) timeout_seconds: Option<u64>,
}

#[derive(Debug, Default, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[cfg_attr(feature = "json-schema", schemars(rename = "Capabilities"))]
pub(crate) struct RawCapabilities {
    #[serde(default)]
    pub(crate) reload: bool,
    #[serde(default)]
    pub(crate) metrics: bool,
    #[serde(default)]
    pub(crate) tls: bool,
    #[serde(default)]
    pub(crate) multi_instance: bool,
    #[serde(default)]
    pub(crate) persistence: bool,
    #[serde(default)]
    pub(crate) backup: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[cfg_attr(feature = "json-schema", schemars(rename = "Resources"))]
pub(crate) struct RawResources {
    #[cfg_attr(feature = "json-schema", schemars(extend("propertyNames" = { "pattern": super::json_schema::ENDPOINT_NAME }, "minProperties" = 1, "maxProperties" = 16)))]
    pub(crate) endpoints: BTreeMap<String, RawEndpoint>,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[cfg_attr(feature = "json-schema", schemars(rename = "Endpoint"))]
pub(crate) struct RawEndpoint {
    pub(crate) transport: String,
    pub(crate) purpose: String,
    pub(crate) default_host: Option<String>,
    #[cfg_attr(feature = "json-schema", schemars(range(min = 1, max = 65535)))]
    pub(crate) default_port: Option<u64>,
    pub(crate) socket_name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[cfg_attr(feature = "json-schema", schemars(rename = "ConfigFile"))]
pub(crate) struct RawConfigFile {
    #[cfg_attr(
        feature = "json-schema",
        schemars(pattern(super::json_schema::TEMPLATE_PATH))
    )]
    pub(crate) template: String,
    pub(crate) format: String,
    pub(crate) destination: String,
    #[serde(default)]
    pub(crate) sensitive: bool,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[cfg_attr(feature = "json-schema", schemars(rename = "Lifecycle"))]
pub(crate) struct RawLifecycle {
    #[serde(default)]
    pub(crate) initialize: Vec<RawStep>,
    pub(crate) start: RawCommand,
    pub(crate) stop: RawStop,
    pub(crate) reload: Option<RawReload>,
}

/// One initialize step; `primitive` picks which other fields belong.
#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[cfg_attr(feature = "json-schema", schemars(rename = "Step"))]
pub(crate) struct RawStep {
    pub(crate) primitive: String,
    pub(crate) file: Option<String>,
    pub(crate) secret: Option<String>,
    pub(crate) destination: Option<String>,
    pub(crate) executable: Option<String>,
    pub(crate) args: Option<Vec<String>>,
    pub(crate) cwd: Option<String>,
    #[cfg_attr(feature = "json-schema", schemars(extend("propertyNames" = { "pattern": super::json_schema::ENV_KEY }, "maxProperties" = 32)))]
    pub(crate) env: Option<BTreeMap<String, String>>,
    pub(crate) stdin: Option<RawStdin>,
    pub(crate) timeout_seconds: Option<u64>,
    pub(crate) success_exit_codes: Option<Vec<u8>>,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[cfg_attr(
    feature = "json-schema",
    schemars(rename = "Stdin", extend("minProperties" = 1, "maxProperties" = 1))
)]
pub(crate) struct RawStdin {
    /// A secret field whose exact bytes go to stdin.
    pub(crate) secret: Option<String>,
    /// Literal text written to stdin exactly (spec 0019).
    pub(crate) text: Option<String>,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[cfg_attr(feature = "json-schema", schemars(rename = "Command"))]
pub(crate) struct RawCommand {
    pub(crate) executable: String,
    #[serde(default)]
    pub(crate) args: Vec<String>,
    pub(crate) cwd: Option<String>,
    #[cfg_attr(feature = "json-schema", schemars(extend("propertyNames" = { "pattern": super::json_schema::ENV_KEY }, "maxProperties" = 32)))]
    #[serde(default)]
    pub(crate) env: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[cfg_attr(feature = "json-schema", schemars(rename = "Stop"))]
pub(crate) struct RawStop {
    pub(crate) strategy: String,
    pub(crate) signal: Option<String>,
    pub(crate) command: Option<RawCommand>,
    pub(crate) success_exit_codes: Option<Vec<u8>>,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[cfg_attr(feature = "json-schema", schemars(rename = "Reload"))]
pub(crate) struct RawReload {
    #[serde(default)]
    pub(crate) render: Vec<String>,
    pub(crate) action: String,
    pub(crate) signal: Option<String>,
    pub(crate) command: Option<RawCommand>,
    pub(crate) success_exit_codes: Option<Vec<u8>>,
    pub(crate) verify: Option<String>,
}

/// One probe; `type` picks which other fields belong.
#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[cfg_attr(feature = "json-schema", schemars(rename = "Probe"))]
pub(crate) struct RawProbe {
    #[serde(rename = "type")]
    pub(crate) kind: String,
    pub(crate) endpoint: Option<String>,
    pub(crate) path: Option<String>,
    pub(crate) expect_status: Option<RawStatusRange>,
    pub(crate) command: Option<RawCommand>,
    pub(crate) success_exit_codes: Option<Vec<u8>>,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[cfg_attr(feature = "json-schema", schemars(rename = "StatusRange"))]
pub(crate) struct RawStatusRange {
    pub(crate) min: u16,
    pub(crate) max: u16,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[cfg_attr(feature = "json-schema", schemars(rename = "Readiness"))]
pub(crate) struct RawReadiness {
    pub(crate) probe: RawProbe,
    pub(crate) interval_milliseconds: Option<u64>,
    pub(crate) attempt_timeout_milliseconds: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[cfg_attr(feature = "json-schema", schemars(rename = "Health"))]
pub(crate) struct RawHealth {
    pub(crate) probe: Option<RawProbe>,
    pub(crate) interval_seconds: Option<u64>,
    pub(crate) attempt_timeout_milliseconds: Option<u64>,
    pub(crate) failure_threshold: Option<u64>,
}

#[derive(Debug, Default, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[cfg_attr(feature = "json-schema", schemars(rename = "Timeouts"))]
pub(crate) struct RawTimeouts {
    pub(crate) init_seconds: Option<u64>,
    pub(crate) readiness_seconds: Option<u64>,
    pub(crate) recovery_seconds: Option<u64>,
    pub(crate) stop_seconds: Option<u64>,
    pub(crate) reload_seconds: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[cfg_attr(feature = "json-schema", schemars(rename = "Logs"))]
pub(crate) struct RawLogs {
    #[serde(default)]
    pub(crate) files: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[cfg_attr(feature = "json-schema", schemars(rename = "Backup"))]
pub(crate) struct RawBackup {
    pub(crate) cold: Option<RawColdBackup>,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[cfg_attr(feature = "json-schema", schemars(rename = "ColdBackup"))]
pub(crate) struct RawColdBackup {
    pub(crate) paths: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[cfg_attr(feature = "json-schema", schemars(rename = "Permissions"))]
pub(crate) struct RawPermissions {
    pub(crate) managed_filesystem: Vec<String>,
    pub(crate) execute_runtime_binary: bool,
    #[serde(default)]
    #[cfg_attr(feature = "json-schema", schemars(extend("propertyNames" = { "pattern": super::json_schema::MANIFEST_NAME }, "maxProperties" = 16)))]
    pub(crate) additional_executables: BTreeMap<String, String>,
}

/// One `connection` entry: a named URI template (spec 0021).
#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[cfg_attr(feature = "json-schema", schemars(rename = "Connection"))]
pub(crate) struct RawConnection {
    #[cfg_attr(
        feature = "json-schema",
        schemars(pattern(super::json_schema::CONNECTION_NAME))
    )]
    pub(crate) name: String,
    #[cfg_attr(feature = "json-schema", schemars(length(min = 1, max = 64)))]
    pub(crate) title: String,
    #[cfg_attr(feature = "json-schema", schemars(length(min = 1, max = 512)))]
    pub(crate) template: String,
}

/// `schemas/settings.schema.json`.
#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
#[cfg_attr(
    feature = "json-schema",
    schemars(
        rename = "SettingsSchema",
        title = "Stackly provider settings schema (v1alpha1 dialect)"
    )
)]
pub(crate) struct RawSettingsSchema {
    #[serde(rename = "$schema")]
    #[cfg_attr(feature = "json-schema", schemars(extend("const" = super::settings::SCHEMA_DIALECT)))]
    pub(crate) schema: String,
    pub(crate) title: String,
    pub(crate) description: Option<String>,
    #[serde(rename = "type")]
    pub(crate) kind: String,
    #[serde(rename = "additionalProperties")]
    pub(crate) additional_properties: bool,
    #[cfg_attr(feature = "json-schema", schemars(extend("propertyNames" = { "pattern": super::json_schema::MANIFEST_NAME }, "maxProperties" = 64)))]
    pub(crate) properties: Ordered<RawSettingField>,
    #[serde(default)]
    pub(crate) required: Vec<String>,
}

/// One root field of the settings schema.
#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "json-schema", schemars(rename = "SettingField"))]
pub(crate) struct RawSettingField {
    #[serde(rename = "type")]
    pub(crate) kind: String,
    pub(crate) title: String,
    pub(crate) description: Option<String>,
    pub(crate) default: Option<serde_json::Value>,
    #[serde(rename = "enum")]
    pub(crate) enum_values: Option<Vec<String>>,
    #[serde(rename = "minLength")]
    pub(crate) min_length: Option<u64>,
    #[serde(rename = "maxLength")]
    pub(crate) max_length: Option<u64>,
    pub(crate) pattern: Option<String>,
    pub(crate) minimum: Option<i64>,
    pub(crate) maximum: Option<i64>,
    pub(crate) items: Option<RawItems>,
    #[serde(rename = "minItems")]
    pub(crate) min_items: Option<u64>,
    #[serde(rename = "maxItems")]
    pub(crate) max_items: Option<u64>,
    #[serde(rename = "x-stackly")]
    pub(crate) x_stackly: Option<RawXStackly>,
}

/// The `items` of an array field: one level of object.
#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "json-schema", schemars(rename = "Items"))]
pub(crate) struct RawItems {
    #[serde(rename = "type")]
    pub(crate) kind: String,
    #[serde(rename = "additionalProperties")]
    pub(crate) additional_properties: bool,
    #[cfg_attr(feature = "json-schema", schemars(extend("propertyNames" = { "pattern": super::json_schema::MANIFEST_NAME }, "maxProperties" = 16)))]
    pub(crate) properties: Ordered<RawItemField>,
    #[serde(default)]
    pub(crate) required: Vec<String>,
}

/// One scalar field of an array item.
#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "json-schema", schemars(rename = "ItemField"))]
pub(crate) struct RawItemField {
    #[serde(rename = "type")]
    pub(crate) kind: String,
    pub(crate) title: String,
    pub(crate) description: Option<String>,
    pub(crate) default: Option<serde_json::Value>,
    #[serde(rename = "enum")]
    pub(crate) enum_values: Option<Vec<String>>,
    #[serde(rename = "minLength")]
    pub(crate) min_length: Option<u64>,
    #[serde(rename = "maxLength")]
    pub(crate) max_length: Option<u64>,
    pub(crate) pattern: Option<String>,
    pub(crate) minimum: Option<i64>,
    pub(crate) maximum: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "json-schema", schemars(rename = "XStackly"))]
pub(crate) struct RawXStackly {
    pub(crate) apply: Option<String>,
    pub(crate) visibility: Option<String>,
    pub(crate) secret: Option<bool>,
    /// Whether a secret value must hold no control character (spec 0019).
    #[serde(rename = "printableOnly")]
    pub(crate) printable_only: Option<bool>,
    pub(crate) managed: Option<String>,
    pub(crate) key: Option<String>,
}

/// `versions.json`, schema `stackly.provider-versions/v1`.
#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[cfg_attr(
    feature = "json-schema",
    schemars(
        rename = "ProviderVersions",
        title = "Stackly provider versions (stackly.provider-versions/v1)"
    )
)]
pub(crate) struct RawVersions {
    #[cfg_attr(feature = "json-schema", schemars(extend("const" = super::versions::VERSIONS_SCHEMA)))]
    pub(crate) schema: String,
    #[cfg_attr(
        feature = "json-schema",
        schemars(pattern(super::json_schema::NAME), length(max = 63))
    )]
    pub(crate) software: String,
    pub(crate) artifacts: Vec<RawArtifact>,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[cfg_attr(feature = "json-schema", schemars(rename = "Artifact"))]
pub(crate) struct RawArtifact {
    #[cfg_attr(
        feature = "json-schema",
        schemars(pattern(super::json_schema::ARTIFACT_ID))
    )]
    pub(crate) id: String,
    #[cfg_attr(
        feature = "json-schema",
        schemars(pattern(super::json_schema::RUNTIME_LINE))
    )]
    pub(crate) line: String,
    #[cfg_attr(
        feature = "json-schema",
        schemars(pattern(super::json_schema::EXACT_VERSION), length(max = 64))
    )]
    pub(crate) version: String,
    pub(crate) os: String,
    pub(crate) arch: String,
    pub(crate) status: String,
    pub(crate) min_os: Option<String>,
    pub(crate) format: Option<String>,
    pub(crate) urls: Option<Vec<String>>,
    pub(crate) sha256: Option<String>,
    pub(crate) size: Option<u64>,
    pub(crate) archive_root: Option<String>,
    pub(crate) signing_policy: Option<String>,
    pub(crate) license: Option<String>,
    pub(crate) license_url: Option<String>,
    pub(crate) executables: Option<Vec<String>>,
    pub(crate) blocked: Option<RawBlocked>,
    pub(crate) stable: Option<bool>,
    #[cfg_attr(feature = "json-schema", schemars(range(min = 1)))]
    pub(crate) build: Option<u32>,
    pub(crate) unpacked_size: Option<u64>,
    pub(crate) entry_count: Option<u64>,
    pub(crate) dependencies: Option<Vec<RawDependency>>,
}

/// One library an available artifact links (spec 0009, fase 02 fields).
#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[cfg_attr(feature = "json-schema", schemars(rename = "Dependency"))]
pub(crate) struct RawDependency {
    pub(crate) name: String,
    pub(crate) kind: String,
    #[cfg_attr(feature = "json-schema", schemars(length(min = 1, max = 1024)))]
    pub(crate) install_name: String,
    pub(crate) version: Option<String>,
    pub(crate) license: Option<String>,
    pub(crate) source_url: Option<String>,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[cfg_attr(feature = "json-schema", schemars(rename = "Blocked"))]
pub(crate) struct RawBlocked {
    pub(crate) reason: String,
    pub(crate) owner: String,
    #[cfg_attr(feature = "json-schema", schemars(pattern(super::json_schema::DATE)))]
    pub(crate) since: String,
}

/// Parses YAML text into a value. Aliases may expand the document to at
/// most [`yaml_node_budget`] nodes, and the rest stays within the recursion
/// and repetition limits of `serde_yaml_ng`; tags and merge keys are
/// rejected afterwards by [`check_yaml_value`].
pub(crate) fn parse_yaml(file: &str, text: &str) -> Result<serde_yaml_ng::Value, Issue> {
    yaml_value(text).map_err(|err| match err {
        YamlError::TooLarge { budget } => Issue::new(
            file,
            Location::File,
            IssueCode::YamlSyntax,
            format!("aliases expand the document past {budget} nodes"),
        ),
        YamlError::Parse(err) => {
            let location = err
                .location()
                .map_or(Location::File, |at| Location::Position {
                    line: at.line(),
                    column: at.column(),
                });
            let message = clean_message(&strip_position(&err.to_string()));
            let code = if message.contains("duplicate entry") {
                IssueCode::DuplicateKey
            } else {
                IssueCode::YamlSyntax
            };
            Issue::new(file, location, code, message)
        }
    })
}

/// Nodes a YAML document may hold for each byte of its text, once its
/// aliases expand. Without aliases a node takes at least one byte, so only
/// alias expansion reaches the budget.
const YAML_NODES_PER_BYTE: usize = 2;
/// Nodes any YAML document may hold besides [`YAML_NODES_PER_BYTE`], so a
/// short text may still use a few aliases.
const YAML_NODES_EXTRA: usize = 1024;

/// The most nodes a YAML text of `len` bytes may expand to.
pub(crate) const fn yaml_node_budget(len: usize) -> usize {
    len.saturating_mul(YAML_NODES_PER_BYTE)
        .saturating_add(YAML_NODES_EXTRA)
}

/// Error of [`yaml_value`].
#[derive(Debug)]
pub(crate) enum YamlError {
    /// Aliases expand the document past its node budget.
    TooLarge {
        /// The budget, in nodes.
        budget: usize,
    },
    /// `serde_yaml_ng` refused the text.
    Parse(serde_yaml_ng::Error),
}

/// Parses YAML text into a value, refusing a document whose aliases expand
/// it past [`yaml_node_budget`] nodes.
///
/// `serde_yaml_ng` copies the anchored node for every alias and limits only
/// the number of alias jumps, so one large anchor behind a short chain of
/// aliases expands to millions of nodes (a billion laughs with a big
/// anchor). A first pass therefore visits the document without storing it
/// and stops at the budget; only a document within the budget is built.
pub(crate) fn yaml_value(text: &str) -> Result<serde_yaml_ng::Value, YamlError> {
    let budget = yaml_node_budget(text.len());
    let counter = NodeCounter {
        left: Cell::new(budget),
        exceeded: Cell::new(false),
    };
    match NodeBudget(&counter).deserialize(serde_yaml_ng::Deserializer::from_str(text)) {
        Ok(()) => serde_yaml_ng::from_str(text).map_err(YamlError::Parse),
        Err(_) if counter.exceeded.get() => Err(YamlError::TooLarge { budget }),
        // The counting visitor accepts every kind of value, so any other
        // failure is one of the deserializer itself (syntax, recursion,
        // repetition), which the real parse would meet the same way: report
        // it rather than parse a hostile document twice.
        Err(err) => Err(YamlError::Parse(err)),
    }
}

/// The nodes a counting pass may still visit.
struct NodeCounter {
    left: Cell<usize>,
    exceeded: Cell<bool>,
}

impl NodeCounter {
    /// Counts one node, failing once the budget is spent.
    fn tick<E: de::Error>(&self) -> Result<(), E> {
        match self.left.get().checked_sub(1) {
            Some(left) => {
                self.left.set(left);
                Ok(())
            }
            None => {
                self.exceeded.set(true);
                Err(E::custom("the yaml document exceeds its node budget"))
            }
        }
    }
}

/// Visits one YAML node and everything under it, aliases expanded, storing
/// nothing.
#[derive(Clone, Copy)]
struct NodeBudget<'a>(&'a NodeCounter);

impl<'de> DeserializeSeed<'de> for NodeBudget<'_> {
    type Value = ();

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for NodeBudget<'_> {
    type Value = ();

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("any YAML value")
    }

    fn visit_bool<E: de::Error>(self, _: bool) -> Result<(), E> {
        self.0.tick()
    }

    fn visit_i64<E: de::Error>(self, _: i64) -> Result<(), E> {
        self.0.tick()
    }

    fn visit_i128<E: de::Error>(self, _: i128) -> Result<(), E> {
        self.0.tick()
    }

    fn visit_u64<E: de::Error>(self, _: u64) -> Result<(), E> {
        self.0.tick()
    }

    fn visit_u128<E: de::Error>(self, _: u128) -> Result<(), E> {
        self.0.tick()
    }

    fn visit_f64<E: de::Error>(self, _: f64) -> Result<(), E> {
        self.0.tick()
    }

    fn visit_char<E: de::Error>(self, _: char) -> Result<(), E> {
        self.0.tick()
    }

    fn visit_str<E: de::Error>(self, _: &str) -> Result<(), E> {
        self.0.tick()
    }

    fn visit_bytes<E: de::Error>(self, _: &[u8]) -> Result<(), E> {
        self.0.tick()
    }

    fn visit_unit<E: de::Error>(self) -> Result<(), E> {
        self.0.tick()
    }

    fn visit_none<E: de::Error>(self) -> Result<(), E> {
        self.0.tick()
    }

    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        self.0.tick()?;
        deserializer.deserialize_any(self)
    }

    fn visit_newtype_struct<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        self.0.tick()?;
        deserializer.deserialize_any(self)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<(), A::Error> {
        self.0.tick()?;
        while seq.next_element_seed(self)?.is_some() {}
        Ok(())
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        self.0.tick()?;
        while map.next_key_seed(self)?.is_some() {
            map.next_value_seed(self)?;
        }
        Ok(())
    }

    /// A tagged node: the tag, then its content.
    fn visit_enum<A: EnumAccess<'de>>(self, data: A) -> Result<(), A::Error> {
        self.0.tick()?;
        let ((), content) = data.variant_seed(self)?;
        content.newtype_variant_seed(self)
    }
}

/// Rejects YAML tags and merge keys anywhere in the value.
pub(crate) fn check_yaml_value(file: &str, value: &serde_yaml_ng::Value) -> Result<(), Issue> {
    fn walk(file: &str, value: &serde_yaml_ng::Value, path: &str) -> Result<(), Issue> {
        use serde_yaml_ng::Value;
        match value {
            Value::Tagged(_) => Err(Issue::new(
                file,
                Location::Field(display_path(path)),
                IssueCode::InvalidValue,
                "yaml tags are not allowed",
            )),
            Value::Sequence(items) => items
                .iter()
                .enumerate()
                .try_for_each(|(index, item)| walk(file, item, &format!("{path}[{index}]"))),
            Value::Mapping(map) => map.iter().try_for_each(|(key, item)| {
                let key = match key {
                    Value::String(key) => key.clone(),
                    Value::Tagged(_) => {
                        return Err(Issue::new(
                            file,
                            Location::Field(display_path(path)),
                            IssueCode::InvalidValue,
                            "yaml tags are not allowed",
                        ));
                    }
                    other => format!("{other:?}"),
                };
                let child = join_path(path, &key);
                if key == "<<" {
                    return Err(Issue::new(
                        file,
                        Location::Field(child),
                        IssueCode::InvalidValue,
                        "yaml merge keys are not allowed",
                    ));
                }
                walk(file, item, &child)
            }),
            _ => Ok(()),
        }
    }
    walk(file, value, "")
}

/// Parses JSON bytes into a value, rejecting a key that appears twice in
/// one object.
pub(crate) fn parse_json(file: &str, text: &str) -> Result<serde_json::Value, Issue> {
    let mut deserializer = serde_json::Deserializer::from_str(text);
    let result = StrictJson::deserialize(&mut deserializer).and_then(|value| {
        deserializer.end()?;
        Ok(value.0)
    });
    result.map_err(|err| {
        let location = if err.line() == 0 {
            Location::File
        } else {
            Location::Position {
                line: err.line(),
                column: err.column(),
            }
        };
        let code = if err.is_data() {
            IssueCode::DuplicateKey
        } else {
            IssueCode::JsonSyntax
        };
        Issue::new(
            file,
            location,
            code,
            clean_message(&strip_position(&err.to_string())),
        )
    })
}

/// Decodes a YAML value into a raw type, reporting the first problem with
/// its field path. Unknown fields get `unknown_code`.
pub(crate) fn decode_yaml<T: DeserializeOwned>(
    file: &str,
    value: serde_yaml_ng::Value,
    unknown_code: IssueCode,
) -> Result<T, Issue> {
    serde_path_to_error::deserialize(value).map_err(|err| {
        path_issue(
            file,
            &err.path().to_string(),
            &err.inner().to_string(),
            unknown_code,
        )
    })
}

/// Decodes JSON text into a raw type, reporting the first problem with its
/// field path, for the types that keep the order of an object
/// ([`Ordered`]), which a [`serde_json::Value`] would sort away. The text
/// must already have passed [`parse_json`], so it is valid and has no
/// repeated key. Unknown fields get `unknown_code`.
pub(crate) fn decode_json_text<T: DeserializeOwned>(
    file: &str,
    text: &str,
    unknown_code: IssueCode,
) -> Result<T, Issue> {
    let mut deserializer = serde_json::Deserializer::from_str(text);
    serde_path_to_error::deserialize(&mut deserializer).map_err(|err| {
        path_issue(
            file,
            &err.path().to_string(),
            &strip_position(&err.inner().to_string()),
            unknown_code,
        )
    })
}

/// Decodes a JSON value into a raw type, reporting the first problem with
/// its field path. Unknown fields get `unknown_code`.
pub(crate) fn decode_json<T: DeserializeOwned>(
    file: &str,
    value: serde_json::Value,
    unknown_code: IssueCode,
) -> Result<T, Issue> {
    serde_path_to_error::deserialize(value).map_err(|err| {
        path_issue(
            file,
            &err.path().to_string(),
            &err.inner().to_string(),
            unknown_code,
        )
    })
}

/// Turns a serde error at `path` into an issue, naming the field that is
/// unknown, missing, or repeated.
fn path_issue(file: &str, path: &str, message: &str, unknown_code: IssueCode) -> Issue {
    let path = if path == "." { "" } else { path };
    let field = |prefix: &str| {
        message
            .strip_prefix(prefix)
            .and_then(|rest| rest.split('`').next())
            .map(|name| join_path(path, name))
    };
    let (code, location) = if message.starts_with("unknown field `") {
        (unknown_code, display_path(path))
    } else if let Some(at) = field("missing field `") {
        (IssueCode::MissingField, at)
    } else if let Some(at) = field("duplicate field `") {
        (IssueCode::DuplicateKey, at)
    } else {
        (IssueCode::InvalidValue, display_path(path))
    };
    Issue::new(
        file,
        Location::Field(location),
        code,
        clean_message(message),
    )
}

/// Joins a field name onto a path.
pub(crate) fn join_path(path: &str, name: &str) -> String {
    if path.is_empty() {
        name.to_owned()
    } else {
        format!("{path}.{name}")
    }
}

/// Shows the document root as `.`.
fn display_path(path: &str) -> String {
    if path.is_empty() {
        ".".to_owned()
    } else {
        path.to_owned()
    }
}

/// Removes the ` at line N column M` suffix serde_json adds.
fn strip_position(message: &str) -> String {
    message
        .rfind(" at line ")
        .map_or(message, |at| &message[..at])
        .to_owned()
}

/// Makes a serde message follow the issue style: no trailing period.
fn clean_message(message: &str) -> String {
    message.trim_end_matches('.').to_owned()
}

/// The entries of a JSON object in the order the document wrote them, which
/// the settings schema keeps as its declaration order (spec 0009).
#[derive(Debug)]
pub(crate) struct Ordered<T>(Vec<(String, T)>);

impl<T> Ordered<T> {
    /// Returns how many entries there are.
    pub(crate) fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns the keys, in document order.
    pub(crate) fn keys(&self) -> impl Iterator<Item = &String> {
        self.0.iter().map(|(key, _)| key)
    }
}

impl<T> IntoIterator for Ordered<T> {
    type Item = (String, T);
    type IntoIter = std::vec::IntoIter<(String, T)>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<'a, T> IntoIterator for &'a Ordered<T> {
    type Item = (&'a String, &'a T);
    type IntoIter = std::iter::Map<
        std::slice::Iter<'a, (String, T)>,
        fn(&'a (String, T)) -> (&'a String, &'a T),
    >;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter().map(|(key, value)| (key, value))
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Ordered<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct OrderedVisitor<T>(std::marker::PhantomData<T>);

        impl<'de, T: Deserialize<'de>> Visitor<'de> for OrderedVisitor<T> {
            type Value = Ordered<T>;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("an object")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut entries: Vec<(String, T)> = Vec::new();
                while let Some(key) = map.next_key::<String>()? {
                    if entries.iter().any(|(seen, _)| *seen == key) {
                        return Err(de::Error::custom(format!("duplicate field `{key}`")));
                    }
                    entries.push((key, map.next_value()?));
                }
                Ok(Ordered(entries))
            }
        }

        deserializer.deserialize_map(OrderedVisitor(std::marker::PhantomData))
    }
}

#[cfg(feature = "json-schema")]
impl<T: schemars::JsonSchema> schemars::JsonSchema for Ordered<T> {
    fn inline_schema() -> bool {
        <BTreeMap<String, T>>::inline_schema()
    }

    fn schema_name() -> std::borrow::Cow<'static, str> {
        <BTreeMap<String, T>>::schema_name()
    }

    fn schema_id() -> std::borrow::Cow<'static, str> {
        <BTreeMap<String, T>>::schema_id()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        <BTreeMap<String, T>>::json_schema(generator)
    }
}

/// A JSON value decoded with duplicate keys rejected.
struct StrictJson(serde_json::Value);

impl<'de> Deserialize<'de> for StrictJson {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(StrictJsonVisitor).map(Self)
    }
}

struct StrictJsonVisitor;

impl<'de> Visitor<'de> for StrictJsonVisitor {
    type Value = serde_json::Value;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("any JSON value")
    }

    fn visit_bool<E: de::Error>(self, v: bool) -> Result<Self::Value, E> {
        Ok(v.into())
    }

    fn visit_i64<E: de::Error>(self, v: i64) -> Result<Self::Value, E> {
        Ok(v.into())
    }

    fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
        Ok(v.into())
    }

    fn visit_f64<E: de::Error>(self, v: f64) -> Result<Self::Value, E> {
        Ok(serde_json::Number::from_f64(v).map_or(serde_json::Value::Null, Into::into))
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
        Ok(v.into())
    }

    fn visit_string<E: de::Error>(self, v: String) -> Result<Self::Value, E> {
        Ok(v.into())
    }

    fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
        Ok(serde_json::Value::Null)
    }

    fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
        Ok(serde_json::Value::Null)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
        let mut items = Vec::new();
        while let Some(StrictJson(item)) = seq.next_element()? {
            items.push(item);
        }
        Ok(serde_json::Value::Array(items))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let mut object = serde_json::Map::new();
        while let Some(key) = map.next_key::<String>()? {
            if object.contains_key(&key) {
                return Err(de::Error::custom(format!("duplicate key `{key}`")));
            }
            let StrictJson(value) = map.next_value()?;
            object.insert(key, value);
        }
        Ok(serde_json::Value::Object(object))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Outer {
        #[expect(dead_code, reason = "decoded to prove the path of an error inside it")]
        inner: Inner,
    }

    #[derive(Debug, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Inner {
        #[expect(dead_code, reason = "decoded to prove the path of an error inside it")]
        list: Vec<u8>,
    }

    fn yaml(text: &str) -> Result<Outer, Issue> {
        let value = parse_yaml("f.yaml", text)?;
        check_yaml_value("f.yaml", &value)?;
        decode_yaml("f.yaml", value, IssueCode::UnknownField)
    }

    #[test]
    fn unknown_fields_are_reported_with_their_own_path() {
        let issue = yaml("inner:\n  list: [1]\n  shell: true\n").unwrap_err();

        assert_eq!(issue.code, IssueCode::UnknownField);
        assert_eq!(issue.location, Location::Field("inner.shell".into()));
        let root = yaml("inner:\n  list: [1]\nshell: true\n").unwrap_err();
        assert_eq!(root.location, Location::Field("shell".into()));
    }

    #[test]
    fn missing_fields_and_bad_values_name_their_path() {
        let missing = yaml("inner: {}\n").unwrap_err();
        let bad = yaml("inner:\n  list: [1, 300]\n").unwrap_err();

        assert_eq!(missing.code, IssueCode::MissingField);
        assert_eq!(missing.location, Location::Field("inner.list".into()));
        assert_eq!(bad.code, IssueCode::InvalidValue);
        assert_eq!(bad.location, Location::Field("inner.list[1]".into()));
    }

    #[test]
    fn yaml_tags_merge_keys_and_duplicate_keys_are_rejected() {
        let tag = yaml("inner:\n  list: !custom [1]\n").unwrap_err();
        let merge = yaml("base: &b {list: [1]}\ninner:\n  <<: *b\n").unwrap_err();
        let duplicate = yaml("inner:\n  list: [1]\n  list: [2]\n").unwrap_err();
        let syntax = yaml("inner: [\n").unwrap_err();

        assert_eq!(tag.code, IssueCode::InvalidValue);
        assert_eq!(tag.location, Location::Field("inner.list".into()));
        assert_eq!(merge.code, IssueCode::InvalidValue);
        assert_eq!(merge.location, Location::Field("inner.<<".into()));
        assert_eq!(duplicate.code, IssueCode::DuplicateKey);
        assert_eq!(syntax.code, IssueCode::YamlSyntax);
        assert!(matches!(syntax.location, Location::Position { .. }));
    }

    #[test]
    fn json_duplicate_keys_and_syntax_errors_carry_a_position() {
        let duplicate = parse_json("f.json", "{\"a\": 1, \"a\": 2}").unwrap_err();
        let syntax = parse_json("f.json", "{\"a\": }").unwrap_err();
        let trailing = parse_json("f.json", "{} x").unwrap_err();

        assert_eq!(duplicate.code, IssueCode::DuplicateKey);
        assert!(matches!(
            duplicate.location,
            Location::Position { line: 1, .. }
        ));
        assert_eq!(syntax.code, IssueCode::JsonSyntax);
        assert_eq!(trailing.code, IssueCode::JsonSyntax);
        assert!(!syntax.message.contains(" at line "), "{}", syntax.message);
    }

    #[test]
    fn nested_aliases_in_a_256_kib_file_fail_within_two_seconds() {
        let mut text = String::from("a0: &a0 [lol, lol, lol, lol, lol, lol, lol, lol, lol]\n");
        for n in 1..10 {
            let prev = format!("*a{}", n - 1);
            let list = [prev.as_str(); 9].join(", ");
            text.push_str(&format!("a{n}: &a{n} [{list}]\n"));
        }
        while text.len() < 256 * 1024 - 80 {
            text.push_str("# padding to the provider.yaml size limit\n");
        }
        let start = std::time::Instant::now();

        let issue = parse_yaml("provider.yaml", &text).unwrap_err();

        assert!(
            start.elapsed() < std::time::Duration::from_secs(2),
            "{:?}",
            start.elapsed()
        );
        assert_eq!(issue.code, IssueCode::YamlSyntax);
        assert!(
            issue.message.contains("repetition limit"),
            "{}",
            issue.message
        );
    }

    /// One large anchor behind a chain of nine way aliases: `width * 9^depth`
    /// nodes after only about `9^depth` alias jumps.
    fn large_anchor_bomb(width: usize, depth: usize) -> String {
        let mut text = format!("x0: &x0 [{}]\n", vec!["a"; width].join(", "));
        for level in 1..=depth {
            let alias = format!("*x{}", level - 1);
            let list = [alias.as_str(); 9].join(", ");
            text.push_str(&format!("x{level}: &x{level} [{list}]\n"));
        }
        text
    }

    #[test]
    fn a_large_anchor_behind_a_short_alias_chain_stops_at_the_node_budget() {
        // 531 million nodes from a 2 KiB text, far below the repetition
        // limit of serde_yaml_ng.
        let text = large_anchor_bomb(1000, 6);
        let start = std::time::Instant::now();

        let issue = parse_yaml("provider.yaml", &text).unwrap_err();

        assert!(
            start.elapsed() < std::time::Duration::from_secs(2),
            "{:?}",
            start.elapsed()
        );
        assert_eq!(issue.code, IssueCode::YamlSyntax);
        assert_eq!(issue.location, Location::File);
        let budget = yaml_node_budget(text.len());
        assert_eq!(
            issue.message,
            format!("aliases expand the document past {budget} nodes")
        );
        assert!(matches!(
            yaml_value(&text),
            Err(YamlError::TooLarge { budget: b }) if b == budget
        ));
    }

    #[test]
    fn documents_without_a_large_alias_expansion_stay_within_the_budget() {
        let flat = format!("[{}]", vec!["a"; 20_000].join(","));
        let empty_lists = format!("[{}]", vec!["[]"; 10_000].join(","));
        let keys: Vec<String> = (0..5_000).map(|n| format!("k{n}")).collect();
        let keys = format!("{{{}}}", keys.join(","));
        let small_aliases = "base: &b [1, 2, 3]\nfirst: *b\nsecond: *b\nthird: *b\n";
        let tagged = "a: !!str 1\nb: !!int \"2\"\nc: !custom [x]\n";

        for text in [flat.as_str(), &empty_lists, &keys, small_aliases, tagged] {
            assert!(yaml_value(text).is_ok(), "{}", &text[..text.len().min(40)]);
        }
        assert_eq!(
            yaml("inner:\n  list: !custom [1]\n").unwrap_err().message,
            "yaml tags are not allowed",
            "a tag still reaches the tag check"
        );
    }

    #[test]
    fn json_keeps_integers_and_floats_apart() {
        let value = parse_json("f.json", "[5, 5.0, 1e3, -2]").unwrap();

        assert!(value[0].is_i64() || value[0].is_u64());
        assert!(value[1].is_f64());
        assert!(value[2].is_f64());
        assert!(value[3].is_i64());
    }
}
