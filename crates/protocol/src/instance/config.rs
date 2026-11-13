//! The DTOs of the config methods (spec 0029, *Method IPC*):
//! `instance.config.get`, `instance.config.validate`,
//! `instance.config.apply`, `instance.config.keep_revision`,
//! `instance.rename`, and `instance.change_endpoints`.
//!
//! A config revision is a numbered snapshot of the settings and the port
//! requests of one instance. `get` shows the active one with its history,
//! `validate` judges a change without any effect, and `apply` writes the
//! change as the next revision and makes it take effect. Secret fields
//! travel as their reference text, never as a value, and file paths are
//! always relative to the instance's `config/` folder.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use typeshare::typeshare;
use uuid::Uuid;

use super::{
    CREATE_ENDPOINTS_MAX, EndpointRequest, EndpointTransport, FormField, INSTANCE_KEY_MAX,
    INSTANCE_NAME_MAX, PortRequest, SettingsDocument, length_code, settings_has_unsafe_integer,
};
use crate::env::InstanceEnvironment;
use crate::error::FieldError;
use crate::json::{FieldCode, Validate};
use crate::operation::OperationAccepted;
use crate::wire::{ConfigRevisionNumber, Revision, Timestamp};

/// Params of `instance.config.get`.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GetConfigParams {
    /// The instance to read; a retained one is refused.
    pub instance_id: Uuid,
}

impl Validate for GetConfigParams {}

/// What a config change asks for. Tagged `type` and `content`.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", content = "content", rename_all = "camelCase")]
pub enum ConfigChangeRequest {
    /// The whole proposed settings document, secrets as their references.
    /// The ports stay as they are.
    Settings(SettingsDocument),
    /// The settings and ports of an older revision of the history.
    Restore(ConfigRevisionNumber),
    /// The settings and ports of the last known good revision.
    Rollback,
}

/// The field errors of a change: the wire only checks the number rules, the
/// control frame bounds the size of the settings document.
fn change_fields(change: &ConfigChangeRequest) -> Vec<FieldError> {
    match change {
        ConfigChangeRequest::Settings(settings) => {
            if settings_has_unsafe_integer(settings.as_map()) {
                vec![FieldError::new("change.content", FieldCode::OutOfRange)]
            } else {
                Vec::new()
            }
        }
        ConfigChangeRequest::Restore(number) if number.get() == 0 => {
            vec![FieldError::new("change.content", FieldCode::OutOfRange)]
        }
        ConfigChangeRequest::Restore(_) | ConfigChangeRequest::Rollback => Vec::new(),
    }
}

fn base_revision_fields(base: ConfigRevisionNumber) -> Vec<FieldError> {
    if base.get() == 0 {
        vec![FieldError::new("baseRevision", FieldCode::OutOfRange)]
    } else {
        Vec::new()
    }
}

/// Params of `instance.config.validate`: judges `change` against the
/// revision the client read, and writes nothing.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ValidateConfigParams {
    /// The instance to judge the change for.
    pub instance_id: Uuid,
    /// The revision the client read; another active revision is
    /// `config.revision_conflict`.
    pub base_revision: ConfigRevisionNumber,
    /// The change.
    pub change: ConfigChangeRequest,
}

impl Validate for ValidateConfigParams {
    fn validate(&self) -> Vec<FieldError> {
        let mut fields = base_revision_fields(self.base_revision);
        fields.extend(change_fields(&self.change));
        fields
    }
}

/// Params of `instance.config.apply`: writes `change` as the next
/// revision and makes it take effect.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApplyConfigParams {
    /// The instance to change.
    pub instance_id: Uuid,
    /// The revision the client read; another active revision is
    /// `config.revision_conflict`, before and after the operation queues.
    pub base_revision: ConfigRevisionNumber,
    /// The change.
    pub change: ConfigChangeRequest,
    /// Refuse with `resource.revision_mismatch` unless the instance is at
    /// this revision.
    #[serde(default)]
    pub expected_revision: Option<Revision>,
}

