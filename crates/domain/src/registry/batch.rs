//! Writes: a batch of ops applied in one transaction, the registry's
//! generation, and what a commit returns.

use std::collections::BTreeMap;
use std::fmt;

use jiff::Timestamp;

use crate::environment::Environment;
use crate::id::OperationId;
use crate::instance::Instance;
use crate::lease::Lease;
use crate::operation::{CreatedRecord, JournalStep, LockKey, Operation, OperationJournal};
use crate::process::ProcessObservation;
use crate::registry::config::ConfigRevisionRecord;
use crate::registry::error::{RegistryError, ResourceRef};
use crate::registry::record::{ArtifactRecord, ProviderRevisionRecord};
use crate::registry::secret::{
    AuditEventRecord, PlatformStoreKey, SecretFieldName, SecretReferenceRecord, SecretUser,
};
use crate::runtime::{InstallMode, Runtime};
use crate::secret::{SecretRef, SecretStatus};
use crate::value::ResourceRevision;

/// The lowest revision a restored secret reference row carries: a row that
/// was deleted had at least revision 1, and the restore raises it by 1.
const RESTORED_MIN: ResourceRevision = ResourceRevision::INITIAL.next();

/// The registry's generation: 0 when it is created, raised by exactly 1 by
/// every successful commit, whatever the commit holds. Every read returns
/// the generation of the snapshot it read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Generation(u64);

impl Generation {
    /// The generation of a new registry.
    pub const ZERO: Self = Self(0);

    /// Wraps a stored generation.
    #[must_use]
    pub const fn new(generation: u64) -> Self {
        Self(generation)
    }

    /// Returns the number.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    /// Returns the generation after this one.
    ///
    /// # Panics
    ///
    /// Panics at `u64::MAX`, which one commit per nanosecond would take
    /// more than 500 years to reach.
    #[must_use]
    pub const fn next(self) -> Self {
        Self(
            self.0
                .checked_add(1)
                .expect("a registry generation never reaches u64::MAX"),
        )
    }
}

impl fmt::Display for Generation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

/// A value read from the registry with the generation of the read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Versioned<T> {
    /// What was read.
    pub value: T,
    /// The generation it was read at, in the same read transaction.
    pub generation: Generation,
}

impl<T> Versioned<T> {
    /// Pairs a value with the generation it was read at.
    #[must_use]
    pub const fn new(value: T, generation: Generation) -> Self {
        Self { value, generation }
    }

    /// Transforms the value and keeps the generation.
    #[must_use]
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Versioned<U> {
        Versioned {
            value: f(self.value),
            generation: self.generation,
        }
    }
}

