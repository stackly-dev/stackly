//! The result types of the create evaluation: the preflight rows, the field
//! issues, and the error each failing row stands for (spec 0020, *Evaluator
//! create* and *Field error*).

use std::net::IpAddr;

use stackly_domain::{InstanceId, Port, RuntimeId, SecretRef};

use crate::secret::InvalidValueReason;

/// What a preflight row is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CheckKind {
    /// The provider package.
    Provider,
    /// The runtime.
    Runtime,
    /// The instance name.
    Name,
    /// One endpoint.
    Endpoint,
    /// The settings.
    Settings,
    /// The data folder.
    Data,
    /// The free disk space.
    Disk,
    /// One secret field.
    Secret,
}

/// How a preflight row went. There is no "skipped": a row that cannot be
/// judged is not there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CheckStatus {
    /// Fine.
    Ok,
    /// Fine as far as Stackly can tell, with a note.
    Caution,
    /// Create would be refused.
    Error,
}

/// One preflight row.
#[derive(Debug, Clone, PartialEq)]
pub struct Check {
    /// `provider`, `runtime`, `name`, `endpoint.<name>`,
    /// `endpoint.request.<i>`, `settings`, `data`, `disk`, or
    /// `secret.<field>`.
    pub id: String,
    /// What the row is about.
    pub kind: CheckKind,
    /// How it went.
    pub status: CheckStatus,
    /// The short English title.
    pub title: String,
    /// The English sentence under the title.
    pub detail: String,
    /// The endpoint name or the field key the row is about.
    pub subject: Option<String>,
    /// The free port offered for a fixed port that is taken.
    pub suggested_port: Option<u16>,
    /// The error create answers with, set exactly when the status is
    /// [`CheckStatus::Error`].
    pub error: Option<CreateError>,
}

/// The fixed code of a field issue, the set of spec 0006.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum FieldCode {
    /// The value has the wrong type or shape.
    InvalidValue,
    /// The value is missing.
    Required,
    /// The value is shorter than allowed.
    TooShort,
    /// The value is longer than allowed.
    TooLong,
    /// The value has the right type but a bad format.
    InvalidFormat,
    /// The number is outside the allowed range.
    OutOfRange,
    /// The list has more entries than allowed.
    TooMany,
    /// The value is not one of the known values.
    UnknownValue,
}

/// One problem with one field of the draft.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FieldIssue {
    /// `name`, `displayName`, `runtimeId`, `provider.name`,
    /// `provider.revision`, `endpoints[<i>].name`, `endpoints[<i>].port`,
    /// `settings`, `settings.<field>`, `secrets[<i>].field`, or
    /// `secrets[<i>].input.content`.
    pub path: String,
    /// The code.
    pub code: FieldCode,
}

impl FieldIssue {
    /// A field issue.
    #[must_use]
    pub fn new(path: impl Into<String>, code: FieldCode) -> Self {
        Self {
            path: path.into(),
            code,
        }
    }
}

