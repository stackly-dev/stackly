//! Commit scenarios: generation, check order, compare and swap, and the
//! existence, reference, and unique rules of every op (AC-8, AC-10).

use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;

use stackly_domain::{
    BatchError, DesiredState, EnvironmentId, InstanceFilter, InstanceId, InstanceName, LeaseFilter,
    LifecycleEvent, OperationEvent, OperationId, RegistryError, ResourceRef, RuntimeId,
    SoftwareName, WriteBatch, WriteOp,
};

use super::{
    Suite, at, batch, domain, expect, expect_eq, fresh_root, parse, rev, same_short_key, shifted,
    suffix,
};
use crate::registry::{
    EnvironmentBuilder, InstanceBuilder, LeaseBuilder, OperationBuilder, ProviderRevisionBuilder,
    RuntimeBuilder,
};

/// Every commit raises the generation by exactly 1, and a batch at a stale
/// generation is refused (AC-8).
pub(super) fn generation(suite: &Suite<'_>) {
    let start = suite.generation();
    let first = suite.commit("an empty batch commits", Vec::new());
    expect_eq(
        "a commit raises the generation by exactly 1",
        first.generation,
        start.next(),
    );
    expect_eq(
        "an empty commit returns no revisions",
        first.revisions,
        Vec::new(),
    );
    expect_eq(
        "a read returns the generation of the last commit",
        suite.generation(),
        first.generation,
    );
    let second = suite.commit_batch(
        "a batch at the current generation commits",
        WriteBatch::new(Vec::new()).at_generation(first.generation),
    );
    expect_eq(
        "a batch at the current generation raises it by 1",
        second.generation,
        first.generation.next(),
    );
    suite.rejected(
        "a batch at a stale generation is refused",
        WriteBatch::new(Vec::new()).at_generation(start),
        BatchError::batch(RegistryError::GenerationConflict {
            expected: start,
            actual: second.generation,
        }),
    );

    let runtime = RuntimeBuilder::new().external(fresh_root()).build();
    let committed = suite.commit(
        "store a runtime",
        vec![WriteOp::InsertRuntime(runtime.clone())],
    );
    let read = suite.read("read the runtime", suite.registry.runtime(runtime.id()));
    expect_eq(
        "a read returns the generation it read at",
        read.generation,
        committed.generation,
    );
    expect_eq(
        "a read returns what the commit stored",
        read.value,
        Some(runtime),
    );
}

