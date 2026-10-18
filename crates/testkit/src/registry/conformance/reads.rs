//! Read scenarios: round trips of every aggregate and state code, list
//! order, stored paths, runtime references, and operation pages (AC-10,
//! AC-11, AC-16).

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::path::PathBuf;

use stackly_domain::{
    Capability, DesiredState, Endpoint, EnvironmentId, HealthState, Instance, InstanceFilter,
    InstanceId, InstanceLifecycle, InstancePathKind, IpEndpoint, LeaseFilter, LineCompatibility,
    LogicalRoot, Operation, OperationCursor, OperationId, OperationKind, OperationQuery,
    OperationResource, OperationState, Ownership, Port, PortRequest, Progress, ReadinessState,
    ReasonCode, RegistryError, ResourceRef, Runtime, RuntimeId, RuntimeState, StartupPolicy,
    Transport, UnixEndpoint, WriteOp,
};
use stackly_platform::{DirectoryLayout, RootSet, instance_path_entries};

use super::{Stack, Suite, domain, expect, expect_eq, fail, fresh_root, parse, shifted, suffix};
use crate::registry::{
    BackupSeed, EnvironmentBuilder, LeaseBuilder, OperationBuilder, ProviderRevisionBuilder,
    RuntimeBuilder, SecretReferenceSeed,
};

