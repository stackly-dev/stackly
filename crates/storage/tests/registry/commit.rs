//! Commits on the SQLite registry: one transaction per batch, the
//! generation, compare and swap, and the check order with its typed errors
//! (spec 0005 step 10, AC-8, AC-10).

use std::collections::BTreeSet;

use jiff::SignedDuration;
use stackly_domain::{
    BatchError, Environment, EnvironmentId, EnvironmentMember, Generation, InstallMode,
    InstanceFilter, InstanceId, Lease, LeaseFilter, LeaseHolder, LeaseId, NewRuntime,
    OperationResource, Ownership, Registry, RegistryError, ResourceRef, ResourceRevision, Runtime,
    RuntimeEvent, RuntimeId, WriteBatch, WriteOp,
};

use crate::common::{Base, Harness, artifact, managed_runtime, retained, stored};

#[test]
fn every_commit_raises_the_generation_by_one_and_reads_return_it() {
    let harness = Harness::new();
    let start = harness.registry.runtimes().unwrap().generation;

    let empty = harness.commit(Vec::new());
    let base = harness.base();
    let read = harness.registry.runtime(base.runtime.id()).unwrap();

    assert_eq!(start, Generation::ZERO);
    assert_eq!(empty.generation, Generation::new(1));
    assert!(empty.revisions.is_empty());
    assert_eq!(read.generation, Generation::new(2));
    assert_eq!(read.value, Some(base.runtime));
}

#[test]
fn a_stale_expected_generation_rejects_the_whole_batch() {
    let harness = Harness::new();
    let base = harness.base();
    let read = harness.registry.known_instance_ids().unwrap();
    harness.commit(Vec::new());
    let instance = harness.instance(&base, "main");

    let err = harness
        .registry
        .commit(
            WriteBatch::new(vec![WriteOp::InsertInstance(instance.clone())])
                .at_generation(read.generation),
        )
        .unwrap_err();

    assert_eq!(
        err,
        BatchError::batch(RegistryError::GenerationConflict {
            expected: read.generation,
            actual: read.generation.next(),
        })
    );
    assert_eq!(
        harness.registry.instance(instance.id()).unwrap().value,
        None
    );
    let current = harness.registry.known_instance_ids().unwrap().generation;
    let committed = harness
        .registry
        .commit(WriteBatch::new(vec![WriteOp::InsertInstance(instance)]).at_generation(current))
        .unwrap();
    assert_eq!(committed.generation, current.next());
}

#[test]
fn a_stale_revision_rejects_the_batch_and_nothing_before_it_applies() {
    let harness = Harness::new();
    let base = harness.base();
    let instance = harness.instance(&base, "main");
    harness.commit(vec![WriteOp::InsertInstance(instance.clone())]);
    let mut renamed = instance.clone();
    renamed.rename("primary".parse().unwrap());
    harness.commit(vec![WriteOp::UpdateInstance {
        instance: renamed.clone(),
        expected: instance.revision(),
    }]);
    let generation = harness.registry.instance(instance.id()).unwrap().generation;
    let other = harness.instance(&base, "other");
    let mut stale = instance.clone();
    stale.rename("stale".parse().unwrap());

    let err = harness.reject(vec![
        WriteOp::InsertInstance(other.clone()),
        WriteOp::UpdateInstance {
            instance: stale,
            expected: instance.revision(),
        },
    ]);

    assert_eq!(
        err,
        BatchError::at(
            1,
            RegistryError::RevisionConflict {
                resource: ResourceRef::Instance(instance.id()),
                expected: ResourceRevision::INITIAL,
                actual: renamed.revision(),
            }
        )
    );
    assert_eq!(harness.registry.instance(other.id()).unwrap().value, None);
    let stored = harness.registry.instance(instance.id()).unwrap();
    assert_eq!(stored.generation, generation);
    assert_eq!(stored.value.unwrap().name().as_str(), "primary");
}

