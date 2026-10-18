//! Rows to aggregates and back (spec 0005 step 11): every aggregate and
//! every state code round trips, JSON columns are stored in order, times
//! keep their nanoseconds, a path that is not UTF-8 is refused, and a code
//! the app does not know reads as `Corrupt` (AC-13, AC-15, AC-16).

use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;

use jiff::{SignedDuration, Timestamp};
use stackly_domain::{
    BatchError, DesiredState, Environment, EnvironmentId, EnvironmentMember, FailureCode,
    InstallMode, Instance, InstanceLifecycle, Lease, LeaseFilter, LeaseHolder, LeaseId, LeaseState,
    NewRuntime, Operation, OperationFailure, OperationId, OperationKind, OperationResource,
    OperationState, Ownership, Progress, ReasonCode, Registry, RegistryError, ResourceRevision,
    Runtime, RuntimeId, RuntimeState, StartupPolicy, StoredLease, StoredOperation, StoredRuntime,
    WriteOp,
};

use crate::common::{Harness, restored, stored, stored_operation};

#[test]
fn records_and_aggregates_read_back_as_written() {
    let harness = Harness::new();
    let base = harness.base();
    let instance = harness.instance(&base, "main");
    let environment = Environment::new(
        EnvironmentId::generate(),
        "shop".parse().unwrap(),
        Some(crate::common::digest()),
        vec![EnvironmentMember::new(
            "db".parse().unwrap(),
            instance.id(),
            Ownership::Dedicated,
        )],
    )
    .unwrap();
    let operation = harness.operation(OperationResource::Instance(instance.id()));
    harness.commit(vec![
        WriteOp::InsertInstance(instance.clone()),
        WriteOp::InsertEnvironment(environment.clone()),
        WriteOp::InsertOperation(operation.clone()),
    ]);
    let registry = &harness.registry;

    assert_eq!(
        registry
            .provider_revision(base.provider.revision.key())
            .unwrap()
            .value,
        Some(base.provider.clone())
    );
    assert_eq!(
        registry.artifact(&base.artifact.sha256).unwrap().value,
        Some(base.artifact.clone())
    );
    assert_eq!(
        registry.runtimes().unwrap().value,
        std::slice::from_ref(&base.runtime)
    );
    assert_eq!(
        registry.environment(environment.id()).unwrap().value,
        Some(environment.clone())
    );
    assert_eq!(registry.environments().unwrap().value, [environment]);
    assert_eq!(
        registry.operation(operation.id()).unwrap().value,
        Some(operation)
    );
    let read = registry.instance(instance.id()).unwrap().value.unwrap();
    assert_eq!(read, restored(&instance, harness.summary.opened_at));
    assert_eq!(read.endpoints(), instance.endpoints());
    assert_eq!(read.readiness().reason(), &ReasonCode::DAEMON_RESTARTED);
    assert_eq!(read.health().observed_at(), harness.summary.opened_at);
    assert_eq!(
        registry.artifact(&crate::common::digest()).unwrap().value,
        None
    );
}

#[test]
fn rewriting_an_instance_and_an_environment_keeps_the_new_list_order() {
    let harness = Harness::new();
    let base = harness.base();
    let instance = harness.instance(&base, "main");
    let cache = harness.instance(&base, "cache");
    let member =
        |alias: &str, id, ownership| EnvironmentMember::new(alias.parse().unwrap(), id, ownership);
    let environment = Environment::new(
        EnvironmentId::generate(),
        "shop".parse().unwrap(),
        None,
        vec![
            member("db", instance.id(), Ownership::Dedicated),
            member("cache", cache.id(), Ownership::Shared),
        ],
    )
    .unwrap();
    harness.commit(vec![
        WriteOp::InsertInstance(instance.clone()),
        WriteOp::InsertInstance(cache.clone()),
        WriteOp::InsertEnvironment(environment.clone()),
    ]);
    let mut reordered = stored(&instance);
    reordered.endpoints.reverse();
    reordered.revision = instance.revision().next();
    let reordered = Instance::restore(reordered, harness.now()).unwrap();
    let swapped = Environment::restore(stackly_domain::StoredEnvironment {
        id: environment.id(),
        name: "store".parse().unwrap(),
        manifest_sha256: None,
        members: vec![
            member("cache", cache.id(), Ownership::Dedicated),
            member("db", instance.id(), Ownership::Shared),
        ],
        revision: environment.revision().next(),
    })
    .unwrap();

    harness.commit(vec![
        WriteOp::UpdateInstance {
            instance: reordered.clone(),
            expected: instance.revision(),
        },
        WriteOp::UpdateEnvironment {
            environment: swapped.clone(),
            expected: environment.revision(),
        },
    ]);

    let read = harness
        .registry
        .instance(instance.id())
        .unwrap()
        .value
        .unwrap();
    assert_eq!(read.endpoints(), reordered.endpoints());
    assert_eq!(
        harness
            .registry
            .environment(environment.id())
            .unwrap()
            .value,
        Some(swapped)
    );
}

