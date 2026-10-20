//! What the secret service fails with, the catalog code of each failure
//! (spec 0011, *Kode error di wire*), and how a create maps a failed
//! `put` ([`classify_create_failure`], AC-23).

use stackly_domain::{
    DetailKey, ErrorCodeName, NeedsSecretItem, NeedsSecretReason, RegistryError, ResourceRevision,
    SecretFieldName, SecretRef, SecretUse,
};
use stackly_platform::SecretStoreError;

use super::rules::{InvalidValueReason, SecretEffect};
use crate::operation::OperationError;

/// The catalog codes the secret service reports. `stacklyd` looks each one
/// up in the `secret` area of the `stackly-protocol` catalog.
pub mod codes {
    use stackly_domain::ErrorCodeName;

    /// A resource needs a secret that cannot be read.
    pub const NEEDS_SECRET: ErrorCodeName = ErrorCodeName::from_static("secret.needs_secret");
    /// The reference is not registered, or the store holds no value.
    pub const NOT_FOUND: ErrorCodeName = ErrorCodeName::from_static("secret.not_found");
    /// A value exists already, or the reference is still used.
    pub const CONFLICT: ErrorCodeName = ErrorCodeName::from_static("secret.conflict");
    /// A replace changes only the stored copy and needs a confirmation.
    pub const CONFIRMATION_REQUIRED: ErrorCodeName =
        ErrorCodeName::from_static("secret.confirmation_required");
    /// The reference names a reserved first segment.
    pub const INVALID_REFERENCE: ErrorCodeName =
        ErrorCodeName::from_static("secret.invalid_reference");
    /// The value is outside the bounds of its secret field.
    pub const INVALID_VALUE: ErrorCodeName = ErrorCodeName::from_static("secret.invalid_value");
    /// An argument would carry a secret value (`ensure_not_in_argv`).
    pub const DELIVERY_REJECTED: ErrorCodeName =
        ErrorCodeName::from_static("secret.delivery_rejected");
    /// The store is locked.
    pub const STORE_LOCKED: ErrorCodeName = ErrorCodeName::from_static("secret.store_locked");
    /// The store refused access.
    pub const STORE_ACCESS_DENIED: ErrorCodeName =
        ErrorCodeName::from_static("secret.store_access_denied");
    /// The store call did not start in time.
    pub const STORE_BUSY: ErrorCodeName = ErrorCodeName::from_static("secret.store_busy");
    /// A change of the store passed its wait limit, or a write back failed.
    pub const STORE_OUTCOME_UNKNOWN: ErrorCodeName =
        ErrorCodeName::from_static("secret.store_outcome_unknown");
    /// The store is not supported on this OS.
    pub const STORE_UNSUPPORTED: ErrorCodeName =
        ErrorCodeName::from_static("secret.store_unsupported");
    /// Any other store failure.
    pub const STORE_BACKEND: ErrorCodeName = ErrorCodeName::from_static("secret.store_backend");

    /// Every code above, for the catalog test of `stackly-protocol`.
    pub const ALL: &[ErrorCodeName] = &[
        NEEDS_SECRET,
        NOT_FOUND,
        CONFLICT,
        CONFIRMATION_REQUIRED,
        INVALID_REFERENCE,
        INVALID_VALUE,
        DELIVERY_REJECTED,
        STORE_LOCKED,
        STORE_ACCESS_DENIED,
        STORE_BUSY,
        STORE_OUTCOME_UNKNOWN,
        STORE_UNSUPPORTED,
        STORE_BACKEND,
    ];
}

/// The references a resource needs but cannot read, each with why and
/// whether a generated value would work, ordered by reference text (AC-11).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct NeedsSecret {
    /// Every reference that could not be read.
    pub references: Vec<NeedsSecretItem>,
}

/// Why [`SecretError::Conflict`] reports `secret.conflict`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConflictReason {
    /// The store already holds a value: `create` on an existing item, or a
    /// new resource naming a registered reference.
    AlreadyExists,
    /// The reference is still used, by each of these.
    InUse(Vec<SecretUse>),
}

impl ConflictReason {
    /// Returns the `reason` detail, `already_exists` or `in_use`.
    #[must_use]
    pub const fn as_code(&self) -> &'static str {
        match self {
            Self::AlreadyExists => "already_exists",
            Self::InUse(_) => "in_use",
        }
    }
}

