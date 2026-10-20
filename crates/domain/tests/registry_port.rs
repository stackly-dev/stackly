//! The domain side of the registry port (spec 0005 step 8): revisions on
//! every aggregate the registry updates, `restore` constructors that
//! rebuild what the registry stored, the shared shape checks of a write,
//! and the port's own values.

use std::collections::BTreeSet;

use jiff::{SignedDuration, Timestamp};
use stackly_domain::{
    BatchError, ConfigRevisionId, ConfigRevisionRecord, ConfigValuesJson, Endpoint, Environment,
    EnvironmentId, EnvironmentMember, Generation, HealthState, InstallMode, Instance, InstanceId,
    InstanceLifecycle, InvariantDetail, Lease, LeaseHolder, LeaseId, LeaseState, LifecycleEvent,
    LineCompatibility, NewInstance, NewOperation, NewRuntime, Operation, OperationCursor,
    OperationEvent, OperationFailure, OperationId, OperationKind, OperationQuery,
    OperationResource, OperationState, Ownership, Pid, ProcessIdentity, ProcessObservation,
    ProcessStart, Progress, ProviderRevision, ProviderRevisionKey, ProviderRevisionRecord,
    PruneReport, ReadinessState, ReasonCode, RecoveryHint, Registry, RegistryError, ResourceRef,
    ResourceRevision, RetentionPolicy, Runtime, RuntimeEvent, RuntimeId, RuntimeReferences,
    RuntimeState, StartupPolicy, StoredEnvironment, StoredInstance, StoredLease, StoredOperation,
    StoredRuntime, UnixEndpoint, Versioned, WriteBatch, WriteOp,
};

/// Returns `2026-01-01T00:00:00Z` plus `seconds`.
fn at(seconds: i64) -> Timestamp {
    let start: Timestamp = "2026-01-01T00:00:00Z"
        .parse()
        .expect("the start time is valid RFC 3339");
    start + SignedDuration::from_secs(seconds)
}

fn provider_key() -> ProviderRevisionKey {
    ProviderRevisionKey::new(
        "official/postgresql"
            .parse()
            .expect("official/postgresql is a provider name"),
        "2026.10.1".parse().expect("2026.10.1 is a revision label"),
        "ab".repeat(32).parse().expect("64 hex characters"),
    )
}

fn runtime(install: InstallMode) -> Runtime {
    Runtime::new(NewRuntime {
        id: RuntimeId::generate(),
        software: "postgresql".parse().expect("postgresql is a software name"),
        version: "17.6".parse().expect("17.6 is an exact version"),
        line: "17".parse().expect("17 is a runtime line"),
        target: "macos-aarch64".parse().expect("macos-aarch64 is a target"),
        install,
        depends_on: BTreeSet::from([RuntimeId::generate()]),
    })
}

fn managed_runtime() -> Runtime {
    runtime(InstallMode::Managed {
        artifact_sha256: "cd".repeat(32).parse().expect("64 hex characters"),
    })
}

fn installed_runtime() -> Runtime {
    let mut runtime = managed_runtime();
    for (seconds, event) in (1..).zip([
        RuntimeEvent::Resolved,
        RuntimeEvent::Downloaded,
        RuntimeEvent::Verified,
        RuntimeEvent::Committed,
    ]) {
        runtime
            .apply(event, at(seconds))
            .expect("the install path follows the runtime table");
    }
    runtime
}

fn instance() -> Instance {
    Instance::new(NewInstance {
        id: InstanceId::generate(),
        name: "orders-db".parse().expect("orders-db is an instance name"),
        display_name: Some("Orders".parse().expect("Orders is a display name")),
        provider_revision: provider_key(),
        software: "postgresql".parse().expect("postgresql is a software name"),
        runtime_id: RuntimeId::generate(),
        data_line: "17".parse().expect("17 is a runtime line"),
        data_version: "17.6".parse().expect("17.6 is an exact version"),
        endpoints: vec![Endpoint::Unix(UnixEndpoint::new(
            "socket".parse().expect("socket is an endpoint name"),
            ".s.PGSQL.5432".parse().expect("a socket name"),
        ))],
        startup: StartupPolicy::AtLogin,
        created_at: at(0),
        created_by: OperationId::generate(),
    })
}

