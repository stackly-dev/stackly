//! The registry port: the only way code reads or writes the registry
//! (spec 0005, "Port registry"), with the values it stores (its own
//! identity and the stored form of instance paths).
//!
//! `stackly-storage` implements [`Registry`] with SQLite and
//! `stackly-testkit` with an in memory fake; both pass the same conformance
//! suite.

mod batch;
mod error;
mod path;
mod query;
mod record;
mod retention;

use std::fmt;
use std::str::FromStr;

use jiff::Timestamp;
use uuid::Uuid;

use crate::environment::Environment;
use crate::id::{EnvironmentId, InstanceId, InvalidId, OperationId, RuntimeId};
use crate::instance::Instance;
use crate::lease::Lease;
use crate::operation::Operation;
use crate::provider::ProviderRevisionKey;
use crate::runtime::{Runtime, RuntimeReferences};
use crate::text::serde_as_text;
use crate::value::{InstanceName, Sha256Digest};

pub use batch::{Committed, Generation, Versioned, WriteBatch, WriteOp};
pub use error::{BatchError, RegistryError, ResourceRef};
pub use path::{InstancePathEntry, InstancePathKind, LogicalRoot, RelativePath};
pub use query::{InstanceFilter, LeaseFilter, OperationCursor, OperationPage, OperationQuery};
pub use record::{ArtifactRecord, ProviderRevisionRecord};
pub use retention::{PruneReport, RetentionPolicy};

/// The registry: every read returns the generation it read at, and every
/// write is a [`WriteBatch`] applied in one transaction with compare and
/// swap on each aggregate's revision (REQ-048, NFR-005).
///
/// The trait is sync and object safe; async callers reach it through
/// `spawn_blocking`. No transaction stays open between two calls: one call
/// is one transaction.
///
/// # Reads
///
/// - A read with a code it does not know fails as a whole with
///   [`RegistryError::Corrupt`], so nothing disappears from a list without
///   notice.
/// - Instances read back with readiness and health `unknown`, reason
///   `daemon_restarted`, observed when the registry was opened; neither is
///   ever stored.
///
/// # Commits
///
/// A commit checks `expected_generation` first, then each op in order, and
/// within an op: the value shape ([`WriteOp::check_shape`]), existence
/// (`NotFound` for an update, `DuplicateResource` for an insert with an
/// existing ID), the revision (`RevisionConflict`), references
/// (`MissingReference`), and last the unique rules (`DuplicateRuntime`,
/// `NameTaken`, `ShortKeyTaken`, `PathTaken`, `DedicatedOwnerExists`,
/// `ActiveLeaseExists`, `SoftwareMismatch`). The first error found is the
/// one returned, and nothing of the batch applies. A successful commit
/// raises the generation by exactly 1, whatever it holds.
pub trait Registry: Send + Sync {
    /// Returns a provider revision by its key.
    ///
    /// # Errors
    ///
    /// Returns `Corrupt`, `Busy`, `Unavailable`, or `Io`.
    fn provider_revision(
        &self,
        key: &ProviderRevisionKey,
    ) -> Result<Versioned<Option<ProviderRevisionRecord>>, RegistryError>;

    /// Returns an artifact by its digest.
    ///
    /// # Errors
    ///
    /// Returns `Corrupt`, `Busy`, `Unavailable`, or `Io`.
    fn artifact(
        &self,
        sha256: &Sha256Digest,
    ) -> Result<Versioned<Option<ArtifactRecord>>, RegistryError>;

    /// Returns a runtime.
    ///
    /// # Errors
    ///
    /// Returns `Corrupt`, `Busy`, `Unavailable`, or `Io`.
    fn runtime(&self, id: RuntimeId) -> Result<Versioned<Option<Runtime>>, RegistryError>;

    /// Returns every runtime, ordered by ID.
    ///
    /// # Errors
    ///
    /// Returns `Corrupt`, `Busy`, `Unavailable`, or `Io`.
    fn runtimes(&self) -> Result<Versioned<Vec<Runtime>>, RegistryError>;

    /// Returns everything that still refers to a runtime (BR-007): its
    /// instances in every lifecycle, the runtimes that depend on it, its
    /// unfinished operations, and the backups that need it, each ordered
    /// by ID.
    ///
    /// # Errors
    ///
    /// Returns `NotFound` for an unknown runtime, or `Corrupt`, `Busy`,
    /// `Unavailable`, or `Io`.
    fn runtime_references(
        &self,
        id: RuntimeId,
    ) -> Result<Versioned<RuntimeReferences>, RegistryError>;

    /// Returns an instance, retained or not.
    ///
    /// # Errors
    ///
    /// Returns `Corrupt`, `Busy`, `Unavailable`, or `Io`.
    fn instance(&self, id: InstanceId) -> Result<Versioned<Option<Instance>>, RegistryError>;

