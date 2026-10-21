//! The create errors as operation errors: the code of each, and the details
//! and items its catalog entry lists (spec 0020, *Kode error*).

use stackly_domain::{
    DetailKey, ErrorCodeName, FailureItem, NeedsSecretItem, NeedsSecretReason, SecretFieldName,
};

use super::check::CreateError;
use crate::catalog::PinError;
use crate::operation::OperationError;
use crate::secret::{NeedsSecret, SecretError};

fn key(text: &str) -> DetailKey {
    text.parse()
        .expect("every detail key of spec 0020 is camelCase")
}

fn omitted() -> (DetailKey, String) {
    (key("omitted"), "0".to_owned())
}

impl CreateError {
    /// The secret error a secret variant stands for, which owns the
    /// details and items of its codes.
    fn secret_error(&self) -> Option<SecretError> {
        match self {
            Self::SecretNotFound { reference } => Some(SecretError::NotFound {
                reference: reference.clone(),
            }),
            Self::SecretNeedsSecret { reference } => Some(SecretError::NeedsSecret {
                needs: NeedsSecret::new(vec![NeedsSecretItem::new(
                    reference.clone(),
                    NeedsSecretReason::Missing,
                    false,
                )]),
            }),
            Self::SecretInvalidValue {
                field,
                min_length,
                max_length,
                reason,
            } => Some(SecretError::InvalidValue {
                field: field.parse::<SecretFieldName>().ok()?,
                min_length: *min_length,
                max_length: *max_length,
                reason: *reason,
            }),
            _ => None,
        }
    }
}

impl std::fmt::Display for CreateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "create refused: {}", self.code())
    }
}

impl std::error::Error for CreateError {}

impl OperationError for CreateError {
    fn code(&self) -> ErrorCodeName {
        ErrorCodeName::from_static(CreateError::code(self))
    }

    fn retryable(&self) -> bool {
        false
    }

    fn details(&self) -> Vec<(DetailKey, String)> {
        if let Some(error) = self.secret_error() {
            return OperationError::details(&error);
        }
        match self {
            Self::CatalogUnavailable { reason } => vec![(key("reason"), (*reason).to_owned())],
            Self::ProviderNotFound { provider, revision }
            | Self::ProviderAmbiguous { provider, revision } => {
                let mut details = vec![(key("provider"), provider.clone())];
                details.extend(revision.clone().map(|revision| (key("revision"), revision)));
                details
            }
            Self::ProviderUnavailable { provider, state } => vec![
                (key("provider"), provider.clone()),
                (key("state"), (*state).to_owned()),
            ],
            Self::CapabilityUnsupported { provider, existing } => vec![
                (key("capability"), "multi_instance".to_owned()),
                (key("provider"), provider.clone()),
                (key("existing"), existing.to_string()),
            ],
            Self::RuntimeNotFound { runtime_id } => {
                vec![(key("runtimeId"), runtime_id.to_string())]
            }
            Self::RuntimeMismatch {
                runtime_software,
                provider_software,
            } => vec![
                (key("runtimeSoftware"), runtime_software.clone()),
                (key("providerSoftware"), provider_software.clone()),
            ],
            Self::RuntimeUnavailable { runtime_id, state } => vec![
                (key("runtimeId"), runtime_id.to_string()),
                (key("state"), (*state).to_owned()),
            ],
            Self::RuntimeIncompatible {
                runtime_id,
                executable,
            } => vec![
                (key("runtimeId"), runtime_id.to_string()),
                (key("executable"), executable.clone()),
            ],
            Self::NameInvalid | Self::SettingsInvalid => vec![omitted()],
            Self::NameTaken { existing } => vec![(key("existing"), existing.to_string())],
            Self::EndpointInvalid { endpoint } => {
                vec![(key("endpoint"), endpoint.clone()), omitted()]
            }
            Self::EndpointConflict {
                endpoint,
                port,
                other,
            } => vec![
                (key("endpoint"), endpoint.clone()),
                (key("port"), port.to_string()),
                (key("otherInstance"), other.to_string()),
                omitted(),
            ],
            Self::EndpointInUse {
                endpoint,
                address,
                port,
            } => vec![
                (key("endpoint"), endpoint.clone()),
                (key("address"), address.to_string()),
                (key("port"), port.to_string()),
                omitted(),
            ],
            Self::EndpointUnavailable {
                endpoint,
                address,
                port,
                reason,
            } => vec![
                (key("endpoint"), endpoint.clone()),
                (key("address"), address.to_string()),
                (key("port"), port.to_string()),
                (key("reason"), (*reason).to_owned()),
                omitted(),
            ],
            Self::DataUnavailable { path, reason } => {
                vec![(key("path"), path.clone()), (key("reason"), reason.clone())]
            }
            Self::StorageUnsupported { fs_type, root } => vec![
                (key("fsType"), fs_type.clone()),
                (key("root"), root.clone()),
            ],
            Self::InsufficientSpace { needed, available } => vec![
                (key("needed"), needed.to_string()),
                (key("available"), available.to_string()),
            ],
            Self::SecretInvalidReference { reference } => vec![
                (key("reference"), reference.clone()),
                (key("reason"), "malformed".to_owned()),
            ],
            Self::SecretStoreUnsupported
            | Self::SecretNotFound { .. }
            | Self::SecretNeedsSecret { .. }
            | Self::SecretInvalidValue { .. } => Vec::new(),
        }
    }

    fn items(&self) -> Vec<FailureItem> {
        self.secret_error()
            .map(|error| OperationError::items(&error))
            .unwrap_or_default()
    }
}

/// A failed pin as an operation error: `catalog.pin_failed` with the
/// provider, the revision, and the reason.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("the provider package could not be pinned: {error}")]
pub struct PinFailure {
    /// The provider.
    pub provider: String,
    /// The revision.
    pub revision: String,
    /// What went wrong.
    pub error: PinError,
}

impl OperationError for PinFailure {
    fn code(&self) -> ErrorCodeName {
        ErrorCodeName::from_static("catalog.pin_failed")
    }

    fn retryable(&self) -> bool {
        false
    }

    fn details(&self) -> Vec<(DetailKey, String)> {
        vec![
            (key("provider"), self.provider.clone()),
            (key("revision"), self.revision.clone()),
            (key("reason"), self.error.reason().to_owned()),
        ]
    }
}