#[test]
fn a_commit_returns_the_new_revision_of_every_aggregate_it_wrote() {
    let harness = Harness::new();
    let base = Base::new("postgresql");
    let mut ops = base.ops();
    let mut runtime = base.runtime.clone();
    runtime
        .apply(RuntimeEvent::Resolved, harness.now())
        .unwrap();
    ops.push(WriteOp::UpdateRuntime {
        runtime: runtime.clone(),
        expected: base.runtime.revision(),
    });

    let committed = harness.commit(ops);

    assert_eq!(
        committed.revisions,
        [
            (
                ResourceRef::Runtime(runtime.id()),
                ResourceRevision::INITIAL
            ),
            (ResourceRef::Runtime(runtime.id()), runtime.revision()),
        ]
    );
    assert_eq!(
        harness.registry.runtime(runtime.id()).unwrap().value,
        Some(runtime)
    );
}

#[test]
fn the_value_shape_is_checked_first_with_the_same_variant_as_the_fake() {
    let harness = Harness::new();
    let base = harness.base();
    let mut moved = base.runtime.clone();
    moved.apply(RuntimeEvent::Resolved, harness.now()).unwrap();

    // A duplicate ID would fail later; the revision of an insert is checked
    // before anything is read.
    let err = harness.reject(vec![WriteOp::InsertRuntime(moved.clone())]);
    assert_eq!(
        err,
        BatchError::at(
            0,
            RegistryError::InvalidRevision {
                resource: ResourceRef::Runtime(moved.id()),
                expected: ResourceRevision::INITIAL,
                actual: moved.revision(),
            }
        )
    );
    let err = harness.reject(vec![WriteOp::UpdateRuntime {
        runtime: moved.clone(),
        expected: moved.revision(),
    }]);
    assert!(
        matches!(err.error, RegistryError::InvalidRevision { .. }),
        "{err:?}"
    );
}

#[test]
fn existence_comes_before_the_revision_and_references() {
    let harness = Harness::new();
    let base = harness.base();
    let instance = harness.instance(&base, "main");
    harness.commit(vec![WriteOp::InsertInstance(instance.clone())]);

    let err = harness.reject(vec![WriteOp::InsertInstance(instance.clone())]);
    assert_eq!(
        err,
        BatchError::at(
            0,
            RegistryError::DuplicateResource {
                resource: ResourceRef::Instance(instance.id()),
            }
        )
    );
    let unknown = harness.instance(&base, "unknown");
    let mut changed = unknown.clone();
    changed.rename("changed".parse().unwrap());
    let err = harness.reject(vec![WriteOp::UpdateInstance {
        instance: changed,
        expected: unknown.revision(),
    }]);
    assert_eq!(
        err,
        BatchError::at(
            0,
            RegistryError::NotFound {
                resource: ResourceRef::Instance(unknown.id()),
            }
        )
    );
    for op in [
        WriteOp::InsertProviderRevision(base.provider.clone()),
        WriteOp::InsertArtifact(base.artifact.clone()),
        WriteOp::InsertRuntime(base.runtime.clone()),
    ] {
        let resource = op.resource();
        let err = harness.reject(vec![op]);
        assert_eq!(
            err,
            BatchError::at(0, RegistryError::DuplicateResource { resource })
        );
    }
}

#[test]
fn the_revision_is_checked_before_references() {
    let harness = Harness::new();
    let base = harness.base();
    let instance = harness.instance(&base, "main");
    harness.commit(vec![WriteOp::InsertInstance(instance.clone())]);
    let mut stored = stored(&instance);
    stored.runtime_id = RuntimeId::generate();
    stored.revision = ResourceRevision::new(3).unwrap();
    let moved = stackly_domain::Instance::restore(stored, harness.now()).unwrap();

    let err = harness.reject(vec![WriteOp::UpdateInstance {
        instance: moved,
        expected: ResourceRevision::new(2).unwrap(),
    }]);

    assert!(
        matches!(err.error, RegistryError::RevisionConflict { .. }),
        "{err:?}"
    );
}

