//! The core form of the equivalent `stackly instance create` command (spec
//! 0020, *Evaluator create*): the data the daemon maps to the wire
//! `CommandDraft` and the CLI turns into text.

use stackly_domain::Runtime;

use super::draft::{CreateDraft, PortRequestDraft, SecretSourceKind};
use super::facts::{CreateFacts, ProviderFound};
use crate::provider::{FieldKind, ScalarKind, SettingField};

/// What the equivalent command says, in the order of its flags.
#[derive(Debug, Clone, PartialEq)]
pub struct CommandSpec {
    /// The instance name, as typed.
    pub name: String,
    /// `software@version` when exactly one `installed` runtime matches it,
    /// else the runtime ID.
    pub runtime_label: String,
    /// The provider, when the draft names one (a draft always does, so
    /// the command always names it).
    pub provider: Option<String>,
    /// The revision, only when the draft names one.
    pub revision: Option<String>,
    /// The display name, as typed.
    pub display_name: Option<String>,
    /// The port requests, in the order of the draft.
    pub ports: Vec<(String, PortRequestDraft)>,
    /// The settings that differ from the default of the package, in schema
    /// order; empty when no package could be determined.
    pub settings: Vec<(String, serde_json::Value)>,
    /// The source of each secret field the draft names.
    pub secrets: Vec<(String, SecretSourceKind)>,
    /// Whether the command starts the instance after create.
    pub start: bool,
}

/// Builds the command of `draft`.
pub(super) fn command_spec(draft: &CreateDraft, facts: &CreateFacts) -> CommandSpec {
    CommandSpec {
        name: draft.name.clone(),
        runtime_label: runtime_label(draft, facts),
        provider: (!draft.provider.name.is_empty()).then(|| draft.provider.name.clone()),
        revision: draft.provider.revision.clone(),
        display_name: draft.display_name.clone(),
        ports: draft.endpoints.clone(),
        settings: facts
            .provider
            .as_ref()
            .map(|found| changed_settings(draft, found))
            .unwrap_or_default(),
        secrets: draft.secrets.clone(),
        start: draft.start,
    }
}

/// `software@version` when that selector, read the way `--runtime` reads it
/// (the exact version first, else the line), picks exactly this runtime
/// among the runtimes that are `installed`.
fn runtime_label(draft: &CreateDraft, facts: &CreateFacts) -> String {
    let id = draft.runtime_id.to_string();
    let Some(runtime) = facts
        .installed_runtimes
        .iter()
        .find(|runtime| runtime.id() == draft.runtime_id)
    else {
        return id;
    };
    let software: Vec<&Runtime> = facts
        .installed_runtimes
        .iter()
        .filter(|candidate| candidate.software() == runtime.software())
        .collect();
    let version = runtime.version().to_string();
    let by_version: Vec<&&Runtime> = software
        .iter()
        .filter(|candidate| candidate.version().to_string() == version)
        .collect();
    if by_version.len() == 1 {
        return format!("{}@{version}", runtime.software());
    }
    id
}

/// The draft's settings that differ from the default of their field,
/// leaving out secrets (their own flag) and managed fields.
fn changed_settings(
    draft: &CreateDraft,
    found: &ProviderFound,
) -> Vec<(String, serde_json::Value)> {
    let Some(document) = draft.settings.as_object() else {
        return Vec::new();
    };
    found
        .package
        .settings_schema()
        .fields()
        .filter(|field| !field.is_secret() && field.managed().is_none())
        .filter_map(|field| {
            let value = document.get(field.name().as_str())?;
            (default_json(field).as_ref() != Some(value))
                .then(|| (field.name().to_string(), value.clone()))
        })
        .collect()
}

/// The default of a field as JSON, when it has one.
fn default_json(field: &SettingField) -> Option<serde_json::Value> {
    match field.kind() {
        FieldKind::Scalar(ScalarKind::Text(rules)) => rules.default.clone().map(Into::into),
        FieldKind::Scalar(ScalarKind::Integer(rules)) => rules.default.map(Into::into),
        FieldKind::Scalar(ScalarKind::Boolean(rules)) => rules.default.map(Into::into),
        FieldKind::Array(rules) => rules
            .default
            .as_ref()
            .filter(|items| items.is_empty())
            .map(|_| serde_json::Value::Array(Vec::new())),
    }
}