impl Validate for ApplyConfigParams {
    fn validate(&self) -> Vec<FieldError> {
        let mut fields = base_revision_fields(self.base_revision);
        fields.extend(change_fields(&self.change));
        fields
    }
}

/// Params of `instance.config.keep_revision`: write the config files of the
/// active revision back over a drift.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct KeepRevisionParams {
    /// The instance whose files go back to the active revision.
    pub instance_id: Uuid,
}

impl Validate for KeepRevisionParams {}

/// Params of `instance.rename`.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RenameInstanceParams {
    /// The instance to rename.
    pub instance_id: Uuid,
    /// The new name; the daemon checks the format and the uniqueness.
    pub name: String,
    /// Refuse with `resource.revision_mismatch` unless the instance is at
    /// this revision.
    #[serde(default)]
    pub expected_revision: Option<Revision>,
}

impl Validate for RenameInstanceParams {
    /// Only the size; an empty or malformed name is `instance.name_invalid`.
    fn validate(&self) -> Vec<FieldError> {
        if self.name.len() > INSTANCE_NAME_MAX {
            vec![FieldError::new("name", FieldCode::TooLong)]
        } else {
            Vec::new()
        }
    }
}

/// Params of `instance.change_endpoints`.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChangeEndpointsParams {
    /// The instance to change.
    pub instance_id: Uuid,
    /// The revision the client read.
    pub base_revision: ConfigRevisionNumber,
    /// The IP endpoints that get another port request; the others stay.
    pub endpoints: Vec<EndpointRequest>,
    /// True to judge the ports and write nothing.
    pub dry_run: bool,
    /// True to change the ports of an instance an environment lease holds;
    /// the leases stay active (spec 0047 AC-15). Without it such a change
    /// is refused with `instance.endpoints_require_override`.
    #[serde(default)]
    pub override_leases: bool,
}

impl Validate for ChangeEndpointsParams {
    fn validate(&self) -> Vec<FieldError> {
        let mut fields = base_revision_fields(self.base_revision);
        if self.endpoints.is_empty() {
            fields.push(FieldError::new("endpoints", FieldCode::Required));
        }
        if self.endpoints.len() > CREATE_ENDPOINTS_MAX {
            fields.push(FieldError::new("endpoints", FieldCode::TooMany));
        }
        for (index, endpoint) in self.endpoints.iter().enumerate().take(CREATE_ENDPOINTS_MAX) {
            if let Some(code) = length_code(&endpoint.name, INSTANCE_KEY_MAX) {
                fields.push(FieldError::new(format!("endpoints[{index}].name"), code));
            }
        }
        fields
    }
}

/// What applying a change does to the instance. Open: newer daemons may add
/// values.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConfigImpact {
    /// Nothing changes.
    None,
    /// The instance does not run: the revision applies at its next start.
    NextStart,
    /// A reload applies it and the process stays.
    Reload,
    /// A restart applies it.
    Restart,
    /// An impact this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// How a config revision came to be. Open: newer daemons may add values.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConfigRevisionOrigin {
    /// Written by `instance.create`, from the provider defaults.
    Created,
    /// A settings change.
    Applied,
    /// A port change.
    Endpoints,
    /// A roll back to the last known good revision.
    Rollback,
    /// A restore of an older revision.
    Restored,
    /// An origin this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// One config revision of the history.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConfigRevisionSummary {
    /// The number, rising by one per revision and never reused.
    pub number: ConfigRevisionNumber,
    /// When it was written.
    pub created_at: Timestamp,
    /// How it came to be.
    pub origin: ConfigRevisionOrigin,
    /// The revision a `rollback` or `restored` revision copies; it may
    /// have left the history since.
    #[serde(default)]
    pub source_number: Option<ConfigRevisionNumber>,
    /// When the instance first reached ready on this revision; absent
    /// while it never did.
    #[serde(default)]
    pub ready_at: Option<Timestamp>,
    /// Whether it is the active revision.
    pub is_active: bool,
    /// Whether it is the last known good revision.
    pub is_last_known_good: bool,
    /// Its settings, secrets as their reference text. Restore loads them
    /// into the form.
    pub values: SettingsDocument,
    /// Its port requests, one per IP endpoint.
    pub endpoints: Vec<EndpointRequest>,
}