fn operation() -> Operation {
    Operation::new(NewOperation {
        id: OperationId::generate(),
        kind: OperationKind::InstallRuntime,
        resource: OperationResource::Runtime(RuntimeId::generate()),
        idempotency_key: Some("req-1".parse().expect("req-1 is an idempotency key")),
        created_at: at(0),
    })
}

fn environment() -> Environment {
    Environment::new(
        EnvironmentId::generate(),
        "shop".parse().expect("shop is an environment name"),
        None,
        vec![EnvironmentMember::new(
            "db".parse().expect("db is a member alias"),
            InstanceId::generate(),
            Ownership::Dedicated,
        )],
    )
    .expect("one member is a valid environment")
}

fn stored_runtime(runtime: &Runtime) -> StoredRuntime {
    StoredRuntime {
        id: runtime.id(),
        software: runtime.software().clone(),
        version: runtime.version().clone(),
        line: runtime.line().clone(),
        target: runtime.target(),
        install: runtime.install().clone(),
        state: runtime.state(),
        installed_at: runtime.installed_at(),
        last_verified_at: runtime.last_verified_at(),
        depends_on: runtime.depends_on().clone(),
        revision: runtime.revision(),
    }
}

fn stored_instance(instance: &Instance) -> StoredInstance {
    StoredInstance {
        id: instance.id(),
        name: instance.name().clone(),
        display_name: instance.display_name().cloned(),
        provider_revision: instance.provider_revision().clone(),
        software: instance.software().clone(),
        runtime_id: instance.runtime_id(),
        data_line: instance.data_line().clone(),
        data_version: instance.data_version().clone(),
        data_initialized_at: instance.data_initialized_at(),
        endpoints: instance.endpoints().to_vec(),
        desired: instance.desired(),
        startup: instance.startup(),
        lifecycle: instance.lifecycle(),
        last_failure: instance.last_failure().cloned(),
        revision: instance.revision(),
        created_at: instance.created_at(),
        created_by: instance.created_by(),
        retained_at: instance.retained_at(),
        retained_by: instance.retained_by(),
    }
}

fn stored_operation(operation: &Operation) -> StoredOperation {
    StoredOperation {
        id: operation.id(),
        kind: operation.kind(),
        resource: operation.resource(),
        idempotency_key: operation.idempotency_key().cloned(),
        state: operation.state(),
        started_at: operation.started_at(),
        stage: operation.stage().cloned(),
        progress: operation.progress(),
        failure: operation.failure().cloned(),
        recovery_hint: operation.recovery_hint(),
        created_at: operation.created_at(),
        updated_at: operation.updated_at(),
        revision: operation.revision(),
    }
}

fn stored_lease(lease: &Lease) -> StoredLease {
    StoredLease {
        id: lease.id(),
        instance_id: lease.instance_id(),
        holder: lease.holder(),
        session: lease.session().cloned(),
        state: lease.state(),
        acquired_at: lease.acquired_at(),
    }
}

fn stored_environment(environment: &Environment) -> StoredEnvironment {
    StoredEnvironment {
        id: environment.id(),
        name: environment.name().clone(),
        manifest_sha256: environment.manifest_sha256().copied(),
        members: environment.members().to_vec(),
        revision: environment.revision(),
    }
}

fn revision(number: u64) -> ResourceRevision {
    ResourceRevision::new(number).expect("revisions start at 1")
}

fn inconsistent_reason(err: &stackly_domain::InvariantError) -> &'static str {
    match err.detail() {
        InvariantDetail::InconsistentRecord { reason, .. } => reason,
        other => panic!("expected an inconsistent record, got {other:?}"),
    }
}

#[test]
fn runtime_mutations_raise_the_revision_by_exactly_one_and_refusals_do_not() {
    let mut runtime = managed_runtime();
    assert_eq!(runtime.revision(), ResourceRevision::INITIAL);

    runtime.apply(RuntimeEvent::Resolved, at(1)).unwrap();
    assert_eq!(runtime.revision(), revision(2));
    runtime.apply(RuntimeEvent::Committed, at(2)).unwrap_err();
    assert_eq!(runtime.revision(), revision(2));

    let mut installed = installed_runtime();
    let before = installed.revision();
    installed.remove(&RuntimeReferences::default()).unwrap();
    assert_eq!(installed.revision(), before.next());
    assert_eq!(installed.state(), RuntimeState::Absent);
}

