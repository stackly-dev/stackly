//! The `secret.*` methods (spec 0011, *Permukaan API*): thin handlers over
//! [`SecretService`], the mapping of its rows to the DTOs, and the error
//! body of every [`SecretError`] variant, store failure, and needs secret
//! result.
//!
//! This module and `expose` (spec 0011, *Penjaga di kode*): it is the one
//! place in `stacklyd` that turns a wire [`SecretText`] into a
//! [`SecretValue`] and back, so `cargo xtask check-deps` allows `expose`
//! here and nowhere else in the crate.

use stackly_core::secret::delivery::SecretInArgv;
use stackly_core::secret::{
    ConflictReason, EffectKind, NeedsSecret, PutOutcome, PutRequest, PutTarget, PutWarning,
    RevealPurpose as CoreRevealPurpose, SecretCaller, SecretEffect, SecretError,
    SecretInput as CoreInput,
};
use stackly_domain::{
    InstanceBlocker, NeedsSecretItem as DomainNeedsItem, NeedsSecretReason as DomainReason,
    ResourceRevision, SecretOrigin, SecretOwner, SecretOwnerKind as DomainOwnerKind, SecretRef,
    SecretReferenceFilter, SecretReferenceView, SecretStatus as DomainStatus, SecretUse,
    SecretUser,
};
use stackly_platform::{SecretStoreError, SecretValue, WriteMode};
use stackly_protocol::codes::{internal, resource, secret};
use stackly_protocol::json::FieldCode;
use stackly_protocol::secret::{
    DeleteSecretParams, DeleteSecretReply, ListSecretsParams, ListSecretsReply, NeedsSecretBlocker,
    NeedsSecretItem, NeedsSecretReason, RevealPurpose, RevealSecretParams, RevealSecretReply,
    SecretEffectInfo, SecretEffectKind, SecretInput, SecretMode, SecretOwnerKind, SecretRefSummary,
    SecretStatus, SecretStatusParams, SecretStatusReply, SecretText, SecretUseInfo,
    SecretValueOrigin, SecretWarning, SetSecretParams, SetSecretReply,
};
use stackly_protocol::{ErrorBody, ErrorItem, FieldError, ResourceKind, ResourceRef, Timestamp};

use super::operation::dto;
use crate::dispatch::{HandlerFuture, RequestContext};
use crate::secrets::store_status;
use crate::state::DaemonCore;

/// The item kind of one unreadable reference.
const REFERENCE_ITEM: &str = "reference";
/// The item kind of one use of a reference.
const USE_ITEM: &str = "use";
/// The item kind of one effect of a replace.
const EFFECT_ITEM: &str = "effect";

/// The real `secret.list`: the cached rows and the store, with no store
/// call (AC-8).
pub fn list<'a>(
    core: &'a DaemonCore,
    params: ListSecretsParams,
) -> HandlerFuture<'a, ListSecretsReply> {
    Box::pin(async move {
        let service = core.secrets().service()?;
        let filter = SecretReferenceFilter {
            owner_kind: params.owner_kind.and_then(domain_owner_kind),
            status: params.status.and_then(domain_status),
        };
        let target = core.secrets().target();
        let listing = blocking(move || service.list(filter)).await?;
        Ok(ListSecretsReply {
            items: listing.items.iter().map(summary).collect(),
            store: store_status(listing.store, target),
        })
    })
}

/// The real `secret.status`: probes the reference again (AC-8).
pub fn status<'a>(
    core: &'a DaemonCore,
    params: SecretStatusParams,
) -> HandlerFuture<'a, SecretStatusReply> {
    Box::pin(async move {
        let service = core.secrets().service()?;
        let reference = parse_reference(&params.reference)?;
        let target = core.secrets().target();
        let listing = blocking(move || service.status(&reference)).await?;
        let view = listing.items.first().ok_or_else(|| {
            unexpected("the status of a secret reference came back without a row")
        })?;
        Ok(SecretStatusReply {
            summary: summary(view),
            store: store_status(listing.store, target),
        })
    })
}

/// The real `secret.set`: `put` on a reference the user names (AC-5).
pub fn set<'a>(
    core: &'a DaemonCore,
    context: &'a RequestContext,
    params: SetSecretParams,
) -> HandlerFuture<'a, SetSecretReply> {
    Box::pin(async move {
        let service = core.secrets().service()?;
        let caller = caller(context);
        let request = put_request(params)?;
        let outcome = blocking(move || service.put(request, &caller)).await?;
        Ok(set_reply(&outcome))
    })
}