/// A commit checks the generation first, then each op in order, and within
/// an op its shape, existence, revision, references, and unique rules, and
/// returns the first error found (spec 0005, "Port registry").
pub(super) fn check_order(suite: &Suite<'_>) {
    let stack = suite.stack("store a runtime for the order checks");
    let first = Suite::instance(&stack).build();
    suite.commit(
        "store an instance for the order checks",
        vec![WriteOp::InsertInstance(first.clone())],
    );
    let stale = suite.generation();
    suite.commit("raise the generation once", Vec::new());

    let bad_shape = WriteOp::InsertRuntime(RuntimeBuilder::new().revision(2).build());
    suite.rejected(
        "the generation is checked before any op",
        WriteBatch::new(vec![bad_shape]).at_generation(stale),
        BatchError::batch(RegistryError::GenerationConflict {
            expected: stale,
            actual: stale.next(),
        }),
    );

    let existing = stack.runtime.id();
    suite.rejected(
        "an insert must carry revision 1, checked before existence",
        batch(vec![WriteOp::InsertRuntime(
            RuntimeBuilder::new().with_id(existing).revision(2).build(),
        )]),
        at(
            0,
            RegistryError::InvalidRevision {
                resource: ResourceRef::Runtime(existing),
                expected: rev(1),
                actual: rev(2),
            },
        ),
    );

    let unknown = RuntimeBuilder::new().revision(3);
    suite.rejected(
        "an update must carry the expected revision plus 1, checked before existence",
        batch(vec![WriteOp::UpdateRuntime {
            runtime: unknown.clone().build(),
            expected: rev(1),
        }]),
        at(
            0,
            RegistryError::InvalidRevision {
                resource: ResourceRef::Runtime(unknown.id()),
                expected: rev(2),
                actual: rev(3),
            },
        ),
    );

    let not_utf8 = PathBuf::from(OsStr::from_bytes(b"/conformance/external/\xff"));
    suite.rejected(
        "an external root that is not UTF-8 is refused",
        batch(vec![WriteOp::InsertRuntime(
            RuntimeBuilder::new().external(not_utf8).build(),
        )]),
        at(
            0,
            RegistryError::NotUtf8Path {
                field: "external_root",
            },
        ),
    );

    let missing = RuntimeBuilder::new().external(fresh_root()).revision(2);
    suite.rejected(
        "an update of a missing aggregate is NotFound",
        batch(vec![WriteOp::UpdateRuntime {
            runtime: missing.clone().build(),
            expected: rev(1),
        }]),
        at(
            0,
            RegistryError::NotFound {
                resource: ResourceRef::Runtime(missing.id()),
            },
        ),
    );

    let stale_update = RuntimeBuilder::new()
        .with_id(existing)
        .external(fresh_root())
        .depends_on(RuntimeId::generate())
        .revision(3)
        .build();
    suite.rejected(
        "the revision is checked before references",
        batch(vec![WriteOp::UpdateRuntime {
            runtime: stale_update,
            expected: rev(2),
        }]),
        at(
            0,
            RegistryError::RevisionConflict {
                resource: ResourceRef::Runtime(existing),
                expected: rev(2),
                actual: rev(1),
            },
        ),
    );

    let unknown_runtime = RuntimeId::generate();
    suite.rejected(
        "references are checked before unique rules",
        batch(vec![WriteOp::InsertInstance(
            Suite::instance(&stack)
                .name(first.name().as_str())
                .runtime_id(unknown_runtime)
                .build(),
        )]),
        at(
            0,
            RegistryError::MissingReference {
                resource: ResourceRef::Runtime(unknown_runtime),
            },
        ),
    );

    let unknown_provider = ProviderRevisionBuilder::new().key();
    suite.rejected(
        "the runtime reference is checked before the provider revision",
        batch(vec![WriteOp::InsertInstance(
            Suite::instance(&stack)
                .runtime_id(unknown_runtime)
                .provider_revision(unknown_provider.clone())
                .build(),
        )]),
        at(
            0,
            RegistryError::MissingReference {
                resource: ResourceRef::Runtime(unknown_runtime),
            },
        ),
    );
    suite.rejected(
        "an instance needs its provider revision",
        batch(vec![WriteOp::InsertInstance(
            Suite::instance(&stack)
                .provider_revision(unknown_provider.clone())
                .build(),
        )]),
        at(
            0,
            RegistryError::MissingReference {
                resource: ResourceRef::ProviderRevision(Box::new(unknown_provider)),
            },
        ),
    );

    let name_taken = RegistryError::NameTaken {
        name: first.name().clone(),
        existing: first.id(),
    };
    let twin = same_short_key(first.id());
    suite.rejected(
        "the name is checked before the short key",
        batch(vec![WriteOp::InsertInstance(
            Suite::instance(&stack)
                .with_id(twin)
                .name(first.name().as_str())
                .build(),
        )]),
        at(0, name_taken.clone()),
    );
    suite.rejected(
        "the name is checked before the software",
        batch(vec![WriteOp::InsertInstance(
            Suite::instance(&stack)
                .name(first.name().as_str())
                .software("mysql")
                .build(),
        )]),
        at(0, name_taken.clone()),
    );
    suite.rejected(
        "the short key is checked before the software",
        batch(vec![WriteOp::InsertInstance(
            Suite::instance(&stack)
                .with_id(twin)
                .name(&format!("twin-{}", suffix(twin)))
                .software("mysql")
                .build(),
        )]),
        at(
            0,
            RegistryError::ShortKeyTaken {
                key: first.id().short_key(),
                existing: first.id(),
            },
        ),
    );

    let operation = OperationBuilder::new().build();
    suite.rejected(
        "the failing op is named by its index",
        batch(vec![
            WriteOp::InsertOperation(operation.clone()),
            WriteOp::InsertInstance(Suite::instance(&stack).name(first.name().as_str()).build()),
        ]),
        at(1, name_taken),
    );
    expect_eq(
        "an op before the failing one is not applied",
        suite.value(
            "read the operation",
            suite.registry.operation(operation.id()),
        ),
        None,
    );
}