#[test]
fn missing_references_are_named_runtime_first() {
    let harness = Harness::new();
    let base = harness.base();
    let orphan = Base::new("postgresql");
    let mut instance = stored(&harness.instance(&base, "main"));
    instance.runtime_id = orphan.runtime.id();
    instance.provider_revision = orphan.provider.revision.key().clone();
    let instance = stackly_domain::Instance::restore(instance, harness.now()).unwrap();

    let err = harness.reject(vec![WriteOp::InsertInstance(instance.clone())]);
    assert_eq!(
        err.error,
        RegistryError::MissingReference {
            resource: ResourceRef::Runtime(orphan.runtime.id()),
        }
    );
    let mut instance = stored(&instance);
    instance.runtime_id = base.runtime.id();
    let instance = stackly_domain::Instance::restore(instance, harness.now()).unwrap();
    let err = harness.reject(vec![WriteOp::InsertInstance(instance)]);
    assert_eq!(
        err.error,
        RegistryError::MissingReference {
            resource: ResourceRef::ProviderRevision(Box::new(
                orphan.provider.revision.key().clone()
            )),
        }
    );
    let err = harness.reject(vec![WriteOp::InsertRuntime(orphan.runtime.clone())]);
    assert_eq!(
        err.error,
        RegistryError::MissingReference {
            resource: ResourceRef::Artifact(orphan.artifact.sha256),
        }
    );
}

#[test]
fn runtime_dependencies_must_exist_and_never_point_at_the_runtime_itself() {
    let harness = Harness::new();
    let base = harness.base();
    let unknown = RuntimeId::generate();
    let extension = artifact("postgresql");
    let with = |depends_on: BTreeSet<RuntimeId>| {
        Runtime::new(NewRuntime {
            id: RuntimeId::generate(),
            software: extension.software.clone(),
            version: extension.version.clone(),
            line: "17".parse().unwrap(),
            target: extension.target,
            install: InstallMode::Managed {
                artifact_sha256: extension.sha256,
            },
            depends_on,
        })
    };
    harness.commit(vec![WriteOp::InsertArtifact(extension.clone())]);

    let err = harness.reject(vec![WriteOp::InsertRuntime(with(BTreeSet::from([
        base.runtime.id(),
        unknown,
    ])))]);
    assert_eq!(
        err.error,
        RegistryError::MissingReference {
            resource: ResourceRef::Runtime(unknown),
        }
    );
    let dependent = with(BTreeSet::from([base.runtime.id()]));
    harness.commit(vec![WriteOp::InsertRuntime(dependent.clone())]);
    let stored = harness.registry.runtime(dependent.id()).unwrap().value;
    assert_eq!(stored, Some(dependent.clone()));

    let mut moved = dependent.clone();
    moved.apply(RuntimeEvent::Resolved, harness.now()).unwrap();
    let mut changed = stackly_domain::StoredRuntime {
        id: moved.id(),
        software: moved.software().clone(),
        version: moved.version().clone(),
        line: moved.line().clone(),
        target: moved.target(),
        install: moved.install().clone(),
        state: moved.state(),
        installed_at: None,
        last_verified_at: None,
        depends_on: moved.depends_on().clone(),
        revision: moved.revision(),
    };
    changed.depends_on.insert(unknown);
    let err = harness.reject(vec![WriteOp::UpdateRuntime {
        runtime: Runtime::restore(changed).unwrap(),
        expected: dependent.revision(),
    }]);
    assert_eq!(
        err.error,
        RegistryError::MissingReference {
            resource: ResourceRef::Runtime(unknown),
        }
    );
    let own = RuntimeId::generate();
    let itself = Runtime::new(NewRuntime {
        id: own,
        software: extension.software.clone(),
        version: "17.7".parse().unwrap(),
        line: "17".parse().unwrap(),
        target: extension.target,
        install: InstallMode::External {
            root: "/opt/pg-self".into(),
            observed_sha256: None,
        },
        depends_on: BTreeSet::from([own]),
    });
    let err = harness.reject(vec![WriteOp::InsertRuntime(itself)]);
    assert_eq!(err.op_index, Some(0));
    assert!(
        matches!(&err.error, RegistryError::Integrity { detail } if detail.contains("itself")),
        "{err:?}"
    );
}

