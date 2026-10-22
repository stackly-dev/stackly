//! Maps the create form of `stackly-core` to the `instance` DTOs of spec
//! 0020: [`CreateFormModel`] to `CreateForm`, and a settings field to
//! `FormField`, typed from the schema in the order the schema declares.
//!
//! Nothing here knows a provider by name: the form is the package.

use stackly_core::catalog::category_code;
use stackly_core::instance::create::{
    Check, CheckKind, CheckStatus, CommandSpec, CreateError, CreateFacts, CreateFormModel,
    Evaluation, FieldCode as CoreFieldCode, FieldIssue, FormEndpoint, FormProvider, PackageSource,
    PortRequestDraft, SecretSourceKind,
};
use stackly_core::provider::{
    ApplyClass, ArrayRules, EndpointPurpose as CorePurpose, EndpointTransport as CoreTransport,
    FieldKind as CoreKind, ItemField, ScalarKind, ScalarValue, SettingField, SettingsItem,
    TextRules, Visibility,
};
use stackly_domain::CapabilitySet;
use stackly_protocol::catalog::{CapabilityStatus, ProviderCapability};
use stackly_protocol::instance as dto;
use stackly_protocol::instance::command::{
    CommandDraft, CommandPort, CommandSecret, CommandSetting, equivalent_command,
};
use stackly_protocol::json::FieldCode;
use stackly_protocol::runtime::RuntimeSummary;
use stackly_protocol::{Count, ErrorBody, FieldError, SignedSafeInteger};

use crate::map::catalog::wire;

/// The form of `model` for the runtime `runtime`.
#[must_use]
pub fn create_form(
    model: &CreateFormModel,
    runtime: RuntimeSummary,
    capabilities: Vec<CapabilityStatus>,
) -> dto::CreateForm {
    dto::CreateForm {
        provider: provider(&model.provider),
        runtime,
        suggested_name: model.suggested_name.as_ref().map(ToString::to_string),
        data_path: model.data_path.to_owned(),
        endpoints: model.endpoints.iter().map(endpoint).collect(),
        fields: model.fields.iter().map(form_field).collect(),
        capabilities,
    }
}

/// The capabilities of a package in the fixed order of spec 0017, each
/// with whether the package declares it.
#[must_use]
pub fn capabilities(declared: &CapabilitySet) -> Vec<CapabilityStatus> {
    ProviderCapability::ALL
        .into_iter()
        .map(|capability| CapabilityStatus {
            capability,
            supported: declared
                .iter()
                .any(|declared| wire::<ProviderCapability>(declared.as_code()) == capability),
        })
        .collect()
}

fn provider(provider: &FormProvider) -> dto::CreateProvider {
    dto::CreateProvider {
        name: provider.name.to_string(),
        revision: provider.revision.to_string(),
        manifest_sha256: provider.manifest_sha256.to_string(),
        display_name: provider.display_name.clone(),
        category: wire(category_code(provider.category)),
        source: match provider.source {
            PackageSource::BundledCatalog => dto::CreateProviderSource::BundledCatalog,
            PackageSource::DevelopmentCatalog => dto::CreateProviderSource::DevelopmentCatalog,
            PackageSource::Pinned => dto::CreateProviderSource::Pinned,
        },
    }
}

fn endpoint(endpoint: &FormEndpoint) -> dto::CreateEndpoint {
    dto::CreateEndpoint {
        name: endpoint.name.clone(),
        transport: match endpoint.transport {
            CoreTransport::Tcp => dto::EndpointTransport::Tcp,
            CoreTransport::Udp => dto::EndpointTransport::Udp,
            CoreTransport::Unix => dto::EndpointTransport::Unix,
            _ => dto::EndpointTransport::Unrecognized,
        },
        purpose: match endpoint.purpose {
            CorePurpose::Client => dto::EndpointPurpose::Client,
            CorePurpose::Admin => dto::EndpointPurpose::Admin,
            CorePurpose::Monitoring => dto::EndpointPurpose::Monitoring,
            _ => dto::EndpointPurpose::Unrecognized,
        },
        host: endpoint.host.clone(),
        default_port: endpoint.default_port,
        socket_name: endpoint.socket_name.clone(),
    }
}