/// One settings field of the Configuration page.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConfigField {
    /// The field as the create form shows it, including its `apply` class
    /// and `secret` and `managed` flags.
    pub field: FormField,
    /// True when no change can edit it: a managed field, a field that is
    /// set once, and a secret.
    pub locked: bool,
}

/// One endpoint of the instance on the Configuration page.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConfigEndpoint {
    /// The endpoint name from the manifest.
    pub name: String,
    /// The transport.
    pub transport: EndpointTransport,
    /// The host of an IP endpoint.
    #[serde(default)]
    pub host: Option<String>,
    /// The port request of the active revision, for an IP endpoint.
    #[serde(default)]
    pub request: Option<PortRequest>,
    /// The port the process has now, once one was bound.
    #[serde(default)]
    pub bound_port: Option<u16>,
    /// Whether `instance.change_endpoints` can change it: an IP endpoint.
    pub changeable: bool,
}

/// One non sensitive config file as the Configuration page shows it.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EffectiveFile {
    /// The path relative to the `config/` folder.
    pub path: String,
    /// The text.
    pub content: String,
    /// The 1 based lines of `content` a proposal changes; empty outside a
    /// preview of a change.
    pub changed_lines: Vec<u32>,
    /// True when the text is rendered from the values now, rather than the
    /// copy of the active revision's last render.
    pub preview: bool,
    /// A note for an Automatic port shown with its default port.
    #[serde(default)]
    pub port_note: Option<String>,
}

/// What one line of a drift diff is. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DiffLineKind {
    /// In both texts.
    Context,
    /// Only on disk.
    Added,
    /// Only in the copy Stackly wrote.
    Removed,
    /// A kind this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// One line of a drift diff.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DiffLine {
    /// What the line is.
    pub kind: DiffLineKind,
    /// The text, without the line end.
    pub text: String,
}

/// How a config file drifted. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DriftKind {
    /// The file differs from the copy.
    Modified,
    /// The file is gone.
    Missing,
    /// A kind this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// One config file that drifted.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DriftFile {
    /// The path relative to the `config/` folder.
    pub path: String,
    /// How it drifted.
    pub kind: DriftKind,
    /// The copy Stackly wrote against what is on disk.
    pub diff: Vec<DiffLine>,
}

/// The drift of the active revision (spec 0029, AC-10).
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConfigDrift {
    /// When the check found it.
    pub observed_at: Timestamp,
    /// The drifted files, ordered by path.
    pub files: Vec<DriftFile>,
}

/// Reply of `instance.config.get`.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct InstanceConfigView {
    /// The active revision, with its settings and port requests.
    pub revision: ConfigRevisionSummary,
    /// The last known good revision: the highest number that reached
    /// ready. Absent when none did.
    #[serde(default)]
    pub last_known_good: Option<ConfigRevisionNumber>,
    /// The settings fields, in the order the schema declares them.
    pub fields: Vec<ConfigField>,
    /// The endpoints of the instance.
    pub endpoints: Vec<ConfigEndpoint>,
    /// The newest revisions, newest first, 20 at most; the active one and
    /// the last known good are always in it.
    pub history: Vec<ConfigRevisionSummary>,
    /// The non sensitive config files.
    pub effective_files: Vec<EffectiveFile>,
    /// Why `effectiveFiles` is empty although the provider renders files:
    /// `Runtime missing, preview unavailable` when the active revision has
    /// no copy and the runtime folder is gone.
    #[serde(default)]
    pub effective_files_note: Option<String>,
    /// The drift of the active revision, absent when there is none.
    #[serde(default)]
    pub drift: Option<ConfigDrift>,
    /// Whether the pinned provider revision can reload.
    pub reload_supported: bool,
}

