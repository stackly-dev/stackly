//! The queries of spec 0005 step 12: known instance IDs, runtime
//! references, stored instance paths, instance lists, and paged operations
//! (AC-10, AC-11).

use std::collections::{BTreeSet, HashMap};

use jiff::{SignedDuration, Timestamp};
use stackly_domain::{
    InstallMode, Instance, InstanceFilter, InstanceId, InstanceLifecycle, InstancePathKind,
    LogicalRoot, NewRuntime, Operation, OperationCursor, OperationEvent, OperationId,
    OperationQuery, OperationResource, Registry, RegistryError, RelativePath, ResourceRef, Runtime,
    RuntimeId, WriteOp,
};

use stackly_testkit::evidence::{Kind, evidence};

use crate::common::{Harness, as_new, retained, stored};

#[test]
fn known_instance_ids_list_every_instance_in_id_order() {
    let harness = Harness::new();
    let base = harness.base();
    let first = harness.instance(&base, "beta");
    let second = harness.instance(&base, "alpha");
    harness.commit(vec![
        WriteOp::InsertInstance(second.clone()),
        WriteOp::InsertInstance(first.clone()),
    ]);
    harness.commit(vec![WriteOp::UpdateInstance {
        instance: retained(&first, harness.now()),
        expected: first.revision(),
    }]);

    let mut expected = vec![first.id(), second.id()];
    expected.sort();
    assert_eq!(
        harness.registry.known_instance_ids().unwrap().value,
        expected
    );
    let active = harness
        .registry
        .instances(InstanceFilter::default())
        .unwrap()
        .value;
    assert_eq!(
        active.iter().map(Instance::id).collect::<Vec<_>>(),
        [second.id()]
    );
    let all = harness
        .registry
        .instances(InstanceFilter {
            include_retained: true,
        })
        .unwrap()
        .value;
    assert_eq!(
        all.iter().map(|i| i.name().as_str()).collect::<Vec<_>>(),
        ["alpha", "beta"]
    );
    assert_eq!(
        harness
            .registry
            .instance_by_name(&"beta".parse().unwrap())
            .unwrap()
            .value,
        None
    );
}

#[test]
fn runtime_references_include_stopped_and_retained_instances() {
    let ev = evidence!(
        covers = ["BR-007"],
        kind = Kind::Fixture,
        expected = "runtime_references on the SQLite registry lists stopped and retained \
                    instances, dependent runtimes, unfinished operations, and backups of the \
                    runtime, and a raw DELETE of the runtime is refused by a foreign key",
    );
    let harness = Harness::new();
    let base = harness.base();
    let at = harness.now();
    let mut stopped = stored(&harness.instance(&base, "stopped"));
    stopped.lifecycle = InstanceLifecycle::Stopped;
    stopped.data_initialized_at = Some(at);
    let stopped = Instance::restore(stopped, at).unwrap();
    let kept = harness.instance(&base, "kept");
    let dependent = Runtime::new(NewRuntime {
        id: RuntimeId::generate(),
        software: "postgis".parse().unwrap(),
        version: "3.5.0".parse().unwrap(),
        line: "3".parse().unwrap(),
        target: base.artifact.target,
        install: InstallMode::External {
            root: "/opt/postgis".into(),
            observed_sha256: None,
        },
        depends_on: BTreeSet::from([base.runtime.id()]),
    });
    let running = {
        let mut operation = harness.operation(OperationResource::Runtime(base.runtime.id()));
        operation.apply(OperationEvent::Start, at).unwrap();
        operation
    };
    let finished = {
        let mut operation = harness.operation(OperationResource::Runtime(base.runtime.id()));
        operation.apply(OperationEvent::Start, at).unwrap();
        operation.apply(OperationEvent::Succeed, at).unwrap();
        operation
    };
    let elsewhere = harness.operation(OperationResource::Manager);
    let insert_running = as_new(&running);
    let insert_finished = as_new(&finished);
    harness.commit(vec![
        WriteOp::InsertInstance(stopped.clone()),
        WriteOp::InsertInstance(kept.clone()),
        WriteOp::InsertRuntime(dependent.clone()),
        WriteOp::InsertOperation(insert_running.clone()),
        WriteOp::InsertOperation(insert_finished),
        WriteOp::InsertOperation(elsewhere),
    ]);
    harness.commit(vec![WriteOp::UpdateInstance {
        instance: retained(&kept, at),
        expected: kept.revision(),
    }]);
    let backup = "0199f000-0000-7000-8000-0000000000b1";
    harness
        .side()
        .execute(
            "INSERT INTO backups (id, instance_id, runtime_id, runtime_identity, method, scope, \
             consistency, path, created_at) VALUES (?1, ?2, ?3, 'postgresql 17.6 macos-aarch64', \
             'cold', 'instance', 'stopped', '/Volumes/backup/main', ?4)",
            (
                backup,
                kept.id().to_string(),
                base.runtime.id().to_string(),
                "2026-01-01T00:00:00.000000000Z",
            ),
        )
        .unwrap();

    let references = harness
        .registry
        .runtime_references(base.runtime.id())
        .unwrap()
        .value;

    let mut instances = vec![stopped.id(), kept.id()];
    instances.sort();
    assert_eq!(references.instances, instances);
    assert_eq!(references.dependent_runtimes, [dependent.id()]);
    assert_eq!(references.active_operations, [insert_running.id()]);
    assert_eq!(references.recovery_snapshots, [backup.parse().unwrap()]);
    let unknown = RuntimeId::generate();
    assert_eq!(
        harness.registry.runtime_references(unknown).unwrap_err(),
        RegistryError::NotFound {
            resource: ResourceRef::Runtime(unknown),
        }
    );
    let err = harness
        .side()
        .execute(
            "DELETE FROM runtimes WHERE id = ?1",
            [base.runtime.id().to_string()],
        )
        .unwrap_err();
    assert!(err.to_string().contains("FOREIGN KEY"), "{err}");
    ev.pass();
}

