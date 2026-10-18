//! One test per business rule that spec 0004 marks `F4` in the domain, each
//! named as the spec's invariant table names it and each writing a
//! `fixture` evidence record that covers its BR (spec 0002).

use std::collections::BTreeSet;
use std::net::IpAddr;
use std::path::PathBuf;

use jiff::{SignedDuration, Timestamp};
use stackly_domain::{
    AcceptedRevisionChange, Arch, Capability, CapabilitySet, DesiredState, Endpoint, EnvironmentId,
    InstallMode, Instance, InstanceId, InstanceLifecycle, InvariantDetail, IpEndpoint, Lease,
    LeaseHolder, LeaseId, LifecycleEvent, LineCompatibility, NewInstance, NewOperation, NewRuntime,
    Operation, OperationEvent, OperationId, OperationKind, OperationResource, Os, Port,
    PortRequest, ProviderRevisionKey, Readiness, ReadinessState, ReasonCode, RemovalBlocker,
    RemovalPlan, Rule, Runtime, RuntimeEvent, RuntimeId, RuntimeLine, RuntimeReferences,
    RuntimeState, SessionId, StartupPolicy, StopDecision, StopRequest, Target, Transport,
    UnixEndpoint, endpoints_conflict, evaluate_stop, plan_runtime_removal, resolve_port,
    should_autostart,
};
use stackly_evidence::{Kind, evidence};

const DIGEST_A: &str = "cb95f16d56a9ab7b938a03732ddf4ab691346124747fbb811ac6c09928f1fa6b";
const DIGEST_B: &str = "da578821d15e9c0f59eb8e4672962c6f032074e802684576debc6dfc7f86815f";

/// Returns `2026-01-01T00:00:00Z` plus `seconds`.
fn at(seconds: i64) -> Timestamp {
    let start: Timestamp = "2026-01-01T00:00:00Z"
        .parse()
        .expect("the start time is valid RFC 3339");
    start + SignedDuration::from_secs(seconds)
}

fn revision_key(revision: &str) -> ProviderRevisionKey {
    ProviderRevisionKey::new(
        "official/postgresql"
            .parse()
            .expect("official/postgresql is a provider name"),
        revision
            .parse()
            .expect("test revisions are revision labels"),
        DIGEST_A.parse().expect("DIGEST_A is 64 hex characters"),
    )
}

fn line(text: &str) -> RuntimeLine {
    text.parse().expect("test lines are runtime lines")
}

fn instance() -> Instance {
    Instance::new(NewInstance {
        id: InstanceId::generate(),
        name: "orders-db".parse().expect("orders-db is an instance name"),
        display_name: None,
        provider_revision: revision_key("1"),
        software: "postgresql".parse().expect("postgresql is a software name"),
        runtime_id: RuntimeId::generate(),
        data_line: line("17"),
        data_version: "17.6".parse().expect("17.6 is an exact version"),
        endpoints: Vec::new(),
        startup: StartupPolicy::AtLogin,
        created_at: at(0),
        created_by: OperationId::generate(),
    })
}

/// Returns an instance whose data initialized, in `stopped`.
fn stopped_instance() -> Instance {
    let mut instance = instance();
    instance
        .apply_lifecycle(LifecycleEvent::InitBegin, at(1))
        .expect("uninitialized accepts init_begin");
    instance
        .apply_lifecycle(LifecycleEvent::InitSucceeded, at(2))
        .expect("initializing accepts init_succeeded");
    instance
}

fn runtime(install: InstallMode) -> Runtime {
    Runtime::new(NewRuntime {
        id: RuntimeId::generate(),
        software: "postgresql".parse().expect("postgresql is a software name"),
        version: "17.6".parse().expect("17.6 is an exact version"),
        line: line("17"),
        target: Target::new(Os::Macos, Arch::Aarch64),
        install,
        depends_on: BTreeSet::new(),
    })
}

