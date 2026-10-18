//! The queries of spec 0005 step 12: known instance IDs, runtime
//! references, stored instance paths, instance lists, and paged operations
//! (AC-10, AC-11).

use std::collections::BTreeSet;

use jiff::{SignedDuration, Timestamp};
use stackly_domain::{
    InstallMode, Instance, InstanceFilter, InstanceId, InstanceLifecycle, InstancePathKind,
    NewRuntime, Operation, OperationCursor, OperationEvent, OperationId, OperationQuery,
    OperationResource, Registry, RegistryError, ResourceRef, Runtime, RuntimeId, WriteOp,
};

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
fn runtime_references_list_instances_dependents_unfinished_operations_and_backups() {
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
}

#[test]
fn stored_paths_are_the_seven_layout_paths_and_survive_a_rename() {
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