/// One write in a [`WriteBatch`].
///
/// An insert carries an aggregate at revision 1. An update carries the
/// aggregate after the domain raised its revision, and `expected`, the
/// revision the caller read: the update applies only while the stored
/// revision still equals `expected`, and the aggregate must carry
/// `expected + 1`. Owner features add variants through spec 0005.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum WriteOp {
    /// Records a provider revision. Fails with `DuplicateResource` when its
    /// key exists.
    InsertProviderRevision(ProviderRevisionRecord),
    /// Records an artifact. Fails with `DuplicateResource` when its digest
    /// exists.
    InsertArtifact(ArtifactRecord),
    /// Records a runtime and its dependencies.
    InsertRuntime(Runtime),
    /// Rewrites a runtime and its dependencies.
    UpdateRuntime {
        /// The runtime after the change.
        runtime: Runtime,
        /// The revision the caller read.
        expected: ResourceRevision,
    },
    /// Records an instance, its endpoints, and its seven paths, which the
    /// registry derives from the ID.
    InsertInstance(Instance),
    /// Rewrites an instance and its endpoints; its paths and its active
    /// config revision never change. Several updates of one instance may
    /// follow each other in a batch, each `expected` the revision the op
    /// before it wrote.
    UpdateInstance {
        /// The instance after the change.
        instance: Instance,
        /// The revision the caller read.
        expected: ResourceRevision,
    },
    /// Records an environment and its members.
    InsertEnvironment(Environment),
    /// Rewrites an environment and its members.
    UpdateEnvironment {
        /// The environment after the change.
        environment: Environment,
        /// The revision the caller read.
        expected: ResourceRevision,
    },
    /// Records a lease.
    InsertLease(Lease),
    /// Rewrites a lease. A lease has no revision; it changes in the same
    /// batch as the instance update that needs it.
    UpdateLease(Lease),
    /// Records an operation.
    InsertOperation(Operation),
    /// Rewrites an operation.
    UpdateOperation {
        /// The operation after the change.
        operation: Operation,
        /// The revision the caller read.
        expected: ResourceRevision,
    },
    /// Records the journal row of an operation (spec 0007). Fails with
    /// `DuplicateResource` when the operation has one, and
    /// `MissingReference` when the operation does not exist.
    InsertJournal(OperationJournal),
    /// Rewrites the journal row of an operation. Fails with `NotFound`
    /// when it has none.
    UpdateJournal(OperationJournal),
    /// Records or rewrites one step of an operation, by its `seq`. Fails
    /// with `MissingReference` when the operation does not exist.
    UpsertStep {
        /// The operation.
        operation: OperationId,
        /// The step.
        step: JournalStep,
    },
    /// Records or rewrites one created item of an operation, by its `seq`.
    /// Fails with `MissingReference` when the operation does not exist, or
    /// a new record names a step the operation does not have.
    UpsertCreated {
        /// The operation.
        operation: OperationId,
        /// The record.
        record: CreatedRecord,
    },
    /// Records the lock keys an operation needs. Fails with
    /// `MissingReference` when the operation does not exist, and with
    /// `NotUtf8Path` for an external root that is not UTF-8.
    InsertLocks {
        /// The operation.
        operation: OperationId,
        /// The keys.
        keys: Vec<LockKey>,
    },
    /// Deletes every lock key of an operation, at its final commit. Fails
    /// with `MissingReference` when the operation does not exist.
    DeleteLocks {
        /// The operation.
        operation: OperationId,
    },
    /// Records a secret reference row at revision 1 (spec 0011). Fails with
    /// `DuplicateResource` when its ID or reference exists. The owner is not
    /// checked: create instance records the reference before the instance.
    InsertSecretReference(SecretReferenceRecord),
    /// Rewrites the value columns of the row with `record.reference`: the
    /// store key, value origin, status, observation and value times,
    /// revision, and `updated_at`. The ID, owner, purpose, and `created_at`
    /// stay as stored. Fails with `NotFound` or `RevisionConflict`.
    UpdateSecretReference {
        /// The row after the change, at `expected + 1`.
        record: SecretReferenceRecord,
        /// The revision the caller read.
        expected: ResourceRevision,
    },
    /// Records what a probe of the store saw: the status, when, and the
    /// store key, and nothing else. The revision and `updated_at` stay, and
    /// the commit returns no revision for it. Fails with `NotFound`.
    ObserveSecretReference {
        /// The reference.
        reference: SecretRef,
        /// What the store showed.
        status: SecretStatus,
        /// When.
        status_observed_at: Timestamp,
        /// The store key the probe used.
        platform_store_key: PlatformStoreKey,
    },
    /// Deletes a reference row. Fails with `NotFound`, `RevisionConflict`,
    /// and then `ReferenceInUse` while any resource uses it.
    DeleteSecretReference {
        /// The reference.
        reference: SecretRef,
        /// The revision the caller read.
        expected: ResourceRevision,
    },
    /// Records again a row that was just deleted, after the store failed
    /// to delete its item (spec 0011 *Hapus* step 4). Checked like
    /// [`WriteOp::InsertSecretReference`], except that the revision is at
    /// least 2.
    RestoreSecretReference(SecretReferenceRecord),
    /// Replaces every use by `user` with one use per entry; an empty map
    /// deletes them all. Fails with `MissingReference` when the user or a
    /// reference does not exist (an earlier op of the same batch counts).
    ReplaceSecretUses {
        /// The resource.
        user: SecretUser,
        /// The reference each field uses.
        uses: BTreeMap<SecretFieldName, SecretRef>,
    },
    /// Records an audit row. Fails with `DuplicateResource` when its ID
    /// exists; the resource is not checked, so history outlives it.
    InsertAuditEvent(AuditEventRecord),
    /// Records one observation of an instance's process (spec 0010), with
    /// no revision. Fails with `NotUtf8Path` for an executable path that
    /// is not UTF-8, and `MissingReference` when the instance does not
    /// exist (an earlier op of the same batch counts).
    RecordProcessObservation(ProcessObservation),
    /// Records a config revision and makes it the instance's active one,
    /// without raising the instance's revision. Fails with
    /// `DuplicateResource` when its ID exists; `MissingReference` when the
    /// instance does not exist (an earlier op of the same batch counts) or
    /// `previous` is not a revision of the same instance; and
    /// `DuplicateResource` with the active revision when `previous` is
    /// empty but the instance already has one.
    InsertConfigRevision(ConfigRevisionRecord),
}