/// Returns a runtime driven to `installed`.
fn installed(install: InstallMode) -> Runtime {
    let mut runtime = runtime(install);
    let events: &[RuntimeEvent] = match runtime.install() {
        InstallMode::Managed { .. } => &[
            RuntimeEvent::Resolved,
            RuntimeEvent::Downloaded,
            RuntimeEvent::Verified,
            RuntimeEvent::Committed,
        ],
        InstallMode::External { .. } => &[
            RuntimeEvent::Resolved,
            RuntimeEvent::Verified,
            RuntimeEvent::Committed,
        ],
    };
    for event in events {
        runtime
            .apply(*event, at(1))
            .expect("the install path follows the runtime table");
    }
    runtime
}

fn managed() -> InstallMode {
    InstallMode::Managed {
        artifact_sha256: DIGEST_B.parse().expect("DIGEST_B is 64 hex characters"),
    }
}

fn external() -> InstallMode {
    InstallMode::External {
        root: PathBuf::from("user-owned/postgresql"),
        observed_sha256: None,
    }
}

fn port(number: u16) -> Port {
    Port::new(number).expect("test ports are not 0")
}

fn ip_endpoint(transport: Transport, address: &str, request: PortRequest) -> IpEndpoint {
    IpEndpoint::new(
        "main".parse().expect("main is an endpoint name"),
        transport,
        address.parse::<IpAddr>().expect("test addresses are valid"),
        request,
        None,
    )
}

fn tcp(address: &str, number: u16) -> Endpoint {
    Endpoint::Ip(ip_endpoint(
        Transport::Tcp,
        address,
        PortRequest::Fixed(port(number)),
    ))
}

fn lease(instance: InstanceId, holder: LeaseHolder, session: Option<&str>) -> Lease {
    Lease::new(
        LeaseId::generate(),
        instance,
        holder,
        session.map(|text| text.parse().expect("test sessions are session IDs")),
        at(0),
    )
    .expect("test leases pair holders and sessions correctly")
}

fn migrate_data(instance: InstanceId, kind: OperationKind, running: bool) -> Operation {
    let mut operation = Operation::new(NewOperation {
        id: OperationId::generate(),
        kind,
        resource: OperationResource::Instance(instance),
        idempotency_key: None,
        created_at: at(0),
    });
    if running {
        operation
            .apply(OperationEvent::Start, at(1))
            .expect("queued accepts start");
    }
    operation
}

#[test]
fn instance_keeps_its_exact_runtime_and_revision_until_rebind() {
    let ev = evidence!(
        covers = ["BR-001"],
        kind = Kind::Fixture,
        expected = "an instance keeps its exact runtime ID and provider revision key through its lifecycle, renames, and desired state changes, and only rebind changes the runtime",
    );
    let mut instance = stopped_instance();
    let (runtime_id, revision) = (instance.runtime_id(), instance.provider_revision().clone());

    instance.set_desired(DesiredState::Running);
    for (seconds, event) in [
        (3, LifecycleEvent::StartBegin),
        (4, LifecycleEvent::Spawned),
        (5, LifecycleEvent::StopBegin),
        (6, LifecycleEvent::Exited),
    ] {
        instance.apply_lifecycle(event, at(seconds)).unwrap();
    }
    instance.rename("sales-db".parse().unwrap());
    assert_eq!(instance.runtime_id(), runtime_id);
    assert_eq!(instance.provider_revision(), &revision);

    let next = RuntimeId::generate();
    instance
        .rebind(
            next,
            &"postgresql".parse().unwrap(),
            &line("17"),
            &LineCompatibility::default(),
        )
        .unwrap();
    assert_eq!(instance.runtime_id(), next);
    assert_eq!(instance.provider_revision(), &revision);
    ev.pass();
}

#[test]
fn renaming_keeps_id_runtime_and_paths() {
    let ev = evidence!(
        covers = ["BR-003"],
        kind = Kind::Fixture,
        expected = "a rename changes only the name and raises the revision by 1; the ID, short key, runtime, and provider revision stay, so every path derived from the ID stays",
    );
    let mut instance = stopped_instance();
    let before = instance.clone();

    instance.rename("sales-db".parse().unwrap());

    assert_eq!(instance.name().as_str(), "sales-db");
    assert_eq!(instance.id(), before.id());
    assert_eq!(instance.id().short_key(), before.id().short_key());
    assert_eq!(instance.runtime_id(), before.runtime_id());
    assert_eq!(instance.provider_revision(), before.provider_revision());
    assert_eq!(instance.data_line(), before.data_line());
    assert_eq!(instance.lifecycle(), before.lifecycle());
    assert_eq!(instance.revision().get(), before.revision().get() + 1);
    ev.pass();
}