#[test]
fn operation_changes_move_updated_at_and_only_touch_raises_the_revision() {
    let mut operation = operation();
    let created = operation.updated_at();
    assert_eq!(operation.revision(), ResourceRevision::INITIAL);

    operation
        .set_progress(Progress::Counted { done: 1, total: 2 }, at(1))
        .unwrap_err();
    assert_eq!(operation.updated_at(), created);
    operation.apply(OperationEvent::Start, at(2)).unwrap();
    operation
        .set_progress(Progress::Counted { done: 1, total: 2 }, at(3))
        .unwrap();
    operation
        .set_stage("downloading".parse().unwrap(), at(4))
        .unwrap();
    assert_eq!(
        (operation.revision(), operation.updated_at()),
        (ResourceRevision::INITIAL, at(4))
    );

    operation.touch(at(5));

    assert_eq!(
        (operation.revision(), operation.updated_at()),
        (revision(2), at(5))
    );
}

#[test]
fn a_new_environment_starts_at_revision_one() {
    assert_eq!(environment().revision(), ResourceRevision::INITIAL);
}

#[test]
fn restored_runtimes_operations_leases_and_environments_equal_the_originals() {
    let mut external = runtime(InstallMode::External {
        root: "/opt/homebrew/opt/postgresql@17".into(),
        observed_sha256: Some("ef".repeat(32).parse().unwrap()),
    });
    external.apply(RuntimeEvent::Resolved, at(1)).unwrap();
    for runtime in [managed_runtime(), installed_runtime(), external] {
        assert_eq!(Runtime::restore(stored_runtime(&runtime)).unwrap(), runtime);
    }

    let mut failed = operation();
    failed.apply(OperationEvent::Start, at(1)).unwrap();
    failed
        .set_progress(Progress::Counted { done: 3, total: 5 }, at(2))
        .unwrap();
    failed
        .apply(
            OperationEvent::Fail(OperationFailure::new(
                "checksum_mismatch".parse().unwrap(),
                true,
            )),
            at(3),
        )
        .unwrap();
    assert_eq!(failed.state(), OperationState::Failed);
    for operation in [operation(), failed] {
        assert_eq!(
            Operation::restore(stored_operation(&operation)).unwrap(),
            operation
        );
    }

    let instance = InstanceId::generate();
    let mut released = Lease::new(
        LeaseId::generate(),
        instance,
        LeaseHolder::Manual,
        None,
        at(0),
    )
    .unwrap();
    released.release(at(5)).unwrap();
    let mut suppressed = Lease::new(
        LeaseId::generate(),
        instance,
        LeaseHolder::Startup,
        Some("login-1".parse().unwrap()),
        at(0),
    )
    .unwrap();
    suppressed.suppress(at(6)).unwrap();
    let environment_lease = Lease::new(
        LeaseId::generate(),
        instance,
        LeaseHolder::Environment(EnvironmentId::generate()),
        None,
        at(0),
    )
    .unwrap();
    for lease in [released, suppressed, environment_lease] {
        assert_eq!(Lease::restore(stored_lease(&lease)).unwrap(), lease);
    }

    let environment = environment();
    let mut stored = stored_environment(&environment);
    assert_eq!(Environment::restore(stored.clone()).unwrap(), environment);
    stored.revision = revision(7);
    assert_eq!(
        Environment::restore(stored).unwrap().revision(),
        revision(7)
    );

    let provider = ProviderRevision::new(
        provider_key(),
        "postgresql".parse().unwrap(),
        BTreeSet::new().into_iter().collect(),
        LineCompatibility::default().allow("16".parse().unwrap(), "17".parse().unwrap()),
    );
    let restored = ProviderRevision::restore(
        provider.key().clone(),
        provider.software().clone(),
        provider.capabilities().clone(),
        provider.line_compatibility().clone(),
    );
    assert_eq!(restored, provider);
}