impl WriteOp {
    /// Returns the resource the op writes.
    #[must_use]
    pub fn resource(&self) -> ResourceRef {
        match self {
            Self::InsertProviderRevision(record) => {
                ResourceRef::ProviderRevision(Box::new(record.revision.key().clone()))
            }
            Self::InsertArtifact(record) => ResourceRef::Artifact(record.sha256),
            Self::InsertRuntime(runtime) | Self::UpdateRuntime { runtime, .. } => {
                ResourceRef::Runtime(runtime.id())
            }
            Self::InsertInstance(instance) | Self::UpdateInstance { instance, .. } => {
                ResourceRef::Instance(instance.id())
            }
            Self::InsertEnvironment(environment) | Self::UpdateEnvironment { environment, .. } => {
                ResourceRef::Environment(environment.id())
            }
            Self::InsertLease(lease) | Self::UpdateLease(lease) => ResourceRef::Lease(lease.id()),
            Self::InsertOperation(operation) | Self::UpdateOperation { operation, .. } => {
                ResourceRef::Operation(operation.id())
            }
            Self::InsertJournal(journal) | Self::UpdateJournal(journal) => {
                ResourceRef::Operation(journal.operation_id())
            }
            Self::UpsertStep { operation, .. }
            | Self::UpsertCreated { operation, .. }
            | Self::InsertLocks { operation, .. }
            | Self::DeleteLocks { operation } => ResourceRef::Operation(*operation),
            Self::InsertSecretReference(record)
            | Self::UpdateSecretReference { record, .. }
            | Self::RestoreSecretReference(record) => {
                ResourceRef::SecretReference(record.reference.clone())
            }
            Self::ObserveSecretReference { reference, .. }
            | Self::DeleteSecretReference { reference, .. } => {
                ResourceRef::SecretReference(reference.clone())
            }
            Self::ReplaceSecretUses { user, .. } => match user {
                SecretUser::Instance(id) => ResourceRef::Instance(*id),
                SecretUser::Environment(id) => ResourceRef::Environment(*id),
            },
            Self::InsertAuditEvent(record) => ResourceRef::AuditEvent(record.id),
            Self::RecordProcessObservation(observation) => {
                ResourceRef::Instance(observation.instance)
            }
            Self::InsertConfigRevision(record) => ResourceRef::ConfigRevision(record.id),
        }
    }

    /// Returns the operation whose journal rows the op writes, for the
    /// journal ops.
    #[must_use]
    pub const fn journal_operation(&self) -> Option<OperationId> {
        match self {
            Self::InsertJournal(journal) | Self::UpdateJournal(journal) => {
                Some(journal.operation_id())
            }
            Self::UpsertStep { operation, .. }
            | Self::UpsertCreated { operation, .. }
            | Self::InsertLocks { operation, .. }
            | Self::DeleteLocks { operation } => Some(*operation),
            _ => None,
        }
    }

    /// Returns the revision of the aggregate the op writes, for the ops on
    /// an aggregate that has one: runtime, instance, environment,
    /// operation, and secret reference. A commit returns these.
    #[must_use]
    pub fn revision(&self) -> Option<ResourceRevision> {
        match self {
            Self::InsertRuntime(runtime) | Self::UpdateRuntime { runtime, .. } => {
                Some(runtime.revision())
            }
            Self::InsertInstance(instance) | Self::UpdateInstance { instance, .. } => {
                Some(instance.revision())
            }
            Self::InsertEnvironment(environment) | Self::UpdateEnvironment { environment, .. } => {
                Some(environment.revision())
            }
            Self::InsertOperation(operation) | Self::UpdateOperation { operation, .. } => {
                Some(operation.revision())
            }
            Self::InsertSecretReference(record)
            | Self::UpdateSecretReference { record, .. }
            | Self::RestoreSecretReference(record) => Some(record.revision),
            Self::InsertProviderRevision(_)
            | Self::InsertArtifact(_)
            | Self::InsertLease(_)
            | Self::UpdateLease(_)
            | Self::InsertJournal(_)
            | Self::UpdateJournal(_)
            | Self::UpsertStep { .. }
            | Self::UpsertCreated { .. }
            | Self::InsertLocks { .. }
            | Self::DeleteLocks { .. }
            | Self::ObserveSecretReference { .. }
            | Self::DeleteSecretReference { .. }
            | Self::ReplaceSecretUses { .. }
            | Self::InsertAuditEvent(_)
            | Self::RecordProcessObservation(_)
            | Self::InsertConfigRevision(_) => None,
        }
    }