#[test]
fn one_runtime_per_managed_artifact_and_per_external_folder() {
    let harness = Harness::new();
    let base = harness.base();
    let twin = managed_runtime(&base.artifact);

    let err = harness.reject(vec![WriteOp::InsertRuntime(twin)]);
    assert_eq!(
        err,
        BatchError::at(
            0,
            RegistryError::DuplicateRuntime {
                existing: base.runtime.id(),
            }
        )
    );
    let external = |root: &str| {
        Runtime::new(NewRuntime {
            id: RuntimeId::generate(),
            software: base.software(),
            version: "17.5".parse().unwrap(),
            line: "17".parse().unwrap(),
            target: base.artifact.target,
            install: InstallMode::External {
                root: root.into(),
                observed_sha256: None,
            },
            depends_on: BTreeSet::new(),
        })
    };
    let first = external("/opt/homebrew/opt/postgresql@17");
    harness.commit(vec![WriteOp::InsertRuntime(first.clone())]);
    let err = harness.reject(vec![WriteOp::InsertRuntime(external(
        "/opt/homebrew/opt/postgresql@17",
    ))]);
    assert_eq!(
        err.error,
        RegistryError::DuplicateRuntime {
            existing: first.id(),
        }
    );
    harness.commit(vec![WriteOp::InsertRuntime(external("/usr/local/pgsql"))]);
}

#[test]
fn instance_names_are_unique_among_instances_that_are_not_retained() {
    let harness = Harness::new();
    let base = harness.base();
    let first = harness.instance(&base, "main");
    harness.commit(vec![WriteOp::InsertInstance(first.clone())]);

    let second = harness.instance(&base, "main");
    let err = harness.reject(vec![WriteOp::InsertInstance(second.clone())]);
    assert_eq!(
        err.error,
        RegistryError::NameTaken {
            name: "main".parse().unwrap(),
            existing: first.id(),
        }
    );
    let gone = retained(&first, harness.now());
    harness.commit(vec![WriteOp::UpdateInstance {
        instance: gone,
        expected: first.revision(),
    }]);
    harness.commit(vec![WriteOp::InsertInstance(second.clone())]);

    let listed = harness
        .registry
        .instances(InstanceFilter {
            include_retained: true,
        })
        .unwrap()
        .value;
    assert_eq!(listed.len(), 2);
    let by_name = harness
        .registry
        .instance_by_name(&"main".parse().unwrap())
        .unwrap()
        .value
        .unwrap();
    assert_eq!(by_name.id(), second.id());
}

#[test]
fn short_keys_are_unique_across_every_instance() {
    let harness = Harness::new();
    let base = harness.base();
    let first = harness.instance(&base, "main");
    harness.commit(vec![WriteOp::InsertInstance(first.clone())]);
    let text = InstanceId::generate().to_string();
    let twin_id: InstanceId = format!("{}{}", &text[..28], &first.id().to_string()[28..])
        .parse()
        .unwrap();
    let mut twin = stored(&harness.instance(&base, "twin"));
    twin.id = twin_id;
    let twin = stackly_domain::Instance::restore(twin, harness.now()).unwrap();

    let err = harness.reject(vec![WriteOp::InsertInstance(twin)]);

    assert_eq!(
        err.error,
        RegistryError::ShortKeyTaken {
            key: first.id().short_key(),
            existing: first.id(),
        }
    );
}

