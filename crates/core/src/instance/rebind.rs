//! The rebind check (spec 0025, *Pemeriksaan rebind*, AC-6, BR-009): whether
//! the data of an instance may run on another runtime. It reads the
//! registry, the pinned package, and the data marker, and writes nothing:
//! no operation, no event, no lock. Feature 58 calls it as the precheck of
//! its rebind operation and again after the controlled stop, right before
//! `Instance::rebind`, which stays the last guard.

use stackly_domain::{
    DetailKey, ErrorCodeName, FailureItem, InstanceId, InstanceLifecycle, Registry, Rule,
    RuntimeId, RuntimeLine,
};
use stackly_platform::Platform;

use super::data_compat::check_data_compatibility;
use crate::PreparedLayout;
use crate::operation::OperationError;
use crate::supervisor::SupervisorError;
use crate::supervisor::prepare::{check_instance_folder, load_package, require_installed};

/// What the check answers when the data may move.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum RebindVerdict {
    /// The runtime is the one the instance runs on now.
    Current,
    /// The data may run on the runtime.
    Compatible {
        /// The target runtime.
        runtime: RuntimeId,
        /// Its line.
        line: RuntimeLine,
    },
}

/// Why the check refused, or could not answer.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum RebindCheckError {
    /// A refusal the start path knows by the same code and details: the
    /// instance is missing, retained, or not initialized, the runtime is not
    /// installed, the package or the folder is unavailable, or the data does
    /// not fit the runtime.
    #[error(transparent)]
    Supervisor(#[from] SupervisorError),
    /// The target runtime does not exist.
    #[error("runtime {runtime_id} does not exist")]
    RuntimeNotFound {
        /// The runtime asked for.
        runtime_id: RuntimeId,
    },
    /// The target runtime is of another software than the instance.
    #[error("runtime of {runtime_software} cannot run an instance of {provider_software}")]
    RuntimeMismatch {
        /// The software of the target runtime.
        runtime_software: String,
        /// The software of the instance.
        provider_software: String,
    },
}

fn key(text: &str) -> DetailKey {
    text.parse()
        .expect("every detail key of spec 0025 is camelCase")
}

impl OperationError for RebindCheckError {
    fn code(&self) -> ErrorCodeName {
        match self {
            Self::Supervisor(error) => error.code(),
            Self::RuntimeNotFound { .. } => {
                ErrorCodeName::from_static("instance.runtime_not_found")
            }
            Self::RuntimeMismatch { .. } => ErrorCodeName::from_static("instance.runtime_mismatch"),
        }
    }

    fn retryable(&self) -> bool {
        match self {
            Self::Supervisor(error) => error.retryable(),
            _ => false,
        }
    }

    fn details(&self) -> Vec<(DetailKey, String)> {
        match self {
            Self::Supervisor(error) => error.details(),
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
        }
    }

    fn items(&self) -> Vec<FailureItem> {
        match self {
            Self::Supervisor(error) => error.items(),
            _ => Vec::new(),
        }
    }

    fn rule(&self) -> Option<Rule> {
        match self {
            Self::Supervisor(error) => error.rule(),
            _ => None,
        }
    }
}

/// Answers whether instance `id` could move to runtime `target`, reading the
/// registry, the pinned package, and the data marker, and writing nothing.
///
/// The first refusal wins, in the order of the spec: the instance exists, is
/// not retained, and has initialized data; the target exists; it is the
/// current runtime (the answer is [`RebindVerdict::Current`] and nothing
/// else is read); it is of the same software and installed with its
/// dependencies; the pinned package is intact; the instance folder is
/// private and the user's; and the preflight of the package accepts the data
/// line on the target line (the same call a start makes). The lifecycle
/// `running`, `stopped`, `failed`, and `unknown` are not refused: the marker
/// does not change while the server runs, and the controlled stop belongs to
/// the rebind operation.
///
/// # Errors
///
/// Returns [`RebindCheckError`] with the code and details of the first
/// refusal, as listed above.
pub fn check(
    registry: &dyn Registry,
    platform: &Platform,
    layout: &PreparedLayout,
    id: InstanceId,
    target: RuntimeId,
) -> Result<RebindVerdict, RebindCheckError> {
    let instance = registry
        .instance(id)
        .map_err(SupervisorError::Registry)?
        .value
        .ok_or(SupervisorError::NotFound(id))?;
    if instance.lifecycle() == InstanceLifecycle::Retained {
        return Err(SupervisorError::InstanceRetained.into());
    }
    if instance.data_initialized_at().is_none() {
        return Err(SupervisorError::NotInitialized {
            lifecycle: instance.lifecycle(),
        }
        .into());
    }
    let runtime = registry
        .runtime(target)
        .map_err(SupervisorError::Registry)?
        .value
        .ok_or(RebindCheckError::RuntimeNotFound { runtime_id: target })?;
    if instance.runtime_id() == target {
        return Ok(RebindVerdict::Current);
    }
    if runtime.software() != instance.software() {
        return Err(RebindCheckError::RuntimeMismatch {
            runtime_software: runtime.software().to_string(),
            provider_software: instance.software().to_string(),
        });
    }
    require_installed(registry, &runtime)?;
    let package = load_package(platform, layout, &instance)?;
    let paths = layout.layout().instance_paths(id);
    check_instance_folder(platform, &paths)?;
    check_data_compatibility(
        platform.fs(),
        &package,
        &paths,
        instance.data_line(),
        runtime.line(),
    )
    .map_err(SupervisorError::from)?;
    Ok(RebindVerdict::Compatible {
        runtime: runtime.id(),
        line: runtime.line().clone(),
    })
}