/// The real `secret.reveal`: the one reply that carries a value (AC-9).
pub fn reveal<'a>(
    core: &'a DaemonCore,
    context: &'a RequestContext,
    params: RevealSecretParams,
) -> HandlerFuture<'a, RevealSecretReply> {
    Box::pin(async move {
        let service = core.secrets().service()?;
        let caller = caller(context);
        let reference = parse_reference(&params.reference)?;
        let purpose = match params.purpose {
            RevealPurpose::Copy => CoreRevealPurpose::Copy,
            _ => CoreRevealPurpose::Show,
        };
        let text = reference.to_string();
        let value = blocking(move || service.reveal(&reference, purpose, &caller)).await?;
        Ok(RevealSecretReply {
            reference: text,
            value: secret_text(&value),
        })
    })
}

/// The real `secret.delete` (AC-10).
pub fn delete<'a>(
    core: &'a DaemonCore,
    context: &'a RequestContext,
    params: DeleteSecretParams,
) -> HandlerFuture<'a, DeleteSecretReply> {
    Box::pin(async move {
        let service = core.secrets().service()?;
        let caller = caller(context);
        let reference = parse_reference(&params.reference)?;
        let expected = params
            .expected_revision
            .map(|revision| {
                ResourceRevision::new(revision.get()).map_err(|_| {
                    Box::new(
                        ErrorBody::new(
                            &stackly_protocol::codes::request::INVALID_PARAMS,
                            "params do not match the method",
                        )
                        .with_detail("method", "secret.delete")
                        .with_field(FieldError::new("expectedRevision", FieldCode::OutOfRange)),
                    )
                })
            })
            .transpose()?;
        let text = reference.to_string();
        blocking(move || service.delete(&reference, expected, &caller)).await?;
        Ok(DeleteSecretReply { reference: text })
    })
}

/// Runs a service call on a blocking thread, as spec 0004 asks of every
/// call into a sync use case, and maps its failure to the wire.
async fn blocking<T: Send + 'static>(
    call: impl FnOnce() -> Result<T, SecretError> + Send + 'static,
) -> Result<T, Box<ErrorBody>> {
    match tokio::task::spawn_blocking(call).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(error)) => Err(Box::new(secret_error(&error))),
        Err(failure) => {
            tracing::error!(error = %failure, "a secret service call stopped before it ended");
            Err(unexpected("the secret service call stopped"))
        }
    }
}

/// Who asks, from the request's context: a client request is the user's,
/// and only an interactive client may see a Keychain dialog (AC-4).
pub(crate) fn caller(context: &RequestContext) -> SecretCaller {
    SecretCaller::client(
        super::operation::client_code(context.client.kind),
        context.peer.uid,
        context.client.is_interactive(),
    )
}

/// The reference a validated request names.
fn parse_reference(text: &str) -> Result<SecretRef, Box<ErrorBody>> {
    SecretRef::parse(text).map_err(|error| {
        Box::new(
            ErrorBody::new(
                &secret::INVALID_REFERENCE,
                "the text is not a secret reference",
            )
            .with_detail("reference", text)
            .with_detail("reason", reference_reason(&error)),
        )
    })
}

fn reference_reason(error: &stackly_domain::SecretRefError) -> &'static str {
    use stackly_domain::SecretRefError as E;
    match error {
        E::TooLong => "too_long",
        E::MissingScheme => "missing_scheme",
        E::UnsupportedScheme => "unsupported_scheme",
        _ => "format",
    }
}

/// The `put` request of validated `secret.set` params.
fn put_request(params: SetSecretParams) -> Result<PutRequest, Box<ErrorBody>> {
    let reference = parse_reference(&params.reference)?;
    let input = match &params.input {
        SecretInput::Value(text) => CoreInput::Value(secret_value(text)),
        SecretInput::Generate => CoreInput::Generate,
        SecretInput::CopyFrom(source) => CoreInput::CopyFrom(parse_reference(source)?),
        // `Validate` refuses `bind` here; a handler that sees one anyway
        // is a bug, never a request to serve.
        SecretInput::Bind(_) => return Err(unexpected("secret.set does not take a bind input")),
    };
    let mode = match params.mode {
        SecretMode::Create => WriteMode::Create,
        SecretMode::Replace => WriteMode::Replace,
        SecretMode::Upsert => WriteMode::Upsert,
        _ => return Err(unexpected("secret.set got a mode it does not know")),
    };
    let mut request = PutRequest::new(PutTarget::Existing(reference), input, mode);
    request.expected_revision = params
        .expected_revision
        .map(|revision| {
            ResourceRevision::new(revision.get()).map_err(|_| {
                Box::new(
                    ErrorBody::new(
                        &stackly_protocol::codes::request::INVALID_PARAMS,
                        "params do not match the method",
                    )
                    .with_detail("method", "secret.set")
                    .with_field(FieldError::new("expectedRevision", FieldCode::OutOfRange)),
                )
            })
        })
        .transpose()?;
    request.confirm_stored_only = params.confirm_stored_only;
    Ok(request)
}