/// Every aggregate and every state code reads back as written, except
/// readiness and health, which read `unknown`, `daemon_restarted`, at the
/// time the registry was opened (AC-16).
pub(super) fn round_trip(suite: &Suite<'_>) {
    let provider = ProviderRevisionBuilder::new()
        .capabilities([Capability::Tls, Capability::Reload, Capability::Persistence])
        .line_compatibility(
            LineCompatibility::default()
                .allow(parse("make a line", "16"), parse("make a line", "17"))
                .allow(parse("make a line", "15"), parse("make a line", "17")),
        )
        .columns(Some("stackly.dev/v1"), Some("stackly"), Some("approved"));
    let record = provider.clone().record();
    let runtime = RuntimeBuilder::new()
        .state(RuntimeState::Installed)
        .installed_at(shifted(suite.loaded_at, -DAY_SECS))
        .last_verified_at(suite.loaded_at);
    let artifact = runtime.artifact();
    let built = runtime.build();
    suite.commit(
        "store a provider revision, artifact, and runtime",
        vec![
            WriteOp::InsertProviderRevision(record.clone()),
            WriteOp::InsertArtifact(artifact.clone()),
            WriteOp::InsertRuntime(built.clone()),
        ],
    );
    expect_eq(
        "a provider revision reads back as written",
        suite.value(
            "read the provider revision",
            suite.registry.provider_revision(&provider.key()),
        ),
        Some(record.clone()),
    );
    expect_eq(
        "an artifact reads back as written",
        suite.value(
            "read the artifact",
            suite.registry.artifact(&artifact.sha256),
        ),
        Some(artifact),
    );

    let runtimes: Vec<Runtime> = RuntimeState::ALL
        .iter()
        .map(|state| {
            RuntimeBuilder::new()
                .external(fresh_root())
                .state(*state)
                .depends_on(built.id())
                .build()
        })
        .collect();
    suite.commit(
        "store a runtime in every state",
        runtimes
            .iter()
            .cloned()
            .map(WriteOp::InsertRuntime)
            .collect(),
    );
    for runtime in runtimes.iter().chain([&built]) {
        expect_eq(
            &format!("a runtime in state {} reads back", runtime.state()),
            suite.value("read a runtime", suite.registry.runtime(runtime.id())),
            Some(runtime.clone()),
        );
    }

    let stack = Stack {
        provider: record.revision,
        runtime: built,
    };
    let instances: Vec<Instance> = InstanceLifecycle::ALL
        .iter()
        .enumerate()
        .map(|(index, lifecycle)| {
            let mut builder = Suite::instance(&stack)
                .lifecycle(*lifecycle)
                .desired(DesiredState::ALL[index % DesiredState::ALL.len()])
                .startup(StartupPolicy::ALL[index % StartupPolicy::ALL.len()])
                .created_at(shifted(suite.loaded_at, -DAY_SECS));
            if *lifecycle == InstanceLifecycle::Failed {
                builder = builder.last_failure("start_failed");
            }
            if index == 0 {
                builder = builder.display_name("Orders DB (main)");
            }
            builder.build()
        })
        .collect();
    suite.commit(
        "store an instance in every lifecycle",
        instances
            .iter()
            .cloned()
            .map(WriteOp::InsertInstance)
            .collect(),
    );
    for instance in &instances {
        let read = suite
            .value("read an instance", suite.registry.instance(instance.id()))
            .unwrap_or_else(|| fail("read an instance", "it is gone"));
        expect_restarted(suite, &read);
        expect_eq(
            &format!(
                "an instance in lifecycle {} reads back",
                instance.lifecycle()
            ),
            read,
            suite.expected_instance(instance),
        );
    }

    let endpoints = vec![
        Endpoint::Unix(UnixEndpoint::new(
            parse("make an endpoint name", "zeta"),
            parse("make a socket name", "z.sock"),
        )),
        Endpoint::Ip(IpEndpoint::new(
            parse("make an endpoint name", "alpha"),
            Transport::Udp,
            IpAddr::V6(Ipv6Addr::LOCALHOST),
            PortRequest::Auto,
            None,
        )),
        Endpoint::Ip(IpEndpoint::new(
            parse("make an endpoint name", "mid"),
            Transport::Tcp,
            IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            PortRequest::Fixed(domain("make a port", Port::new(6000))),
            Some(domain("make a port", Port::new(6000))),
        )),
    ];
    let ordered = Suite::instance(&stack).endpoints(endpoints.clone()).build();
    suite.commit(
        "store an instance with endpoints out of name order",
        vec![WriteOp::InsertInstance(ordered.clone())],
    );
    expect_eq(
        "endpoints read back in their order",
        suite
            .value("read the instance", suite.registry.instance(ordered.id()))
            .map(|instance| instance.endpoints().to_vec()),
        Some(endpoints),
    );

    let members: Vec<InstanceId> = instances.iter().take(3).map(Instance::id).collect();
    let environment = EnvironmentBuilder::new()
        .manifest_sha256(RuntimeBuilder::new().artifact().sha256)
        .member("zeta", members[0], Ownership::Shared)
        .member("alpha", members[1], Ownership::Dedicated)
        .member("mid", members[2], Ownership::Shared)
        .build();
    suite.commit(
        "store an environment with members out of alias order",
        vec![WriteOp::InsertEnvironment(environment.clone())],
    );
    expect_eq(
        "an environment reads back with its members in order",
        suite.value(
            "read the environment",
            suite.registry.environment(environment.id()),
        ),
        Some(environment.clone()),
    );

    let holder = ordered.id();
    let t = suite.loaded_at;
    let leases = vec![
        LeaseBuilder::new(holder)
            .environment(environment.id())
            .acquired_at(shifted(t, -60))
            .build(),
        LeaseBuilder::new(holder)
            .acquired_at(shifted(t, -50))
            .build(),
        LeaseBuilder::new(holder)
            .manual(Some("session-2"))
            .acquired_at(shifted(t, -40))
            .build(),
        LeaseBuilder::new(holder)
            .startup("session-2")
            .acquired_at(shifted(t, -30))
            .build(),
        LeaseBuilder::new(holder)
            .startup("session-1")
            .suppressed(shifted(t, -5))
            .acquired_at(shifted(t, -20))
            .build(),
        LeaseBuilder::new(holder)
            .released(shifted(t, -1))
            .acquired_at(shifted(t, -10))
            .build(),
    ];
    suite.commit(
        "store a lease of every holder and state",
        leases.iter().cloned().map(WriteOp::InsertLease).collect(),
    );
    expect_eq(
        "every lease reads back as written",
        suite.value(
            "list the leases",
            suite.registry.leases(holder, LeaseFilter::All),
        ),
        leases,
    );

    let resources = [
        OperationResource::Runtime(RuntimeId::generate()),
        OperationResource::Instance(InstanceId::generate()),
        OperationResource::Environment(EnvironmentId::generate()),
        OperationResource::Manager,
    ];
    let mut operations: Vec<Operation> = OperationKind::ALL
        .iter()
        .enumerate()
        .map(|(index, kind)| {
            let seconds = i64::try_from(index).unwrap_or(i64::MAX);
            OperationBuilder::new()
                .kind(*kind)
                .state(OperationState::ALL[index % OperationState::ALL.len()])
                .resource(resources[index % resources.len()])
                .created_at(shifted(t, seconds))
                .updated_at(shifted(t, seconds + 1))
                .build()
        })
        .collect();
    operations.push(
        OperationBuilder::new()
            .state(OperationState::Running)
            .idempotency_key("req-7f3a")
            .stage("download")
            .progress(Progress::Counted {
                done: 512,
                total: 2048,
            })
            .failure("network_unreachable", true)
            .build(),
    );
    suite.commit(
        "store an operation of every kind and state",
        operations
            .iter()
            .cloned()
            .map(WriteOp::InsertOperation)
            .collect(),
    );
    for operation in &operations {
        expect_eq(
            &format!(
                "an operation of kind {} in state {} reads back",
                operation.kind(),
                operation.state()
            ),
            suite.value(
                "read an operation",
                suite.registry.operation(operation.id()),
            ),
            Some(operation.clone()),
        );
    }
}