#[test]
fn a_restored_instance_keeps_every_stored_field_and_reads_unknown_since_the_restart() {
    let mut original = instance();
    let by = OperationId::generate();
    original
        .apply_lifecycle(LifecycleEvent::Retain { by }, at(9))
        .unwrap();
    let loaded_at = at(3600);

    let restored = Instance::restore(stored_instance(&original), loaded_at).unwrap();

    assert_eq!(stored_instance(&restored), stored_instance(&original));
    assert_eq!(restored.retained_by(), Some(by));
    assert_eq!(restored.lifecycle(), InstanceLifecycle::Retained);
    assert_eq!(restored.readiness().state(), ReadinessState::Unknown);
    assert_eq!(restored.health().state(), HealthState::Unknown);
    for (reason, observed_at) in [
        (
            restored.readiness().reason(),
            restored.readiness().observed_at(),
        ),
        (restored.health().reason(), restored.health().observed_at()),
    ] {
        assert_eq!(reason, &ReasonCode::DAEMON_RESTARTED);
        assert_eq!(observed_at, loaded_at);
    }
}

#[test]
fn restore_rejects_an_instance_whose_fields_disagree() {
    let base = stored_instance(&instance());
    let cases = [
        (
            StoredInstance {
                lifecycle: InstanceLifecycle::Retained,
                ..base.clone()
            },
            "retained_at is set exactly when it is retained",
        ),
        (
            StoredInstance {
                lifecycle: InstanceLifecycle::Retained,
                retained_at: Some(at(1)),
                ..base.clone()
            },
            "retained_at and retained_by are set together",
        ),
        (
            StoredInstance {
                retained_by: Some(OperationId::generate()),
                ..base.clone()
            },
            "retained_at and retained_by are set together",
        ),
        (
            StoredInstance {
                lifecycle: InstanceLifecycle::Running,
                ..base.clone()
            },
            "its lifecycle needs initialized data (BR-008)",
        ),
    ];

    for (stored, expected) in cases {
        let err = Instance::restore(stored, at(0)).unwrap_err();
        assert_eq!(inconsistent_reason(&err), expected);
        assert_eq!(err.rule(), None);
    }
    let initialized = StoredInstance {
        lifecycle: InstanceLifecycle::Running,
        data_initialized_at: Some(at(1)),
        ..base
    };
    assert!(Instance::restore(initialized, at(0)).is_ok());
}

#[test]
fn restore_rejects_runtimes_operations_and_leases_that_break_an_invariant() {
    let runtime = managed_runtime();
    let self_dependent = StoredRuntime {
        depends_on: BTreeSet::from([runtime.id()]),
        ..stored_runtime(&runtime)
    };
    let err = Runtime::restore(self_dependent).unwrap_err();
    assert_eq!(inconsistent_reason(&err), "it depends on itself");
    for state in [
        RuntimeState::Installed,
        RuntimeState::Missing,
        RuntimeState::Drifted,
    ] {
        let never_committed = StoredRuntime {
            state,
            ..stored_runtime(&runtime)
        };
        let err = Runtime::restore(never_committed).unwrap_err();
        assert_eq!(
            inconsistent_reason(&err),
            "an installed runtime has no installed_at"
        );
    }

    let base = stored_operation(&operation());
    let err = Operation::restore(StoredOperation {
        state: OperationState::Failed,
        ..base.clone()
    })
    .unwrap_err();
    assert_eq!(
        inconsistent_reason(&err),
        "a failed operation has no failure"
    );
    let err = Operation::restore(StoredOperation {
        recovery_hint: Some(RecoveryHint::Resume),
        ..base.clone()
    })
    .unwrap_err();
    assert_eq!(
        inconsistent_reason(&err),
        "a recovery hint is set outside needs_recovery"
    );
    let err = Operation::restore(StoredOperation {
        progress: Progress::Counted { done: 6, total: 5 },
        ..base
    })
    .unwrap_err();
    assert_eq!(inconsistent_reason(&err), "progress is past its total");

    let lease = StoredLease {
        id: LeaseId::generate(),
        instance_id: InstanceId::generate(),
        holder: LeaseHolder::Manual,
        session: None,
        state: LeaseState::Suppressed { at: at(1) },
        acquired_at: at(0),
    };
    let err = Lease::restore(lease.clone()).unwrap_err();
    assert_eq!(
        inconsistent_reason(&err),
        "only a startup lease can be suppressed"
    );
    let err = Lease::restore(StoredLease {
        holder: LeaseHolder::Startup,
        state: LeaseState::Active,
        ..lease
    })
    .unwrap_err();
    assert!(
        matches!(
            err.detail(),
            InvariantDetail::StartupLeaseWithoutSession { .. }
        ),
        "{err}"
    );
}