#[test]
fn every_runtime_state_and_install_mode_round_trips() {
    let harness = Harness::new();
    let base = harness.base();
    let at = Timestamp::constant(1_767_225_600, 123_456_789);
    let mut runtimes = Vec::new();
    for (index, state) in RuntimeState::ALL.iter().enumerate() {
        let observed = (index % 2 == 0).then(crate::common::digest);
        let runtime = Runtime::restore(StoredRuntime {
            id: RuntimeId::generate(),
            software: base.software(),
            version: "17.6".parse().unwrap(),
            line: "17".parse().unwrap(),
            target: base.artifact.target,
            install: InstallMode::External {
                root: PathBuf::from(format!("/opt/postgresql-{index}")),
                observed_sha256: observed,
            },
            state: *state,
            installed_at: Some(at),
            last_verified_at: (index % 3 == 0).then_some(at),
            depends_on: BTreeSet::from([base.runtime.id()]),
            revision: ResourceRevision::INITIAL,
        })
        .unwrap();
        runtimes.push(runtime);
    }
    harness.commit(
        runtimes
            .iter()
            .cloned()
            .map(WriteOp::InsertRuntime)
            .collect(),
    );

    for runtime in &runtimes {
        assert_eq!(
            harness
                .registry
                .runtime(runtime.id())
                .unwrap()
                .value
                .as_ref(),
            Some(runtime),
            "{}",
            runtime.state()
        );
    }
    let mut all = runtimes.clone();
    all.push(base.runtime.clone());
    all.sort_by_key(Runtime::id);
    assert_eq!(harness.registry.runtimes().unwrap().value, all);
}

#[test]
fn every_lifecycle_desired_and_startup_code_round_trips() {
    let harness = Harness::new();
    let base = harness.base();
    let at = Timestamp::constant(1_767_225_600, 999_999_999);
    let mut instances = Vec::new();
    for (index, lifecycle) in InstanceLifecycle::ALL.iter().enumerate() {
        let mut stored = stored(&harness.instance(&base, &format!("i{index}")));
        stored.lifecycle = *lifecycle;
        stored.data_initialized_at = Some(at);
        stored.desired = DesiredState::ALL[index % DesiredState::ALL.len()];
        stored.startup = StartupPolicy::ALL[index % StartupPolicy::ALL.len()];
        stored.last_failure = (index % 2 == 0).then(|| FailureCode::from_static("crashed"));
        stored.display_name = None;
        if *lifecycle == InstanceLifecycle::Retained {
            stored.retained_at = Some(at);
            stored.retained_by = Some(OperationId::generate());
        }
        instances.push(Instance::restore(stored, at).unwrap());
    }
    harness.commit(
        instances
            .iter()
            .cloned()
            .map(WriteOp::InsertInstance)
            .collect(),
    );

    let loaded_at = harness.summary.opened_at;
    for instance in &instances {
        assert_eq!(
            harness.registry.instance(instance.id()).unwrap().value,
            Some(restored(instance, loaded_at)),
            "{}",
            instance.lifecycle()
        );
    }
}

