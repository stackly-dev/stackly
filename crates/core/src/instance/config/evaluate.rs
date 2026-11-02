//! The pure evaluation of a proposed settings document against the active
//! revision (spec 0029, *Evaluator config*): the same validation as create,
//! the two field codes only a change can earn, the per setting diff, and
//! the impact on the instance.

use std::collections::BTreeSet;

use stackly_domain::{ApplyGate, InstanceLifecycle, apply_allowed};

use crate::instance::create::{FieldCode, FieldIssue, field_code, settings_path};
use crate::provider::{ApplyClass, FieldKind, SettingField, SettingsSchema, SettingsValues};

/// What a config change does to the instance (spec 0029, AC-2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConfigImpact {
    /// Nothing changes.
    None,
    /// The instance does not run: the revision applies at its next start.
    NextStart,
    /// A reload applies it without a restart.
    Reload,
    /// A restart applies it.
    Restart,
}

/// One changed setting, in schema order. `key` is the field name, an item
/// of a keyed list as `routes[name=api]` (or one of its fields as
/// `routes[name=api].rule`), or an item of a list without a key by index
/// as `items[2].x`. `old` is absent for an added item, `new` for a
/// removed one. A secret field shows its reference, never a value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingChange {
    /// What changed.
    pub key: String,
    /// The value in the active revision.
    pub old: Option<serde_json::Value>,
    /// The proposed value.
    pub new: Option<serde_json::Value>,
}

/// The result of evaluating one proposed settings document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeEvaluation {
    /// Every problem with one field, ordered by path. Empty means valid.
    pub field_issues: Vec<FieldIssue>,
    /// Every changed setting; empty while the document does not validate.
    pub changes: Vec<SettingChange>,
    /// The impact of applying it now.
    pub impact: ConfigImpact,
    /// The validated proposal, there only when valid.
    pub values: Option<SettingsValues>,
}

impl ChangeEvaluation {
    /// Whether the proposal is valid.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.field_issues.is_empty() && self.values.is_some()
    }
}

/// Evaluates `proposed`, a settings document, against `active`, the
/// settings of the active revision (spec 0029, AC-2): schema validation
/// with the field codes of create, then for every changed field
/// `secretReadOnly` for a secret and `immutableAfterInit` for a set once
/// field (a secret that is also set once gets `secretReadOnly`). The impact
/// counts `endpoints_changed` as a change that needs a restart.
#[must_use]
pub fn evaluate_change(
    schema: &SettingsSchema,
    active: &SettingsValues,
    proposed: &serde_json::Value,
    lifecycle: InstanceLifecycle,
    reload_supported: bool,
    endpoints_changed: bool,
) -> ChangeEvaluation {
    let values = match schema.validate(proposed) {
        Ok(values) => values,
        Err(error) => {
            let mut field_issues: Vec<FieldIssue> = error
                .issues
                .iter()
                .map(|issue| {
                    let code = issue
                        .settings_kind
                        .map_or(FieldCode::InvalidValue, field_code);
                    FieldIssue::new(settings_path(&issue.location), code)
                })
                .collect();
            sort_issues(&mut field_issues);
            return ChangeEvaluation {
                field_issues,
                changes: Vec::new(),
                impact: ConfigImpact::None,
                values: None,
            };
        }
    };
    let changes = diff_settings(schema, active, &values);
    let changed = changed_fields(schema, &changes);
    let mut field_issues = Vec::new();
    for field in &changed {
        if field.is_secret() {
            field_issues.push(FieldIssue::new(
                format!("settings.{}", field.name()),
                FieldCode::SecretReadOnly,
            ));
        } else if field.apply() == Some(ApplyClass::ImmutableAfterInit) {
            field_issues.push(FieldIssue::new(
                format!("settings.{}", field.name()),
                FieldCode::ImmutableAfterInit,
            ));
        }
    }
    sort_issues(&mut field_issues);
    let impact = impact_of(&changed, lifecycle, reload_supported, endpoints_changed);
    let valid = field_issues.is_empty();
    ChangeEvaluation {
        field_issues,
        changes,
        impact,
        values: valid.then_some(values),
    }
}

fn sort_issues(issues: &mut Vec<FieldIssue>) {
    issues.sort_by(|a, b| a.path.cmp(&b.path));
    issues.dedup();
}

/// The root fields `changes` touch, in schema order.
fn changed_fields<'a>(
    schema: &'a SettingsSchema,
    changes: &[SettingChange],
) -> Vec<&'a SettingField> {
    let roots: BTreeSet<&str> = changes
        .iter()
        .map(|change| {
            change
                .key
                .split(['[', '.'])
                .next()
                .unwrap_or(change.key.as_str())
        })
        .collect();
    schema
        .fields()
        .filter(|field| roots.contains(field.name().as_str()))
        .collect()
}