#[test]
fn restore_rejects_an_environment_with_a_duplicate_member() {
    let environment = environment();
    let mut stored = stored_environment(&environment);
    let member = stored.members[0].clone();
    stored.members.push(EnvironmentMember::new(
        member.alias().clone(),
        InstanceId::generate(),
        Ownership::Shared,
    ));

    let err = Environment::restore(stored).unwrap_err();

    assert!(
        matches!(err.detail(), InvariantDetail::DuplicateMemberAlias { .. }),
        "{err}"
    );
}

#[test]
fn write_shape_checks_the_revision_before_the_path() {
    let fresh = managed_runtime();
    let mut changed = fresh.clone();
    changed.apply(RuntimeEvent::Resolved, at(1)).unwrap();
    changed.apply(RuntimeEvent::Downloaded, at(2)).unwrap();

    WriteOp::InsertRuntime(fresh.clone()).check_shape().unwrap();
    let err = WriteOp::InsertRuntime(changed.clone())
        .check_shape()
        .unwrap_err();
    assert_eq!(
        err,
        RegistryError::InvalidRevision {
            resource: ResourceRef::Runtime(changed.id()),
            expected: revision(1),
            actual: revision(3),
        }
    );
    WriteOp::UpdateRuntime {
        runtime: changed.clone(),
        expected: revision(2),
    }
    .check_shape()
    .unwrap();
    let err = WriteOp::UpdateRuntime {
        runtime: changed.clone(),
        expected: revision(1),
    }
    .check_shape()
    .unwrap_err();
    assert_eq!(
        err,
        RegistryError::InvalidRevision {
            resource: ResourceRef::Runtime(changed.id()),
            expected: revision(2),
            actual: revision(3),
        }
    );

    let mut instance = instance();
    WriteOp::InsertInstance(instance.clone())
        .check_shape()
        .unwrap();
    instance.set_desired(stackly_domain::DesiredState::Running);
    WriteOp::UpdateInstance {
        instance,
        expected: ResourceRevision::INITIAL,
    }
    .check_shape()
    .unwrap();
    WriteOp::InsertOperation(operation()).check_shape().unwrap();
    WriteOp::InsertEnvironment(environment())
        .check_shape()
        .unwrap();
    let lease = Lease::new(
        LeaseId::generate(),
        InstanceId::generate(),
        LeaseHolder::Manual,
        None,
        at(0),
    )
    .unwrap();
    WriteOp::UpdateLease(lease).check_shape().unwrap();
}

#[cfg(unix)]
#[test]
fn an_external_root_that_is_not_utf8_is_refused_after_the_revision_check() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    let root = OsStr::from_bytes(b"/opt/runtime-\xff").into();
    let mut runtime = runtime(InstallMode::External {
        root,
        observed_sha256: None,
    });

    let err = WriteOp::InsertRuntime(runtime.clone())
        .check_shape()
        .unwrap_err();
    assert_eq!(
        err,
        RegistryError::NotUtf8Path {
            field: "external_root"
        }
    );
    runtime.apply(RuntimeEvent::Resolved, at(1)).unwrap();
    let err = WriteOp::InsertRuntime(runtime).check_shape().unwrap_err();
    assert!(
        matches!(err, RegistryError::InvalidRevision { .. }),
        "{err:?}"
    );
}