#[test]
fn every_operation_state_kind_and_resource_round_trips() {
    let harness = Harness::new();
    let base = harness.base();
    let created = Timestamp::constant(1_767_225_600, 1);
    let resources = [
        OperationResource::Runtime(base.runtime.id()),
        OperationResource::Instance(stackly_domain::InstanceId::generate()),
        OperationResource::Environment(EnvironmentId::generate()),
        OperationResource::Manager,
    ];
    let mut operations = Vec::new();
    for (index, state) in OperationState::ALL.iter().enumerate() {
        let failed = *state == OperationState::Failed || index % 2 == 1;
        operations.push(
            Operation::restore(StoredOperation {
                id: OperationId::generate(),
                kind: OperationKind::ALL[index],
                resource: resources[index % resources.len()],
                idempotency_key: Some(format!("key-{index}").parse().unwrap()),
                state: *state,
                started_at: Some(created),
                stage: Some("download".parse().unwrap()),
                progress: if index % 2 == 0 {
                    Progress::Counted { done: 3, total: 7 }
                } else {
                    Progress::Indeterminate
                },
                failure: failed.then(|| {
                    OperationFailure::new(FailureCode::from_static("disk_full"), index % 3 == 0)
                }),
                created_at: created,
                updated_at: created.checked_add(SignedDuration::from_nanos(5)).unwrap(),
                revision: ResourceRevision::INITIAL,
            })
            .unwrap(),
        );
    }
    for kind in &OperationKind::ALL[OperationState::ALL.len()..] {
        let mut operation = harness.operation(OperationResource::Manager);
        operation = Operation::restore(StoredOperation {
            kind: *kind,
            ..stored_operation(&operation)
        })
        .unwrap();
        operations.push(operation);
    }
    harness.commit(
        operations
            .iter()
            .cloned()
            .map(WriteOp::InsertOperation)
            .collect(),
    );

    for operation in &operations {
        assert_eq!(
            harness
                .registry
                .operation(operation.id())
                .unwrap()
                .value
                .as_ref(),
            Some(operation),
            "{} {}",
            operation.kind(),
            operation.state()
        );
    }
}

#[test]
fn every_lease_holder_and_state_round_trips() {
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
    let at = |nanos| Timestamp::constant(1_767_225_600, nanos);
    let session = || Some("login-1".parse().unwrap());
    let cases = [
        (
            LeaseHolder::Environment(environment.id()),
            None,
            LeaseState::Active,
        ),
        (
            LeaseHolder::Environment(environment.id()),
            None,
            LeaseState::Released { at: at(7) },
        ),
        (LeaseHolder::Manual, None, LeaseState::Active),
        (
            LeaseHolder::Manual,
            session(),
            LeaseState::Released { at: at(8) },
        ),
        (LeaseHolder::Startup, session(), LeaseState::Active),
        (
            LeaseHolder::Startup,
            session(),
            LeaseState::Suppressed { at: at(9) },
        ),
    ];
    let leases: Vec<Lease> = cases
        .into_iter()
        .enumerate()
        .map(|(index, (holder, session, state))| {
            Lease::restore(StoredLease {
                id: LeaseId::generate(),
                instance_id: instance.id(),
                holder,
                session,
                state,
                acquired_at: at(i32::try_from(index).unwrap()),
            })
            .unwrap()
        })
        .collect();
    harness.commit(leases.iter().cloned().map(WriteOp::InsertLease).collect());

    let all = harness
        .registry
        .leases(instance.id(), LeaseFilter::All)
        .unwrap();
    assert_eq!(all.value, leases);
    let unreleased = harness
        .registry
        .leases(instance.id(), LeaseFilter::Unreleased)
        .unwrap()
        .value;
    assert_eq!(
        unreleased,
        leases
            .iter()
            .filter(|lease| !matches!(lease.state(), LeaseState::Released { .. }))
            .cloned()
            .collect::<Vec<_>>()
    );
    assert!(
        harness
            .registry
            .leases(stackly_domain::InstanceId::generate(), LeaseFilter::All)
            .unwrap()
            .value
            .is_empty()
    );
}