/// A wire value as the platform's: the one place a [`SecretText`] is read.
///
/// `Validate` of every params type that carries a [`SecretInput`] already
/// refused a value that is empty, over 65536 bytes, or holding NUL
/// (AC-7), and `SecretValue::new` refuses exactly those.
///
/// # Panics
///
/// Panics when the text breaks that rule, which means a params type skipped
/// its `Validate`.
#[must_use]
pub fn secret_value(text: &SecretText) -> SecretValue {
    SecretValue::new(text.expose().to_owned())
        .expect("Validate admitted a value of 1 to 65536 bytes without NUL (spec 0011 AC-7)")
}

/// A platform value as the wire's: the one place a value leaves for a
/// reply, which only `secret.reveal` sends (AC-9).
#[must_use]
pub fn secret_text(value: &SecretValue) -> SecretText {
    SecretText::new(value.expose().to_owned())
}

/// The input of a secret field of `instance.create`, as the service takes
/// it: the one place a wire secret of a create turns into a platform value
/// (K15). `bind` is valid here, unlike for `secret.set`.
///
/// # Errors
///
/// Returns `request.invalid_params` for a reference that does not parse.
pub(crate) fn create_input(input: &SecretInput) -> Result<CoreInput, Box<ErrorBody>> {
    Ok(match input {
        SecretInput::Value(text) => CoreInput::Value(secret_value(text)),
        SecretInput::Generate => CoreInput::Generate,
        SecretInput::CopyFrom(source) => CoreInput::CopyFrom(parse_reference(source)?),
        SecretInput::Bind(reference) => CoreInput::Bind(parse_reference(reference)?),
    })
}

fn set_reply(outcome: &PutOutcome) -> SetSecretReply {
    SetSecretReply {
        summary: summary(&outcome.reference),
        warnings: outcome
            .warnings
            .iter()
            .map(|warning| match warning {
                PutWarning::ShortValueNotScrubbed => SecretWarning::ShortValueNotScrubbed,
                _ => SecretWarning::Unrecognized,
            })
            .collect(),
        effects: outcome.effects.iter().map(effect_info).collect(),
    }
}

fn effect_info(effect: &SecretEffect) -> SecretEffectInfo {
    SecretEffectInfo {
        resource_kind: user_kind(&effect.user),
        resource_id: effect.user.id().to_string(),
        field: effect.field.to_string(),
        effect: match effect.effect {
            EffectKind::StoredOnly => SecretEffectKind::StoredOnly,
            EffectKind::NextReload => SecretEffectKind::NextReload,
            EffectKind::NextStart => SecretEffectKind::NextStart,
            _ => SecretEffectKind::Unrecognized,
        },
    }
}

const fn user_kind(user: &SecretUser) -> ResourceKind {
    match user {
        SecretUser::Instance(_) => ResourceKind::Instance,
        SecretUser::Environment(_) => ResourceKind::Environment,
    }
}

fn use_info(used: &SecretUse) -> SecretUseInfo {
    SecretUseInfo {
        resource_kind: user_kind(&used.user),
        resource_id: used.user.id().to_string(),
        field: used.field.to_string(),
    }
}

/// One reference row as `secret.list` and `secret.status` report it:
/// never a value.
#[must_use]
pub fn summary(view: &SecretReferenceView) -> SecretRefSummary {
    let record = &view.record;
    SecretRefSummary {
        reference: record.reference.to_string(),
        owner_kind: match record.owner {
            SecretOwner::Instance(_) => SecretOwnerKind::Instance,
            SecretOwner::Environment(_) => SecretOwnerKind::Environment,
            _ => SecretOwnerKind::User,
        },
        owner_id: record.owner.id(),
        purpose: record.purpose.to_string(),
        status: match record.status {
            DomainStatus::Present => SecretStatus::Present,
            DomainStatus::Missing => SecretStatus::Missing,
            DomainStatus::Unknown => SecretStatus::Unknown,
        },
        status_observed_at: record.status_observed_at.map(Timestamp::from_jiff),
        value_origin: record.value_origin.map(|origin| match origin {
            SecretOrigin::Entered => SecretValueOrigin::Entered,
            SecretOrigin::Generated => SecretValueOrigin::Generated,
            SecretOrigin::Copied => SecretValueOrigin::Copied,
        }),
        value_updated_at: record.value_updated_at.map(Timestamp::from_jiff),
        used_by: view.used_by.iter().map(use_info).collect(),
        revision: dto::revision(record.revision),
    }
}

