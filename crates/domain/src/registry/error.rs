//! What a registry call can fail with, and how a failure names the
//! resource it is about.

use std::fmt;

use crate::id::{EnvironmentId, InstanceId, LeaseId, OperationId, RuntimeId, ShortKey};
use crate::provider::ProviderRevisionKey;
use crate::registry::batch::Generation;
use crate::registry::path::{LogicalRoot, RelativePath};
use crate::value::{InstanceName, ResourceRevision, Sha256Digest, SoftwareName};

/// One resource the registry holds, as errors and commit results name it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ResourceRef {
    /// A runtime.
    Runtime(RuntimeId),
    /// An instance.
    Instance(InstanceId),
    /// An environment.
    Environment(EnvironmentId),
    /// An operation.
    Operation(OperationId),
    /// A lease on an instance.
    Lease(LeaseId),
    /// A provider revision, boxed to keep errors small.
    ProviderRevision(Box<ProviderRevisionKey>),
    /// A downloaded artifact, by its digest.
    Artifact(Sha256Digest),
}

impl fmt::Display for ResourceRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Runtime(id) => write!(f, "runtime {id}"),
            Self::Instance(id) => write!(f, "instance {id}"),
            Self::Environment(id) => write!(f, "environment {id}"),
            Self::Operation(id) => write!(f, "operation {id}"),
            Self::Lease(id) => write!(f, "lease {id}"),
            Self::ProviderRevision(key) => write!(
                f,
                "provider revision {}@{}#{}",
                key.provider(),
                key.revision(),
                key.manifest_sha256()
            ),
            Self::Artifact(sha256) => write!(f, "artifact {sha256}"),
        }
    }
}