/// One settings field as the form shows it. A secret field is a text
/// field whose value travels in `secrets`, so it carries the text rules
/// that `SecretFieldRule` checks and no default.
#[must_use]
pub fn form_field(field: &SettingField) -> dto::FormField {
    let mut out = dto::FormField {
        key: field.name().to_string(),
        title: field.title().to_owned(),
        description: field.description().map(str::to_owned),
        kind: dto::FieldKind::Text,
        required: field.is_required(),
        secret: field.is_secret(),
        printable_only: field.is_printable_only(),
        managed: field.managed().is_some(),
        visibility: match field.visibility() {
            Visibility::Advanced => dto::FieldVisibility::Advanced,
            _ => dto::FieldVisibility::Basic,
        },
        apply: field.apply().map(|apply| match apply {
            ApplyClass::Reload => dto::FieldApply::Reload,
            ApplyClass::Restart => dto::FieldApply::Restart,
            ApplyClass::ImmutableAfterInit => dto::FieldApply::ImmutableAfterInit,
            _ => dto::FieldApply::Unrecognized,
        }),
        default: None,
        min_length: None,
        max_length: None,
        minimum: None,
        maximum: None,
        choices: Vec::new(),
        min_items: None,
        max_items: None,
        item_key: None,
        items: Vec::new(),
    };
    match field.kind() {
        CoreKind::Scalar(scalar) => scalar_field(&mut out, scalar, field.is_secret()),
        CoreKind::Array(rules) => array_field(&mut out, rules),
        _ => out.kind = dto::FieldKind::Unrecognized,
    }
    out
}

fn scalar_field(out: &mut dto::FormField, scalar: &ScalarKind, secret: bool) {
    match scalar {
        ScalarKind::Text(rules) => {
            text_rules(out, rules);
            if secret {
                out.default = None;
            }
        }
        ScalarKind::Integer(rules) => {
            out.kind = dto::FieldKind::Integer;
            out.minimum = Some(SignedSafeInteger::saturating(rules.minimum));
            out.maximum = Some(SignedSafeInteger::saturating(rules.maximum));
            out.default = rules
                .default
                .map(|value| dto::SettingValue::Integer(SignedSafeInteger::saturating(value)));
        }
        ScalarKind::Boolean(rules) => {
            out.kind = dto::FieldKind::Boolean;
            out.default = rules.default.map(dto::SettingValue::Boolean);
        }
        _ => out.kind = dto::FieldKind::Unrecognized,
    }
}

fn text_rules(out: &mut dto::FormField, rules: &TextRules) {
    match &rules.enum_values {
        Some(values) => {
            out.kind = dto::FieldKind::Choice;
            out.choices.clone_from(values);
        }
        None => {
            out.kind = dto::FieldKind::Text;
            out.min_length = Some(Count::saturating(rules.min_length));
            out.max_length = rules.max_length.map(Count::saturating);
        }
    }
    out.default = rules.default.clone().map(dto::SettingValue::Text);
}

fn array_field(out: &mut dto::FormField, rules: &ArrayRules) {
    out.kind = dto::FieldKind::List;
    out.min_items = Some(Count::saturating(rules.min_items));
    out.max_items = Some(Count::saturating(rules.max_items));
    out.item_key = rules.key.as_ref().map(ToString::to_string);
    out.items = rules.item_fields().map(item_field).collect();
    out.default = rules.default.as_ref().map(|items| {
        dto::SettingValue::List(items.iter().map(|item| setting_item(rules, item)).collect())
    });
}

fn setting_item(rules: &ArrayRules, item: &SettingsItem) -> dto::SettingItem {
    dto::SettingItem {
        entries: rules
            .item_fields()
            .filter_map(|field| {
                let value = item.get(field.name.as_str())?;
                Some(dto::SettingEntry {
                    key: field.name.to_string(),
                    value: scalar_setting(value),
                })
            })
            .collect(),
    }
}