/// An update applies only at the stored revision, and a stale one fails
/// the whole batch (AC-8, REQ-048).
pub(super) fn compare_and_swap(suite: &Suite<'_>) {
    let stack = suite.stack("store a runtime for compare and swap");
    let original = Suite::instance(&stack).build();
    let id = original.id();
    suite.commit(
        "store an instance for compare and swap",
        vec![WriteOp::InsertInstance(original.clone())],
    );

    let mut renamed = original.clone();
    renamed.rename(parse("make a name", &format!("renamed-{}", suffix(id))));
    let committed = suite.commit(
        "an update at the stored revision applies",
        vec![WriteOp::UpdateInstance {
            instance: renamed.clone(),
            expected: rev(1),
        }],
    );
    expect_eq(
        "an update returns the new revision",
        committed.revisions,
        vec![(ResourceRef::Instance(id), rev(2))],
    );
    expect_eq(
        "an update is what reads back",
        suite.value("read the instance", suite.registry.instance(id)),
        Some(suite.expected_instance(&renamed)),
    );

    let mut stale = original;
    stale.set_desired(DesiredState::Running);
    let operation = OperationBuilder::new().build();
    suite.rejected(
        "stale_revision_is_rejected_and_nothing_in_the_batch_applies",
        batch(vec![
            WriteOp::InsertOperation(operation.clone()),
            WriteOp::UpdateInstance {
                instance: stale,
                expected: rev(1),
            },
        ]),
        at(
            1,
            RegistryError::RevisionConflict {
                resource: ResourceRef::Instance(id),
                expected: rev(1),
                actual: rev(2),
            },
        ),
    );
    expect_eq(
        "a stale update leaves the stored instance",
        suite.value("read the instance", suite.registry.instance(id)),
        Some(suite.expected_instance(&renamed)),
    );
    expect_eq(
        "a stale update applies nothing else of its batch",
        suite.value(
            "read the operation",
            suite.registry.operation(operation.id()),
        ),
        None,
    );

    let operation = OperationBuilder::new().build();
    suite.commit(
        "store an operation for compare and swap",
        vec![WriteOp::InsertOperation(operation.clone())],
    );
    let mut started = operation.clone();
    domain(
        "start the operation",
        started.apply(OperationEvent::Start, suite.loaded_at),
    );
    suite.commit(
        "an operation update at the stored revision applies",
        vec![WriteOp::UpdateOperation {
            operation: started.clone(),
            expected: rev(1),
        }],
    );
    suite.rejected(
        "a stale operation update is refused",
        batch(vec![WriteOp::UpdateOperation {
            operation: started,
            expected: rev(1),
        }]),
        at(
            0,
            RegistryError::RevisionConflict {
                resource: ResourceRef::Operation(operation.id()),
                expected: rev(1),
                actual: rev(2),
            },
        ),
    );

    let environment = EnvironmentBuilder::new();
    suite.commit(
        "store an environment for compare and swap",
        vec![WriteOp::InsertEnvironment(environment.clone().build())],
    );
    let next = environment.clone().revision(2).build();
    suite.commit(
        "an environment update at the stored revision applies",
        vec![WriteOp::UpdateEnvironment {
            environment: next.clone(),
            expected: rev(1),
        }],
    );
    suite.rejected(
        "a stale environment update is refused",
        batch(vec![WriteOp::UpdateEnvironment {
            environment: next,
            expected: rev(1),
        }]),
        at(
            0,
            RegistryError::RevisionConflict {
                resource: ResourceRef::Environment(environment.id()),
                expected: rev(1),
                actual: rev(2),
            },
        ),
    );

    let runtime = RuntimeBuilder::new().external(fresh_root());
    suite.commit(
        "store a runtime for compare and swap",
        vec![WriteOp::InsertRuntime(runtime.clone().build())],
    );
    let installed = runtime
        .clone()
        .state(stackly_domain::RuntimeState::Installed)
        .revision(2)
        .build();
    suite.commit(
        "a runtime update at the stored revision applies",
        vec![WriteOp::UpdateRuntime {
            runtime: installed.clone(),
            expected: rev(1),
        }],
    );
    suite.rejected(
        "a stale runtime update is refused",
        batch(vec![WriteOp::UpdateRuntime {
            runtime: installed,
            expected: rev(1),
        }]),
        at(
            0,
            RegistryError::RevisionConflict {
                resource: ResourceRef::Runtime(runtime.id()),
                expected: rev(1),
                actual: rev(2),
            },
        ),
    );
}