#[test]
fn the_software_of_an_instance_matches_its_runtime_and_provider_revision() {
    let harness = Harness::new();
    let base = harness.base();
    let other = harness.base_of("mysql");
    let mut instance = stored(&harness.instance(&base, "main"));
    instance.provider_revision = other.provider.revision.key().clone();
    let instance = stackly_domain::Instance::restore(instance, harness.now()).unwrap();

    let err = harness.reject(vec![WriteOp::InsertInstance(instance)]);

    assert_eq!(
        err.error,
        RegistryError::SoftwareMismatch {
            instance: "postgresql".parse().unwrap(),
            runtime: "postgresql".parse().unwrap(),
            provider_revision: "mysql".parse().unwrap(),
        }
    );
}

#[test]
fn a_name_conflict_is_reported_before_a_software_mismatch() {
    let harness = Harness::new();
    let base = harness.base();
    let other = harness.base_of("mysql");
    let first = harness.instance(&base, "main");
    harness.commit(vec![WriteOp::InsertInstance(first.clone())]);
    let mut clash = stored(&harness.instance(&base, "main"));
    clash.runtime_id = other.runtime.id();

    let err = harness.reject(vec![WriteOp::InsertInstance(
        stackly_domain::Instance::restore(clash, harness.now()).unwrap(),
    )]);

    assert!(
        matches!(err.error, RegistryError::NameTaken { .. }),
        "{err:?}"
    );
}

#[test]
fn an_instance_has_at_most_one_dedicated_environment() {
    let harness = Harness::new();
    let base = harness.base();
    let instance = harness.instance(&base, "main");
    harness.commit(vec![WriteOp::InsertInstance(instance.clone())]);
    let environment = |name: &str, ownership| {
        Environment::new(
            EnvironmentId::generate(),
            name.parse().unwrap(),
            None,
            vec![EnvironmentMember::new(
                "db".parse().unwrap(),
                instance.id(),
                ownership,
            )],
        )
        .unwrap()
    };
    let owner = environment("shop", Ownership::Dedicated);
    harness.commit(vec![WriteOp::InsertEnvironment(owner.clone())]);
    harness.commit(vec![WriteOp::InsertEnvironment(environment(
        "reports",
        Ownership::Shared,
    ))]);

    let err = harness.reject(vec![WriteOp::InsertEnvironment(environment(
        "blog",
        Ownership::Dedicated,
    ))]);

    assert_eq!(
        err.error,
        RegistryError::DedicatedOwnerExists {
            instance: instance.id(),
            environment: owner.id(),
        }
    );
    let missing = Environment::new(
        EnvironmentId::generate(),
        "ghost".parse().unwrap(),
        None,
        vec![EnvironmentMember::new(
            "db".parse().unwrap(),
            InstanceId::generate(),
            Ownership::Shared,
        )],
    )
    .unwrap();
    let err = harness.reject(vec![WriteOp::InsertEnvironment(missing.clone())]);
    assert_eq!(
        err.error,
        RegistryError::MissingReference {
            resource: ResourceRef::Instance(missing.members()[0].instance_id()),
        }
    );
}

#[test]
fn an_environment_holds_at_most_one_active_lease_per_instance() {
    let harness = Harness::new();
    let base = harness.base();
    let instance = harness.instance(&base, "main");
    let environment = Environment::new(
        EnvironmentId::generate(),
        "shop".parse().unwrap(),
        None,
        vec![],
    )
    .unwrap();
    harness.commit(vec![
        WriteOp::InsertInstance(instance.clone()),
        WriteOp::InsertEnvironment(environment.clone()),
    ]);
    let now = harness.now();
    let lease = |holder| Lease::new(LeaseId::generate(), instance.id(), holder, None, now).unwrap();
    let mut first = lease(LeaseHolder::Environment(environment.id()));
    harness.commit(vec![WriteOp::InsertLease(first.clone())]);

    let err = harness.reject(vec![WriteOp::InsertLease(lease(LeaseHolder::Environment(
        environment.id(),
    )))]);
    assert_eq!(
        err.error,
        RegistryError::ActiveLeaseExists {
            instance: instance.id(),
            environment: environment.id(),
        }
    );
    first.release(now).unwrap();
    harness.commit(vec![
        WriteOp::UpdateLease(first.clone()),
        WriteOp::InsertLease(lease(LeaseHolder::Environment(environment.id()))),
        WriteOp::InsertLease(lease(LeaseHolder::Manual)),
    ]);
    let leases = harness
        .registry
        .leases(instance.id(), LeaseFilter::All)
        .unwrap()
        .value;
    assert_eq!(leases.len(), 3);
    let unknown = lease(LeaseHolder::Environment(EnvironmentId::generate()));
    let err = harness.reject(vec![WriteOp::InsertLease(unknown.clone())]);
    assert!(
        matches!(
            err.error,
            RegistryError::MissingReference {
                resource: ResourceRef::Environment(_)
            }
        ),
        "{err:?}"
    );
    let err = harness.reject(vec![WriteOp::UpdateLease(unknown.clone())]);
    assert_eq!(
        err.error,
        RegistryError::NotFound {
            resource: ResourceRef::Lease(unknown.id()),
        }
    );
}