fn scalar_setting(value: &ScalarValue) -> dto::ScalarSetting {
    match value {
        ScalarValue::Text(text) => dto::ScalarSetting::Text(text.clone()),
        ScalarValue::Integer(number) => {
            dto::ScalarSetting::Integer(SignedSafeInteger::saturating(*number))
        }
        ScalarValue::Boolean(on) => dto::ScalarSetting::Boolean(*on),
        // `ScalarValue` is non exhaustive; every value of this build is
        // covered above, so a newer one cannot be shown as a default.
        _ => dto::ScalarSetting::Text(String::new()),
    }
}

fn item_field(field: &ItemField) -> dto::FormItemField {
    let mut out = dto::FormItemField {
        key: field.name.to_string(),
        title: field.title.clone(),
        description: field.description.clone(),
        kind: dto::FieldKind::Text,
        required: field.required,
        default: None,
        min_length: None,
        max_length: None,
        minimum: None,
        maximum: None,
        choices: Vec::new(),
    };
    match &field.kind {
        ScalarKind::Text(rules) => match &rules.enum_values {
            Some(values) => {
                out.kind = dto::FieldKind::Choice;
                out.choices.clone_from(values);
                out.default = rules.default.clone().map(dto::ScalarSetting::Text);
            }
            None => {
                out.min_length = Some(Count::saturating(rules.min_length));
                out.max_length = rules.max_length.map(Count::saturating);
                out.default = rules.default.clone().map(dto::ScalarSetting::Text);
            }
        },
        ScalarKind::Integer(rules) => {
            out.kind = dto::FieldKind::Integer;
            out.minimum = Some(SignedSafeInteger::saturating(rules.minimum));
            out.maximum = Some(SignedSafeInteger::saturating(rules.maximum));
            out.default = rules
                .default
                .map(|value| dto::ScalarSetting::Integer(SignedSafeInteger::saturating(value)));
        }
        ScalarKind::Boolean(rules) => {
            out.kind = dto::FieldKind::Boolean;
            out.default = rules.default.map(dto::ScalarSetting::Boolean);
        }
        _ => out.kind = dto::FieldKind::Unrecognized,
    }
    out
}

/// The longest title or detail of a preflight row, in characters.
const ROW_TEXT_MAX: usize = 200;

/// `text` cut to [`ROW_TEXT_MAX`] characters.
fn row_text(text: &str) -> String {
    text.chars().take(ROW_TEXT_MAX).collect()
}

/// The field issues as `FieldError`s, in the order given (by path).
#[must_use]
pub fn field_errors(issues: &[FieldIssue]) -> Vec<FieldError> {
    issues
        .iter()
        .map(|issue| {
            FieldError::new(
                issue.path.clone(),
                match issue.code {
                    CoreFieldCode::Required => FieldCode::Required,
                    CoreFieldCode::TooShort => FieldCode::TooShort,
                    CoreFieldCode::TooLong => FieldCode::TooLong,
                    CoreFieldCode::InvalidFormat => FieldCode::InvalidFormat,
                    CoreFieldCode::OutOfRange => FieldCode::OutOfRange,
                    CoreFieldCode::TooMany => FieldCode::TooMany,
                    CoreFieldCode::UnknownValue => FieldCode::UnknownValue,
                    _ => FieldCode::InvalidValue,
                },
            )
        })
        .collect()
}

/// The error body of a failing row: the error `instance.create` answers
/// with. `fields` carries every field issue only for a code that lists
/// `omitted` and no `items` (spec 0020 D7); every other code carries none.
#[must_use]
pub fn create_error(error: &CreateError, issues: &[FieldIssue]) -> ErrorBody {
    let body = crate::ipc::operation::operation_error_body(error);
    if error.carries_fields() {
        body.with_fields(field_errors(issues))
    } else {
        body
    }
}