/// Error returned by a [`Registry`](crate::Registry) call.
///
/// Fakes and the SQLite registry return the same variant for the same
/// situation: a commit checks in the fixed order the trait documents and
/// returns the first error it finds.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum RegistryError {
    /// The resource does not exist.
    #[error("{resource} does not exist")]
    NotFound {
        /// The resource looked for.
        resource: ResourceRef,
    },
    /// A write refers to a resource that does not exist.
    #[error("the referenced {resource} does not exist")]
    MissingReference {
        /// The missing resource.
        resource: ResourceRef,
    },
    /// An insert names a resource that already exists.
    #[error("{resource} already exists")]
    DuplicateResource {
        /// The existing resource.
        resource: ResourceRef,
    },
    /// The stored revision is not the one the caller read (compare and
    /// swap, REQ-048).
    #[error("{resource} is at revision {actual}, not the expected revision {expected}")]
    RevisionConflict {
        /// The resource.
        resource: ResourceRef,
        /// The revision the caller read.
        expected: ResourceRevision,
        /// The revision stored now.
        actual: ResourceRevision,
    },
    /// The registry changed since the caller read it.
    #[error("the registry is at generation {actual}, not the expected generation {expected}")]
    GenerationConflict {
        /// The generation the caller read.
        expected: Generation,
        /// The generation now.
        actual: Generation,
    },
    /// The aggregate handed to a write carries the wrong revision: 1 for an
    /// insert, the expected revision plus 1 for an update. A caller bug.
    #[error("{resource} carries revision {actual}, expected revision {expected}")]
    InvalidRevision {
        /// The resource.
        resource: ResourceRef,
        /// The revision it should carry.
        expected: ResourceRevision,
        /// The revision it carries.
        actual: ResourceRevision,
    },
    /// A read query is out of range, for example a page limit of 0.
    #[error("invalid registry query: {reason}")]
    InvalidQuery {
        /// What is wrong.
        reason: &'static str,
    },
    /// A path to store is not valid UTF-8.
    #[error("{field} is not a UTF-8 path")]
    NotUtf8Path {
        /// The field that holds it, for example `external_root`.
        field: &'static str,
    },
    /// Another instance that is not retained has the name.
    #[error("instance name {name} is taken by instance {existing}")]
    NameTaken {
        /// The name.
        name: InstanceName,
        /// The instance that has it.
        existing: InstanceId,
    },
    /// Another instance has the same short key.
    #[error("short key {key} is taken by instance {existing}")]
    ShortKeyTaken {
        /// The short key.
        key: ShortKey,
        /// The instance that has it.
        existing: InstanceId,
    },
    /// Another instance stores the same path under the same root.
    #[error("path {root}:{path} is taken by instance {existing}")]
    PathTaken {
        /// The root.
        root: LogicalRoot,
        /// The path under it.
        path: RelativePath,
        /// The instance that stores it.
        existing: InstanceId,
    },
    /// A runtime for the same managed artifact or external folder exists.
    #[error("the runtime already exists as runtime {existing}")]
    DuplicateRuntime {
        /// The existing runtime.
        existing: RuntimeId,
    },
    /// An instance's software differs from its runtime's or provider
    /// revision's.
    #[error(
        "instance software {instance} does not match runtime software {runtime} and provider \
         revision software {provider_revision}"
    )]
    SoftwareMismatch {
        /// The instance's software.
        instance: SoftwareName,
        /// The runtime's software.
        runtime: SoftwareName,
        /// The provider revision's software.
        provider_revision: SoftwareName,
    },
    /// Another environment already owns the instance as a dedicated member.
    #[error("instance {instance} is already owned by environment {environment}")]
    DedicatedOwnerExists {
        /// The instance.
        instance: InstanceId,
        /// The environment that owns it.
        environment: EnvironmentId,
    },
    /// The environment already holds an active lease on the instance.
    #[error("environment {environment} already holds an active lease on instance {instance}")]
    ActiveLeaseExists {
        /// The instance.
        instance: InstanceId,
        /// The environment.
        environment: EnvironmentId,
    },
    /// A database constraint refused the write: a check before it was
    /// missed. The detail is for logs only.
    #[error("a registry constraint failed: {detail}")]
    Integrity {
        /// What the database reported.
        detail: String,
    },
    /// A stored value cannot be read, for example an unknown code. The
    /// whole read fails, so nothing disappears from a list without notice.
    #[error("registry value {table}.{column} cannot be read: {detail}")]
    Corrupt {
        /// The table.
        table: &'static str,
        /// The column.
        column: &'static str,
        /// What is wrong, for logs only.
        detail: String,
    },
    /// The disk holding the registry is full.
    #[error("the disk holding the registry is full")]
    NoSpace,
    /// The registry stayed locked past its busy timeout.
    #[error("the registry is busy")]
    Busy,
    /// The registry cannot serve calls any more, for example after its
    /// thread stopped. The daemon treats it as fatal.
    #[error("the registry is not available")]
    Unavailable,
    /// Any other I/O failure. The detail is for logs only.
    #[error("registry I/O failed: {detail}")]
    Io {
        /// What failed.
        detail: String,
    },
}

/// Error returned by [`Registry::commit`](crate::Registry::commit): the
/// error, and the index of the op that caused it.
///
/// `op_index` is `None` for an error of the batch as a whole:
/// [`RegistryError::GenerationConflict`], `Busy`, `NoSpace`, `Unavailable`,
/// and `Io`. Nothing of a failed batch is applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchError {
    /// The op that failed, counted from 0, or `None` for the whole batch.
    pub op_index: Option<usize>,
    /// What failed.
    pub error: RegistryError,
}

impl BatchError {
    /// An error of the batch as a whole.
    #[must_use]
    pub const fn batch(error: RegistryError) -> Self {
        Self {
            op_index: None,
            error,
        }
    }

    /// An error caused by the op at `index`.
    #[must_use]
    pub const fn at(index: usize, error: RegistryError) -> Self {
        Self {
            op_index: Some(index),
            error,
        }
    }
}

impl fmt::Display for BatchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.op_index {
            Some(index) => write!(f, "registry batch op {index} failed: {}", self.error),
            None => write!(f, "registry batch failed: {}", self.error),
        }
    }
}

impl std::error::Error for BatchError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.error)
    }
}