/// Seconds in one day.
const DAY_SECS: i64 = super::DAY;

/// Readiness and health are never stored: both read back `unknown` with
/// reason `daemon_restarted`, observed when the registry was opened.
fn expect_restarted(suite: &Suite<'_>, instance: &Instance) {
    expect_eq(
        "readiness reads back unknown, daemon_restarted, at the open time",
        (
            instance.readiness().state(),
            instance.readiness().reason().clone(),
            instance.readiness().observed_at(),
        ),
        (
            ReadinessState::Unknown,
            ReasonCode::DAEMON_RESTARTED,
            suite.loaded_at,
        ),
    );
    expect_eq(
        "health reads back unknown, daemon_restarted, at the open time",
        (
            instance.health().state(),
            instance.health().reason().clone(),
            instance.health().observed_at(),
        ),
        (
            HealthState::Unknown,
            ReasonCode::DAEMON_RESTARTED,
            suite.loaded_at,
        ),
    );
}

/// Lists come in their documented order, `known_instance_ids` includes
/// retained instances, and lookups of unknown records find nothing
/// (AC-11).
pub(super) fn lists(suite: &Suite<'_>) {
    let stack = suite.stack("store a runtime for lists");
    let retained = Suite::instance(&stack)
        .retained(OperationId::generate(), suite.loaded_at)
        .build();
    suite.commit(
        "store a retained instance",
        vec![WriteOp::InsertInstance(retained.clone())],
    );

    let runtimes = suite.value("list runtimes", suite.registry.runtimes());
    expect(
        "runtimes list by ID",
        runtimes.windows(2).all(|pair| pair[0].id() < pair[1].id()),
        || format!("{:?}", runtimes.iter().map(Runtime::id).collect::<Vec<_>>()),
    );
    let environments = suite.value("list environments", suite.registry.environments());
    expect(
        "environments list by ID",
        environments
            .windows(2)
            .all(|pair| pair[0].id() < pair[1].id()),
        || {
            format!(
                "{:?}",
                environments.iter().map(|e| e.id()).collect::<Vec<_>>()
            )
        },
    );
    let known = suite.value("list known IDs", suite.registry.known_instance_ids());
    expect(
        "known instance IDs list by ID",
        known.windows(2).all(|pair| pair[0] < pair[1]),
        || format!("{known:?}"),
    );
    let all = suite.value(
        "list every instance",
        suite.registry.instances(InstanceFilter {
            include_retained: true,
        }),
    );
    expect(
        "instances list by name, then ID",
        all.windows(2)
            .all(|pair| (pair[0].name(), pair[0].id()) < (pair[1].name(), pair[1].id())),
        || {
            format!(
                "{:?}",
                all.iter().map(|i| (i.name(), i.id())).collect::<Vec<_>>()
            )
        },
    );
    let mut all_ids: Vec<InstanceId> = all.iter().map(Instance::id).collect();
    all_ids.sort();
    expect_eq(
        "known instance IDs are every instance, retained ones included",
        &known,
        &all_ids,
    );
    expect(
        "known instance IDs include a retained instance",
        known.contains(&retained.id()),
        || format!("{} is missing", retained.id()),
    );
    let active = suite.value(
        "list instances that are not retained",
        suite.registry.instances(InstanceFilter::default()),
    );
    expect(
        "the default instance list skips retained instances",
        active
            .iter()
            .all(|instance| instance.retained_at().is_none()),
        || "a retained instance is listed".to_owned(),
    );

    expect_eq(
        "an unknown runtime is None",
        suite.value(
            "read an unknown runtime",
            suite.registry.runtime(RuntimeId::generate()),
        ),
        None,
    );
    expect_eq(
        "an unknown instance is None",
        suite.value(
            "read an unknown instance",
            suite.registry.instance(InstanceId::generate()),
        ),
        None,
    );
    expect_eq(
        "an unknown name is None",
        suite.value(
            "find an unknown name",
            suite.registry.instance_by_name(&parse(
                "make a name",
                &format!("nobody-{}", suffix(retained.id())),
            )),
        ),
        None,
    );
    expect_eq(
        "the name of a retained instance alone finds nothing",
        suite.value(
            "find a retained name",
            suite.registry.instance_by_name(retained.name()),
        ),
        None,
    );
    expect_eq(
        "unknown instance paths are None",
        suite.value(
            "read unknown paths",
            suite.registry.instance_paths(InstanceId::generate()),
        ),
        None,
    );
    expect_eq(
        "an unknown environment is None",
        suite.value(
            "read an unknown environment",
            suite.registry.environment(EnvironmentId::generate()),
        ),
        None,
    );
    expect_eq(
        "an unknown operation is None",
        suite.value(
            "read an unknown operation",
            suite.registry.operation(OperationId::generate()),
        ),
        None,
    );
    expect_eq(
        "an unknown provider revision is None",
        suite.value(
            "read an unknown provider revision",
            suite
                .registry
                .provider_revision(&ProviderRevisionBuilder::new().key()),
        ),
        None,
    );
    expect_eq(
        "an unknown artifact is None",
        suite.value(
            "read an unknown artifact",
            suite
                .registry
                .artifact(&RuntimeBuilder::new().artifact().sha256),
        ),
        None,
    );
}