#[test]
fn operations_insert_once_and_update_with_compare_and_swap() {
    let harness = Harness::new();
    let operation = harness.operation(OperationResource::Manager);
    harness.commit(vec![WriteOp::InsertOperation(operation.clone())]);
    let mut started = operation.clone();
    harness.clock().advance(SignedDuration::from_secs(1));
    started
        .apply(stackly_domain::OperationEvent::Start, harness.now())
        .unwrap();

    let err = harness.reject(vec![WriteOp::InsertOperation(operation.clone())]);
    assert!(
        matches!(err.error, RegistryError::DuplicateResource { .. }),
        "{err:?}"
    );
    harness.commit(vec![WriteOp::UpdateOperation {
        operation: started.clone(),
        expected: operation.revision(),
    }]);
    let err = harness.reject(vec![WriteOp::UpdateOperation {
        operation: started.clone(),
        expected: operation.revision(),
    }]);
    assert!(
        matches!(err.error, RegistryError::RevisionConflict { .. }),
        "{err:?}"
    );
    assert_eq!(
        harness.registry.operation(operation.id()).unwrap().value,
        Some(started)
    );
}

#[test]
fn a_sql_constraint_that_still_refuses_a_write_is_integrity_at_its_op() {
    let harness = Harness::new();
    let base = harness.base();
    let mut instance = stored(&harness.instance(&base, "main"));
    let endpoint = instance.endpoints[0].clone();
    instance.endpoints.push(endpoint);
    let instance = stackly_domain::Instance::restore(instance, harness.now()).unwrap();

    let err = harness.reject(vec![
        WriteOp::InsertOperation(harness.operation(OperationResource::Manager)),
        WriteOp::InsertInstance(instance.clone()),
    ]);

    assert_eq!(err.op_index, Some(1));
    assert!(
        matches!(&err.error, RegistryError::Integrity { detail } if detail.contains("UNIQUE")),
        "{err:?}"
    );
    assert_eq!(
        harness.registry.instance(instance.id()).unwrap().value,
        None
    );
}

#[test]
fn concurrent_inserts_of_one_name_leave_exactly_one_instance() {
    let harness = Harness::new();
    let base = harness.base();
    let candidates: Vec<_> = (0..16).map(|_| harness.instance(&base, "main")).collect();
    let barrier = std::sync::Barrier::new(candidates.len());

    let results: Vec<_> = std::thread::scope(|scope| {
        let handles: Vec<_> = candidates
            .iter()
            .map(|instance| {
                let registry = harness.registry.clone();
                let barrier = &barrier;
                scope.spawn(move || {
                    barrier.wait();
                    registry.commit(WriteBatch::new(vec![WriteOp::InsertInstance(
                        instance.clone(),
                    )]))
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });

    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert!(
        results
            .iter()
            .filter_map(|r| r.as_ref().err())
            .all(|err| matches!(err.error, RegistryError::NameTaken { .. }))
    );
}