/// One batch stores every kind of record, an op sees the ops before it in
/// the same batch, a commit returns one revision per aggregate op, and an
/// insert of an existing record is `DuplicateResource`.
pub(super) fn inserts(suite: &Suite<'_>) {
    let provider = ProviderRevisionBuilder::new();
    let runtime = RuntimeBuilder::new();
    let built = runtime.clone().build();
    let instance = InstanceBuilder::new(&built, &provider.key()).build();
    let lease = LeaseBuilder::new(instance.id()).build();
    let operation = OperationBuilder::new().build();
    let environment = EnvironmentBuilder::new().shared(instance.id()).build();
    let ops = vec![
        WriteOp::InsertProviderRevision(provider.clone().record()),
        WriteOp::InsertArtifact(runtime.artifact()),
        WriteOp::InsertRuntime(built.clone()),
        WriteOp::InsertInstance(instance.clone()),
        WriteOp::InsertLease(lease.clone()),
        WriteOp::InsertOperation(operation.clone()),
        WriteOp::InsertEnvironment(environment.clone()),
    ];
    let committed = suite.commit(
        "one batch stores records that refer to each other",
        ops.clone(),
    );
    expect_eq(
        "a commit returns one revision per aggregate op, in op order",
        committed.revisions,
        vec![
            (ResourceRef::Runtime(built.id()), rev(1)),
            (ResourceRef::Instance(instance.id()), rev(1)),
            (ResourceRef::Operation(operation.id()), rev(1)),
            (ResourceRef::Environment(environment.id()), rev(1)),
        ],
    );

    for op in ops {
        let resource = op.resource();
        suite.rejected(
            &format!("inserting the existing {resource} again is refused"),
            batch(vec![op]),
            at(0, RegistryError::DuplicateResource { resource }),
        );
    }
}

/// A managed runtime needs its artifact, a runtime its dependencies, and
/// one artifact or external folder holds one runtime (AC-9 single thread).
pub(super) fn runtimes(suite: &Suite<'_>) {
    let lonely = RuntimeBuilder::new();
    suite.rejected(
        "a managed runtime needs its artifact",
        batch(vec![WriteOp::InsertRuntime(lonely.clone().build())]),
        at(
            0,
            RegistryError::MissingReference {
                resource: ResourceRef::Artifact(lonely.artifact().sha256),
            },
        ),
    );

    let first = RuntimeBuilder::new();
    suite.commit(
        "store a managed runtime",
        vec![
            WriteOp::InsertArtifact(first.artifact()),
            WriteOp::InsertRuntime(first.clone().build()),
        ],
    );
    suite.rejected(
        "one artifact holds one managed runtime",
        batch(vec![WriteOp::InsertRuntime(
            RuntimeBuilder::new()
                .managed(first.artifact().sha256)
                .build(),
        )]),
        at(
            0,
            RegistryError::DuplicateRuntime {
                existing: first.id(),
            },
        ),
    );

    let root = fresh_root();
    let external = RuntimeBuilder::new().external(&root);
    suite.commit(
        "store an external runtime",
        vec![WriteOp::InsertRuntime(external.clone().build())],
    );
    suite.rejected(
        "one external folder holds one runtime",
        batch(vec![WriteOp::InsertRuntime(
            RuntimeBuilder::new().external(&root).build(),
        )]),
        at(
            0,
            RegistryError::DuplicateRuntime {
                existing: external.id(),
            },
        ),
    );

    let (a, b) = (RuntimeId::generate(), RuntimeId::generate());
    suite.rejected(
        "missing dependencies are reported lowest ID first",
        batch(vec![WriteOp::InsertRuntime(
            RuntimeBuilder::new()
                .external(fresh_root())
                .depends_on(a)
                .depends_on(b)
                .build(),
        )]),
        at(
            0,
            RegistryError::MissingReference {
                resource: ResourceRef::Runtime(a.min(b)),
            },
        ),
    );

    let base = RuntimeBuilder::new().external(fresh_root()).build();
    let dependent = RuntimeBuilder::new()
        .external(fresh_root())
        .depends_on(base.id())
        .depends_on(first.id())
        .build();
    suite.commit(
        "a dependency stored earlier in the same batch counts",
        vec![
            WriteOp::InsertRuntime(base),
            WriteOp::InsertRuntime(dependent.clone()),
        ],
    );
    expect_eq(
        "a runtime reads back with its dependencies",
        suite.value("read the runtime", suite.registry.runtime(dependent.id())),
        Some(dependent),
    );
}