#[test]
fn renaming_keeps_id_runtime_and_stored_paths() {
    let ev = evidence!(
        covers = ["BR-003"],
        kind = Kind::Fixture,
        expected = "renaming an instance in the SQLite registry changes only its name, revision, \
                    and updated_at; its ID, runtime, creation, and seven stored paths stay, and \
                    the paths still resolve to the layout",
    );
    let harness = Harness::new();
    let base = harness.base();
    let instance = harness.instance(&base, "main");
    harness.commit(vec![WriteOp::InsertInstance(instance.clone())]);
    let before = harness
        .registry
        .instance_paths(instance.id())
        .unwrap()
        .value
        .unwrap();
    let mut renamed = instance.clone();
    renamed.rename("primary".parse().unwrap());
    harness.commit(vec![WriteOp::UpdateInstance {
        instance: renamed,
        expected: instance.revision(),
    }]);

    let after = harness
        .registry
        .instance_paths(instance.id())
        .unwrap()
        .value
        .unwrap();
    let read = harness
        .registry
        .instance(instance.id())
        .unwrap()
        .value
        .unwrap();
    let roots = harness.layout.roots();
    let resolved: Vec<_> = after
        .iter()
        .map(|entry| roots.resolve(entry.root(), entry.path()))
        .collect();
    let layout = harness.layout.instance_paths(instance.id());

    assert_eq!(before, after);
    assert_eq!(
        after.iter().map(|entry| entry.kind()).collect::<Vec<_>>(),
        InstancePathKind::ALL
    );
    assert_eq!(
        resolved,
        [
            layout.root,
            layout.data,
            layout.config,
            layout.config_history,
            layout.temp,
            layout.logs,
            layout.run,
        ]
    );
    assert_eq!(read.name().as_str(), "primary");
    assert_eq!(
        (read.id(), read.runtime_id()),
        (instance.id(), instance.runtime_id())
    );
    assert_eq!(
        harness
            .registry
            .instance_paths(InstanceId::generate())
            .unwrap()
            .value,
        None
    );
    let id = instance.id().to_string();
    assert_eq!(
        harness.raw(
            "SELECT created_at || ' ' || created_by_operation || ' ' || \
             coalesce(config_revision_id, '-') FROM instances WHERE id = ?1",
            &id
        ),
        Some(format!(
            "2026-01-01T00:00:00.000000000Z {} -",
            instance.created_by()
        ))
    );
    ev.pass();
}