const fn domain_owner_kind(kind: SecretOwnerKind) -> Option<DomainOwnerKind> {
    match kind {
        SecretOwnerKind::Instance => Some(DomainOwnerKind::Instance),
        SecretOwnerKind::Environment => Some(DomainOwnerKind::Environment),
        SecretOwnerKind::User => Some(DomainOwnerKind::User),
        _ => None,
    }
}

const fn domain_status(status: SecretStatus) -> Option<DomainStatus> {
    match status {
        SecretStatus::Present => Some(DomainStatus::Present),
        SecretStatus::Missing => Some(DomainStatus::Missing),
        SecretStatus::Unknown => Some(DomainStatus::Unknown),
        _ => None,
    }
}

/// The `needsSecret` blocker content of an instance's blocker, or `None`
/// for another kind of blocker. The instance DTO of feature 10 carries it
/// in its `blocker` slot (spec 0011, *Needs secret*).
#[must_use]
pub fn needs_secret_blocker(blocker: &InstanceBlocker) -> Option<NeedsSecretBlocker> {
    match blocker {
        InstanceBlocker::NeedsSecret {
            references,
            observed_at,
        } => Some(NeedsSecretBlocker {
            references: references.iter().map(needs_item).collect(),
            observed_at: Timestamp::from_jiff(*observed_at),
        }),
        _ => None,
    }
}

fn needs_item(item: &DomainNeedsItem) -> NeedsSecretItem {
    NeedsSecretItem {
        reference: item.reference.to_string(),
        reason: needs_reason(item.reason),
        generate_allowed: item.generate_allowed,
    }
}

const fn needs_reason(reason: DomainReason) -> NeedsSecretReason {
    match reason {
        DomainReason::Missing => NeedsSecretReason::Missing,
        DomainReason::StoreLocked => NeedsSecretReason::StoreLocked,
        DomainReason::AccessDenied => NeedsSecretReason::AccessDenied,
        DomainReason::StoreBusy => NeedsSecretReason::StoreBusy,
        DomainReason::StoreUnsupported => NeedsSecretReason::StoreUnsupported,
    }
}

/// The hint every `secret.needs_secret` error carries.
const NEEDS_SECRET_HINT: &str =
    "fill them with stackly secret set, or unlock the keychain, then try again";

/// The error body of a needs secret result (spec 0011, *Needs secret*):
/// one `reference` item per unreadable reference, in the order given.
#[must_use]
pub fn needs_secret_error(needs: &NeedsSecret) -> ErrorBody {
    needs_secret_items(&needs.references)
}

fn needs_secret_items(references: &[DomainNeedsItem]) -> ErrorBody {
    let count = references.len();
    ErrorBody::new(
        &secret::NEEDS_SECRET,
        if count == 1 {
            "1 secret reference cannot be read".to_owned()
        } else {
            format!("{count} secret references cannot be read")
        },
    )
    .with_detail("count", count.to_string())
    .with_items(references.iter().map(|item| {
        ErrorItem::new(REFERENCE_ITEM)
            .with_field("reference", item.reference.to_string())
            .with_field("reason", item.reason.as_code())
            .with_field("generateAllowed", item.generate_allowed.to_string())
    }))
    .with_hint(NEEDS_SECRET_HINT)
}

/// The error body of an argument that would carry a secret value to a
/// provider process (`secret.delivery_rejected`). It names the rule, never
/// the argument or the value.
#[must_use]
pub fn delivery_error(error: &SecretInArgv) -> ErrorBody {
    ErrorBody::new(
        &secret::DELIVERY_REJECTED,
        "an argument would carry a secret value",
    )
    .with_detail("reason", error.reason.as_code())
    .with_hint("the provider package must deliver the secret by stdin, a file, or the environment")
}