/// One setting a change touches. A value is its compact JSON text, except
/// that a string is its text without quotes; a secret shows its reference.
/// A key is a field name, `routes[name=api]` for an item of a list with a
/// key, `routes[name=api].rule` for one of its fields, or `items[2].x` for
/// an item of a list without one. `old` is absent for an added item and
/// `new` for a removed one.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SettingChange {
    /// What changes.
    pub key: String,
    /// The value in the active revision.
    #[serde(default)]
    pub old: Option<String>,
    /// The proposed value.
    #[serde(default)]
    pub new: Option<String>,
}

/// The port request of one endpoint, before and after a change.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EndpointChange {
    /// The endpoint name from the manifest.
    pub name: String,
    /// The request in the active revision.
    pub old: PortRequest,
    /// The proposed request.
    pub new: PortRequest,
}

/// Reply of `instance.config.validate`.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConfigValidation {
    /// True exactly when `fieldErrors` is empty.
    pub valid: bool,
    /// Every field with a problem: `settings.<key>` or an item path such
    /// as `settings.routes[2].rule`, with the codes of create plus
    /// `immutable_after_init` and `secret_read_only`.
    pub field_errors: Vec<FieldError>,
    /// The settings that change, in schema order; empty while invalid.
    pub changes: Vec<SettingChange>,
    /// The ports that change, for a restore or a roll back.
    pub endpoint_changes: Vec<EndpointChange>,
    /// What applying it now does.
    pub impact: ConfigImpact,
    /// The non sensitive config files the change would render, with the
    /// changed lines marked. Empty while invalid or when the runtime is
    /// missing.
    pub preview: Vec<EffectiveFile>,
}

/// Reply of `instance.config.apply`.
///
/// A change that starts an operation returns it in `accepted`. A change
/// equal to the active revision answers `unchanged` with no operation, no
/// revision, and no bound idempotency key.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConfigApplyAccepted {
    /// The operation, as created or as it stands now; absent when nothing
    /// ran.
    #[serde(default)]
    pub accepted: Option<OperationAccepted>,
    /// True when the change equals the active revision.
    pub unchanged: bool,
    /// The revision the operation writes (the active one plus one), or
    /// the active revision when `unchanged`.
    pub revision: ConfigRevisionNumber,
    /// What the operation does to the instance.
    pub impact: ConfigImpact,
}

/// One port request that cannot be applied.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EndpointProblem {
    /// The endpoint the request names.
    pub endpoint: String,
    /// The error code, such as `supervisor.endpoint_conflict`.
    pub code: String,
    /// The first free port after the requested one, for a fixed port that
    /// is taken.
    #[serde(default)]
    pub suggested_port: Option<u16>,
}

/// Reply of a dry run of `instance.change_endpoints`.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EndpointValidation {
    /// True exactly when `problems` is empty.
    pub valid: bool,
    /// Every request that cannot be applied, in the order asked.
    pub problems: Vec<EndpointProblem>,
    /// The ports that change; empty while invalid or when nothing moves.
    pub changes: Vec<EndpointChange>,
    /// What applying it now does.
    pub impact: ConfigImpact,
    /// The environments whose lease on the instance is active, ordered by
    /// name, so their part shows before Apply (spec 0047 AC-15). Empty from
    /// an older daemon.
    #[serde(default)]
    pub leases: Vec<InstanceEnvironment>,
}