#[test]
fn endpoint_conflicts_follow_address_transport_and_port() {
    let ev = evidence!(
        covers = ["BR-004"],
        kind = Kind::Fixture,
        expected = "IP endpoints conflict only with the same transport, the same effective port, and overlapping addresses (equal, 0.0.0.0 with IPv4, :: with anything, IPv4 mapped IPv6 as IPv4); TCP and UDP never conflict, Unix never conflicts with IP, and Unix sockets conflict only by name",
    );
    let udp = |address: &str, number: u16| {
        Endpoint::Ip(ip_endpoint(
            Transport::Udp,
            address,
            PortRequest::Fixed(port(number)),
        ))
    };
    let auto = Endpoint::Ip(ip_endpoint(Transport::Tcp, "0.0.0.0", PortRequest::Auto));
    let unix = |socket: &str| {
        Endpoint::Unix(UnixEndpoint::new(
            "socket".parse().unwrap(),
            socket.parse().unwrap(),
        ))
    };
    let cases = [
        (
            "same address",
            tcp("127.0.0.1", 5432),
            tcp("127.0.0.1", 5432),
            true,
        ),
        (
            "ipv4 wildcard",
            tcp("0.0.0.0", 5432),
            tcp("10.0.0.7", 5432),
            true,
        ),
        (
            "ipv6 wildcard and ipv4",
            tcp("::", 5432),
            tcp("127.0.0.1", 5432),
            true,
        ),
        (
            "ipv6 wildcard and ipv6",
            tcp("::", 5432),
            tcp("::1", 5432),
            true,
        ),
        (
            "mapped ipv4",
            tcp("::ffff:127.0.0.1", 5432),
            tcp("127.0.0.1", 5432),
            true,
        ),
        (
            "other port",
            tcp("127.0.0.1", 5432),
            tcp("127.0.0.1", 5433),
            false,
        ),
        (
            "other address",
            tcp("127.0.0.1", 5432),
            tcp("127.0.0.2", 5432),
            false,
        ),
        (
            "ipv4 wildcard and ipv6",
            tcp("0.0.0.0", 5432),
            tcp("::1", 5432),
            false,
        ),
        (
            "tcp and udp",
            tcp("127.0.0.1", 5432),
            udp("127.0.0.1", 5432),
            false,
        ),
        (
            "udp and udp",
            udp("0.0.0.0", 53),
            udp("127.0.0.1", 53),
            true,
        ),
        ("auto without bind", auto, tcp("0.0.0.0", 5432), false),
        (
            "unix and ip",
            unix(".s.PGSQL.5432"),
            tcp("0.0.0.0", 5432),
            false,
        ),
        (
            "same socket",
            unix(".s.PGSQL.5432"),
            unix(".s.PGSQL.5432"),
            true,
        ),
        (
            "other socket",
            unix(".s.PGSQL.5432"),
            unix("mysqlx.sock"),
            false,
        ),
    ];
    for (case, a, b, conflict) in cases {
        assert_eq!(endpoints_conflict(&a, &b), conflict, "{case}");
        assert_eq!(endpoints_conflict(&b, &a), conflict, "{case}, reversed");
    }
    ev.pass();
}

#[test]
fn fixed_port_conflict_is_an_error_and_never_substituted() {
    let ev = evidence!(
        covers = ["BR-005"],
        kind = Kind::Fixture,
        expected = "a fixed port that conflicts with another endpoint's effective port fails with FixedPortConflict and BR-005, even when a free candidate is offered, and a free fixed port is used as is",
    );
    let fixed = ip_endpoint(Transport::Tcp, "127.0.0.1", PortRequest::Fixed(port(5432)));
    let taken = [tcp("0.0.0.0", 5432)];

    let err = resolve_port(&fixed, Some(port(5433)), &taken).unwrap_err();
    assert_eq!(
        err.detail(),
        &InvariantDetail::FixedPortConflict { port: port(5432) }
    );
    assert_eq!(err.rule(), Some(Rule::Br005));
    assert_eq!(
        resolve_port(&fixed, Some(port(5433)), &[tcp("0.0.0.0", 5433)]),
        Ok(port(5432))
    );
    ev.pass();
}