/// The error body of a secret service failure: one catalog code for every
/// variant of [`SecretError`] and of [`SecretStoreError`] (AC-21). No
/// message or detail holds a value.
#[must_use]
pub fn secret_error(error: &SecretError) -> ErrorBody {
    let about = |body: ErrorBody, reference: &SecretRef| {
        body.with_resource(ResourceRef {
            kind: ResourceKind::Secret,
            id: reference.to_string(),
        })
    };
    match error {
        SecretError::NotFound { reference } => not_found(reference),
        SecretError::InvalidReference { reference, reason } => about(
            ErrorBody::new(
                &secret::INVALID_REFERENCE,
                "the first segment of this reference is reserved",
            )
            .with_detail("reference", reference.to_string())
            .with_detail("reason", *reason),
            reference,
        ),
        SecretError::Conflict { reference, reason } => conflict(reference, reason),
        SecretError::ConfirmationRequired { reference, effects } => about(
            ErrorBody::new(
                &secret::CONFIRMATION_REQUIRED,
                "replacing this secret changes only the stored copy",
            )
            .with_detail("reference", reference.to_string())
            .with_items(effects.iter().map(|effect| {
                ErrorItem::new(EFFECT_ITEM)
                    .with_field("resourceKind", user_kind_code(&effect.user))
                    .with_field("resourceId", effect.user.id().to_string())
                    .with_field("field", effect.field.to_string())
                    .with_field("effect", effect.effect.as_code())
            }))
            .with_hint("send the request again with confirmStoredOnly"),
            reference,
        ),
        SecretError::InvalidValue {
            field,
            min_length,
            max_length,
            reason,
        } => {
            let mut body = ErrorBody::new(
                &secret::INVALID_VALUE,
                format!("the value does not fit secret field {field}"),
            )
            .with_detail("field", field.to_string())
            .with_detail("minLength", min_length.to_string());
            if let Some(max) = max_length {
                body = body.with_detail("maxLength", max.to_string());
            }
            body.with_detail("reason", reason.as_code())
        }
        SecretError::RevisionMismatch {
            reference,
            expected,
            actual,
        } => about(
            ErrorBody::new(
                &resource::REVISION_MISMATCH,
                "the resource changed since it was read",
            )
            .with_detail("expected", expected.to_string())
            .with_detail("actual", actual.to_string())
            .with_hint("refresh the resource, then try again"),
            reference,
        ),
        SecretError::NeedsSecret { needs } => needs_secret_error(needs),
        SecretError::Store { reference, error } => store_error(reference, *error),
        SecretError::WriteBackFailed { .. } => ErrorBody::new(
            &secret::STORE_OUTCOME_UNKNOWN,
            "the secret store change could not be undone after the registry failed",
        )
        .with_hint("check the reference with stackly secret status, then fill it again"),
        SecretError::Registry(error) => crate::map::registry_error(error),
        SecretError::Internal { reason } => {
            tracing::error!(
                reason = reason.as_str(),
                "the secret service failed inside the daemon"
            );
            ErrorBody::new(
                &internal::UNEXPECTED,
                "the secret service failed inside the daemon",
            )
        }
        other => {
            tracing::error!(error = %other, "a secret failure this build cannot name");
            ErrorBody::new(
                &internal::UNEXPECTED,
                "a secret failure this build cannot name",
            )
        }
    }
}

fn not_found(reference: &SecretRef) -> ErrorBody {
    ErrorBody::new(&secret::NOT_FOUND, "the secret reference has no value")
        .with_detail("reference", reference.to_string())
        .with_resource(ResourceRef {
            kind: ResourceKind::Secret,
            id: reference.to_string(),
        })
}

fn conflict(reference: &SecretRef, reason: &ConflictReason) -> ErrorBody {
    let body = ErrorBody::new(
        &secret::CONFLICT,
        match reason {
            ConflictReason::AlreadyExists => "the secret already has a value",
            _ => "the secret reference is still used",
        },
    )
    .with_detail("reference", reference.to_string())
    .with_detail("reason", reason.as_code())
    .with_resource(ResourceRef {
        kind: ResourceKind::Secret,
        id: reference.to_string(),
    });
    match reason {
        ConflictReason::InUse(uses) => body
            .with_items(uses.iter().map(|used| {
                ErrorItem::new(USE_ITEM)
                    .with_field("resourceKind", user_kind_code(&used.user))
                    .with_field("resourceId", used.user.id().to_string())
                    .with_field("field", used.field.to_string())
            }))
            .with_hint("stop using the reference, then delete it"),
        _ => body.with_hint("use replace to change the value"),
    }
}

const fn user_kind_code(user: &SecretUser) -> &'static str {
    user.kind_code()
}