    /// Returns the instance that is not retained and has `name`.
    ///
    /// # Errors
    ///
    /// Returns `Corrupt`, `Busy`, `Unavailable`, or `Io`.
    fn instance_by_name(
        &self,
        name: &InstanceName,
    ) -> Result<Versioned<Option<Instance>>, RegistryError>;

    /// Returns the instances `filter` selects, ordered by name and then ID.
    ///
    /// # Errors
    ///
    /// Returns `Corrupt`, `Busy`, `Unavailable`, or `Io`.
    fn instances(&self, filter: InstanceFilter) -> Result<Versioned<Vec<Instance>>, RegistryError>;

    /// Returns the ID of every instance, retained ones included, ordered by
    /// ID.
    ///
    /// # Errors
    ///
    /// Returns `Busy`, `Unavailable`, or `Io`.
    fn known_instance_ids(&self) -> Result<Versioned<Vec<InstanceId>>, RegistryError>;

    /// Returns the seven stored paths of an instance, in
    /// [`InstancePathKind::ALL`] order.
    ///
    /// # Errors
    ///
    /// Returns `Corrupt`, `Busy`, `Unavailable`, or `Io`.
    fn instance_paths(
        &self,
        id: InstanceId,
    ) -> Result<Versioned<Option<Vec<InstancePathEntry>>>, RegistryError>;

    /// Returns an environment.
    ///
    /// # Errors
    ///
    /// Returns `Corrupt`, `Busy`, `Unavailable`, or `Io`.
    fn environment(
        &self,
        id: EnvironmentId,
    ) -> Result<Versioned<Option<Environment>>, RegistryError>;

    /// Returns every environment, ordered by ID.
    ///
    /// # Errors
    ///
    /// Returns `Corrupt`, `Busy`, `Unavailable`, or `Io`.
    fn environments(&self) -> Result<Versioned<Vec<Environment>>, RegistryError>;

    /// Returns the leases of an instance that `filter` selects, ordered by
    /// `acquired_at` and then ID.
    ///
    /// # Errors
    ///
    /// Returns `Corrupt`, `Busy`, `Unavailable`, or `Io`.
    fn leases(
        &self,
        instance: InstanceId,
        filter: LeaseFilter,
    ) -> Result<Versioned<Vec<Lease>>, RegistryError>;

    /// Returns an operation.
    ///
    /// # Errors
    ///
    /// Returns `Corrupt`, `Busy`, `Unavailable`, or `Io`.
    fn operation(&self, id: OperationId) -> Result<Versioned<Option<Operation>>, RegistryError>;

    /// Returns one page of operations, newest `(created_at, id)` first.
    ///
    /// # Errors
    ///
    /// Returns `InvalidQuery` for a limit outside 1 to 500, or `Corrupt`,
    /// `Busy`, `Unavailable`, or `Io`.
    fn operations(&self, query: OperationQuery) -> Result<Versioned<OperationPage>, RegistryError>;

    /// Applies every op of `batch` in one transaction, or none of them.
    ///
    /// # Errors
    ///
    /// Returns a [`BatchError`] with the first error found, in the order the
    /// trait documents.
    fn commit(&self, batch: WriteBatch) -> Result<Committed, BatchError>;

    /// Deletes history beyond `policy`, measured from `now`, a few
    /// thousand rows per transaction. It never raises the generation.
    ///
    /// # Errors
    ///
    /// Returns `Busy`, `Unavailable`, `NoSpace`, or `Io`.
    fn prune(&self, policy: &RetentionPolicy, now: Timestamp)
    -> Result<PruneReport, RegistryError>;
}

/// Length of a hyphenated UUID, the only text form an ID accepts. The
/// `uuid_id!` macro from `id.rs` expects it in scope.
const HYPHENATED_LEN: usize = 36;

uuid_id! {
    /// Stable identifier of one registry file, always a UUID version 7.
    ///
    /// Only the storage adapter creates it, once, when it creates the
    /// registry; it never changes afterwards, so a restored or copied
    /// registry can be told apart from a new one.
    RegistryId
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_ids_are_version_7_and_round_trip_through_text_and_serde() {
        let id = RegistryId::generate();
        let upper = id.to_string().to_uppercase();

        assert_eq!(id.as_uuid().get_version_num(), 7);
        assert_eq!(upper.parse::<RegistryId>().unwrap(), id);
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, format!("\"{id}\""));
        assert_eq!(serde_json::from_str::<RegistryId>(&json).unwrap(), id);
        assert_eq!(
            "9f1c2d3e-4b5a-4c6d-8e7f-0a1b2c3d4e5f".parse::<RegistryId>(),
            Err(InvalidId::WrongVersion(4))
        );
    }
}
