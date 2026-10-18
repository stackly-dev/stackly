//! [`FakeRegistry`]: the registry in memory, with the commit checks, the
//! generation, and the typed errors of the SQLite registry.

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use jiff::{SignedDuration, Timestamp};
use stackly_domain::{
    ArtifactRecord, BackupId, BatchError, Committed, CreatedRecord, CreatedSeq, Environment,
    EnvironmentId, Generation, IdempotencyKey, InstallMode, Instance, InstanceFilter, InstanceId,
    InstanceName, InstancePathEntry, JournalStep, JournalView, Lease, LeaseFilter, LeaseHolder,
    LeaseId, LeaseState, LockKey, Operation, OperationCursor, OperationId, OperationJournal,
    OperationPage, OperationQuery, OperationResource, OperationState, Ownership,
    ProviderRevisionKey, ProviderRevisionRecord, PruneReport, Registry, RegistryError, ResourceRef,
    ResourceRevision, RetentionPolicy, Runtime, RuntimeId, RuntimeReferences, Sha256Digest,
    StepSeq, Versioned, WriteBatch, WriteOp,
};
use stackly_platform::{Clock, instance_path_entries};
use uuid::Uuid;

use super::seed::{BackupSeed, RegistrySeed, SecretReferenceSeed};
use super::stored_instance;

/// When the registry stamped a row with the commit time, for the columns
/// that have no field in the domain (spec 0005, "Pemetaan aggregate").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WrittenAt {
    /// When the row was inserted. For an instance this is its own
    /// `created_at`, since that field exists in the domain.
    pub created_at: Timestamp,
    /// When the row was last written.
    pub updated_at: Timestamp,
}

/// The registry in memory, for core and daemon tests that need no
/// database.
///
/// It behaves like the SQLite registry: a commit checks in the order the
/// [`Registry`] trait documents and returns the same error variants, a batch
/// applies whole or not at all (the fake copies its state, applies the ops
/// to the copy, and swaps it in), every commit raises the generation by
/// exactly 1, and every row stamp comes from one `clock.now()` per commit.
/// Instances read back with readiness and health `unknown`, reason
/// `daemon_restarted`, observed at `loaded_at`. Both pass
/// [`registry_conformance`](crate::registry_conformance), and the fake
/// passes [`registry_journal_conformance`](crate::registry_journal_conformance)
/// for the operation journal tables of spec 0007, whose rows prune deletes
/// with their operation.
///
/// ```
/// use std::sync::Arc;
///
/// use stackly_domain::{Registry, WriteBatch};
/// use stackly_platform::Clock;
/// use stackly_testkit::{FakeClock, FakeRegistry};
///
/// let clock = FakeClock::new();
/// let registry = FakeRegistry::new(Arc::new(clock.clone()), clock.now());
/// let committed = registry.commit(WriteBatch::new(Vec::new()))?;
/// assert_eq!(committed.generation.get(), 1);
/// # Ok::<(), stackly_domain::BatchError>(())
/// ```
pub struct FakeRegistry {
    clock: Arc<dyn Clock>,
    loaded_at: Timestamp,
    state: Mutex<State>,
    next_fault: Mutex<Option<RegistryError>>,
}

impl fmt::Debug for FakeRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FakeRegistry")
            .field("loaded_at", &self.loaded_at)
            .field("generation", &self.state().generation)
            .finish_non_exhaustive()
    }
}

impl FakeRegistry {
    /// Creates an empty registry at generation 0 that stamps rows with
    /// `clock` and reads instances back as observed at `loaded_at`, the
    /// time the SQLite registry would have been opened.
    #[must_use]
    pub fn new(clock: Arc<dyn Clock>, loaded_at: Timestamp) -> Self {
        Self {
            clock,
            loaded_at,
            state: Mutex::new(State::new()),
            next_fault: Mutex::new(None),
        }
    }

    /// Makes the next commit fail with `error` for the batch as a whole
    /// (`op_index` `None`), applying nothing, as a full disk or a busy
    /// registry would.
    pub fn fail_next_commit(&self, error: RegistryError) {
        *lock(&self.next_fault) = Some(error);
    }

    /// Returns when a provider revision, artifact, runtime, instance, or
    /// environment row was inserted and last written.
    #[must_use]
    pub fn written_at(&self, resource: &ResourceRef) -> Option<WrittenAt> {
        self.state().written.get(resource).copied()
    }