/// The registry derives each instance's seven stored paths from its ID,
/// they resolve to the layout paths, and no stored path equals or lies
/// inside another instance's path under the same root (AC-10, BR-002).
pub(super) fn stored_paths(suite: &Suite<'_>) {
    const CHECK: &str = "stored_paths_resolve_to_the_layout_and_never_repeat";
    let stack = suite.stack("store a runtime for paths");
    let instances: Vec<Instance> = (0..1000).map(|_| Suite::instance(&stack).build()).collect();
    suite.commit(
        "store 1000 instances in one batch",
        instances
            .iter()
            .cloned()
            .map(WriteOp::InsertInstance)
            .collect(),
    );

    let roots = domain(
        "make conformance roots",
        RootSet::new(
            PathBuf::from("/conformance/data"),
            PathBuf::from("/conformance/logs"),
            PathBuf::from("/conformance/cache"),
            PathBuf::from("/conformance/run"),
        ),
    );
    let layout = DirectoryLayout::new(roots);
    let known = suite.value("list known IDs", suite.registry.known_instance_ids());
    let mut owners: HashMap<(LogicalRoot, String), InstanceId> = HashMap::new();
    for id in known {
        let stored = suite
            .value("read stored paths", suite.registry.instance_paths(id))
            .unwrap_or_else(|| fail(CHECK, &format!("instance {id} has no stored paths")));
        expect_eq(
            &format!("{CHECK}: the stored paths are the ones derived from the ID"),
            &stored,
            &instance_path_entries(id).to_vec(),
        );
        let paths = layout.instance_paths(id);
        for entry in &stored {
            let expected = match entry.kind() {
                InstancePathKind::Root => &paths.root,
                InstancePathKind::Data => &paths.data,
                InstancePathKind::Config => &paths.config,
                InstancePathKind::ConfigHistory => &paths.config_history,
                InstancePathKind::Temp => &paths.temp,
                InstancePathKind::Logs => &paths.logs,
                InstancePathKind::Run => &paths.run,
            };
            expect_eq(
                &format!("{CHECK}: a stored path resolves to the layout path"),
                &layout.roots().resolve(entry.root(), entry.path()),
                expected,
            );
            let key = (entry.root(), entry.path().as_str().to_owned());
            if let Some(other) = owners.insert(key, id)
                && other != id
            {
                fail(
                    CHECK,
                    &format!(
                        "{}:{} is stored for {other} and {id}",
                        entry.root(),
                        entry.path()
                    ),
                );
            }
        }
    }
    for ((root, path), owner) in &owners {
        let parts: Vec<&str> = path.split('/').collect();
        for len in 1..parts.len() {
            let prefix = (*root, parts[..len].join("/"));
            if let Some(other) = owners.get(&prefix)
                && other != owner
            {
                fail(
                    CHECK,
                    &format!(
                        "{root}:{path} of {owner} lies inside {root}:{} of {other}",
                        prefix.1
                    ),
                );
            }
        }
    }
}

