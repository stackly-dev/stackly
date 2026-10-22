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
    /// The message of the error: fixed English text that names neither a
    /// value of the settings nor a secret (spec 0020, *Kode error*).
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CatalogUnavailable { .. } => f.write_str("the provider catalog is not available"),
            Self::ProviderNotFound { .. } => {
                f.write_str("no pinned package or provider revision with that name")
            }
            Self::ProviderAmbiguous { .. } => {
                f.write_str("more than one package digest matches the provider and revision")
            }
            Self::ProviderUnavailable { provider, .. } => {
                write!(f, "{provider} can't be used to create instances")
            }
            Self::CapabilityUnsupported { provider, .. } => {
                write!(f, "{provider} runs one instance at a time")
            }
            Self::RuntimeNotFound { .. } => f.write_str("no runtime has that id"),
            Self::RuntimeMismatch { .. } => {
                f.write_str("the runtime runs other software than the provider")
            }
            Self::RuntimeUnavailable { .. } => f.write_str("the runtime is not installed"),
            Self::RuntimeIncompatible { .. } => {
                f.write_str("the runtime lacks a program the provider needs")
            }
            Self::NameInvalid => f.write_str("the instance name is not valid"),
            Self::NameTaken { .. } => f.write_str("another instance has the name"),
            Self::EndpointInvalid { endpoint } => {
                write!(f, "the port request for endpoint {endpoint} is not valid")
            }
            Self::EndpointConflict { endpoint, port, .. } => write!(
                f,
                "port {port} of endpoint {endpoint} is used by another instance"
            ),
            Self::EndpointInUse {
                endpoint,
                address,
                port,
            } => write!(
                f,
                "port {port} of endpoint {endpoint} is in use at {address}"
            ),
            Self::EndpointUnavailable { endpoint, port, .. } => write!(
                f,
                "the operating system refused port {port} for endpoint {endpoint}"
            ),
            Self::SettingsInvalid => f.write_str("the settings break the schema of the package"),
            Self::DataUnavailable { .. } => {
                f.write_str("the instance folder is missing, not the user's, or not private")
            }
            Self::StorageUnsupported { .. } => {
                f.write_str("the data folder is on an unsupported file system")
            }
            Self::InsufficientSpace { .. } => {
                f.write_str("not enough free disk space to create an instance")
            }
            Self::SecretStoreUnsupported => f.write_str("this Mac has no supported secret store"),
            Self::SecretInvalidReference { .. } => f.write_str("the secret reference is not valid"),
            Self::SecretNotFound { .. } => f.write_str("no secret has that reference"),
            Self::SecretNeedsSecret { .. } => {
                f.write_str("a secret the instance needs has no value")
            }
            Self::SecretInvalidValue { .. } => {
                f.write_str("a secret value breaks the rules of its field")
            }
        }
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