/// Error returned by the secret service. No message or detail ever holds a
/// value.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum SecretError {
    /// The reference is not registered, or the store holds no value where
    /// one is needed (`replace` on a missing item, the source of a copy).
    #[error("secret reference {reference} has no value")]
    NotFound {
        /// The reference.
        reference: SecretRef,
    },
    /// The reference names the reserved first segment `stackly` and is not
    /// registered.
    #[error("secret reference {reference} is reserved: {reason}")]
    InvalidReference {
        /// The reference.
        reference: SecretRef,
        /// Why, for the `reason` detail.
        reason: &'static str,
    },
    /// A value exists already, or the reference is still used.
    #[error("secret reference {reference} conflicts: {}", reason.as_code())]
    Conflict {
        /// The reference.
        reference: SecretRef,
        /// Why.
        reason: ConflictReason,
    },
    /// A use of the reference keeps the old value inside its data
    /// (`stored_only`), so the replace needs `confirm_stored_only`.
    #[error("replacing secret {reference} changes only the stored copy and needs a confirmation")]
    ConfirmationRequired {
        /// The reference.
        reference: SecretRef,
        /// The effect of the replace on every use.
        effects: Vec<SecretEffect>,
    },
    /// The value breaks the rules of a secret field that uses the
    /// reference: it is outside the field's `minLength` and `maxLength`,
    /// or holds a control character the field refuses (AC-22).
    #[error("the value does not fit secret field {field}: {reason}")]
    InvalidValue {
        /// The field.
        field: SecretFieldName,
        /// Its fewest characters.
        min_length: u64,
        /// Its most characters.
        max_length: Option<u64>,
        /// Which rule the value breaks.
        reason: InvalidValueReason,
    },
    /// The row is not at the revision the request read.
    #[error("secret reference {reference} is at revision {actual}, not {expected}")]
    RevisionMismatch {
        /// The reference.
        reference: SecretRef,
        /// The revision the request named.
        expected: ResourceRevision,
        /// The revision stored now.
        actual: ResourceRevision,
    },
    /// A resource needs references that cannot be read.
    #[error("{} secret reference(s) cannot be read", needs.references.len())]
    NeedsSecret {
        /// Which and why.
        needs: NeedsSecret,
    },
    /// The store failed on a reference: the source of a copy or a bind,
    /// or the target a put reads or writes (AC-23).
    #[error("{error} ({reference})")]
    Store {
        /// The reference whose store call failed.
        reference: SecretRef,
        /// How it failed.
        error: SecretStoreError,
    },
    /// The registry failed after the store changed, and putting the store
    /// back failed too, so what the store holds is unknown until a probe.
    #[error("the secret store change could not be undone after the registry failed")]
    WriteBackFailed {
        /// The registry failure that came first.
        registry: RegistryError,
    },
    /// The registry failed.
    #[error("{0}")]
    Registry(RegistryError),
    /// A caller bug or an impossible state, for logs only.
    #[error("internal secret service error: {reason}")]
    Internal {
        /// What went wrong.
        reason: String,
    },
}

impl From<RegistryError> for SecretError {
    fn from(error: RegistryError) -> Self {
        match error {
            RegistryError::ReferenceInUse { reference, used_by } => Self::Conflict {
                reference,
                reason: ConflictReason::InUse(used_by),
            },
            other => Self::Registry(other),
        }
    }
}

impl SecretError {
    /// Returns the catalog code (spec 0011 *Kode error di wire*, spec 0005
    /// for the registry), the code an audit row records as `errorCode`.
    #[must_use]
    pub fn code(&self) -> ErrorCodeName {
        match self {
            Self::NotFound { .. } => codes::NOT_FOUND,
            Self::InvalidReference { .. } => codes::INVALID_REFERENCE,
            Self::Conflict { .. } => codes::CONFLICT,
            Self::ConfirmationRequired { .. } => codes::CONFIRMATION_REQUIRED,
            Self::InvalidValue { .. } => codes::INVALID_VALUE,
            Self::RevisionMismatch { .. } => crate::operation::codes::REVISION_MISMATCH,
            Self::NeedsSecret { .. } => codes::NEEDS_SECRET,
            Self::Store { error, .. } => store_code(*error),
            Self::WriteBackFailed { .. } => codes::STORE_OUTCOME_UNKNOWN,
            Self::Registry(error) => registry_code(error),
            Self::Internal { .. } => crate::operation::codes::INTERNAL_UNEXPECTED,
        }
    }
}