#[test]
fn only_purge_deletes_data() {
    let ev = evidence!(
        covers = ["BR-006"],
        kind = Kind::Fixture,
        expected = "purge_instance is the only operation kind that deletes data; remove_instance and retain keep it",
    );
    for kind in OperationKind::ALL {
        assert_eq!(
            kind.deletes_data(),
            *kind == OperationKind::PurgeInstance,
            "{kind}"
        );
    }

    let mut instance = stopped_instance();
    let before = instance.clone();
    instance
        .apply_lifecycle(LifecycleEvent::Retain, at(9))
        .unwrap();
    assert_eq!(instance.lifecycle(), InstanceLifecycle::Retained);
    assert_eq!(instance.data_initialized_at(), before.data_initialized_at());
    assert_eq!(instance.data_line(), before.data_line());
    ev.pass();
}

#[test]
fn runtime_in_use_cannot_be_removed() {
    let ev = evidence!(
        covers = ["BR-007"],
        kind = Kind::Fixture,
        expected = "a runtime with an instance in any lifecycle, a dependent runtime, an active operation, or a recovery snapshot cannot be removed; RemovalBlocked lists every blocker and the runtime stays installed",
    );
    let instance = InstanceId::generate();
    let dependent = RuntimeId::generate();
    let operation = OperationId::generate();
    let snapshot = stackly_domain::BackupId::generate();
    let one_of_each = [
        RuntimeReferences {
            instances: vec![instance],
            ..RuntimeReferences::default()
        },
        RuntimeReferences {
            dependent_runtimes: vec![dependent],
            ..RuntimeReferences::default()
        },
        RuntimeReferences {
            active_operations: vec![operation],
            ..RuntimeReferences::default()
        },
        RuntimeReferences {
            recovery_snapshots: vec![snapshot],
            ..RuntimeReferences::default()
        },
    ];
    let mut runtime = installed(managed());
    for refs in &one_of_each {
        let err = runtime.remove(refs).unwrap_err();
        assert_eq!(err.rule(), Some(Rule::Br007));
        assert_eq!(runtime.state(), RuntimeState::Installed);
    }

    let all = RuntimeReferences {
        instances: vec![instance],
        dependent_runtimes: vec![dependent],
        active_operations: vec![operation],
        recovery_snapshots: vec![snapshot],
    };
    let err = plan_runtime_removal(&runtime, &all).unwrap_err();
    assert_eq!(
        err.detail(),
        &InvariantDetail::RemovalBlocked {
            blockers: vec![
                RemovalBlocker::Instance(instance),
                RemovalBlocker::DependentRuntime(dependent),
                RemovalBlocker::ActiveOperation(operation),
                RemovalBlocker::RecoverySnapshot(snapshot),
            ]
        }
    );
    ev.pass();
}

#[test]
fn start_is_refused_until_data_is_initialized() {
    let ev = evidence!(
        covers = ["BR-008"],
        kind = Kind::Fixture,
        expected = "an instance whose init failed is refused start_begin with DataNotInitialized and BR-008 and does not change, while an instance whose init succeeded starts",
    );
    let mut failed_init = instance();
    failed_init
        .apply_lifecycle(LifecycleEvent::InitBegin, at(1))
        .unwrap();
    failed_init
        .apply_lifecycle(
            LifecycleEvent::InitFailed {
                reason: stackly_domain::FailureCode::INTERRUPTED,
            },
            at(2),
        )
        .unwrap();
    let before = failed_init.clone();

    let err = failed_init
        .apply_lifecycle(LifecycleEvent::StartBegin, at(3))
        .unwrap_err();
    assert_eq!(err.rule(), Some(Rule::Br008));
    assert_eq!(failed_init, before);
    assert_eq!(failed_init.lifecycle(), InstanceLifecycle::Failed);

    let mut initialized = stopped_instance();
    assert_eq!(
        initialized.apply_lifecycle(LifecycleEvent::StartBegin, at(3)),
        Ok(InstanceLifecycle::Starting)
    );
    ev.pass();
}