#[test]
fn write_ops_name_their_resource_revision_and_expected_revision() {
    let runtime = managed_runtime();
    let provider = ProviderRevisionRecord::new(
        ProviderRevision::new(
            provider_key(),
            "postgresql".parse().unwrap(),
            BTreeSet::new().into_iter().collect(),
            LineCompatibility::default(),
        ),
        None,
        None,
        None,
    );
    let insert = WriteOp::InsertRuntime(runtime.clone());
    let update = WriteOp::UpdateRuntime {
        runtime: runtime.clone(),
        expected: revision(4),
    };
    let record = WriteOp::InsertProviderRevision(provider);

    assert_eq!(insert.resource(), ResourceRef::Runtime(runtime.id()));
    assert_eq!(insert.revision(), Some(ResourceRevision::INITIAL));
    assert_eq!(insert.expected(), None);
    assert_eq!(update.expected(), Some(revision(4)));
    assert_eq!(
        record.resource(),
        ResourceRef::ProviderRevision(Box::new(provider_key()))
    );
    assert_eq!(record.revision(), None);
    record.check_shape().unwrap();
}

fn observation(executable: std::path::PathBuf) -> ProcessObservation {
    ProcessObservation {
        instance: InstanceId::generate(),
        identity: ProcessIdentity {
            pid: Pid::new(4242).expect("4242 is a process ID"),
            start: ProcessStart::new(1_700_000_000, 0).expect("0 microseconds is in range"),
            executable,
            session: Pid::new(4242).expect("4242 is a process ID"),
            boot: "6f9619ff-8b86-d011-b42d-00c04fc964ff"
                .parse()
                .expect("a hyphenated UUID is a boot ID"),
        },
        observed_at: at(0),
    }
}

#[test]
fn observation_and_config_revision_ops_carry_no_revision() {
    let observation = observation("/opt/stackly/bin/postgres".into());
    let instance = observation.instance;
    let config = ConfigRevisionRecord::first(
        ConfigRevisionId::generate(),
        instance,
        ConfigValuesJson::new("{}".to_owned()).unwrap(),
        Some(OperationId::generate()),
        at(0),
    );
    let observe = WriteOp::RecordProcessObservation(observation);
    let configure = WriteOp::InsertConfigRevision(config.clone());

    assert_eq!(observe.resource(), ResourceRef::Instance(instance));
    assert_eq!(configure.resource(), ResourceRef::ConfigRevision(config.id));
    for op in [&observe, &configure] {
        assert_eq!((op.revision(), op.expected()), (None, None));
        assert_eq!(op.journal_operation(), None);
        op.check_shape().unwrap();
    }
    assert_eq!(
        RegistryError::DuplicateResource {
            resource: ResourceRef::ConfigRevision(config.id),
        }
        .to_string(),
        format!("config revision {} already exists", config.id)
    );
}

#[cfg(unix)]
#[test]
fn an_executable_that_is_not_utf8_is_refused_by_the_shape_check() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    let executable = OsStr::from_bytes(b"/opt/stackly/bin/postgres-\xff").into();
    let err = WriteOp::RecordProcessObservation(observation(executable))
        .check_shape()
        .unwrap_err();

    assert_eq!(
        err,
        RegistryError::NotUtf8Path {
            field: "executable_identity"
        }
    );
}

#[test]
fn a_batch_carries_its_ops_and_optionally_the_generation_it_read() {
    let batch = WriteBatch::new(vec![WriteOp::InsertOperation(operation())]);
    assert_eq!(batch.expected_generation, None);

    let batch = batch.at_generation(Generation::new(4));

    assert_eq!(batch.expected_generation, Some(Generation::new(4)));
    assert_eq!(batch.ops.len(), 1);
}

#[test]
fn operation_page_limits_are_1_to_500() {
    let query = |limit| OperationQuery {
        active_only: false,
        resource: None,
        limit,
        before: None,
    };

    for limit in [1, 250, 500] {
        query(limit).validate().unwrap();
    }
    for limit in [0, 501, u16::MAX] {
        assert!(
            matches!(
                query(limit).validate(),
                Err(RegistryError::InvalidQuery { .. })
            ),
            "{limit}"
        );
    }
}

#[test]
fn an_operation_cursor_points_at_its_operation_and_orders_by_time_then_id() {
    let first = operation();
    let cursor = OperationCursor::of(&first);
    assert_eq!(cursor.created_at, first.created_at());
    assert_eq!(cursor.id, first.id());

    let later_id = OperationCursor {
        created_at: first.created_at(),
        id: OperationId::generate(),
    };
    let later_time = OperationCursor {
        created_at: at(1),
        id: first.id(),
    };
    assert!(cursor < later_id);
    assert!(later_id < later_time);
}