/// `runtime_references` lists the instances on a runtime in every
/// lifecycle, the runtimes that depend on it, its unfinished operations,
/// and the backups that need it, each by ID (AC-11, BR-007).
pub(super) fn runtime_references(suite: &Suite<'_>) {
    const CHECK: &str = "runtime_references_include_stopped_and_retained_instances";
    let stack = suite.stack("store a runtime for references");
    let runtime = stack.runtime.id();
    let instances = [
        Suite::instance(&stack)
            .lifecycle(InstanceLifecycle::Stopped)
            .build(),
        Suite::instance(&stack)
            .retained(OperationId::generate(), suite.loaded_at)
            .build(),
        Suite::instance(&stack).build(),
    ];
    let dependent = RuntimeBuilder::new()
        .external(fresh_root())
        .depends_on(runtime)
        .build();
    let on_runtime = |state| {
        OperationBuilder::new()
            .kind(OperationKind::RemoveRuntime)
            .resource(OperationResource::Runtime(runtime))
            .state(state)
            .build()
    };
    let unfinished = [
        on_runtime(OperationState::Queued),
        on_runtime(OperationState::Running),
        on_runtime(OperationState::CancelRequested),
        on_runtime(OperationState::NeedsRecovery),
    ];
    let finished = [
        on_runtime(OperationState::Succeeded),
        on_runtime(OperationState::Failed),
        on_runtime(OperationState::Cancelled),
    ];
    let elsewhere = OperationBuilder::new()
        .resource(OperationResource::Runtime(dependent.id()))
        .state(OperationState::Running)
        .build();
    let mut ops: Vec<WriteOp> = instances
        .iter()
        .cloned()
        .map(WriteOp::InsertInstance)
        .collect();
    ops.push(WriteOp::InsertRuntime(dependent.clone()));
    ops.extend(
        unfinished
            .iter()
            .chain(&finished)
            .chain([&elsewhere])
            .cloned()
            .map(WriteOp::InsertOperation),
    );
    suite.commit("store what refers to a runtime", ops);
    let before = suite.generation();
    let backup = BackupSeed::new(suite.loaded_at)
        .with_runtime(runtime)
        .with_instance(instances[0].id());
    suite.seed.seed_backup(backup.clone());
    expect_eq(
        "seeding a backup leaves the generation",
        suite.generation(),
        before,
    );

    let references = suite.read(
        "read the runtime references",
        suite.registry.runtime_references(runtime),
    );
    expect_eq(
        "runtime references carry the generation they were read at",
        references.generation,
        before,
    );
    let references = references.value;
    let mut expected_instances: Vec<InstanceId> = instances.iter().map(Instance::id).collect();
    expected_instances.sort();
    expect_eq(
        &format!("{CHECK}: instances in every lifecycle"),
        references.instances,
        expected_instances,
    );
    expect_eq(
        &format!("{CHECK}: runtimes that depend on it"),
        references.dependent_runtimes,
        vec![dependent.id()],
    );
    let mut expected_operations: Vec<OperationId> = unfinished.iter().map(Operation::id).collect();
    expected_operations.sort();
    expect_eq(
        &format!("{CHECK}: only unfinished operations on it"),
        references.active_operations,
        expected_operations,
    );
    expect_eq(
        &format!("{CHECK}: backups that need it"),
        references.recovery_snapshots,
        vec![backup.id],
    );
    let unknown = RuntimeId::generate();
    match suite.registry.runtime_references(unknown) {
        Err(RegistryError::NotFound {
            resource: ResourceRef::Runtime(id),
        }) if id == unknown => {}
        other => fail(
            "the references of an unknown runtime are NotFound",
            &format!("{other:?}"),
        ),
    }
}