/// The impact of changing `changed` (and the ports, with
/// `endpoints_changed`) in `lifecycle` (spec 0029, *State transitions*):
/// none without a change; next start while the instance does not run;
/// reload only when it runs, the provider can reload, every changed field
/// is `apply: reload`, and no port changed; restart otherwise. A lifecycle
/// that refuses a change reads as restart, the label the refused apply
/// would have needed.
#[must_use]
pub fn impact_of(
    changed: &[&SettingField],
    lifecycle: InstanceLifecycle,
    reload_supported: bool,
    endpoints_changed: bool,
) -> ConfigImpact {
    if changed.is_empty() && !endpoints_changed {
        return ConfigImpact::None;
    }
    match apply_allowed(lifecycle) {
        Ok(ApplyGate::Save) => ConfigImpact::NextStart,
        Ok(ApplyGate::Reload)
            if reload_supported
                && !endpoints_changed
                && changed
                    .iter()
                    .all(|field| field.apply() == Some(ApplyClass::Reload)) =>
        {
            ConfigImpact::Reload
        }
        Ok(ApplyGate::Reload | ApplyGate::Restart) | Err(_) => ConfigImpact::Restart,
    }
}

/// Every changed setting from `active` to `proposed`, in schema order
/// (spec 0029, *Evaluator config*). A list whose items have a key compares
/// items by that key, so moving an item is no change; a list without one
/// compares by index.
#[must_use]
pub fn diff_settings(
    schema: &SettingsSchema,
    active: &SettingsValues,
    proposed: &SettingsValues,
) -> Vec<SettingChange> {
    let before = active.to_json();
    let after = proposed.to_json();
    let mut changes = Vec::new();
    for field in schema.fields() {
        let name = field.name().as_str();
        let old = before.get(name);
        let new = after.get(name);
        if old == new {
            continue;
        }
        match (field.kind(), old, new) {
            (
                FieldKind::Array(rules),
                Some(serde_json::Value::Array(old)),
                Some(serde_json::Value::Array(new)),
            ) => match &rules.key {
                Some(key) => diff_keyed(name, key.as_str(), old, new, &mut changes),
                None => diff_indexed(name, old, new, &mut changes),
            },
            _ => changes.push(SettingChange {
                key: name.to_owned(),
                old: old.cloned(),
                new: new.cloned(),
            }),
        }
    }
    changes
}

fn key_text(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

fn diff_keyed(
    name: &str,
    key: &str,
    old: &[serde_json::Value],
    new: &[serde_json::Value],
    changes: &mut Vec<SettingChange>,
) {
    let key_of = |item: &serde_json::Value| item.get(key).map(key_text);
    let find = |items: &[serde_json::Value], wanted: &str| {
        items
            .iter()
            .find(|item| key_of(item).as_deref() == Some(wanted))
            .cloned()
    };
    let mut keys: Vec<String> = Vec::new();
    for item in old.iter().chain(new) {
        if let Some(text) = key_of(item)
            && !keys.contains(&text)
        {
            keys.push(text);
        }
    }
    for text in keys {
        let label = format!("{name}[{key}={text}]");
        match (find(old, &text), find(new, &text)) {
            (Some(before), Some(after)) => diff_item(&label, &before, &after, changes),
            (before, after) => changes.push(SettingChange {
                key: label,
                old: before,
                new: after,
            }),
        }
    }
}

fn diff_indexed(
    name: &str,
    old: &[serde_json::Value],
    new: &[serde_json::Value],
    changes: &mut Vec<SettingChange>,
) {
    for index in 0..old.len().max(new.len()) {
        let label = format!("{name}[{index}]");
        match (old.get(index), new.get(index)) {
            (Some(before), Some(after)) => diff_item(&label, before, after, changes),
            (before, after) => changes.push(SettingChange {
                key: label,
                old: before.cloned(),
                new: after.cloned(),
            }),
        }
    }
}

/// The changed fields of one list item, in key order.
fn diff_item(
    label: &str,
    before: &serde_json::Value,
    after: &serde_json::Value,
    changes: &mut Vec<SettingChange>,
) {
    let empty = serde_json::Map::new();
    let old = before.as_object().unwrap_or(&empty);
    let new = after.as_object().unwrap_or(&empty);
    let mut fields: Vec<&String> = old.keys().chain(new.keys()).collect();
    fields.sort();
    fields.dedup();
    for field in fields {
        let (old, new) = (old.get(field), new.get(field));
        if old != new {
            changes.push(SettingChange {
                key: format!("{label}.{field}"),
                old: old.cloned(),
                new: new.cloned(),
            });
        }
    }
}