/// The catalog code of a store failure.
#[must_use]
pub const fn store_code(error: SecretStoreError) -> ErrorCodeName {
    match error {
        SecretStoreError::NotFound => codes::NOT_FOUND,
        SecretStoreError::AlreadyExists => codes::CONFLICT,
        SecretStoreError::Locked => codes::STORE_LOCKED,
        SecretStoreError::AccessDenied { .. } => codes::STORE_ACCESS_DENIED,
        SecretStoreError::Busy => codes::STORE_BUSY,
        SecretStoreError::OutcomeUnknown => codes::STORE_OUTCOME_UNKNOWN,
        SecretStoreError::NotSupported { .. } => codes::STORE_UNSUPPORTED,
        _ => codes::STORE_BACKEND,
    }
}

/// The catalog code of a registry failure (spec 0005 *Kode error di wire*).
#[must_use]
pub fn registry_code(error: &RegistryError) -> ErrorCodeName {
    let code = match error {
        RegistryError::NotFound { .. } => "resource.not_found",
        RegistryError::RevisionConflict { .. } => "resource.revision_mismatch",
        RegistryError::MissingReference { .. } => "registry.missing_reference",
        RegistryError::DuplicateResource { .. } => "registry.duplicate_resource",
        RegistryError::GenerationConflict { .. } => "registry.generation_conflict",
        RegistryError::NameTaken { .. } => "registry.name_taken",
        RegistryError::ShortKeyTaken { .. } => "registry.short_key_taken",
        RegistryError::PathTaken { .. } => "registry.path_taken",
        RegistryError::DuplicateRuntime { .. } => "registry.duplicate_runtime",
        RegistryError::DedicatedOwnerExists { .. } => "registry.dedicated_owner_exists",
        RegistryError::ActiveLeaseExists { .. } => "registry.active_lease_exists",
        RegistryError::SoftwareMismatch { .. } => "registry.software_mismatch",
        RegistryError::NotUtf8Path { .. } => "registry.path_not_utf8",
        RegistryError::ReferenceInUse { .. } => "secret.conflict",
        RegistryError::IdempotencyKeyTaken { .. } => "operation.idempotency_key_reused",
        RegistryError::Busy => "registry.busy",
        RegistryError::Unavailable => "registry.unavailable",
        RegistryError::NoSpace => "registry.no_space",
        RegistryError::Io { .. } => "registry.io_error",
        RegistryError::Corrupt { .. } => "registry.corrupt",
        RegistryError::Integrity { .. } => "registry.integrity",
        _ => "internal.unexpected",
    };
    code.parse()
        .expect("every registry code of spec 0005 is an area.reason code")
}

/// Builds a detail key fixed in the source.
fn key(text: &str) -> DetailKey {
    text.parse()
        .expect("the detail keys of spec 0011 are camelCase constants")
}

/// The most items an error lists; the rest is counted in `omitted`.
const ITEMS_MAX: usize = 100;

fn omitted(count: usize) -> String {
    count.saturating_sub(ITEMS_MAX).to_string()
}

impl OperationError for SecretError {
    fn code(&self) -> ErrorCodeName {
        Self::code(self)
    }

    fn retryable(&self) -> bool {
        false
    }

