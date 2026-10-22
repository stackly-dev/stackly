//! Maps the duplicate model of `stackly-core` to the DTOs of
//! `instance.get_duplicate_form` (spec 0024): the model to `DuplicateForm`,
//! and the [`CreateDraft`] of a duplicate to the `PreflightCreateParams` a
//! client sends back unchanged to `instance.preflight`.

use stackly_core::instance::create::{
    CreateDraft, CreateError, PortRequestDraft, SecretSourceKind,
};
use stackly_core::instance::duplicate::{DuplicateError, DuplicateModel};
use stackly_core::supervisor::SupervisorError;
use stackly_protocol::catalog::CapabilityStatus;
use stackly_protocol::codes::{instance, internal, resource};
use stackly_protocol::instance::{
    DuplicateForm, DuplicateSecret, DuplicateSource, EndpointRequest, PortRequest,
    PreflightCreateParams, ProviderSelector, SecretPlan, SecretSource, SettingsDocument,
};
use stackly_protocol::runtime::RuntimeSummary;
use stackly_protocol::{ErrorBody, ResourceKind, ResourceRef};
use uuid::Uuid;

use super::create;

/// The reply of `instance.get_duplicate_form` for `model`, whose runtime
/// the caller already summarized.
#[must_use]
pub fn duplicate_form(
    model: &DuplicateModel,
    runtime: RuntimeSummary,
    capabilities: Vec<CapabilityStatus>,
) -> DuplicateForm {
    DuplicateForm {
        source: DuplicateSource {
            instance_id: model.source.0.as_uuid(),
            name: model.source.1.to_string(),
        },
        form: create::create_form(&model.form, runtime, capabilities),
        draft: draft_params(&model.draft),
        secrets: model
            .secret_sources
            .iter()
            .map(|(field, reference)| DuplicateSecret {
                field: field.as_str().to_owned(),
                source_reference: reference.as_ref().map(ToString::to_string),
            })
            .collect(),
    }
}

/// The params of a preflight that carry `draft`.
///
/// The inverse of the mapping `draft::from_preflight` does for the
/// handlers of spec 0020, for the sources a duplicate draft can hold.
#[must_use]
pub fn draft_params(draft: &CreateDraft) -> PreflightCreateParams {
    let settings = match &draft.settings {
        serde_json::Value::Object(map) => map.clone(),
        _ => serde_json::Map::new(),
    };
    PreflightCreateParams {
        name: draft.name.clone(),
        display_name: draft.display_name.clone(),
        provider: ProviderSelector {
            name: draft.provider.name.clone(),
            revision: draft.provider.revision.clone(),
        },
        runtime_id: draft.runtime_id.as_uuid(),
        endpoints: draft
            .endpoints
            .iter()
            .map(|(name, port)| EndpointRequest {
                name: name.clone(),
                port: match port {
                    PortRequestDraft::Auto => PortRequest::Auto,
                    PortRequestDraft::Fixed(port) => PortRequest::Fixed(*port),
                },
            })
            .collect(),
        settings: SettingsDocument::new(settings),
        secrets: draft
            .secrets
            .iter()
            .map(|(field, source)| SecretPlan {
                field: field.clone(),
                source: match source {
                    SecretSourceKind::Generate => SecretSource::Generate,
                    SecretSourceKind::Value => SecretSource::Value,
                    SecretSourceKind::CopyFrom(reference) => {
                        SecretSource::CopyFrom(reference.clone())
                    }
                    SecretSourceKind::Bind(reference) => SecretSource::Bind(reference.clone()),
                },
            })
            .collect(),
        start: draft.start,
    }
}

/// The error body of a refused duplicate form for the instance `source`.
///
/// Each variant answers the code of the same name in the method table of
/// spec 0024; the supervisor and create codes keep the details they carry
/// everywhere else.
#[must_use]
pub fn duplicate_error(error: &DuplicateError, source: Uuid) -> ErrorBody {
    match error {
        DuplicateError::NotFound => {
            ErrorBody::new(&resource::NOT_FOUND, "the instance does not exist").with_resource(
                ResourceRef {
                    kind: ResourceKind::Instance,
                    id: source.to_string(),
                },
            )
        }
        DuplicateError::Retained => {
            crate::supervisor::supervisor_error(&SupervisorError::InstanceRetained)
        }
        DuplicateError::PackageUnavailable { provider, reason } => {
            crate::supervisor::supervisor_error(&SupervisorError::PackageUnavailable {
                provider: provider.clone(),
                reason: reason.clone(),
            })
        }
        DuplicateError::ProviderAmbiguous { provider, revision } => {
            create::with_create_hint(create::create_error(
                &CreateError::ProviderAmbiguous {
                    provider: provider.to_string(),
                    revision: Some(revision.to_string()),
                },
                &[],
            ))
        }
        DuplicateError::CapabilityUnsupported { provider, existing } => create::with_create_hint(
            ErrorBody::new(
                &instance::CAPABILITY_UNSUPPORTED,
                format!("{provider} runs one instance at a time, and {existing} is it"),
            )
            .with_detail("capability", "multi_instance")
            .with_detail("provider", provider.to_string())
            .with_detail("existing", existing.to_string()),
        ),
        DuplicateError::RuntimeNotFound { runtime_id } => {
            create::with_create_hint(create::create_error(
                &CreateError::RuntimeNotFound {
                    runtime_id: *runtime_id,
                },
                &[],
            ))
        }
        DuplicateError::RuntimeUnavailable { runtime_id, state } => {
            crate::supervisor::supervisor_error(&SupervisorError::RuntimeUnavailable {
                runtime_id: runtime_id.to_string(),
                state: state.as_code().to_owned(),
            })
        }
        DuplicateError::Registry(error) => super::registry_error(error),
        _ => ErrorBody::new(
            &internal::UNEXPECTED,
            "the duplicate form was refused for a reason this build cannot name",
        ),
    }
}