#[test]
fn runtime_of_another_line_is_rejected_on_existing_data() {
    let ev = evidence!(
        covers = ["BR-009"],
        kind = Kind::Fixture,
        expected = "rebinding data of line 17 to a runtime of line 18 fails with IncompatibleRuntimeLine and BR-009 unless the provider's line compatibility allows it, and the same line is always allowed",
    );
    let postgresql = "postgresql".parse().unwrap();
    let mut instance = stopped_instance();
    let before = instance.clone();

    let err = instance
        .rebind(
            RuntimeId::generate(),
            &postgresql,
            &line("18"),
            &LineCompatibility::default(),
        )
        .unwrap_err();
    assert_eq!(err.rule(), Some(Rule::Br009));
    assert_eq!(
        err.detail(),
        &InvariantDetail::IncompatibleRuntimeLine {
            data_line: line("17"),
            runtime_line: line("18"),
        }
    );
    assert_eq!(instance, before);

    let allowed = LineCompatibility::default().allow(line("17"), line("18"));
    assert!(
        instance
            .rebind(RuntimeId::generate(), &postgresql, &line("18"), &allowed)
            .is_ok()
    );
    assert!(
        instance
            .rebind(
                RuntimeId::generate(),
                &postgresql,
                &line("17"),
                &LineCompatibility::default()
            )
            .is_ok()
    );
    ev.pass();
}

#[test]
fn running_process_alone_is_not_ready() {
    let ev = evidence!(
        covers = ["BR-010"],
        kind = Kind::Fixture,
        expected = "spawned without a probe leaves readiness not_ready with reason awaiting_probe, and ready is refused with ReadyWithoutProcess and BR-010 while the lifecycle is not running",
    );
    let mut instance = stopped_instance();
    let ready = |seconds| {
        Readiness::new(
            ReadinessState::Ready,
            at(seconds),
            "probe_ok".parse().unwrap(),
            None,
        )
    };

    let err = instance.observe_readiness(ready(3)).unwrap_err();
    assert_eq!(err.rule(), Some(Rule::Br010));
    instance
        .apply_lifecycle(LifecycleEvent::StartBegin, at(4))
        .unwrap();
    instance
        .apply_lifecycle(LifecycleEvent::Spawned, at(5))
        .unwrap();

    assert_eq!(instance.lifecycle(), InstanceLifecycle::Running);
    assert_eq!(instance.readiness().state(), ReadinessState::NotReady);
    assert_eq!(instance.readiness().reason(), &ReasonCode::AWAITING_PROBE);
    assert!(instance.observe_readiness(ready(6)).is_ok());
    assert_eq!(instance.readiness().state(), ReadinessState::Ready);
    ev.pass();
}

#[test]
fn stale_ready_reads_as_unknown() {
    let ev = evidence!(
        covers = ["BR-010"],
        kind = Kind::Fixture,
        expected = "a ready observation read 31 s later with a 30 s max age reads as unknown with reason stale, while the stored value stays ready",
    );
    let mut instance = stopped_instance();
    instance
        .apply_lifecycle(LifecycleEvent::StartBegin, at(3))
        .unwrap();
    instance
        .apply_lifecycle(LifecycleEvent::Spawned, at(4))
        .unwrap();
    instance
        .observe_readiness(Readiness::new(
            ReadinessState::Ready,
            at(10),
            "probe_ok".parse().unwrap(),
            None,
        ))
        .unwrap();
    let max_age = std::time::Duration::from_secs(30);

    assert_eq!(
        instance.readiness().effective(at(40), max_age).state(),
        ReadinessState::Ready
    );
    let effective = instance.readiness().effective(at(41), max_age);
    assert_eq!(effective.state(), ReadinessState::Unknown);
    assert_eq!(effective.reason(), &ReasonCode::STALE);
    assert_eq!(instance.readiness().state(), ReadinessState::Ready);
    ev.pass();
}