/// The error a failing row stands for: the first one in row order is the
/// error `instance.create` answers with. The daemon maps each variant to the
/// wire code [`code`](Self::code) names.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum CreateError {
    /// The catalog is not loaded (`catalog.unavailable`).
    CatalogUnavailable {
        /// Why: `not_configured`, `missing`, `index_invalid`, or
        /// `unsupported_schema`.
        reason: &'static str,
    },
    /// No catalog entry or pinned package matches (`instance.provider_not_found`).
    ProviderNotFound {
        /// The provider asked for.
        provider: String,
        /// The revision asked for.
        revision: Option<String>,
    },
    /// More than one pinned package matches (`instance.provider_ambiguous`).
    ProviderAmbiguous {
        /// The provider asked for.
        provider: String,
        /// The revision asked for.
        revision: Option<String>,
    },
    /// The entry is planned or invalid (`instance.provider_unavailable`).
    ProviderUnavailable {
        /// The provider asked for.
        provider: String,
        /// `planned` or `invalid`.
        state: &'static str,
    },
    /// The provider runs one instance at a time (`instance.capability_unsupported`).
    CapabilityUnsupported {
        /// The provider.
        provider: String,
        /// An instance of that provider that is not retained.
        existing: InstanceId,
    },
    /// No runtime has that ID (`instance.runtime_not_found`).
    RuntimeNotFound {
        /// The runtime asked for.
        runtime_id: RuntimeId,
    },
    /// The runtime runs other software (`instance.runtime_mismatch`).
    RuntimeMismatch {
        /// The software of the runtime.
        runtime_software: String,
        /// The software of the provider.
        provider_software: String,
    },
    /// The runtime is not `installed` (`supervisor.runtime_unavailable`).
    RuntimeUnavailable {
        /// The runtime.
        runtime_id: RuntimeId,
        /// Its state.
        state: &'static str,
    },
    /// The runtime lacks a program the package runs (`instance.runtime_incompatible`).
    RuntimeIncompatible {
        /// The runtime.
        runtime_id: RuntimeId,
        /// The program, relative to the runtime root.
        executable: String,
    },
    /// The name or display name is not valid (`instance.name_invalid`).
    NameInvalid,
    /// An instance that is not retained has the name (`instance.name_taken`).
    NameTaken {
        /// That instance.
        existing: InstanceId,
    },
    /// A port request is not valid (`instance.endpoint_invalid`).
    EndpointInvalid {
        /// The endpoint named in the request.
        endpoint: String,
    },
    /// A fixed port conflicts with an instance (`supervisor.endpoint_conflict`).
    EndpointConflict {
        /// The endpoint.
        endpoint: String,
        /// The port.
        port: Port,
        /// The other instance.
        other: InstanceId,
    },
    /// Another socket holds the exact address (`supervisor.endpoint_in_use`).
    EndpointInUse {
        /// The endpoint.
        endpoint: String,
        /// The address.
        address: IpAddr,
        /// The port.
        port: Port,
    },
    /// The OS refuses the bind (`supervisor.endpoint_unavailable`).
    EndpointUnavailable {
        /// The endpoint.
        endpoint: String,
        /// The address.
        address: IpAddr,
        /// The port.
        port: Port,
        /// `denied` or `unavailable`.
        reason: &'static str,
    },
    /// The settings break the schema or a plan (`instance.settings_invalid`).
    SettingsInvalid,
    /// `instances/` or `retained/` is not a private folder of the user
    /// (`supervisor.data_unavailable`).
    DataUnavailable {
        /// The folder, relative to the data root.
        path: String,
        /// Why.
        reason: String,
    },
    /// The data root is not a local APFS or HFS volume (`instance.storage_unsupported`).
    StorageUnsupported {
        /// The file system type.
        fs_type: String,
        /// The data root.
        root: String,
    },
    /// Less than [`CREATE_MIN_FREE_BYTES`](super::CREATE_MIN_FREE_BYTES)
    /// is free (`instance.insufficient_space`).
    InsufficientSpace {
        /// The bytes needed.
        needed: u64,
        /// The bytes free.
        available: u64,
    },
    /// There is no secret store (`secret.store_unsupported`).
    SecretStoreUnsupported,
    /// A source reference does not parse (`secret.invalid_reference`).
    SecretInvalidReference {
        /// The secret field.
        field: String,
    },
    /// A source reference is not registered (`secret.not_found`).
    SecretNotFound {
        /// The reference.
        reference: SecretRef,
    },
    /// A source reference has no stored value (`secret.needs_secret`).
    SecretNeedsSecret {
        /// The reference.
        reference: SecretRef,
    },
    /// A `value` secret breaks the rules of its field (`secret.invalid_value`).
    SecretInvalidValue {
        /// The secret field.
        field: String,
        /// Which rule.
        reason: InvalidValueReason,
    },
}

impl CreateError {
    /// Returns the wire code of the error.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::CatalogUnavailable { .. } => "catalog.unavailable",
            Self::ProviderNotFound { .. } => "instance.provider_not_found",
            Self::ProviderAmbiguous { .. } => "instance.provider_ambiguous",
            Self::ProviderUnavailable { .. } => "instance.provider_unavailable",
            Self::CapabilityUnsupported { .. } => "instance.capability_unsupported",
            Self::RuntimeNotFound { .. } => "instance.runtime_not_found",
            Self::RuntimeMismatch { .. } => "instance.runtime_mismatch",
            Self::RuntimeUnavailable { .. } => "supervisor.runtime_unavailable",
            Self::RuntimeIncompatible { .. } => "instance.runtime_incompatible",
            Self::NameInvalid => "instance.name_invalid",
            Self::NameTaken { .. } => "instance.name_taken",
            Self::EndpointInvalid { .. } => "instance.endpoint_invalid",
            Self::EndpointConflict { .. } => "supervisor.endpoint_conflict",
            Self::EndpointInUse { .. } => "supervisor.endpoint_in_use",
            Self::EndpointUnavailable { .. } => "supervisor.endpoint_unavailable",
            Self::SettingsInvalid => "instance.settings_invalid",
            Self::DataUnavailable { .. } => "supervisor.data_unavailable",
            Self::StorageUnsupported { .. } => "instance.storage_unsupported",
            Self::InsufficientSpace { .. } => "instance.insufficient_space",
            Self::SecretStoreUnsupported => "secret.store_unsupported",
            Self::SecretInvalidReference { .. } => "secret.invalid_reference",
            Self::SecretNotFound { .. } => "secret.not_found",
            Self::SecretNeedsSecret { .. } => "secret.needs_secret",
            Self::SecretInvalidValue { .. } => "secret.invalid_value",
        }
    }

    /// Whether the synchronous error carries every field issue as `fields`:
    /// the codes that register `omitted` and allow no `items` (spec 0006,
    /// D7).
    #[must_use]
    pub const fn carries_fields(&self) -> bool {
        matches!(
            self,
            Self::NameInvalid
                | Self::SettingsInvalid
                | Self::EndpointInvalid { .. }
                | Self::EndpointInUse { .. }
                | Self::EndpointConflict { .. }
                | Self::EndpointUnavailable { .. }
                | Self::SecretInvalidValue { .. }
        )
    }
}