/// Names are unique among instances that are not retained, short keys
/// among all, and renaming keeps the ID, runtime, and stored paths
/// (AC-10, BR-003).
pub(super) fn names_and_short_keys(suite: &Suite<'_>) {
    let stack = suite.stack("store a runtime for names");
    let first = Suite::instance(&stack).build();
    suite.commit(
        "store an instance for names",
        vec![WriteOp::InsertInstance(first.clone())],
    );
    let name = first.name().clone();
    let clash = Suite::instance(&stack).name(name.as_str()).build();
    suite.rejected(
        "a second instance with the same name is refused",
        batch(vec![WriteOp::InsertInstance(clash.clone())]),
        at(
            0,
            RegistryError::NameTaken {
                name: name.clone(),
                existing: first.id(),
            },
        ),
    );

    let mut retained = first.clone();
    domain(
        "retain the instance",
        retained.apply_lifecycle(
            LifecycleEvent::Retain {
                by: OperationId::generate(),
            },
            suite.loaded_at,
        ),
    );
    suite.commit(
        "retain the first instance",
        vec![WriteOp::UpdateInstance {
            instance: retained,
            expected: rev(1),
        }],
    );
    suite.commit(
        "the name of a retained instance can be used again",
        vec![WriteOp::InsertInstance(clash.clone())],
    );
    expect_eq(
        "instance_by_name finds the instance that is not retained",
        suite
            .value("find by name", suite.registry.instance_by_name(&name))
            .map(|instance| instance.id()),
        Some(clash.id()),
    );
    let listed = suite.value(
        "list instances",
        suite.registry.instances(InstanceFilter::default()),
    );
    expect(
        "a list without retained instances skips the retained one",
        listed.iter().any(|i| i.id() == clash.id()) && listed.iter().all(|i| i.id() != first.id()),
        || format!("{:?}", listed.iter().map(|i| i.id()).collect::<Vec<_>>()),
    );
    let all = suite.value(
        "list every instance",
        suite.registry.instances(InstanceFilter {
            include_retained: true,
        }),
    );
    expect(
        "a list with retained instances holds both",
        all.iter().any(|i| i.id() == clash.id()) && all.iter().any(|i| i.id() == first.id()),
        || format!("{:?}", all.iter().map(|i| i.id()).collect::<Vec<_>>()),
    );

    let other = Suite::instance(&stack).build();
    suite.commit(
        "store an instance to rename",
        vec![WriteOp::InsertInstance(other.clone())],
    );
    let mut into_taken = other.clone();
    into_taken.rename(name.clone());
    suite.rejected(
        "renaming to a taken name is refused",
        batch(vec![WriteOp::UpdateInstance {
            instance: into_taken,
            expected: rev(1),
        }]),
        at(
            0,
            RegistryError::NameTaken {
                name,
                existing: clash.id(),
            },
        ),
    );

    let paths = suite.value("read the paths", suite.registry.instance_paths(other.id()));
    let new_name: InstanceName = parse("make a name", &format!("moved-{}", suffix(other.id())));
    let mut renamed = other.clone();
    renamed.rename(new_name.clone());
    suite.commit(
        "renaming an instance applies",
        vec![WriteOp::UpdateInstance {
            instance: renamed,
            expected: rev(1),
        }],
    );
    let after = suite
        .value(
            "read the renamed instance",
            suite.registry.instance(other.id()),
        )
        .unwrap_or_else(|| super::fail("read the renamed instance", "it is gone"));
    expect_eq(
        "renaming_keeps_id_runtime_and_stored_paths: the name changes",
        after.name(),
        &new_name,
    );
    expect_eq(
        "renaming_keeps_id_runtime_and_stored_paths: the runtime stays",
        after.runtime_id(),
        other.runtime_id(),
    );
    expect_eq(
        "renaming_keeps_id_runtime_and_stored_paths: the stored paths stay",
        suite.value(
            "read the paths again",
            suite.registry.instance_paths(other.id()),
        ),
        paths,
    );

    suite.rejected(
        "a short key is unique among all instances, retained ones included",
        batch(vec![WriteOp::InsertInstance(
            Suite::instance(&stack)
                .with_id(same_short_key(first.id()))
                .name(&format!("twin-{}", suffix(clash.id())))
                .build(),
        )]),
        at(
            0,
            RegistryError::ShortKeyTaken {
                key: first.id().short_key(),
                existing: first.id(),
            },
        ),
    );
}