#[test]
fn only_data_migration_changes_the_data_line() {
    let ev = evidence!(
        covers = ["BR-013"],
        kind = Kind::Fixture,
        expected = "the data line and version change only through a running migrate_data operation on the same instance; another kind, a queued migration, or a migration of another instance fails with BR-013, and rebind never changes the data line",
    );
    let mut instance = stopped_instance();
    let id = instance.id();
    let refused = [
        migrate_data(id, OperationKind::RebindRuntime, true),
        migrate_data(id, OperationKind::RestoreInstance, true),
        migrate_data(id, OperationKind::MigrateData, false),
        migrate_data(InstanceId::generate(), OperationKind::MigrateData, true),
    ];
    for operation in &refused {
        let err = instance
            .record_data_migration(operation, line("18"), "18.1".parse().unwrap())
            .unwrap_err();
        assert_eq!(err.rule(), Some(Rule::Br013), "{:?}", operation.kind());
        assert_eq!(instance.data_line(), &line("17"));
    }
    let allowed = LineCompatibility::default().allow(line("17"), line("18"));
    instance
        .rebind(
            RuntimeId::generate(),
            &"postgresql".parse().unwrap(),
            &line("18"),
            &allowed,
        )
        .unwrap();
    assert_eq!(instance.data_line(), &line("17"));

    let migration = migrate_data(id, OperationKind::MigrateData, true);
    instance
        .record_data_migration(&migration, line("18"), "18.1".parse().unwrap())
        .unwrap();
    assert_eq!(instance.data_line(), &line("18"));
    assert_eq!(instance.data_version().as_str(), "18.1");
    ev.pass();
}

#[test]
fn environment_down_keeps_a_shared_instance_with_other_leases() {
    let ev = evidence!(
        covers = ["BR-014"],
        kind = Kind::Fixture,
        expected = "when one environment goes down, its lease is released and an instance that another environment, a manual start, or a startup lease still holds keeps running",
    );
    let instance = InstanceId::generate();
    let (shop, billing) = (EnvironmentId::generate(), EnvironmentId::generate());
    let shop_lease = lease(instance, LeaseHolder::Environment(shop), None);
    for other in [
        lease(instance, LeaseHolder::Environment(billing), None),
        lease(instance, LeaseHolder::Manual, None),
        lease(instance, LeaseHolder::Startup, Some("login-1")),
    ] {
        let decision = evaluate_stop(
            instance,
            &[shop_lease.clone(), other.clone()],
            &StopRequest::EnvironmentDown(shop),
        );

        assert_eq!(
            decision,
            StopDecision::KeepRunning {
                release: vec![shop_lease.id()],
                remaining: vec![other.id()],
            }
        );
    }
    ev.pass();
}

#[test]
fn manual_stop_needs_override_when_an_environment_holds_a_lease() {
    let ev = evidence!(
        covers = ["BR-015"],
        kind = Kind::Fixture,
        expected = "a manual stop of an instance an environment holds returns RequiresOverride with that lease; with the override it stops and lists the environment lease as overridden, not released",
    );
    let instance = InstanceId::generate();
    let env = lease(
        instance,
        LeaseHolder::Environment(EnvironmentId::generate()),
        None,
    );
    let manual = lease(instance, LeaseHolder::Manual, None);
    let leases = [env.clone(), manual.clone()];
    let request = |override_leases| StopRequest::Manual {
        session: None,
        override_leases,
    };

    assert_eq!(
        evaluate_stop(instance, &leases, &request(false)),
        StopDecision::RequiresOverride {
            blocking: vec![env.id()]
        }
    );
    assert_eq!(
        evaluate_stop(instance, &leases, &request(true)),
        StopDecision::Stop {
            release: vec![manual.id()],
            suppress: Vec::new(),
            overridden: vec![env.id()],
        }
    );
    ev.pass();
}