#[test]
fn stored_paths_resolve_to_the_layout_and_never_repeat() {
    let ev = evidence!(
        covers = ["BR-002"],
        kind = Kind::Fixture,
        expected = "for 1000 instances in the SQLite registry, every stored path resolves to the \
                    layout path of its instance, and no stored path equals or lies inside a path \
                    of another instance under the same root",
    );
    let harness = Harness::new();
    let base = harness.base();
    let instances: Vec<Instance> = (0..1000)
        .map(|index| harness.instance(&base, &format!("i{index}")))
        .collect();
    harness.commit(
        instances
            .iter()
            .cloned()
            .map(WriteOp::InsertInstance)
            .collect(),
    );

    let roots = harness.layout.roots();
    let mut owners: HashMap<(LogicalRoot, RelativePath), InstanceId> = HashMap::new();
    for instance in &instances {
        let entries = harness
            .registry
            .instance_paths(instance.id())
            .unwrap()
            .value
            .expect("a stored instance has stored paths");
        let layout = harness.layout.instance_paths(instance.id());
        let expected = [
            layout.root,
            layout.data,
            layout.config,
            layout.config_history,
            layout.temp,
            layout.logs,
            layout.run,
        ];
        let resolved: Vec<_> = entries
            .iter()
            .map(|entry| roots.resolve(entry.root(), entry.path()))
            .collect();
        assert_eq!(resolved, expected, "{}", instance.id());
        for entry in entries {
            let key = (entry.root(), entry.path().clone());
            assert!(
                owners.insert(key, instance.id()).is_none(),
                "a stored path repeats: {}:{}",
                entry.root(),
                entry.path()
            );
        }
    }
    for ((root, path), owner) in &owners {
        let parts: Vec<&str> = path.components().collect();
        for depth in 1..=parts.len() {
            let ancestor: RelativePath = parts[..depth].join("/").parse().unwrap();
            if let Some(other) = owners.get(&(*root, ancestor.clone()))
                && other != owner
            {
                panic!("{root}:{path} of {owner} lies inside {root}:{ancestor} of {other}");
            }
        }
    }
    assert_eq!(owners.len(), 7000);
    ev.pass();
}

#[test]
fn operations_page_newest_first_and_filter_by_state_and_resource() {
    let harness = Harness::new();
    let base = harness.base();
    let start = harness.now();
    let mut operations = Vec::new();
    for index in 0..7_i64 {
        // Pairs share a creation time, so the ID breaks ties.
        harness.clock().set(
            start
                .checked_add(SignedDuration::from_secs(index / 2))
                .unwrap(),
        );
        let resource = if index % 3 == 0 {
            OperationResource::Runtime(base.runtime.id())
        } else {
            OperationResource::Manager
        };
        operations.push(harness.operation(resource));
    }
    let mut finished = operations[6].clone();
    let at = harness.now();
    finished.apply(OperationEvent::Start, at).unwrap();
    finished.apply(OperationEvent::Succeed, at).unwrap();
    operations[6] = as_new(&finished);
    harness.commit(
        operations
            .iter()
            .cloned()
            .map(WriteOp::InsertOperation)
            .collect(),
    );
    let mut newest_first = operations.clone();
    newest_first.sort_by_key(|operation| std::cmp::Reverse(OperationCursor::of(operation)));
    let query = |limit, before| OperationQuery {
        active_only: false,
        resource: None,
        limit,
        before,
    };

    let mut pages = Vec::new();
    let mut before = None;
    loop {
        let page = harness.registry.operations(query(3, before)).unwrap().value;
        pages.push(page.items.iter().map(Operation::id).collect::<Vec<_>>());
        match page.next {
            Some(next) => before = Some(next),
            None => break,
        }
    }
    let ids = |list: &[Operation]| list.iter().map(Operation::id).collect::<Vec<OperationId>>();
    assert_eq!(pages.len(), 3);
    assert_eq!(pages.concat(), ids(&newest_first));
    assert_eq!(pages[2].len(), 1);

    let active = harness
        .registry
        .operations(OperationQuery {
            active_only: true,
            ..query(500, None)
        })
        .unwrap()
        .value;
    assert_eq!(active.items.len(), 6);
    assert_eq!(active.next, None);
    let on_runtime = harness
        .registry
        .operations(OperationQuery {
            resource: Some(OperationResource::Runtime(base.runtime.id())),
            ..query(500, None)
        })
        .unwrap()
        .value;
    let expected: Vec<_> = newest_first
        .iter()
        .filter(|operation| operation.resource() == OperationResource::Runtime(base.runtime.id()))
        .cloned()
        .collect();
    assert_eq!(on_runtime.items, expected);
    let manager = harness
        .registry
        .operations(OperationQuery {
            resource: Some(OperationResource::Manager),
            ..query(500, None)
        })
        .unwrap()
        .value;
    assert_eq!(manager.items.len(), 4);
    for limit in [0, 501] {
        assert!(matches!(
            harness.registry.operations(query(limit, None)),
            Err(RegistryError::InvalidQuery { .. })
        ));
    }
    let past = OperationCursor {
        created_at: Timestamp::constant(0, 0),
        id: OperationId::generate(),
    };
    let empty = harness.registry.operations(query(10, Some(past))).unwrap();
    assert!(empty.value.items.is_empty());
}
