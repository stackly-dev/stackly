//! Why a duplicate form cannot be built (spec 0024 AC-1).

use stackly_domain::{
    InstanceName, ProviderName, RegistryError, RevisionLabel, RuntimeId, RuntimeState,
};

/// Why [`gather_duplicate`](super::gather_duplicate) found no form to build.
/// `stacklyd` maps each variant to the error code of the same name in the
/// method table of spec 0024.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum DuplicateError {
    /// No instance has this ID.
    #[error("the instance does not exist")]
    NotFound,
    /// The instance is retained, so it cannot be duplicated.
    #[error("the instance is retained")]
    Retained,
    /// The pinned package is missing, invalid, or has another digest.
    #[error("the provider package is unavailable: {reason}")]
    PackageUnavailable {
        /// The provider revision key of the instance, as text.
        provider: String,
        /// Why, with the text `load_package` gives the start.
        reason: String,
    },
    /// The loaded catalog holds the same revision label with another digest,
    /// so create would use another package than the instance runs.
    #[error("the catalog holds another package under revision {revision} of {provider}")]
    ProviderAmbiguous {
        /// The provider.
        provider: ProviderName,
        /// The revision label both packages carry.
        revision: RevisionLabel,
    },
    /// The package cannot run a second instance.
    #[error("{provider} runs one instance at a time, and {existing} is it")]
    CapabilityUnsupported {
        /// The provider.
        provider: ProviderName,
        /// The instance that already exists.
        existing: InstanceName,
    },
    /// The runtime of the instance is not in the registry.
    #[error("runtime {runtime_id} does not exist")]
    RuntimeNotFound {
        /// The runtime the instance records.
        runtime_id: RuntimeId,
    },
    /// The runtime of the instance is not `installed`.
    #[error("runtime {runtime_id} is {}, not installed", .state.as_code())]
    RuntimeUnavailable {
        /// The runtime the instance records.
        runtime_id: RuntimeId,
        /// Its state.
        state: RuntimeState,
    },
    /// The registry failed or holds a value it cannot read.
    #[error("the registry could not be read: {0}")]
    Registry(#[from] RegistryError),
}