#[test]
fn unsupported_capability_is_reported_not_attempted() {
    let ev = evidence!(
        covers = ["BR-016"],
        kind = Kind::Fixture,
        expected = "requiring a capability the provider revision lacks returns Unsupported with that capability and BR-016, and a supported one passes",
    );
    let caps: CapabilitySet = [Capability::Reload, Capability::ColdBackup]
        .into_iter()
        .collect();

    let err = caps.require(Capability::OnlineBackup).unwrap_err();
    assert_eq!(
        err.detail(),
        &InvariantDetail::Unsupported {
            capability: Capability::OnlineBackup
        }
    );
    assert_eq!(err.rule(), Some(Rule::Br016));
    assert_eq!(caps.require(Capability::ColdBackup), Ok(()));
    ev.pass();
}

#[test]
fn provider_revision_changes_only_with_an_accepted_diff() {
    let ev = evidence!(
        covers = ["BR-017"],
        kind = Kind::Fixture,
        expected = "an accepted change from the current revision moves the instance to the new revision; a change from another revision or to another provider fails with BR-017 and changes nothing",
    );
    let mut instance = stopped_instance();
    let diff = DIGEST_B.parse().unwrap();
    let other_provider = ProviderRevisionKey::new(
        "community/postgresql".parse().unwrap(),
        "2".parse().unwrap(),
        DIGEST_A.parse().unwrap(),
    );
    let refused = [
        AcceptedRevisionChange::new(revision_key("0"), revision_key("2"), diff),
        AcceptedRevisionChange::new(revision_key("1"), other_provider, diff),
    ];
    for change in &refused {
        let before = instance.clone();
        let err = instance.apply_provider_revision(change).unwrap_err();
        assert_eq!(err.rule(), Some(Rule::Br017));
        assert_eq!(instance, before);
    }

    let accepted = AcceptedRevisionChange::new(revision_key("1"), revision_key("2"), diff);
    let revision = instance.revision();
    instance.apply_provider_revision(&accepted).unwrap();
    assert_eq!(instance.provider_revision(), &revision_key("2"));
    assert_eq!(instance.revision(), revision.next());
    ev.pass();
}

#[test]
fn external_runtime_removal_never_deletes_files() {
    let ev = evidence!(
        covers = ["BR-019"],
        kind = Kind::Fixture,
        expected = "removing an external runtime only unregisters it and never deletes files, while a managed runtime's files are deleted",
    );
    let mut external = installed(external());
    let mut managed = installed(managed());

    assert_eq!(
        plan_runtime_removal(&external, &RuntimeReferences::default()),
        Ok(RemovalPlan::UnregisterOnly)
    );
    assert_eq!(
        external.remove(&RuntimeReferences::default()),
        Ok(RemovalPlan::UnregisterOnly)
    );
    assert_eq!(external.state(), RuntimeState::Absent);
    assert_eq!(
        managed.remove(&RuntimeReferences::default()),
        Ok(RemovalPlan::DeleteManagedFiles)
    );
    ev.pass();
}

#[test]
fn explicit_stop_suppresses_autostart_for_the_session() {
    let ev = evidence!(
        covers = ["BR-024"],
        kind = Kind::Fixture,
        expected = "a manual stop suppresses the startup lease of its session, autostart is then false for that session, and the next session's new startup lease turns it back on",
    );
    let instance = InstanceId::generate();
    let first: SessionId = "login-1".parse().unwrap();
    let mut startup = lease(instance, LeaseHolder::Startup, Some("login-1"));
    assert!(should_autostart(
        StartupPolicy::AtLogin,
        std::slice::from_ref(&startup),
        &first
    ));

    let decision = evaluate_stop(
        instance,
        std::slice::from_ref(&startup),
        &StopRequest::Manual {
            session: Some(first.clone()),
            override_leases: false,
        },
    );
    assert_eq!(
        decision,
        StopDecision::Stop {
            release: Vec::new(),
            suppress: vec![startup.id()],
            overridden: Vec::new(),
        }
    );
    startup.suppress(at(10)).unwrap();
    assert!(!should_autostart(
        StartupPolicy::AtLogin,
        std::slice::from_ref(&startup),
        &first
    ));

    let second: SessionId = "login-2".parse().unwrap();
    let next = lease(instance, LeaseHolder::Startup, Some("login-2"));
    assert!(should_autostart(
        StartupPolicy::AtLogin,
        &[startup, next],
        &second
    ));
    ev.pass();
}