#[test]
fn generations_start_at_zero_and_rise_by_one() {
    assert_eq!(Generation::ZERO.get(), 0);
    assert_eq!(Generation::ZERO.next(), Generation::new(1));
    assert_eq!(Generation::new(41).next().to_string(), "42");

    let read = Versioned::new(3_u8, Generation::new(9));
    let mapped = read.map(u32::from);
    assert_eq!(mapped, Versioned::new(3_u32, Generation::new(9)));
}

#[test]
fn the_default_retention_keeps_30_days_100k_rows_and_20_observations() {
    let day = std::time::Duration::from_secs(24 * 60 * 60);
    let policy = RetentionPolicy::DEFAULT;

    assert_eq!(policy.operation_max_age, day * 30);
    assert_eq!(policy.audit_max_age, day * 30);
    assert_eq!(policy.released_lease_max_age, day * 30);
    assert_eq!(policy.operation_max_rows, 100_000);
    assert_eq!(policy.audit_max_rows, 100_000);
    assert_eq!(policy.observations_per_instance, 20);
    assert_eq!(RetentionPolicy::default(), policy);

    let mut report = PruneReport::default();
    report.operations = 2;
    report.process_observations = 3;
    assert_eq!(report.total(), 5);
}

#[test]
fn registry_errors_read_as_lowercase_sentences_and_batch_errors_name_the_op() {
    let instance = InstanceId::generate();
    let errors = [
        RegistryError::NotFound {
            resource: ResourceRef::Instance(instance),
        },
        RegistryError::MissingReference {
            resource: ResourceRef::Runtime(RuntimeId::generate()),
        },
        RegistryError::RevisionConflict {
            resource: ResourceRef::ProviderRevision(Box::new(provider_key())),
            expected: revision(1),
            actual: revision(2),
        },
        RegistryError::GenerationConflict {
            expected: Generation::new(1),
            actual: Generation::new(2),
        },
        RegistryError::NameTaken {
            name: "orders-db".parse().unwrap(),
            existing: instance,
        },
        RegistryError::ShortKeyTaken {
            key: instance.short_key(),
            existing: instance,
        },
        RegistryError::PathTaken {
            root: stackly_domain::LogicalRoot::Data,
            path: "instances/x".parse().unwrap(),
            existing: instance,
        },
        RegistryError::NotUtf8Path {
            field: "external_root",
        },
        RegistryError::Corrupt {
            table: "operations",
            column: "kind",
            detail: "unknown code".to_owned(),
        },
        RegistryError::Unavailable,
    ];

    for err in &errors {
        let message = err.to_string();
        assert!(
            message.starts_with(|c: char| c.is_ascii_lowercase()),
            "{message}"
        );
        assert!(!message.ends_with('.'), "{message}");
    }
    assert_eq!(
        errors[2].to_string(),
        format!(
            "provider revision official/postgresql@2026.10.1#{} is at revision 2, not the \
             expected revision 1",
            "ab".repeat(32)
        )
    );
    assert_eq!(
        errors[6].to_string(),
        format!("path data:instances/x is taken by instance {instance}")
    );

    let at_op = BatchError::at(2, RegistryError::Busy);
    let whole = BatchError::batch(RegistryError::NoSpace);
    assert_eq!(
        at_op.to_string(),
        "registry batch op 2 failed: the registry is busy"
    );
    assert_eq!(
        whole.to_string(),
        "registry batch failed: the disk holding the registry is full"
    );
    let source = std::error::Error::source(&whole).map(ToString::to_string);
    assert_eq!(
        source.as_deref(),
        Some("the disk holding the registry is full")
    );
}

#[test]
fn the_registry_port_is_object_safe_send_and_sync() {
    fn assert_send_sync<T: Send + Sync + ?Sized>() {}
    fn takes_a_registry(_registry: Option<&dyn Registry>) {}

    assert_send_sync::<dyn Registry>();
    takes_a_registry(None);
}
