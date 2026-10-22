//! The wire draft of `instance.preflight` and `instance.create` as the one
//! [`CreateDraft`] the evaluator judges (spec 0020, *Evaluator create*):
//! preflight and create build it the same way, so they judge it the same
//! way. A secret value never enters it; `create` hands the values to the
//! work apart, and only their shape (length, control characters) reaches
//! the draft.

use stackly_core::instance::create::{
    CreateDraft, PortRequestDraft, ProviderChoice, SecretSourceKind, placeholder_instance_id,
};
use stackly_domain::RuntimeId;
use stackly_platform::SecretValue;
use stackly_protocol::instance::{
    CreateInstanceParams, EndpointRequest, PortRequest, PreflightCreateParams, ProviderSelector,
    SecretSource, SettingsDocument,
};
use stackly_protocol::secret::SecretInput;

use crate::ipc::secret::secret_value;

/// The runtime ID of a wire UUID. A UUID that is not a version 7 UUID names
/// no runtime; the nil runtime stands for it, which no registry holds, so
/// the evaluator answers `instance.runtime_not_found` as for any unknown
/// runtime.
fn runtime_id(uuid: uuid::Uuid) -> RuntimeId {
    RuntimeId::from_uuid(uuid).unwrap_or_else(|_| {
        RuntimeId::from_uuid(placeholder_instance_id().as_uuid())
            .expect("the placeholder is a version 7 UUID")
    })
}

fn provider(selector: &ProviderSelector) -> ProviderChoice {
    ProviderChoice {
        name: selector.name.clone(),
        revision: selector.revision.clone(),
    }
}

fn endpoints(requests: &[EndpointRequest]) -> Vec<(String, PortRequestDraft)> {
    requests
        .iter()
        .map(|request| {
            (
                request.name.clone(),
                match request.port {
                    PortRequest::Auto => PortRequestDraft::Auto,
                    PortRequest::Fixed(port) => PortRequestDraft::Fixed(port),
                },
            )
        })
        .collect()
}

fn settings(document: &SettingsDocument) -> serde_json::Value {
    serde_json::Value::Object(document.as_map().clone())
}

/// The draft of a preflight.
pub(super) fn from_preflight(params: &PreflightCreateParams) -> CreateDraft {
    CreateDraft {
        name: params.name.clone(),
        display_name: params.display_name.clone(),
        provider: provider(&params.provider),
        runtime_id: runtime_id(params.runtime_id),
        endpoints: endpoints(&params.endpoints),
        settings: settings(&params.settings),
        secrets: params
            .secrets
            .iter()
            .map(|plan| {
                (
                    plan.field.clone(),
                    match &plan.source {
                        SecretSource::Generate => SecretSourceKind::Generate,
                        SecretSource::Value => SecretSourceKind::Value,
                        SecretSource::CopyFrom(reference) => {
                            SecretSourceKind::CopyFrom(reference.clone())
                        }
                        SecretSource::Bind(reference) => SecretSourceKind::Bind(reference.clone()),
                    },
                )
            })
            .collect(),
        secret_values: Vec::new(),
        start: false,
    }
}

/// The draft of a create and the values of its `value` secrets, in the
/// order of the request. `CreateInstanceWork::new` reads the shape of each
/// value into the draft.
pub(super) fn from_create(
    params: &CreateInstanceParams,
) -> (CreateDraft, Vec<(String, SecretValue)>) {
    let mut values = Vec::new();
    let secrets = params
        .secrets
        .iter()
        .map(|secret| {
            let source = match &secret.input {
                SecretInput::Generate => SecretSourceKind::Generate,
                SecretInput::Value(text) => {
                    values.push((secret.field.clone(), secret_value(text)));
                    SecretSourceKind::Value
                }
                SecretInput::CopyFrom(reference) => SecretSourceKind::CopyFrom(reference.clone()),
                SecretInput::Bind(reference) => SecretSourceKind::Bind(reference.clone()),
            };
            (secret.field.clone(), source)
        })
        .collect();
    let mut draft = CreateDraft {
        name: params.name.clone(),
        display_name: params.display_name.clone(),
        provider: provider(&params.provider),
        runtime_id: runtime_id(params.runtime_id),
        endpoints: endpoints(&params.endpoints),
        settings: settings(&params.settings),
        secrets,
        secret_values: Vec::new(),
        start: false,
    };
    draft.secret_values = values
        .iter()
        .map(|(field, value)| (field.clone(), stackly_core::secret::value_shape(value)))
        .collect();
    (draft, values)
}