/// `body` with the static hint of its `instance.*` or `catalog.pin_failed`
/// code, unless it carries one.
#[must_use]
pub fn with_create_hint(body: ErrorBody) -> ErrorBody {
    if body.hint().is_some() {
        return body;
    }
    let hint = if body.code().starts_with("instance.") {
        stackly_protocol::codes::instance::hint(body.code(), body.details())
    } else if body.code() == stackly_protocol::codes::catalog::PIN_FAILED.code {
        stackly_protocol::codes::catalog::hint(body.code(), body.details())
    } else {
        None
    };
    match hint {
        Some(hint) => body.with_hint(hint),
        None => body,
    }
}

fn check(check: &Check, issues: &[FieldIssue]) -> dto::PreflightCheck {
    dto::PreflightCheck {
        id: check.id.clone(),
        kind: match check.kind {
            CheckKind::Provider => dto::PreflightCheckKind::Provider,
            CheckKind::Runtime => dto::PreflightCheckKind::Runtime,
            CheckKind::Name => dto::PreflightCheckKind::Name,
            CheckKind::Endpoint => dto::PreflightCheckKind::Endpoint,
            CheckKind::Settings => dto::PreflightCheckKind::Settings,
            CheckKind::Data => dto::PreflightCheckKind::Data,
            CheckKind::Disk => dto::PreflightCheckKind::Disk,
            CheckKind::Secret => dto::PreflightCheckKind::Secret,
        },
        status: match check.status {
            CheckStatus::Ok => dto::PreflightStatus::Ok,
            CheckStatus::Caution => dto::PreflightStatus::Caution,
            CheckStatus::Error => dto::PreflightStatus::Error,
        },
        title: row_text(&check.title),
        detail: row_text(&check.detail),
        subject: check.subject.clone(),
        suggested_port: check.suggested_port,
        error: check
            .error
            .as_ref()
            .map(|error| create_error(error, issues)),
    }
}

/// The preflight of `evaluation`, with the equivalent command written.
#[must_use]
pub fn preflight(evaluation: &Evaluation, facts: &CreateFacts) -> dto::CreatePreflight {
    dto::CreatePreflight {
        ready: evaluation.ready(),
        provider: facts.provider.as_ref().ok().map(|found| {
            let key = found.package.key();
            dto::ResolvedProvider {
                name: key.provider().to_string(),
                revision: key.revision().to_string(),
                manifest_sha256: key.manifest_sha256().to_string(),
            }
        }),
        checks: evaluation
            .checks
            .iter()
            .map(|row| check(row, &evaluation.field_issues))
            .collect(),
        fields: field_errors(&evaluation.field_issues),
        command: equivalent_command(&command_draft(&evaluation.command)),
    }
}

/// The wire draft of the command `spec` stands for.
#[must_use]
pub fn command_draft(spec: &CommandSpec) -> CommandDraft {
    CommandDraft {
        name: spec.name.clone(),
        runtime: spec.runtime_label.clone(),
        provider: spec.provider.clone(),
        revision: spec.revision.clone(),
        display_name: spec.display_name.clone(),
        ports: spec
            .ports
            .iter()
            .map(|(endpoint, request)| CommandPort {
                endpoint: endpoint.clone(),
                port: match request {
                    PortRequestDraft::Auto => dto::PortRequest::Auto,
                    PortRequestDraft::Fixed(port) => dto::PortRequest::Fixed(*port),
                },
            })
            .collect(),
        settings: spec
            .settings
            .iter()
            .filter_map(|(field, value)| {
                Some(CommandSetting {
                    field: field.clone(),
                    value: dto::SettingValue::from_json(value)?,
                })
            })
            .collect(),
        secrets: spec
            .secrets
            .iter()
            .map(|(field, source)| CommandSecret {
                field: field.clone(),
                source: match source {
                    SecretSourceKind::Generate => dto::SecretSource::Generate,
                    SecretSourceKind::Value => dto::SecretSource::Value,
                    SecretSourceKind::CopyFrom(reference) => {
                        dto::SecretSource::CopyFrom(reference.clone())
                    }
                    SecretSourceKind::Bind(reference) => dto::SecretSource::Bind(reference.clone()),
                },
            })
            .collect(),
        start: spec.start,
    }
}