    /// Returns `expected` of an update on an aggregate with a revision.
    #[must_use]
    pub const fn expected(&self) -> Option<ResourceRevision> {
        match self {
            Self::UpdateRuntime { expected, .. }
            | Self::UpdateInstance { expected, .. }
            | Self::UpdateEnvironment { expected, .. }
            | Self::UpdateOperation { expected, .. }
            | Self::UpdateSecretReference { expected, .. } => Some(*expected),
            _ => None,
        }
    }

    /// Checks the shape of the values, the first step of every op in a
    /// commit, before anything is read: the aggregate's revision (1 for an
    /// insert, `expected + 1` for an update, at least 2 for a restored
    /// secret reference), then every path to store being UTF-8, lock keys
    /// and a process's executable included. Fakes and the SQLite registry
    /// both call it, so they fail the same way.
    ///
    /// # Errors
    ///
    /// Returns [`RegistryError::InvalidRevision`] or
    /// [`RegistryError::NotUtf8Path`].
    pub fn check_shape(&self) -> Result<(), RegistryError> {
        if let Self::RestoreSecretReference(record) = self {
            if record.revision < RESTORED_MIN {
                return Err(RegistryError::InvalidRevision {
                    resource: self.resource(),
                    expected: RESTORED_MIN,
                    actual: record.revision,
                });
            }
        } else if let Some(actual) = self.revision() {
            let expected = self
                .expected()
                .map_or(ResourceRevision::INITIAL, ResourceRevision::next);
            if actual != expected {
                return Err(RegistryError::InvalidRevision {
                    resource: self.resource(),
                    expected,
                    actual,
                });
            }
        }
        if let Self::InsertRuntime(runtime) | Self::UpdateRuntime { runtime, .. } = self
            && let InstallMode::External { root, .. } = runtime.install()
            && root.to_str().is_none()
        {
            return Err(RegistryError::NotUtf8Path {
                field: "external_root",
            });
        }
        if let Self::InsertLocks { keys, .. } = self
            && keys.iter().any(|key| key.to_text().is_none())
        {
            return Err(RegistryError::NotUtf8Path { field: "lock_key" });
        }
        if let Self::RecordProcessObservation(observation) = self
            && observation.identity.executable.to_str().is_none()
        {
            return Err(RegistryError::NotUtf8Path {
                field: "executable_identity",
            });
        }
        Ok(())
    }
}

/// Ops applied together in one transaction: all of them, or none.
///
/// With `expected_generation`, the batch applies only while the registry is
/// still at that generation, which guards checks the caller ran on what it
/// read (endpoint conflicts, for example). A caller retries the read and
/// the commit at most five times on `GenerationConflict`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WriteBatch {
    /// The generation the caller read, if the batch depends on it.
    pub expected_generation: Option<Generation>,
    /// The ops, applied in order.
    pub ops: Vec<WriteOp>,
}

impl WriteBatch {
    /// A batch of `ops` that does not depend on the generation.
    #[must_use]
    pub const fn new(ops: Vec<WriteOp>) -> Self {
        Self {
            expected_generation: None,
            ops,
        }
    }

    /// Returns the batch, applying only at `generation`.
    #[must_use]
    pub const fn at_generation(mut self, generation: Generation) -> Self {
        self.expected_generation = Some(generation);
        self
    }
}

/// What a successful commit returns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Committed {
    /// The registry's generation after the commit.
    pub generation: Generation,
    /// The new revision of each aggregate an op created or changed, in op
    /// order.
    pub revisions: Vec<(ResourceRef, ResourceRevision)>,
}