/// Operations page newest `(created_at, id)` first, with a cursor while
/// rows remain, and a limit outside 1 to 500 is `InvalidQuery`.
pub(super) fn operation_pages(suite: &Suite<'_>) {
    let resource = OperationResource::Environment(EnvironmentId::generate());
    let t = suite.loaded_at;
    let offsets = [1, 2, 3, 3, 3, 4, 5];
    let operations: Vec<Operation> = offsets
        .iter()
        .enumerate()
        .map(|(index, offset)| {
            let state = if index % 2 == 0 {
                OperationState::Succeeded
            } else {
                OperationState::Running
            };
            OperationBuilder::new()
                .resource(resource)
                .state(state)
                .created_at(shifted(t, *offset))
                .build()
        })
        .collect();
    suite.commit(
        "store operations to page through",
        operations
            .iter()
            .cloned()
            .map(WriteOp::InsertOperation)
            .collect(),
    );
    let mut expected = operations.clone();
    expected.sort_by_key(|operation| std::cmp::Reverse(OperationCursor::of(operation)));

    let query = |limit, before, active_only| OperationQuery {
        active_only,
        resource: Some(resource),
        limit,
        before,
    };
    let mut seen = Vec::new();
    let mut sizes = Vec::new();
    let mut before = None;
    for _ in 0..expected.len() {
        let page = suite.value(
            "read a page of operations",
            suite.registry.operations(query(3, before, false)),
        );
        sizes.push(page.items.len());
        let last = page.items.last().map(OperationCursor::of);
        seen.extend(page.items);
        match page.next {
            Some(next) => {
                expect_eq(
                    "the next cursor points at the last operation of the page",
                    Some(next),
                    last,
                );
                before = Some(next);
            }
            None => break,
        }
    }
    expect_eq(
        "pages list operations newest first, without gaps or repeats",
        &seen,
        &expected,
    );
    expect_eq("pages hold up to the limit", sizes, vec![3, 3, 1]);
    expect_eq(
        "a page that ends at the last row has no next cursor",
        suite
            .value(
                "read one full page",
                suite.registry.operations(query(7, None, false)),
            )
            .next,
        None,
    );
    let active: Vec<Operation> = expected
        .iter()
        .filter(|operation| !operation.state().is_final())
        .cloned()
        .collect();
    expect_eq(
        "active_only skips finished operations",
        suite
            .value(
                "read active operations",
                suite.registry.operations(query(500, None, true)),
            )
            .items,
        active,
    );
    for limit in [0, OperationQuery::LIMIT_MAX + 1] {
        let invalid = query(limit, None, false);
        let expected = invalid
            .validate()
            .err()
            .unwrap_or_else(|| fail("make an invalid query", "it validates"));
        match suite.registry.operations(invalid) {
            Err(err) => expect_eq(
                &format!("a page limit of {limit} is an invalid query"),
                err,
                expected,
            ),
            Ok(page) => fail(
                &format!("a page limit of {limit} is an invalid query"),
                &format!("{} operations", page.value.items.len()),
            ),
        }
    }
}

/// Seeding a secret reference works and leaves the generation; the row
/// holds a reference, never a value (BR-018).
pub(super) fn seeding(suite: &Suite<'_>) {
    let before = suite.generation();
    suite.seed.seed_secret_reference(SecretReferenceSeed::new(
        format!("stackly/conformance/{}", OperationId::generate()),
        suite.loaded_at,
    ));
    expect_eq(
        "seeding a secret reference leaves the generation",
        suite.generation(),
        before,
    );
}