/// Reply of `instance.change_endpoints`: the operation that changes the
/// ports, or the verdict of a dry run. Tagged `type` and `content`.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", content = "content", rename_all = "camelCase")]
pub enum ChangeEndpointsReply {
    /// The change was accepted or equals the active revision.
    Applied(Box<ConfigApplyAccepted>),
    /// The verdict of `dryRun: true`; nothing was written.
    DryRun(EndpointValidation),
}

#[cfg(test)]
mod tests {
    use super::*;

    const INSTANCE: &str = "0192f3a0-7c1e-7b2a-9d4e-5f6a7b8c9d04";

    fn id() -> Uuid {
        INSTANCE.parse().unwrap()
    }

    fn number(n: u64) -> ConfigRevisionNumber {
        ConfigRevisionNumber::new(n).unwrap()
    }

    fn codes(fields: Vec<FieldError>) -> Vec<(String, String)> {
        fields
            .iter()
            .map(|field| (field.path().to_owned(), field.code().to_owned()))
            .collect()
    }

    #[test]
    fn a_zero_base_revision_is_out_of_range() {
        let params = ValidateConfigParams {
            instance_id: id(),
            base_revision: number(0),
            change: ConfigChangeRequest::Rollback,
        };

        assert_eq!(
            codes(params.validate()),
            [("baseRevision".to_owned(), "out_of_range".to_owned())]
        );
    }

    #[test]
    fn restoring_revision_zero_is_out_of_range() {
        let params = ApplyConfigParams {
            instance_id: id(),
            base_revision: number(1),
            change: ConfigChangeRequest::Restore(number(0)),
            expected_revision: None,
        };

        assert_eq!(
            codes(params.validate()),
            [("change.content".to_owned(), "out_of_range".to_owned())]
        );
    }

    #[test]
    fn a_settings_change_with_an_unsafe_integer_is_out_of_range() {
        let settings: SettingsDocument =
            serde_json::from_str(r#"{"maxConnections":9007199254740993}"#).unwrap();
        let params = ApplyConfigParams {
            instance_id: id(),
            base_revision: number(1),
            change: ConfigChangeRequest::Settings(settings),
            expected_revision: None,
        };

        assert_eq!(
            codes(params.validate()),
            [("change.content".to_owned(), "out_of_range".to_owned())]
        );
    }

    #[test]
    fn change_endpoints_needs_between_one_and_sixteen_named_requests() {
        let request = |name: &str| EndpointRequest {
            name: name.to_owned(),
            port: PortRequest::Auto,
        };
        let mut params = ChangeEndpointsParams {
            instance_id: id(),
            base_revision: number(1),
            endpoints: Vec::new(),
            dry_run: false,
            override_leases: false,
        };
        assert_eq!(
            codes(params.validate()),
            [("endpoints".to_owned(), "required".to_owned())]
        );

        params.endpoints = vec![request("")];
        assert_eq!(
            codes(params.validate()),
            [("endpoints[0].name".to_owned(), "too_short".to_owned())]
        );

        params.endpoints = (0..17).map(|n| request(&format!("e{n}"))).collect();
        assert_eq!(
            codes(params.validate()),
            [("endpoints".to_owned(), "too_many".to_owned())]
        );
    }

    #[test]
    fn a_rename_only_checks_the_size_of_the_name() {
        let rename = |name: String| RenameInstanceParams {
            instance_id: id(),
            name,
            expected_revision: None,
        };

        assert!(rename(String::new()).validate().is_empty());
        assert_eq!(
            codes(rename("a".repeat(INSTANCE_NAME_MAX + 1)).validate()),
            [("name".to_owned(), "too_long".to_owned())]
        );
    }

    #[test]
    fn a_change_request_is_tagged_with_type_and_content() {
        assert_eq!(
            serde_json::to_string(&ConfigChangeRequest::Rollback).unwrap(),
            r#"{"type":"rollback"}"#
        );
        assert_eq!(
            serde_json::to_string(&ConfigChangeRequest::Restore(number(3))).unwrap(),
            r#"{"type":"restore","content":3}"#
        );
    }
}