#[test]
fn json_columns_hold_sorted_codes_and_sorted_lines() {
    let harness = Harness::new();
    let base = harness.base();
    let key = base.provider.revision.key();
    let raw = |column: &str| {
        harness
            .raw(
                &format!("SELECT {column} FROM provider_revisions WHERE manifest_sha256 = ?1"),
                &key.manifest_sha256().to_string(),
            )
            .unwrap()
    };

    assert_eq!(raw("capabilities"), r#"["cold_backup","reload","tls"]"#);
    assert_eq!(
        raw("line_compatibility"),
        r#"{"16":["17"],"17":["18","19"]}"#
    );
    assert_eq!(raw("created_at"), "2026-01-01T00:00:00.000000000Z");
}

#[test]
fn times_keep_their_nanoseconds_and_commits_stamp_the_clock() {
    let harness = Harness::new();
    let base = harness.base();
    let instance = harness.instance(&base, "main");
    harness
        .clock()
        .set(Timestamp::constant(1_767_225_600, 123_456_789));
    harness.commit(vec![WriteOp::InsertInstance(instance.clone())]);
    let id = instance.id().to_string();

    assert_eq!(
        harness.raw("SELECT updated_at FROM instances WHERE id = ?1", &id),
        Some("2026-01-01T00:00:00.123456789Z".to_owned())
    );
    assert_eq!(
        harness.raw("SELECT created_at FROM instances WHERE id = ?1", &id),
        Some("2026-01-01T00:00:00.000000000Z".to_owned())
    );
    assert_eq!(
        harness.raw("SELECT ownership FROM instances WHERE id = ?1", &id),
        Some("created".to_owned())
    );
    let mut renamed = instance.clone();
    renamed.rename("primary".parse().unwrap());
    harness.clock().advance(SignedDuration::from_nanos(1));
    harness.commit(vec![WriteOp::UpdateInstance {
        instance: renamed,
        expected: instance.revision(),
    }]);
    assert_eq!(
        harness.raw("SELECT updated_at FROM instances WHERE id = ?1", &id),
        Some("2026-01-01T00:00:00.123456790Z".to_owned())
    );
    let runtime_id = base.runtime.id().to_string();
    assert_eq!(
        harness.raw(
            "SELECT created_at || ' ' || updated_at FROM runtimes WHERE id = ?1",
            &runtime_id
        ),
        Some("2026-01-01T00:00:00.000000000Z 2026-01-01T00:00:00.000000000Z".to_owned())
    );
}

#[test]
fn an_external_root_that_is_not_utf8_is_refused() {
    let harness = Harness::new();
    let base = harness.base();
    let runtime = Runtime::new(NewRuntime {
        id: RuntimeId::generate(),
        software: base.software(),
        version: "17.6".parse().unwrap(),
        line: "17".parse().unwrap(),
        target: base.artifact.target,
        install: InstallMode::External {
            root: PathBuf::from(OsStr::from_bytes(b"/opt/pg\xff")),
            observed_sha256: None,
        },
        depends_on: BTreeSet::new(),
    });

    let err = harness.reject(vec![
        WriteOp::InsertOperation(harness.operation(OperationResource::Manager)),
        WriteOp::InsertRuntime(runtime),
    ]);

    assert_eq!(
        err,
        BatchError::at(
            1,
            RegistryError::NotUtf8Path {
                field: "external_root",
            }
        )
    );
}

#[test]
fn an_unknown_open_set_code_makes_the_whole_read_corrupt() {
    let harness = Harness::new();
    let base = harness.base();
    let operation = harness.operation(OperationResource::Manager);
    harness.commit(vec![WriteOp::InsertOperation(operation.clone())]);
    let side = harness.side();
    side.execute(
        "UPDATE operations SET kind = 'teleport_instance' WHERE id = ?1",
        [operation.id().to_string()],
    )
    .unwrap();
    side.execute("UPDATE provider_revisions SET approval = 'approved'", [])
        .unwrap();

    let err = harness.registry.operation(operation.id()).unwrap_err();
    assert!(
        matches!(
            &err,
            RegistryError::Corrupt { table: "operations", column: "kind", detail }
                if !detail.contains("teleport")
        ),
        "{err:?}"
    );
    let page = harness.registry.operations(stackly_domain::OperationQuery {
        active_only: false,
        resource: None,
        limit: 10,
        before: None,
    });
    assert!(
        matches!(page, Err(RegistryError::Corrupt { .. })),
        "{page:?}"
    );
    let record = harness
        .registry
        .provider_revision(base.provider.revision.key())
        .unwrap()
        .value
        .unwrap();
    assert_eq!(record.approval.as_deref(), Some("approved"));

    // Capabilities are an open set too: an unknown code fails the read.
    harness
        .side()
        .execute_batch(
            "DROP TRIGGER provider_revisions_immutable; \
         UPDATE provider_revisions SET capabilities = '[\"teleport\"]';",
        )
        .unwrap();
    let err = harness
        .registry
        .provider_revision(base.provider.revision.key())
        .unwrap_err();
    assert!(
        matches!(
            err,
            RegistryError::Corrupt {
                table: "provider_revisions",
                column: "capabilities",
                ..
            }
        ),
        "{err:?}"
    );
}

#[test]
fn a_stored_row_that_breaks_a_domain_invariant_reads_as_corrupt() {
    let harness = Harness::new();
    let base = harness.base();
    let operation = harness.operation(OperationResource::Manager);
    harness.commit(vec![WriteOp::InsertOperation(operation.clone())]);
    harness
        .side()
        .execute(
            "UPDATE runtimes SET state = 'installed', installed_at = NULL WHERE id = ?1",
            [base.runtime.id().to_string()],
        )
        .unwrap();

    let err = harness.registry.runtimes().unwrap_err();

    assert!(
        matches!(
            err,
            RegistryError::Corrupt {
                table: "runtimes",
                column: "*",
                ..
            }
        ),
        "{err:?}"
    );
}