    fn details(&self) -> Vec<(DetailKey, String)> {
        let reference = |reference: &SecretRef| (key("reference"), reference.to_string());
        match self {
            Self::NotFound { reference: r } => vec![reference(r)],
            Self::InvalidReference {
                reference: r,
                reason,
            } => vec![reference(r), (key("reason"), (*reason).to_owned())],
            Self::Conflict {
                reference: r,
                reason,
            } => {
                let listed = match reason {
                    ConflictReason::AlreadyExists => 0,
                    ConflictReason::InUse(uses) => uses.len(),
                };
                vec![
                    reference(r),
                    (key("reason"), reason.as_code().to_owned()),
                    (key("omitted"), omitted(listed)),
                ]
            }
            Self::ConfirmationRequired {
                reference: r,
                effects,
            } => vec![reference(r), (key("omitted"), omitted(effects.len()))],
            Self::InvalidValue {
                field,
                min_length,
                max_length,
                reason,
            } => {
                let mut details = vec![
                    (key("field"), field.to_string()),
                    (key("minLength"), min_length.to_string()),
                ];
                if let Some(max) = max_length {
                    details.push((key("maxLength"), max.to_string()));
                }
                details.push((key("reason"), reason.as_code().to_owned()));
                details
            }
            Self::RevisionMismatch {
                expected, actual, ..
            } => vec![
                (key("expected"), expected.to_string()),
                (key("actual"), actual.to_string()),
            ],
            Self::NeedsSecret { needs } => vec![
                (key("count"), needs.references.len().to_string()),
                (key("omitted"), omitted(needs.references.len())),
            ],
            Self::Store {
                reference: r,
                error: SecretStoreError::NotFound,
            } => vec![reference(r)],
            Self::Store {
                error: SecretStoreError::AccessDenied { by_user },
                ..
            } => vec![(key("byUser"), by_user.to_string())],
            Self::Store {
                error: SecretStoreError::Backend { code },
                ..
            } => vec![(key("code"), code.to_string())],
            Self::Registry(RegistryError::RevisionConflict {
                expected, actual, ..
            }) => vec![
                (key("expected"), expected.to_string()),
                (key("actual"), actual.to_string()),
            ],
            Self::Store { .. }
            | Self::WriteBackFailed { .. }
            | Self::Registry(_)
            | Self::Internal { .. } => Vec::new(),
        }
    }
}

/// Maps the failure of a `put` that a create or restore runs for one secret
/// field to what the create reports (AC-23). Create minimal (spec 0010)
/// and create (spec 0020) both call it.
///
/// A store failure on a registered source reference (the reference of a
/// `bind` or a `copyFrom`, so not `target`) that the user can fix becomes
/// one `secret.needs_secret` item: `Locked` is `store_locked`,
/// `AccessDenied` is `access_denied`, `Busy` is `store_busy`, and a source
/// the store holds no value for is `missing`. The item never offers a
/// generated value, since the create asked for the source's own value.
/// Every other failure stays as it is: a failed write or read of `target`
/// is its `secret.store_*` code, `NotSupported` on either reference is
/// `secret.store_unsupported`, `OutcomeUnknown` and `Backend` keep their
/// codes, and a source that is not registered stays `secret.not_found`.
///
/// ```
/// use stackly_core::secret::{SecretError, classify_create_failure};
/// use stackly_domain::{NeedsSecretReason, SecretRef};
/// use stackly_platform::SecretStoreError;
///
/// let target = SecretRef::parse("local:instance/0199f000-0000-7000-8000-000000000001/admin-password")?;
/// let source = SecretRef::parse("local:shared/postgres/admin")?;
///
/// let from_source = classify_create_failure(
///     SecretError::Store { reference: source.clone(), error: SecretStoreError::Locked },
///     &target,
/// );
/// let SecretError::NeedsSecret { needs } = &from_source else { panic!("{from_source:?}") };
/// assert_eq!(needs.references[0].reference, source);
/// assert_eq!(needs.references[0].reason, NeedsSecretReason::StoreLocked);
///
/// let on_target = classify_create_failure(
///     SecretError::Store { reference: target.clone(), error: SecretStoreError::Locked },
///     &target,
/// );
/// assert_eq!(on_target.code().as_str(), "secret.store_locked");
/// # Ok::<(), stackly_domain::SecretRefError>(())
/// ```
#[must_use]
pub fn classify_create_failure(error: SecretError, target: &SecretRef) -> SecretError {
    let SecretError::Store {
        reference,
        error: store,
    } = &error
    else {
        return error;
    };
    if reference == target {
        return error;
    }
    let reason = match store {
        SecretStoreError::Locked => NeedsSecretReason::StoreLocked,
        SecretStoreError::AccessDenied { .. } => NeedsSecretReason::AccessDenied,
        SecretStoreError::Busy => NeedsSecretReason::StoreBusy,
        SecretStoreError::NotFound => NeedsSecretReason::Missing,
        _ => return error,
    };
    SecretError::NeedsSecret {
        needs: NeedsSecret {
            references: vec![NeedsSecretItem::new(reference.clone(), reason, false)],
        },
    }
}