/// The error body of a store failure on `reference`, one code per
/// [`SecretStoreError`] variant (spec 0011, *Kode error di wire*).
#[must_use]
pub fn store_error(reference: &SecretRef, error: SecretStoreError) -> ErrorBody {
    let body = match error {
        SecretStoreError::NotFound => return not_found(reference),
        SecretStoreError::AlreadyExists => {
            return conflict(reference, &ConflictReason::AlreadyExists);
        }
        SecretStoreError::Locked => ErrorBody::new(&secret::STORE_LOCKED, "the keychain is locked")
            .with_hint("unlock the keychain, then try again"),
        SecretStoreError::AccessDenied { by_user } => ErrorBody::new(
            &secret::STORE_ACCESS_DENIED,
            "access to the keychain item was denied",
        )
        .with_detail("byUser", by_user.to_string())
        .with_hint("allow stacklyd to use the item when the keychain asks, then try again"),
        SecretStoreError::Busy => ErrorBody::new(
            &secret::STORE_BUSY,
            "the keychain is busy with another request",
        )
        .with_hint("try again in a moment"),
        SecretStoreError::OutcomeUnknown => ErrorBody::new(
            &secret::STORE_OUTCOME_UNKNOWN,
            "the keychain did not answer in time, so the change may have happened",
        )
        .with_hint("check the reference with stackly secret status before trying again"),
        SecretStoreError::NotSupported { .. } => {
            ErrorBody::new(&secret::STORE_UNSUPPORTED, "this OS has no secret store")
        }
        SecretStoreError::Backend { code } => {
            ErrorBody::new(&secret::STORE_BACKEND, "the keychain failed")
                .with_detail("code", code.to_string())
        }
        other => {
            tracing::error!(error = %other, "a secret store failure this build cannot name");
            return ErrorBody::new(
                &internal::UNEXPECTED,
                "a secret store failure this build cannot name",
            );
        }
    };
    body.with_resource(ResourceRef {
        kind: ResourceKind::Secret,
        id: reference.to_string(),
    })
}

fn unexpected(message: &str) -> Box<ErrorBody> {
    Box::new(ErrorBody::new(&internal::UNEXPECTED, message))
}

#[cfg(test)]
mod tests {
    use stackly_core::secret::{InvalidValueReason, classify_create_failure};
    use stackly_domain::{
        InstanceId, NeedsSecretReason as Reason, ResourceRevision, SecretFieldName, SecretRefError,
    };
    use stackly_protocol::codes;

    use super::*;

    fn reference(text: &str) -> SecretRef {
        SecretRef::parse(text).expect("the test reference is valid")
    }

    fn admin() -> SecretRef {
        reference("local:erp/postgres/admin")
    }

    fn revision(n: u64) -> ResourceRevision {
        ResourceRevision::new(n).expect("a revision from 1")
    }

    fn field(name: &str) -> SecretFieldName {
        name.parse().expect("the test field is valid")
    }

    fn instance_user() -> SecretUser {
        SecretUser::Instance(InstanceId::generate())
    }

    /// The body is in the catalog with the entry's category and
    /// `retryable`, every detail and item key from the entry's lists.
    fn checked(body: &ErrorBody) -> &ErrorBody {
        let spec = codes::lookup(body.code())
            .unwrap_or_else(|| panic!("{} is not in the catalog", body.code()));
        assert_eq!(body.category(), spec.category, "{}", spec.code);
        assert_eq!(body.retryable(), spec.retryable, "{}", spec.code);
        for detail in body.details() {
            assert!(
                spec.details.contains(&detail.key()),
                "{}: {detail:?}",
                spec.code
            );
        }
        assert!(body.message().chars().count() <= 200);
        match spec.items {
            None => assert!(body.items().is_empty(), "{}", spec.code),
            Some(items) => {
                for item in body.items() {
                    assert_eq!(item.kind(), items.kind);
                    for field in item.fields() {
                        assert!(items.fields.contains(&field.key()), "{}", spec.code);
                    }
                }
            }
        }
        body
    }