    fn state(&self) -> MutexGuard<'_, State> {
        lock(&self.state)
    }

    /// Runs one read on the current state and pairs the value with the
    /// generation it was read at.
    fn read<T>(
        &self,
        read: impl FnOnce(&State) -> Result<T, RegistryError>,
    ) -> Result<Versioned<T>, RegistryError> {
        let state = self.state();
        let value = read(&state)?;
        Ok(Versioned::new(value, state.generation))
    }

    /// Rebuilds a stored instance the way the SQLite registry reads one.
    fn restored(&self, instance: &Instance) -> Result<Instance, RegistryError> {
        Instance::restore(stored_instance(instance), self.loaded_at).map_err(|err| {
            RegistryError::Corrupt {
                table: "instances",
                column: "lifecycle",
                detail: err.to_string(),
            }
        })
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Registry for FakeRegistry {
    fn provider_revision(
        &self,
        key: &ProviderRevisionKey,
    ) -> Result<Versioned<Option<ProviderRevisionRecord>>, RegistryError> {
        self.read(|state| Ok(state.provider_revisions.get(key).cloned()))
    }

    fn artifact(
        &self,
        sha256: &Sha256Digest,
    ) -> Result<Versioned<Option<ArtifactRecord>>, RegistryError> {
        self.read(|state| Ok(state.artifacts.get(sha256).cloned()))
    }

    fn runtime(&self, id: RuntimeId) -> Result<Versioned<Option<Runtime>>, RegistryError> {
        self.read(|state| Ok(state.runtimes.get(&id).cloned()))
    }

    fn runtimes(&self) -> Result<Versioned<Vec<Runtime>>, RegistryError> {
        self.read(|state| Ok(state.runtimes.values().cloned().collect()))
    }

    fn runtime_references(
        &self,
        id: RuntimeId,
    ) -> Result<Versioned<RuntimeReferences>, RegistryError> {
        self.read(|state| {
            if !state.runtimes.contains_key(&id) {
                return Err(RegistryError::NotFound {
                    resource: ResourceRef::Runtime(id),
                });
            }
            Ok(RuntimeReferences {
                instances: state
                    .instances
                    .values()
                    .filter(|instance| instance.runtime_id() == id)
                    .map(Instance::id)
                    .collect(),
                dependent_runtimes: state
                    .runtimes
                    .values()
                    .filter(|runtime| runtime.depends_on().contains(&id))
                    .map(Runtime::id)
                    .collect(),
                active_operations: state
                    .operations
                    .values()
                    .filter(|operation| {
                        operation.resource() == OperationResource::Runtime(id)
                            && !operation.state().is_final()
                    })
                    .map(Operation::id)
                    .collect(),
                recovery_snapshots: state
                    .backups
                    .values()
                    .filter(|backup| backup.runtime_id == Some(id))
                    .map(|backup| backup.id)
                    .collect(),
            })
        })
    }

    fn instance(&self, id: InstanceId) -> Result<Versioned<Option<Instance>>, RegistryError> {
        self.read(|state| {
            state
                .instances
                .get(&id)
                .map(|instance| self.restored(instance))
                .transpose()
        })
    }

    fn instance_by_name(
        &self,
        name: &InstanceName,
    ) -> Result<Versioned<Option<Instance>>, RegistryError> {
        self.read(|state| {
            state
                .instances
                .values()
                .find(|instance| instance.retained_at().is_none() && instance.name() == name)
                .map(|instance| self.restored(instance))
                .transpose()
        })
    }

    fn instances(&self, filter: InstanceFilter) -> Result<Versioned<Vec<Instance>>, RegistryError> {
        self.read(|state| {
            let mut instances = state
                .instances
                .values()
                .filter(|instance| filter.include_retained || instance.retained_at().is_none())
                .map(|instance| self.restored(instance))
                .collect::<Result<Vec<_>, _>>()?;
            instances.sort_by(|a, b| (a.name(), a.id()).cmp(&(b.name(), b.id())));
            Ok(instances)
        })
    }

    fn known_instance_ids(&self) -> Result<Versioned<Vec<InstanceId>>, RegistryError> {
        self.read(|state| Ok(state.instances.keys().copied().collect()))
    }

    fn instance_paths(
        &self,
        id: InstanceId,
    ) -> Result<Versioned<Option<Vec<InstancePathEntry>>>, RegistryError> {
        self.read(|state| Ok(state.paths.get(&id).map(|entries| entries.to_vec())))
    }

    fn environment(
        &self,
        id: EnvironmentId,
    ) -> Result<Versioned<Option<Environment>>, RegistryError> {
        self.read(|state| Ok(state.environments.get(&id).cloned()))
    }

    fn environments(&self) -> Result<Versioned<Vec<Environment>>, RegistryError> {
        self.read(|state| Ok(state.environments.values().cloned().collect()))
    }

    fn leases(
        &self,
        instance: InstanceId,
        filter: LeaseFilter,
    ) -> Result<Versioned<Vec<Lease>>, RegistryError> {
        self.read(|state| {
            let mut leases: Vec<Lease> = state
                .leases
                .values()
                .filter(|lease| lease.instance_id() == instance)
                .filter(|lease| match filter {
                    LeaseFilter::All => true,
                    LeaseFilter::Unreleased => {
                        !matches!(lease.state(), LeaseState::Released { .. })
                    }
                })
                .cloned()
                .collect();
            leases.sort_by_key(|lease| (lease.acquired_at(), lease.id()));
            Ok(leases)
        })
    }

    fn operation(&self, id: OperationId) -> Result<Versioned<Option<Operation>>, RegistryError> {
        self.read(|state| Ok(state.operations.get(&id).cloned()))
    }

    fn operation_journal(
        &self,
        id: OperationId,
    ) -> Result<Versioned<Option<JournalView>>, RegistryError> {
        self.read(|state| Ok(state.journal_view(id)))
    }

    fn operation_journals(
        &self,
        ids: &[OperationId],
    ) -> Result<Versioned<Vec<OperationJournal>>, RegistryError> {
        self.read(|state| {
            Ok(ids
                .iter()
                .filter_map(|id| state.journals.get(id).cloned())
                .collect())
        })
    }

    fn operation_by_idempotency_key(
        &self,
        key: &IdempotencyKey,
    ) -> Result<Versioned<Option<(Operation, OperationJournal)>>, RegistryError> {
        self.read(|state| {
            Ok(state
                .operations
                .values()
                .find(|operation| operation.idempotency_key() == Some(key))
                .and_then(|operation| {
                    let journal = state.journals.get(&operation.id())?;
                    Some((operation.clone(), journal.clone()))
                }))
        })
    }

    fn unfinished_operations(
        &self,
    ) -> Result<Versioned<Vec<(Operation, JournalView)>>, RegistryError> {
        self.read(|state| {
            let mut unfinished: Vec<(Operation, JournalView)> = state
                .operations
                .values()
                .filter(|operation| {
                    matches!(
                        operation.state(),
                        OperationState::Queued
                            | OperationState::Running
                            | OperationState::CancelRequested
                            | OperationState::NeedsRecovery
                    )
                })
                .filter_map(|operation| {
                    Some((operation.clone(), state.journal_view(operation.id())?))
                })
                .collect();
            unfinished.sort_by_key(|(operation, _)| (operation.created_at(), operation.id()));
            Ok(unfinished)
        })
    }

    fn operations(&self, query: OperationQuery) -> Result<Versioned<OperationPage>, RegistryError> {
        query.validate()?;
        self.read(|state| {
            let mut matching: Vec<&Operation> = state
                .operations
                .values()
                .filter(|operation| !query.active_only || !operation.state().is_final())
                .filter(|operation| query.resource.is_none_or(|r| operation.resource() == r))
                .filter(|operation| {
                    query
                        .before
                        .is_none_or(|before| OperationCursor::of(operation) < before)
                })
                .collect();
            matching.sort_by_key(|operation| std::cmp::Reverse(OperationCursor::of(operation)));
            let limit = usize::from(query.limit);
            let next = if matching.len() > limit {
                matching
                    .get(limit - 1)
                    .map(|operation| OperationCursor::of(operation))
            } else {
                None
            };
            let items = matching.into_iter().take(limit).cloned().collect();
            Ok(OperationPage { items, next })
        })
    }

    fn commit(&self, batch: WriteBatch) -> Result<Committed, BatchError> {
        let mut state = self.state();
        if let Some(error) = lock(&self.next_fault).take() {
            return Err(BatchError::batch(error));
        }
        if let Some(expected) = batch.expected_generation
            && expected != state.generation
        {
            return Err(BatchError::batch(RegistryError::GenerationConflict {
                expected,
                actual: state.generation,
            }));
        }
        let now = self.clock.now();
        let mut next = state.clone();
        let mut revisions = Vec::new();
        for (index, op) in batch.ops.into_iter().enumerate() {
            let resource = op.resource();
            let revision = op.revision();
            next.apply(op, now)
                .map_err(|error| BatchError::at(index, error))?;
            if let Some(revision) = revision {
                revisions.push((resource, revision));
            }
        }
        next.generation = next.generation.next();
        let generation = next.generation;
        *state = next;
        Ok(Committed {
            generation,
            revisions,
        })
    }

    fn prune(
        &self,
        policy: &RetentionPolicy,
        now: Timestamp,
    ) -> Result<PruneReport, RegistryError> {
        let mut state = self.state();
        let mut report = PruneReport::default();
        report.operations = state.prune_operations(policy, now);
        report.audit_events = state.prune_audit_events(policy, now);
        report.released_leases = state.prune_released_leases(policy, now);
        report.process_observations = state.prune_observations(policy);
        Ok(report)
    }
}

impl RegistrySeed for FakeRegistry {
    fn seed_backup(&self, backup: BackupSeed) {
        let mut state = self.state();
        assert!(
            backup
                .runtime_id
                .is_none_or(|id| state.runtimes.contains_key(&id)),
            "a seeded backup refers to a runtime the registry holds"
        );
        assert!(
            backup
                .instance_id
                .is_none_or(|id| state.instances.contains_key(&id)),
            "a seeded backup refers to an instance the registry holds"
        );
        assert!(
            !state.backups.contains_key(&backup.id),
            "a seeded backup has a new ID"
        );
        state.backups.insert(backup.id, backup);
    }

    fn seed_audit_events(&self, count: u32, at: Timestamp) {
        let mut state = self.state();
        for _ in 0..count {
            state.audit_events.push(AuditRow {
                id: Uuid::now_v7(),
                occurred_at: at,
            });
        }
    }

    fn seed_observations(&self, instance: InstanceId, count: u32, at: Timestamp) {
        let mut state = self.state();
        assert!(
            state.instances.contains_key(&instance),
            "seeded observations belong to an instance the registry holds"
        );
        for _ in 0..count {
            state.next_observation += 1;
            let id = state.next_observation;
            state.observations.push(ObservationRow {
                id,
                instance,
                observed_at: at,
            });
        }
    }

    fn seed_secret_reference(&self, reference: SecretReferenceSeed) {
        let mut state = self.state();
        assert!(
            !state.secret_references.contains_key(&reference.id)
                && state
                    .secret_references
                    .values()
                    .all(|other| other.store_key != reference.store_key),
            "a seeded secret reference has a new ID and a new store key"
        );
        state.secret_references.insert(reference.id, reference);
    }
}

/// One seeded audit event.
#[derive(Debug, Clone, Copy)]
struct AuditRow {
    id: Uuid,
    occurred_at: Timestamp,
}

/// One seeded process observation; `id` grows like SQLite's rowid.
#[derive(Debug, Clone, Copy)]
struct ObservationRow {
    id: u64,
    instance: InstanceId,
    observed_at: Timestamp,
}

/// Everything the fake holds. A commit applies its ops to a copy.
#[derive(Debug, Clone)]
struct State {
    generation: Generation,
    provider_revisions: BTreeMap<ProviderRevisionKey, ProviderRevisionRecord>,
    artifacts: BTreeMap<Sha256Digest, ArtifactRecord>,
    runtimes: BTreeMap<RuntimeId, Runtime>,
    instances: BTreeMap<InstanceId, Instance>,
    paths: BTreeMap<InstanceId, [InstancePathEntry; 7]>,
    environments: BTreeMap<EnvironmentId, Environment>,
    leases: BTreeMap<LeaseId, Lease>,
    operations: BTreeMap<OperationId, Operation>,
    journals: BTreeMap<OperationId, OperationJournal>,
    steps: BTreeMap<(OperationId, StepSeq), JournalStep>,
    created: BTreeMap<(OperationId, CreatedSeq), CreatedRecord>,
    locks: BTreeMap<(OperationId, String), LockKey>,
    backups: BTreeMap<BackupId, BackupSeed>,
    audit_events: Vec<AuditRow>,
    observations: Vec<ObservationRow>,
    next_observation: u64,
    secret_references: BTreeMap<Uuid, SecretReferenceSeed>,
    written: HashMap<ResourceRef, WrittenAt>,
}

impl State {
    fn new() -> Self {
        Self {
            generation: Generation::ZERO,
            provider_revisions: BTreeMap::new(),
            artifacts: BTreeMap::new(),
            runtimes: BTreeMap::new(),
            instances: BTreeMap::new(),
            paths: BTreeMap::new(),
            environments: BTreeMap::new(),
            leases: BTreeMap::new(),
            operations: BTreeMap::new(),
            journals: BTreeMap::new(),
            steps: BTreeMap::new(),
            created: BTreeMap::new(),
            locks: BTreeMap::new(),
            backups: BTreeMap::new(),
            audit_events: Vec::new(),
            observations: Vec::new(),
            next_observation: 0,
            secret_references: BTreeMap::new(),
            written: HashMap::new(),
        }
    }

    /// Applies one op after its checks, in the order the trait documents:
    /// shape, existence, revision, references, then the unique rules.
    fn apply(&mut self, op: WriteOp, now: Timestamp) -> Result<(), RegistryError> {
        op.check_shape()?;
        match op {
            WriteOp::InsertProviderRevision(record) => self.insert_provider_revision(record, now),
            WriteOp::InsertArtifact(record) => self.insert_artifact(record, now),
            WriteOp::InsertRuntime(runtime) => self.write_runtime(runtime, None, now),
            WriteOp::UpdateRuntime { runtime, expected } => {
                self.write_runtime(runtime, Some(expected), now)
            }
            WriteOp::InsertInstance(instance) => self.write_instance(instance, None, now),
            WriteOp::UpdateInstance { instance, expected } => {
                self.write_instance(instance, Some(expected), now)
            }
            WriteOp::InsertEnvironment(environment) => {
                self.write_environment(environment, None, now)
            }
            WriteOp::UpdateEnvironment {
                environment,
                expected,
            } => self.write_environment(environment, Some(expected), now),
            WriteOp::InsertLease(lease) => self.write_lease(lease, false),
            WriteOp::UpdateLease(lease) => self.write_lease(lease, true),
            WriteOp::InsertOperation(operation) => self.write_operation(operation, None),
            WriteOp::UpdateOperation {
                operation,
                expected,
            } => self.write_operation(operation, Some(expected)),
            WriteOp::InsertJournal(journal) => self.write_journal(journal, false),
            WriteOp::UpdateJournal(journal) => self.write_journal(journal, true),
            WriteOp::UpsertStep { operation, step } => self.upsert_step(operation, step),
            WriteOp::UpsertCreated { operation, record } => self.upsert_created(operation, record),
            WriteOp::InsertLocks { operation, keys } => self.insert_locks(operation, keys),
            WriteOp::DeleteLocks { operation } => {
                self.expect_operation(operation)?;
                self.locks.retain(|(owner, _), _| *owner != operation);
                Ok(())
            }
            other => panic!(
                "FakeRegistry has no case for the op on {}; add one with the spec 0005 change \
                 that added it",
                other.resource()
            ),
        }
    }

    fn insert_provider_revision(
        &mut self,
        record: ProviderRevisionRecord,
        now: Timestamp,
    ) -> Result<(), RegistryError> {
        let key = record.revision.key().clone();
        let resource = ResourceRef::ProviderRevision(Box::new(key.clone()));
        if self.provider_revisions.contains_key(&key) {
            return Err(RegistryError::DuplicateResource { resource });
        }
        self.stamp(resource, None, now);
        self.provider_revisions.insert(key, record);
        Ok(())
    }

    fn insert_artifact(
        &mut self,
        record: ArtifactRecord,
        now: Timestamp,
    ) -> Result<(), RegistryError> {
        let resource = ResourceRef::Artifact(record.sha256);
        if self.artifacts.contains_key(&record.sha256) {
            return Err(RegistryError::DuplicateResource { resource });
        }
        self.stamp(resource, None, now);
        self.artifacts.insert(record.sha256, record);
        Ok(())
    }

    fn write_runtime(
        &mut self,
        runtime: Runtime,
        expected: Option<ResourceRevision>,
        now: Timestamp,
    ) -> Result<(), RegistryError> {
        let id = runtime.id();
        let resource = ResourceRef::Runtime(id);
        check_existing(
            self.runtimes.get(&id).map(Runtime::revision),
            expected,
            &resource,
        )?;
        if let InstallMode::Managed { artifact_sha256 } = runtime.install()
            && !self.artifacts.contains_key(artifact_sha256)
        {
            return Err(RegistryError::MissingReference {
                resource: ResourceRef::Artifact(*artifact_sha256),
            });
        }
        if let Some(missing) = runtime
            .depends_on()
            .iter()
            .find(|dependency| !self.runtimes.contains_key(dependency))
        {
            return Err(RegistryError::MissingReference {
                resource: ResourceRef::Runtime(*missing),
            });
        }
        if let Some(existing) = self
            .runtimes
            .values()
            .find(|other| other.id() != id && same_install(other.install(), runtime.install()))
        {
            return Err(RegistryError::DuplicateRuntime {
                existing: existing.id(),
            });
        }
        if runtime.depends_on().contains(&id) {
            return Err(RegistryError::Integrity {
                detail: "runtime_dependencies: a runtime cannot depend on itself".to_owned(),
            });
        }
        self.stamp(resource, None, now);
        self.runtimes.insert(id, runtime);
        Ok(())
    }

    fn write_instance(
        &mut self,
        instance: Instance,
        expected: Option<ResourceRevision>,
        now: Timestamp,
    ) -> Result<(), RegistryError> {
        let id = instance.id();
        let resource = ResourceRef::Instance(id);
        check_existing(
            self.instances.get(&id).map(Instance::revision),
            expected,
            &resource,
        )?;
        let runtime_software = self
            .runtimes
            .get(&instance.runtime_id())
            .ok_or(RegistryError::MissingReference {
                resource: ResourceRef::Runtime(instance.runtime_id()),
            })?
            .software()
            .clone();
        let provider_software = self
            .provider_revisions
            .get(instance.provider_revision())
            .ok_or_else(|| RegistryError::MissingReference {
                resource: ResourceRef::ProviderRevision(Box::new(
                    instance.provider_revision().clone(),
                )),
            })?
            .revision
            .software()
            .clone();
        if instance.retained_at().is_none()
            && let Some(existing) = self.instances.values().find(|other| {
                other.id() != id && other.retained_at().is_none() && other.name() == instance.name()
            })
        {
            return Err(RegistryError::NameTaken {
                name: instance.name().clone(),
                existing: existing.id(),
            });
        }
        let entries = instance_path_entries(id);
        if expected.is_none() {
            let key = id.short_key();
            if let Some(existing) = self.instances.keys().find(|other| other.short_key() == key) {
                return Err(RegistryError::ShortKeyTaken {
                    key,
                    existing: *existing,
                });
            }
            for entry in &entries {
                if let Some(existing) = self.path_owner(entry) {
                    return Err(RegistryError::PathTaken {
                        root: entry.root(),
                        path: entry.path().clone(),
                        existing,
                    });
                }
            }
        }
        if instance.software() != &runtime_software || instance.software() != &provider_software {
            return Err(RegistryError::SoftwareMismatch {
                instance: instance.software().clone(),
                runtime: runtime_software,
                provider_revision: provider_software,
            });
        }
        self.stamp(resource, Some(instance.created_at()), now);
        if expected.is_none() {
            self.paths.insert(id, entries);
        }
        self.instances.insert(id, instance);
        Ok(())
    }

    /// Returns the instance that stores `entry`'s root and path.
    fn path_owner(&self, entry: &InstancePathEntry) -> Option<InstanceId> {
        self.paths.iter().find_map(|(owner, stored)| {
            stored
                .iter()
                .any(|other| other.root() == entry.root() && other.path() == entry.path())
                .then_some(*owner)
        })
    }

    fn write_environment(
        &mut self,
        environment: Environment,
        expected: Option<ResourceRevision>,
        now: Timestamp,
    ) -> Result<(), RegistryError> {
        let id = environment.id();
        let resource = ResourceRef::Environment(id);
        check_existing(
            self.environments.get(&id).map(Environment::revision),
            expected,
            &resource,
        )?;
        if let Some(missing) = environment
            .members()
            .iter()
            .find(|member| !self.instances.contains_key(&member.instance_id()))
        {
            return Err(RegistryError::MissingReference {
                resource: ResourceRef::Instance(missing.instance_id()),
            });
        }
        for member in environment.members() {
            if member.ownership() != Ownership::Dedicated {
                continue;
            }
            if let Some(owner) = self.environments.values().find(|other| {
                other.id() != id
                    && other.members().iter().any(|theirs| {
                        theirs.instance_id() == member.instance_id()
                            && theirs.ownership() == Ownership::Dedicated
                    })
            }) {
                return Err(RegistryError::DedicatedOwnerExists {
                    instance: member.instance_id(),
                    environment: owner.id(),
                });
            }
        }
        self.stamp(resource, None, now);
        self.environments.insert(id, environment);
        Ok(())
    }

    fn write_lease(&mut self, lease: Lease, update: bool) -> Result<(), RegistryError> {
        let id = lease.id();
        let resource = ResourceRef::Lease(id);
        match (self.leases.contains_key(&id), update) {
            (true, false) => return Err(RegistryError::DuplicateResource { resource }),
            (false, true) => return Err(RegistryError::NotFound { resource }),
            _ => {}
        }
        if !self.instances.contains_key(&lease.instance_id()) {
            return Err(RegistryError::MissingReference {
                resource: ResourceRef::Instance(lease.instance_id()),
            });
        }
        if let LeaseHolder::Environment(environment) = lease.holder() {
            if !self.environments.contains_key(&environment) {
                return Err(RegistryError::MissingReference {
                    resource: ResourceRef::Environment(environment),
                });
            }
            if lease.is_active()
                && self.leases.values().any(|other| {
                    other.id() != id
                        && other.instance_id() == lease.instance_id()
                        && other.holder() == lease.holder()
                        && other.is_active()
                })
            {
                return Err(RegistryError::ActiveLeaseExists {
                    instance: lease.instance_id(),
                    environment,
                });
            }
        }
        self.leases.insert(id, lease);
        Ok(())
    }

    fn write_operation(
        &mut self,
        operation: Operation,
        expected: Option<ResourceRevision>,
    ) -> Result<(), RegistryError> {
        let id = operation.id();
        check_existing(
            self.operations.get(&id).map(Operation::revision),
            expected,
            &ResourceRef::Operation(id),
        )?;
        if let Some(key) = operation.idempotency_key()
            && let Some(existing) = self
                .operations
                .values()
                .find(|other| other.id() != id && other.idempotency_key() == Some(key))
        {
            return Err(RegistryError::IdempotencyKeyTaken {
                key: key.clone(),
                existing: existing.id(),
            });
        }
        self.operations.insert(id, operation);
        Ok(())
    }

    /// Fails with `MissingReference` unless the operation exists.
    fn expect_operation(&self, id: OperationId) -> Result<(), RegistryError> {
        if self.operations.contains_key(&id) {
            Ok(())
        } else {
            Err(RegistryError::MissingReference {
                resource: ResourceRef::Operation(id),
            })
        }
    }

    fn write_journal(
        &mut self,
        journal: OperationJournal,
        update: bool,
    ) -> Result<(), RegistryError> {
        let id = journal.operation_id();
        let resource = ResourceRef::Operation(id);
        match (self.journals.contains_key(&id), update) {
            (true, false) => return Err(RegistryError::DuplicateResource { resource }),
            (false, true) => return Err(RegistryError::NotFound { resource }),
            _ => {}
        }
        self.expect_operation(id)?;
        self.journals.insert(id, journal);
        Ok(())
    }

    fn upsert_step(
        &mut self,
        operation: OperationId,
        step: JournalStep,
    ) -> Result<(), RegistryError> {
        self.expect_operation(operation)?;
        if self.steps.iter().any(|((owner, seq), other)| {
            *owner == operation && *seq != step.seq() && other.name() == step.name()
        }) {
            return Err(unique_failed(
                "operation_steps.operation_id, operation_steps.name",
            ));
        }
        self.steps.insert((operation, step.seq()), step);
        Ok(())
    }

    fn upsert_created(
        &mut self,
        operation: OperationId,
        record: CreatedRecord,
    ) -> Result<(), RegistryError> {
        self.expect_operation(operation)?;
        if !self.steps.contains_key(&(operation, record.step_seq())) {
            return Err(RegistryError::MissingReference {
                resource: ResourceRef::OperationStep(operation, record.step_seq()),
            });
        }
        if self.created.iter().any(|((owner, seq), other)| {
            *owner == operation && *seq != record.seq() && other.item() == record.item()
        }) {
            return Err(unique_failed(
                "operation_created.operation_id, operation_created.item_kind, \
                 operation_created.item_ref",
            ));
        }
        self.created.insert((operation, record.seq()), record);
        Ok(())
    }

    fn insert_locks(
        &mut self,
        operation: OperationId,
        keys: Vec<LockKey>,
    ) -> Result<(), RegistryError> {
        self.expect_operation(operation)?;
        for key in keys {
            // check_shape already refused a key without a text form.
            let text = key.to_string();
            if self.locks.insert((operation, text), key).is_some() {
                return Err(unique_failed(
                    "operation_locks.operation_id, operation_locks.lock_key",
                ));
            }
        }
        Ok(())
    }

    /// The journal of an operation with its rows, or `None` without one.
    fn journal_view(&self, id: OperationId) -> Option<JournalView> {
        let journal = self.journals.get(&id)?.clone();
        let steps = self
            .steps
            .range((id, StepSeq::FIRST)..)
            .take_while(|((owner, _), _)| *owner == id)
            .map(|(_, step)| step.clone())
            .collect();
        let created = self
            .created
            .range((id, CreatedSeq::FIRST)..)
            .take_while(|((owner, _), _)| *owner == id)
            .map(|(_, record)| record.clone())
            .collect();
        let locks = self
            .locks
            .iter()
            .filter(|((owner, _), _)| *owner == id)
            .map(|(_, key)| key.clone())
            .collect();
        Some(JournalView::new(journal, steps, created, locks))
    }

    /// Records the commit time for a row; an update keeps its first
    /// `created_at`.
    fn stamp(&mut self, resource: ResourceRef, created_at: Option<Timestamp>, now: Timestamp) {
        let created_at = created_at
            .or_else(|| {
                self.written
                    .get(&resource)
                    .map(|written| written.created_at)
            })
            .unwrap_or(now);
        self.written.insert(
            resource,
            WrittenAt {
                created_at,
                updated_at: now,
            },
        );
    }

    fn prune_operations(&mut self, policy: &RetentionPolicy, now: Timestamp) -> u64 {
        let finished = self
            .operations
            .values()
            .filter(|operation| operation.state().is_final())
            .map(|operation| (operation.updated_at(), operation.id()))
            .collect();
        let doomed = beyond_retention(
            finished,
            cutoff(now, policy.operation_max_age),
            policy.operation_max_rows,
        );
        for id in &doomed {
            self.operations.remove(id);
            self.journals.remove(id);
        }
        // The journal rows go with their operation, as the cascade does.
        self.steps.retain(|(owner, _), _| !doomed.contains(owner));
        self.created.retain(|(owner, _), _| !doomed.contains(owner));
        self.locks.retain(|(owner, _), _| !doomed.contains(owner));
        count(doomed.len())
    }

    fn prune_audit_events(&mut self, policy: &RetentionPolicy, now: Timestamp) -> u64 {
        let rows = self
            .audit_events
            .iter()
            .map(|row| (row.occurred_at, row.id))
            .collect();
        let doomed = beyond_retention(
            rows,
            cutoff(now, policy.audit_max_age),
            policy.audit_max_rows,
        );
        self.audit_events.retain(|row| !doomed.contains(&row.id));
        count(doomed.len())
    }

    fn prune_released_leases(&mut self, policy: &RetentionPolicy, now: Timestamp) -> u64 {
        let cutoff = cutoff(now, policy.released_lease_max_age);
        let before = self.leases.len();
        self.leases.retain(
            |_, lease| !matches!(lease.state(), LeaseState::Released { at } if at < cutoff),
        );
        count(before - self.leases.len())
    }

    fn prune_observations(&mut self, policy: &RetentionPolicy) -> u64 {
        let mut doomed = Vec::new();
        let instances: Vec<InstanceId> = self.instances.keys().copied().collect();
        for instance in instances {
            let rows = self
                .observations
                .iter()
                .filter(|row| row.instance == instance)
                .map(|row| (row.observed_at, row.id))
                .collect();
            doomed.extend(beyond_retention(
                rows,
                Timestamp::MIN,
                policy.observations_per_instance,
            ));
        }
        self.observations.retain(|row| !doomed.contains(&row.id));
        count(doomed.len())
    }
}

/// Checks that an insert names a new resource, and that an update names an
/// existing one still at the expected revision.
fn check_existing(
    stored: Option<ResourceRevision>,
    expected: Option<ResourceRevision>,
    resource: &ResourceRef,
) -> Result<(), RegistryError> {
    match (stored, expected) {
        (Some(_), None) => Err(RegistryError::DuplicateResource {
            resource: resource.clone(),
        }),
        (None, Some(_)) => Err(RegistryError::NotFound {
            resource: resource.clone(),
        }),
        (Some(actual), Some(expected)) if actual != expected => {
            Err(RegistryError::RevisionConflict {
                resource: resource.clone(),
                expected,
                actual,
            })
        }
        _ => Ok(()),
    }
}

/// The error SQLite gives for a unique constraint a check before it
/// missed.
fn unique_failed(columns: &str) -> RegistryError {
    RegistryError::Integrity {
        detail: format!("UNIQUE constraint failed: {columns}"),
    }
}

/// Two runtimes are the same install when they share a managed artifact
/// or an external folder.
fn same_install(a: &InstallMode, b: &InstallMode) -> bool {
    match (a, b) {
        (
            InstallMode::Managed { artifact_sha256: a },
            InstallMode::Managed { artifact_sha256: b },
        ) => a == b,
        (InstallMode::External { root: a, .. }, InstallMode::External { root: b, .. }) => a == b,
        _ => false,
    }
}

/// Returns the oldest time a row may have to survive an age limit.
fn cutoff(now: Timestamp, max_age: Duration) -> Timestamp {
    SignedDuration::try_from(max_age)
        .ok()
        .and_then(|age| now.checked_sub(age).ok())
        .unwrap_or(Timestamp::MIN)
}

/// Returns the keys of the rows to delete: older than `cutoff`, or beyond
/// the `max_rows` newest by time and then key.
fn beyond_retention<K: Ord + Copy>(
    mut rows: Vec<(Timestamp, K)>,
    cutoff: Timestamp,
    max_rows: u32,
) -> Vec<K> {
    rows.sort_by(|a, b| b.cmp(a));
    let keep = usize::try_from(max_rows).unwrap_or(usize::MAX);
    rows.into_iter()
        .enumerate()
        .filter(|(rank, (at, _))| *at < cutoff || *rank >= keep)
        .map(|(_, (_, key))| key)
        .collect()
}

fn count(rows: usize) -> u64 {
    u64::try_from(rows).unwrap_or(u64::MAX)
}