/// An instance runs the software of its runtime and provider revision, and
/// an update keeps its references.
pub(super) fn software(suite: &Suite<'_>) {
    let stack = suite.stack("store a runtime for software");
    let postgresql: SoftwareName = parse("make a software name", "postgresql");
    let mysql: SoftwareName = parse("make a software name", "mysql");
    suite.rejected(
        "an instance runs its runtime's software",
        batch(vec![WriteOp::InsertInstance(
            Suite::instance(&stack).software("mysql").build(),
        )]),
        at(
            0,
            RegistryError::SoftwareMismatch {
                instance: mysql.clone(),
                runtime: postgresql.clone(),
                provider_revision: postgresql.clone(),
            },
        ),
    );

    let other = ProviderRevisionBuilder::new()
        .provider("official/mysql")
        .software("mysql");
    suite.commit(
        "store a provider revision of other software",
        vec![WriteOp::InsertProviderRevision(other.clone().record())],
    );
    suite.rejected(
        "an instance runs its provider revision's software",
        batch(vec![WriteOp::InsertInstance(
            Suite::instance(&stack)
                .provider_revision(other.key())
                .build(),
        )]),
        at(
            0,
            RegistryError::SoftwareMismatch {
                instance: postgresql.clone(),
                runtime: postgresql,
                provider_revision: mysql,
            },
        ),
    );

    let instance = Suite::instance(&stack).build();
    suite.commit(
        "store an instance for software",
        vec![WriteOp::InsertInstance(instance.clone())],
    );
    let unknown = RuntimeId::generate();
    suite.rejected(
        "an instance update needs its runtime",
        batch(vec![WriteOp::UpdateInstance {
            instance: Suite::instance(&stack)
                .with_id(instance.id())
                .runtime_id(unknown)
                .revision(2)
                .build(),
            expected: rev(1),
        }]),
        at(
            0,
            RegistryError::MissingReference {
                resource: ResourceRef::Runtime(unknown),
            },
        ),
    );
}

/// An environment needs its members, and an instance has at most one
/// dedicated owner.
pub(super) fn environments(suite: &Suite<'_>) {
    let stack = suite.stack("store a runtime for environments");
    let a = Suite::instance(&stack).build();
    let b = Suite::instance(&stack).build();
    suite.commit(
        "store instances for environments",
        vec![
            WriteOp::InsertInstance(a.clone()),
            WriteOp::InsertInstance(b.clone()),
        ],
    );
    let missing = InstanceId::generate();
    suite.rejected(
        "an environment needs its members",
        batch(vec![WriteOp::InsertEnvironment(
            EnvironmentBuilder::new().shared(missing).build(),
        )]),
        at(
            0,
            RegistryError::MissingReference {
                resource: ResourceRef::Instance(missing),
            },
        ),
    );

    let owner = EnvironmentBuilder::new().dedicated(a.id()).shared(b.id());
    suite.commit(
        "store an environment that owns an instance",
        vec![WriteOp::InsertEnvironment(owner.clone().build())],
    );
    let owned = RegistryError::DedicatedOwnerExists {
        instance: a.id(),
        environment: owner.id(),
    };
    suite.rejected(
        "an instance has one dedicated owner",
        batch(vec![WriteOp::InsertEnvironment(
            EnvironmentBuilder::new().dedicated(a.id()).build(),
        )]),
        at(0, owned.clone()),
    );
    let sharing = EnvironmentBuilder::new().shared(a.id());
    suite.commit(
        "another environment can share an owned instance",
        vec![WriteOp::InsertEnvironment(sharing.clone().build())],
    );
    let kept = owner.clone().revision(2).build();
    suite.commit(
        "an owner keeps its dedicated members on update",
        vec![WriteOp::UpdateEnvironment {
            environment: kept.clone(),
            expected: rev(1),
        }],
    );
    suite.rejected(
        "an update cannot take an owned instance from its owner",
        batch(vec![WriteOp::UpdateEnvironment {
            environment: sharing.no_members().dedicated(a.id()).revision(2).build(),
            expected: rev(1),
        }]),
        at(0, owned),
    );
    expect_eq(
        "an environment update is what reads back",
        suite.value(
            "read the environment",
            suite.registry.environment(owner.id()),
        ),
        Some(kept),
    );
}