    #[test]
    fn every_store_failure_has_its_own_code_and_resource() {
        let cases = [
            (SecretStoreError::NotFound, "secret.not_found"),
            (SecretStoreError::AlreadyExists, "secret.conflict"),
            (SecretStoreError::Locked, "secret.store_locked"),
            (
                SecretStoreError::AccessDenied { by_user: true },
                "secret.store_access_denied",
            ),
            (SecretStoreError::Busy, "secret.store_busy"),
            (
                SecretStoreError::OutcomeUnknown,
                "secret.store_outcome_unknown",
            ),
            (
                SecretStoreError::NotSupported {
                    os: stackly_domain::Os::Linux,
                },
                "secret.store_unsupported",
            ),
            (
                SecretStoreError::Backend { code: -25293 },
                "secret.store_backend",
            ),
        ];
        for (error, code) in cases {
            let body = store_error(&admin(), error);

            assert_eq!(checked(&body).code(), code, "{error:?}");
            assert_eq!(
                body.resource().map(|resource| resource.id.as_str()),
                Some("local:erp/postgres/admin"),
                "{code}"
            );
        }
        let denied = store_error(&admin(), SecretStoreError::AccessDenied { by_user: true });
        assert_eq!(denied.detail("byUser"), Some("true"));
        let backend = store_error(&admin(), SecretStoreError::Backend { code: -25293 });
        assert_eq!(backend.detail("code"), Some("-25293"));
        let exists = store_error(&admin(), SecretStoreError::AlreadyExists);
        assert_eq!(exists.detail("reason"), Some("already_exists"));
    }

    #[test]
    fn every_service_failure_maps_to_the_code_the_service_reports() {
        let used = SecretUse::new(admin(), instance_user(), field("adminPassword"));
        let errors = vec![
            SecretError::NotFound { reference: admin() },
            SecretError::InvalidReference {
                reference: reference("local:stackly/internal/key"),
                reason: "reserved",
            },
            SecretError::Conflict {
                reference: admin(),
                reason: ConflictReason::AlreadyExists,
            },
            SecretError::Conflict {
                reference: admin(),
                reason: ConflictReason::InUse(vec![used.clone()]),
            },
            SecretError::ConfirmationRequired {
                reference: admin(),
                effects: vec![SecretEffect::new(
                    used.user,
                    field("adminPassword"),
                    EffectKind::StoredOnly,
                )],
            },
            SecretError::InvalidValue {
                field: field("adminPassword"),
                min_length: 12,
                max_length: Some(64),
                reason: InvalidValueReason::ControlCharacters,
            },
            SecretError::InvalidValue {
                field: field("adminPassword"),
                min_length: 0,
                max_length: None,
                reason: InvalidValueReason::Length,
            },
            SecretError::RevisionMismatch {
                reference: admin(),
                expected: revision(1),
                actual: revision(2),
            },
            classify_create_failure(
                SecretError::Store {
                    reference: admin(),
                    error: SecretStoreError::Locked,
                },
                &reference("local:erp/postgres/other"),
            ),
            SecretError::Store {
                reference: admin(),
                error: SecretStoreError::Locked,
            },
            SecretError::WriteBackFailed {
                registry: stackly_domain::RegistryError::Busy,
            },
            SecretError::Registry(stackly_domain::RegistryError::Busy),
            SecretError::Internal {
                reason: "a caller bug".to_owned(),
            },
        ];
        for error in errors {
            let body = secret_error(&error);

            assert_eq!(checked(&body).code(), error.code().as_str(), "{error:?}");
        }
    }

    #[test]
    fn a_conflict_in_use_lists_each_use_and_a_confirmation_each_effect() {
        let user = instance_user();
        let uses = vec![
            SecretUse::new(admin(), user, field("adminPassword")),
            SecretUse::new(admin(), user, field("replicaPassword")),
        ];
        let conflict = secret_error(&SecretError::Conflict {
            reference: admin(),
            reason: ConflictReason::InUse(uses),
        });
        let confirmation = secret_error(&SecretError::ConfirmationRequired {
            reference: admin(),
            effects: vec![SecretEffect::new(
                user,
                field("adminPassword"),
                EffectKind::StoredOnly,
            )],
        });

        assert_eq!(conflict.detail("reason"), Some("in_use"));
        assert_eq!(conflict.items().len(), 2);
        assert_eq!(conflict.items()[1].field("field"), Some("replicaPassword"));
        assert_eq!(conflict.items()[0].field("resourceKind"), Some("instance"));
        assert_eq!(confirmation.items().len(), 1);
        assert_eq!(confirmation.items()[0].field("effect"), Some("stored_only"));
    }

    #[test]
    fn invalid_value_names_the_field_bounds_and_reason_and_never_a_value() {
        let body = secret_error(&SecretError::InvalidValue {
            field: field("adminPassword"),
            min_length: 12,
            max_length: Some(64),
            reason: InvalidValueReason::Length,
        });

        assert_eq!(body.detail("field"), Some("adminPassword"));
        assert_eq!(body.detail("minLength"), Some("12"));
        assert_eq!(body.detail("maxLength"), Some("64"));
        assert_eq!(body.detail("reason"), Some("length"));
        assert!(body.items().is_empty());
    }

