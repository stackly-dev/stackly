//! The form and the create draft of a duplicate (spec 0024, *Draft
//! duplicate*): pure, from the facts [`gather_duplicate`](super::gather_duplicate)
//! read.

use stackly_domain::{InstanceId, InstanceName, SecretFieldName, SecretRef};

use super::{DuplicateFacts, duplicate_name};
use crate::instance::create::{
    CreateDraft, CreateFormModel, PackageSource, PortRequestDraft, ProviderChoice,
    SecretSourceKind, build_form_from_package,
};
use crate::provider::EndpointTransport;

/// The form and the draft of a duplicate.
#[derive(Debug, Clone)]
pub struct DuplicateModel {
    /// The ID and name of the source.
    pub source: (InstanceId, InstanceName),
    /// The create form of the pinned package and the runtime of the
    /// source; `suggested_name` is the duplicate name.
    pub form: CreateFormModel,
    /// The draft the form starts with.
    pub draft: CreateDraft,
    /// The reference the source uses for each secret field, in form order,
    /// or none when it uses none.
    pub secret_sources: Vec<(SecretFieldName, Option<SecretRef>)>,
}

/// Builds the form and the draft of a duplicate.
///
/// The draft copies the settings of the active config revision of the
/// source, restricted to the fields of the schema that are neither secret
/// nor managed. Every port is `auto` and every secret is `generate`, so
/// nothing but a user action ever picks a fixed port or an existing secret
/// reference. The display name is empty and the start flag is false.
#[must_use]
pub fn build_duplicate(facts: &DuplicateFacts) -> DuplicateModel {
    let name = duplicate_name(facts.source.name(), &facts.taken);
    let form = build_form_from_package(
        &facts.package,
        PackageSource::Pinned,
        &facts.runtime,
        name.clone(),
    );
    let key = facts.source.provider_revision();
    let secret_fields: Vec<_> = facts
        .package
        .settings_schema()
        .fields()
        .filter(|field| field.is_secret())
        .map(|field| field.name().as_str().to_owned())
        .collect();
    let draft = CreateDraft {
        name: name.map(|name| name.to_string()).unwrap_or_default(),
        display_name: None,
        provider: ProviderChoice {
            name: key.provider().to_string(),
            revision: Some(key.revision().to_string()),
        },
        runtime_id: facts.source.runtime_id(),
        endpoints: form
            .endpoints
            .iter()
            .filter(|endpoint| endpoint.transport != EndpointTransport::Unix)
            .map(|endpoint| (endpoint.name.clone(), PortRequestDraft::Auto))
            .collect(),
        settings: copied_settings(facts),
        secrets: secret_fields
            .iter()
            .map(|field| (field.clone(), SecretSourceKind::Generate))
            .collect(),
        secret_values: Vec::new(),
        start: false,
    };
    let secret_sources = secret_fields
        .iter()
        .filter_map(|field| field.parse::<SecretFieldName>().ok())
        .map(|field| {
            let reference = facts
                .uses
                .iter()
                .find(|used| used.field == field)
                .map(|used| used.reference.clone());
            (field, reference)
        })
        .collect();
    DuplicateModel {
        source: (facts.source.id(), facts.source.name().clone()),
        form,
        draft,
        secret_sources,
    }
}

/// The values of the source, in schema order, without a key the schema
/// does not declare and without a secret or managed field.
fn copied_settings(facts: &DuplicateFacts) -> serde_json::Value {
    let mut copied = serde_json::Map::new();
    if let Some(serde_json::Value::Object(values)) = &facts.values {
        for field in facts.package.settings_schema().fields() {
            if field.is_secret() || field.managed().is_some() {
                continue;
            }
            if let Some(value) = values.get(field.name().as_str()) {
                copied.insert(field.name().as_str().to_owned(), value.clone());
            }
        }
    }
    serde_json::Value::Object(copied)
}