/// A lease needs its instance and environment, and an environment holds at
/// most one active lease per instance.
pub(super) fn leases(suite: &Suite<'_>) {
    let stack = suite.stack("store a runtime for leases");
    let instance = Suite::instance(&stack).build();
    let environment = EnvironmentBuilder::new().shared(instance.id()).build();
    suite.commit(
        "store an instance and environment for leases",
        vec![
            WriteOp::InsertInstance(instance.clone()),
            WriteOp::InsertEnvironment(environment.clone()),
        ],
    );
    let missing = InstanceId::generate();
    suite.rejected(
        "a lease needs its instance",
        batch(vec![WriteOp::InsertLease(
            LeaseBuilder::new(missing).build(),
        )]),
        at(
            0,
            RegistryError::MissingReference {
                resource: ResourceRef::Instance(missing),
            },
        ),
    );
    let unknown = EnvironmentId::generate();
    suite.rejected(
        "an environment lease needs its environment",
        batch(vec![WriteOp::InsertLease(
            LeaseBuilder::new(instance.id())
                .environment(unknown)
                .build(),
        )]),
        at(
            0,
            RegistryError::MissingReference {
                resource: ResourceRef::Environment(unknown),
            },
        ),
    );

    let t = suite.loaded_at;
    let first = LeaseBuilder::new(instance.id())
        .environment(environment.id())
        .acquired_at(t);
    suite.commit(
        "store an environment lease",
        vec![WriteOp::InsertLease(first.clone().build())],
    );
    let second = LeaseBuilder::new(instance.id())
        .environment(environment.id())
        .acquired_at(shifted(t, 1));
    let active = RegistryError::ActiveLeaseExists {
        instance: instance.id(),
        environment: environment.id(),
    };
    suite.rejected(
        "an environment holds one active lease per instance",
        batch(vec![WriteOp::InsertLease(second.clone().build())]),
        at(0, active.clone()),
    );
    let unknown_lease = LeaseBuilder::new(instance.id());
    suite.rejected(
        "an update of a missing lease is NotFound",
        batch(vec![WriteOp::UpdateLease(unknown_lease.clone().build())]),
        at(
            0,
            RegistryError::NotFound {
                resource: ResourceRef::Lease(unknown_lease.id()),
            },
        ),
    );
    let released = first.clone().released(shifted(t, 2)).build();
    let committed = suite.commit(
        "releasing a lease applies",
        vec![WriteOp::UpdateLease(released.clone())],
    );
    expect_eq(
        "a lease op returns no revision",
        committed.revisions,
        Vec::new(),
    );
    suite.commit(
        "a new active lease is allowed once the old one is released",
        vec![WriteOp::InsertLease(second.clone().build())],
    );
    suite.rejected(
        "an update cannot make a second active lease",
        batch(vec![WriteOp::UpdateLease(first.active().build())]),
        at(0, active),
    );
    let manual = LeaseBuilder::new(instance.id())
        .manual(Some("session-1"))
        .acquired_at(shifted(t, 3))
        .build();
    suite.commit(
        "a manual lease needs no environment",
        vec![WriteOp::InsertLease(manual.clone())],
    );
    let second = second.build();
    expect_eq(
        "leases list by acquired_at",
        suite.value(
            "list the leases",
            suite.registry.leases(instance.id(), LeaseFilter::All),
        ),
        vec![released, second.clone(), manual.clone()],
    );
    expect_eq(
        "unreleased leases skip released ones",
        suite.value(
            "list the unreleased leases",
            suite
                .registry
                .leases(instance.id(), LeaseFilter::Unreleased),
        ),
        vec![second, manual],
    );
    expect_eq(
        "an instance without leases lists none",
        suite.value(
            "list the leases of an unknown instance",
            suite
                .registry
                .leases(InstanceId::generate(), LeaseFilter::All),
        ),
        Vec::new(),
    );
}