    #[test]
    fn a_needs_secret_error_lists_every_reference_with_its_reason_and_whether_generate_works() {
        let references = [
            DomainNeedsItem::new(admin(), Reason::StoreLocked, true),
            DomainNeedsItem::new(reference("local:erp/postgres/root"), Reason::Missing, false),
        ];

        let body = needs_secret_items(&references);

        assert_eq!(checked(&body).code(), "secret.needs_secret");
        assert_eq!(
            body.category(),
            stackly_protocol::ErrorCategory::Unavailable
        );
        assert!(!body.retryable());
        assert_eq!(body.detail("count"), Some("2"));
        assert_eq!(
            body.items()[0].field("reference"),
            Some("local:erp/postgres/admin")
        );
        assert_eq!(body.items()[0].field("reason"), Some("store_locked"));
        assert_eq!(body.items()[0].field("generateAllowed"), Some("true"));
        assert_eq!(body.items()[1].field("generateAllowed"), Some("false"));
        assert!(body.hint().is_some());
    }

    #[test]
    fn more_than_a_hundred_unreadable_references_are_capped_and_counted() {
        let references = (0..120)
            .map(|n| {
                DomainNeedsItem::new(
                    reference(&format!("local:erp/many/ref{n}")),
                    Reason::Missing,
                    true,
                )
            })
            .collect::<Vec<_>>();

        let body = needs_secret_items(&references);

        assert_eq!(body.items().len(), 100);
        assert_eq!(body.detail("count"), Some("120"));
        assert_eq!(body.detail("omitted"), Some("20"));
    }

    #[test]
    fn a_rejected_argument_names_the_rule_and_never_the_argument() {
        let field: stackly_core::provider::SettingName =
            "adminPassword".parse().expect("a setting name");
        let secrets = stackly_core::secret::ResolvedSecrets::for_tests([(
            field,
            SecretValue::new("fixture-secret-0001".to_owned()).expect("a valid value"),
        )]);
        let error = stackly_core::secret::delivery::ensure_not_in_argv(
            &["--user=app", "--password=fixture-secret-0001"],
            &secrets,
        )
        .expect_err("the second argument assigns the value");

        let body = delivery_error(&error);

        assert_eq!(checked(&body).code(), "secret.delivery_rejected");
        assert_eq!(body.detail("reason"), Some("assigns_value"));
    }

    #[test]
    fn the_blocker_of_an_instance_becomes_its_dto_in_reference_order() {
        let at = stackly_platform::Clock::now(&stackly_platform::SystemClock);
        let blocker = InstanceBlocker::needs_secret(
            vec![
                DomainNeedsItem::new(admin(), Reason::Missing, true),
                DomainNeedsItem::new(reference("local:erp/b/two"), Reason::AccessDenied, false),
            ],
            at,
        );

        let dto = needs_secret_blocker(&blocker).expect("a needs secret blocker has a DTO");

        assert_eq!(dto.references.len(), 2);
        assert_eq!(dto.references[0].reference, "local:erp/b/two");
        assert_eq!(dto.references[0].reason, NeedsSecretReason::AccessDenied);
        assert!(!dto.references[0].generate_allowed);
        assert_eq!(dto.references[1].reference, "local:erp/postgres/admin");
        assert_eq!(dto.references[1].reason, NeedsSecretReason::Missing);
        assert_eq!(dto.observed_at, Timestamp::from_jiff(at));
    }

    #[test]
    fn a_reference_that_fails_the_domain_parse_is_secret_invalid_reference() {
        let error = parse_reference("keychain:erp/postgres/admin").expect_err("a bad scheme");

        assert_eq!(checked(&error).code(), "secret.invalid_reference");
        assert_eq!(error.detail("reason"), Some("unsupported_scheme"));
        assert_eq!(
            reference_reason(&SecretRefError::TooLong),
            "too_long",
            "every parse failure has a stable reason"
        );
    }

    #[test]
    fn a_wire_value_goes_to_the_platform_and_back_without_changing() {
        let text = SecretText::new("fixture-secret-0001".to_owned());

        let value = secret_value(&text);
        let back = secret_text(&value);

        assert_eq!(value.len(), 19);
        assert_eq!(back.expose(), "fixture-secret-0001");
        assert_eq!(
            format!("{value:?}{back:?}"),
            "SecretValue(****)SecretText(****)"
        );
    }
}
