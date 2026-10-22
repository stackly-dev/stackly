//! PostgreSQL 17 and 18 side by side through the real daemon (spec 0025,
//! milestone 3, JOURNEY-03): the runtime 18 is installed with
//! `runtime.install` from the runtime cache of `cargo xtask`, with no
//! network, next to two running or stopped PostgreSQL 17 instances, and
//! `pg18-dev` is created on it. Nothing of the instances that were there
//! changes, the three clusters never read each other, and data of one major
//! is refused by a runtime of the other, at start and by the read-only
//! rebind check.
//!
//! Both tests are native: `#[ignore = "native: ..."]`. An artifact that is
//! not in the cache ends a test as `BLOCKED`, never as a download.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::Duration;

use stackly_core::instance::rebind::{self, RebindVerdict};
use stackly_core::operation::OperationError;
use stackly_domain::{Instance, Registry, Runtime, RuntimeId};
use stackly_protocol::codes::supervisor;
use stackly_protocol::instance::{
    EndpointStatus, InstanceDesired, InstanceLifecycle, InstanceReadinessState, InstanceRestart,
    InstanceSummary, ListInstancesParams, ProviderPin,
};
use stackly_protocol::operation::{OperationDetail, OperationState, StepState};
use stackly_protocol::{ErrorBody, ErrorDetail, Timestamp};
use stackly_testkit::evidence::{Evidence, Kind, Source, evidence};
use uuid::Uuid;

use crate::harness::{
    Installed, Made, OWNER, PG17, PG18, Site, artifact_sha256, cached_artifact, create_on,
    install_runtime, instance_id, runtime_pair, tree_digest,
};
use crate::same_runtime::{TwoPostgres17, two_postgresql_17_instances};
use crate::sentinel::SentinelCluster;
use crate::support::{Api, PATIENCE, serving};

type Tree = BTreeMap<String, (String, u32)>;
/// Registry rows with the generation they were read at.
type Rows = (Vec<(Instance, u64)>, Vec<(Runtime, u64)>);

/// The fields of `instance.get` the spec names, plus the identity of the
/// process.
#[derive(Debug, Clone, PartialEq)]
struct Facts {
    runtime_id: Uuid,
    provider: ProviderPin,
    lifecycle: InstanceLifecycle,
    desired: InstanceDesired,
    readiness: InstanceReadinessState,
    endpoints: Vec<EndpointStatus>,
    process: Option<(u32, Timestamp)>,
    restart: InstanceRestart,
}

impl Facts {
    fn of(summary: &InstanceSummary) -> Self {
        Self {
            runtime_id: summary.runtime_id,
            provider: summary.provider.clone(),
            lifecycle: summary.lifecycle,
            desired: summary.desired,
            readiness: summary.readiness.state,
            endpoints: summary.endpoints.clone(),
            process: summary
                .process
                .as_ref()
                .map(|process| (process.pid, process.started_at)),
            restart: summary.restart.clone(),
        }
    }
}

/// What a reader of the registry and the disk sees of the instances that
/// were there before the runtime 18.
#[derive(Debug, PartialEq)]
struct Snapshot {
    ids: BTreeSet<Uuid>,
    running: Facts,
    stopped: Facts,
    stopped_trees: [Tree; 2],
}

/// The digests of the instance folder and the log folder of `id`, taken
/// again until two reads 200 ms apart agree, so the log writer of a process
/// that just stopped is done.
async fn settled(site: &Site, id: Uuid) -> [Tree; 2] {
    let paths = site.instance_paths(id);
    let read = || [tree_digest(&paths.root), tree_digest(&paths.logs)];
    let deadline = tokio::time::Instant::now() + PATIENCE;
    let mut last = read();
    loop {
        tokio::time::sleep(Duration::from_millis(200)).await;
        let now = read();
        if now == last {
            return now;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the folders of the stopped instance kept changing"
        );
        last = now;
    }
}

async fn snapshot(api: &Api, site: &Site, running: Uuid, stopped: Uuid) -> Snapshot {
    let page = api.list(ListInstancesParams::default()).await;
    Snapshot {
        ids: page.items.iter().map(|summary| summary.id).collect(),
        running: Facts::of(&api.get(running).await.expect("the running instance reads")),
        stopped: Facts::of(&api.get(stopped).await.expect("the stopped instance reads")),
        stopped_trees: settled(site, stopped).await,
    }
}

/// Stops the instance `id` and waits until the stop succeeded.
async fn stop(api: &Api, id: Uuid) {
    let accepted = api
        .stop_when_free(id)
        .await
        .accepted
        .expect("a stop starts an operation");
    api.wait_state(accepted.operation.id, OperationState::Succeeded)
        .await;
}

/// A sentinel value no other cluster writes.
fn unique(name: &str) -> String {
    format!("{name}-{}", Uuid::now_v7())
}

/// The port of the `client` endpoint of a running instance.
fn port_of(summary: &InstanceSummary) -> u16 {
    summary.endpoints[0]
        .port
        .expect("a ready instance shows its port")
}

/// The three running clusters of [`three_postgresql_clusters`].
pub struct ThreeClusters {
    /// The root with the runtime 17, the package, and the registry.
    pub site: Site,
    /// The daemon in this process.
    pub daemon: crate::harness::InProcess,
    /// A client of that daemon.
    pub api: Api,
    /// `pg17-dev` (running) and `pg17-test` (stopped), with their sentinels.
    pub two: TwoPostgres17,
    /// `pg18-dev`, running on the runtime 18.
    pub pg18: SentinelCluster,
    /// The create of `pg18-dev`.
    pub made18: Made,
    /// The runtime 18 installed by `runtime.install`.
    pub runtime18: Installed,
    /// The sentinel of `pg17-dev`.
    pub dev_value: String,
    /// The sentinel of `pg17-test`.
    pub test_value: String,
    /// The sentinel of `pg18-dev`.
    pub pg18_value: String,
}

/// The shared set up of the native tests of spec 0025 (*Lingkungan test
/// native*): the two PostgreSQL 17 instances of spec 0024 with their own
/// sentinels, `pg17-test` stopped, the runtime 18 installed from the cache
/// with no network while `pg17-dev` runs (AC-1: nothing the install could
/// touch changed), and `pg18-dev` created, started, and written. Notes `ev`
/// as `BLOCKED` and returns `None` when the cache lacks an artifact.
pub async fn three_postgresql_clusters(ev: &Evidence) -> Option<ThreeClusters> {
    let missing: Vec<String> = [PG17, PG18]
        .into_iter()
        .filter(|pg| cached_artifact(&artifact_sha256(*pg)).is_none())
        .map(|pg| format!("artifact {} is not in the runtime cache", pg.artifact))
        .collect();
    if !missing.is_empty() {
        ev.blocked(&missing.join("; "), OWNER);
        return None;
    }
    let site = Site::open(ev, PG17)?;
    if !site.stage_download(ev, PG18) {
        return None;
    }
    let daemon = site.in_process_offline(|_| {}).await;
    let api = site.api().await;
    let two = two_postgresql_17_instances(&site, &api).await;
    let dev_value = unique("pg17-dev");
    let test_value = unique("pg17-test");
    two.dev.write(&dev_value);
    two.test.write(&test_value);
    stop(&api, two.test.id).await;

    // AC-1: the install changes nothing of the instances that exist.
    let before = snapshot(&api, &site, two.dev.id, two.test.id).await;
    assert_eq!(before.ids.len(), 2);
    assert_eq!(before.running.lifecycle, InstanceLifecycle::Running);
    assert_eq!(before.running.readiness, InstanceReadinessState::Ready);
    assert!(before.running.process.is_some());
    assert!(before.running.endpoints.iter().any(|e| e.bound));
    assert_eq!(before.stopped.lifecycle, InstanceLifecycle::Stopped);
    assert!(before.stopped.process.is_none());
    assert!(!before.stopped_trees[0].is_empty());
    let runtime18 = install_runtime(&api, PG18).await;
    assert_ne!(runtime18.id, site.runtime_id);
    let after = snapshot(&api, &site, two.dev.id, two.test.id).await;
    assert_eq!(after, before, "runtime.install changed an instance");

    let made18 = create_on(&api, runtime18.id, "pg18-dev").await;
    let up = api.start_ready(made18.id).await;
    assert_eq!(port_of(&up), made18.port);
    let pg18 = SentinelCluster::new(
        "pg18-dev",
        made18.id,
        site.psql_in(&runtime18.dir, made18.id, made18.port),
        &made18.password,
    );
    let pg18_value = unique("pg18-dev");
    pg18.write(&pg18_value);
    Some(ThreeClusters {
        site,
        daemon,
        api,
        two,
        pg18,
        made18,
        runtime18,
        dev_value,
        test_value,
        pg18_value,
    })
}

/// Notes the runtimes of both lines as subjects of `ev`, with the versions
/// the running servers report.
fn subjects(ev: &Evidence, version17: &str, version18: &str) {
    ev.subject(
        "postgresql",
        version17,
        Some(&artifact_sha256(PG17)),
        Source::Managed,
    );
    ev.subject(
        "postgresql",
        version18,
        Some(&artifact_sha256(PG18)),
        Source::Managed,
    );
}

/// The content of a file, trimmed.
fn text(path: &Path) -> String {
    std::fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("{} can be read: {error}", path.display()))
        .trim()
        .to_owned()
}

/// `data.initializedLine` of the manifest of the instance `id`.
fn initialized_line(site: &Site, id: Uuid) -> String {
    let manifest: serde_json::Value = serde_json::from_slice(
        &std::fs::read(site.instance_paths(id).manifest).expect("the manifest can be read"),
    )
    .expect("the manifest is JSON");
    manifest["data"]["initializedLine"]
        .as_str()
        .expect("the manifest names the line the data was initialized with")
        .to_owned()
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs both managed PostgreSQL artifacts in the runtime cache"]
async fn postgresql_18_beside_17_journey() {
    let ev = evidence!(
        covers = ["REQ-035", "AC-035", "AC-009", "REQ-043", "BR-001"],
        kind = Kind::Native,
        expected = "JOURNEY-03: runtime.install of PostgreSQL 18 from the cache, with no network and while pg17-dev runs and pg17-test is stopped, changes nothing of either instance (same registry fields, the same process and restart count, the same bytes under the stopped instance); pg18-dev is created on the new runtime and the three instances run ready together on runtime versions 17.11, 17.11, and 18.6, each psql of its own runtime reports a server of its own major, system identifiers and folders differ, and the three sentinels stay isolated over a stop and a start of each instance in turn with every runtime ID unchanged (the backup and restore part of AC-035 belongs to features 51 and 52)",
    );
    let Some(three) = three_postgresql_clusters(&ev).await else {
        return;
    };
    let ThreeClusters {
        site,
        daemon,
        api,
        two,
        pg18,
        made18,
        runtime18,
        dev_value,
        test_value,
        pg18_value,
    } = three;

    // AC-2: pg17-test starts again, and all three are ready together.
    let test_up = api.start_ready(two.test.id).await;
    assert_eq!(
        test_up.runtime_id, site.runtime_id,
        "pg17-test stayed on its runtime"
    );
    let version17 = two
        .dev
        .psql
        .scalar(&two.dev.password, "SHOW server_version");
    let version18 = pg18.psql.scalar(&pg18.password, "SHOW server_version");
    let test_version = two
        .test
        .psql
        .scalar(&two.test.password, "SHOW server_version");
    subjects(&ev, &version17, &version18);
    assert!(version17.starts_with("17."), "{version17}");
    assert_eq!(test_version, version17);
    assert!(version18.starts_with("18."), "{version18}");
    assert_eq!(version17, PG17.version);
    assert_eq!(version18, PG18.version);

    let page = api.list(ListInstancesParams::default()).await;
    let by_name: BTreeMap<&str, &InstanceSummary> = page
        .items
        .iter()
        .map(|summary| (summary.name.as_str(), summary))
        .collect();
    assert_eq!(by_name.len(), 3);
    assert_eq!(by_name["pg17-dev"].runtime_version, "17.11");
    assert_eq!(by_name["pg17-test"].runtime_version, "17.11");
    assert_eq!(by_name["pg18-dev"].runtime_version, "18.6");
    assert_eq!(by_name["pg17-dev"].runtime_id, site.runtime_id);
    assert_eq!(by_name["pg17-test"].runtime_id, site.runtime_id);
    assert_eq!(by_name["pg18-dev"].runtime_id, runtime18.id);
    assert!(
        by_name
            .values()
            .all(|summary| summary.readiness.state == InstanceReadinessState::Ready)
    );

    let clusters = [&two.dev, &two.test, &pg18];
    let identifiers: BTreeSet<String> = clusters
        .iter()
        .map(|cluster| cluster.system_identifier())
        .collect();
    assert_eq!(
        identifiers.len(),
        3,
        "the clusters share a system identifier"
    );
    let ports: BTreeSet<u16> = by_name.values().map(|summary| port_of(summary)).collect();
    assert_eq!(ports.len(), 3, "two instances share a port");

    // PG_VERSION and the manifest say 17, 17, and 18.
    for (cluster, line) in [(&two.dev, "17"), (&two.test, "17"), (&pg18, "18")] {
        assert_eq!(
            text(&site.instance_paths(cluster.id).data.join("PG_VERSION")),
            line,
            "PG_VERSION of {}",
            cluster.name
        );
        assert_eq!(
            initialized_line(&site, cluster.id),
            line,
            "data.initializedLine of {}",
            cluster.name
        );
    }

    // Nothing of pg18-dev sits where another instance sits.
    let theirs = [
        site.instance_paths(two.dev.id),
        site.instance_paths(two.test.id),
    ];
    let mine = site.instance_paths(made18.id);
    for other in &theirs {
        for (own, foreign) in [
            (&mine.root, &other.root),
            (&mine.manifest, &other.manifest),
            (&mine.lock, &other.lock),
            (&mine.data, &other.data),
            (&mine.config, &other.config),
            (&mine.config_history, &other.config_history),
            (&mine.temp, &other.temp),
            (&mine.logs, &other.logs),
            (&mine.run, &other.run),
        ] {
            assert_ne!(own, foreign);
        }
    }

    // AC-3: three sentinels, isolated, also over a stop and a start of each.
    let values = [
        (&two.dev, dev_value.as_str()),
        (&two.test, test_value.as_str()),
        (&pg18, pg18_value.as_str()),
    ];
    SentinelCluster::assert_isolated(&values);
    let identity_before: Vec<(Uuid, ProviderPin)> = {
        let mut pairs = Vec::new();
        for (cluster, _) in &values {
            let summary = api.get(cluster.id).await.expect("the instance reads");
            pairs.push((summary.runtime_id, summary.provider));
        }
        pairs
    };
    for down in 0..values.len() {
        let (cluster, _) = values[down];
        let port_before = port_of(&api.get(cluster.id).await.expect("the instance reads"));
        stop(&api, cluster.id).await;
        let stopped = api.get(cluster.id).await.expect("the instance reads");
        assert_eq!(stopped.lifecycle, InstanceLifecycle::Stopped);
        for (other, value) in values.iter().filter(|(other, _)| other.id != cluster.id) {
            let summary = api.get(other.id).await.expect("the instance reads");
            assert_eq!(
                summary.readiness.state,
                InstanceReadinessState::Ready,
                "{} went down with {}",
                other.name,
                cluster.name
            );
            SentinelCluster::assert_isolated(&[(*other, *value)]);
        }
        let up = api.start_ready(cluster.id).await;
        assert_eq!(port_of(&up), port_before, "the port stays the same");
        SentinelCluster::assert_isolated(&values);
    }
    for ((cluster, _), (runtime_id, provider)) in values.iter().zip(&identity_before) {
        let summary = api.get(cluster.id).await.expect("the instance reads");
        assert_eq!(
            &summary.runtime_id, runtime_id,
            "{} moved to another runtime",
            cluster.name
        );
        assert_eq!(&summary.provider, provider);
    }
    assert_eq!(
        two.dev
            .psql
            .scalar(&two.dev.password, "SHOW server_version"),
        version17
    );
    assert_eq!(
        pg18.psql.scalar(&pg18.password, "SHOW server_version"),
        version18
    );

    stop(&api, two.test.id).await;
    stop(&api, made18.id).await;
    crate::harness::stop_and_shutdown(&api, daemon, two.dev.id).await;
    ev.pass_with(&format!(
        "PostgreSQL {version17} and {version18} ran ready together (runtime versions 17.11, 17.11, 18.6; runtime {} and {}); the install of 18 from the cache changed nothing of pg17-dev or pg17-test; three system identifiers {} differ; the sentinels stayed isolated over a stop and a start of each instance in turn, and every runtime ID and provider pin was unchanged",
        site.runtime_id,
        runtime18.id,
        identifiers.into_iter().collect::<Vec<_>>().join(", ")
    ));
}

/// The failure of a start the data refuses.
struct Refused {
    error: ErrorBody,
    detail: OperationDetail,
}

/// Starts `id`, waits for the operation to fail, and returns it.
async fn refused_start(api: &Api, id: Uuid) -> Refused {
    let accepted = api
        .start(id)
        .await
        .expect("the start is accepted")
        .accepted
        .expect("a start from stopped begins an operation");
    let detail = api.wait_final(accepted.operation.id).await;
    assert_eq!(
        detail.operation.state,
        OperationState::Failed,
        "the start was not refused: {detail:?}"
    );
    let error = detail
        .operation
        .failure
        .clone()
        .expect("the failed operation carries its error")
        .error;
    Refused { error, detail }
}

/// The hint the catalog of the area gives for an `incompatible` outcome.
fn incompatible_hint() -> &'static str {
    supervisor::hint(
        supervisor::DATA_INCOMPATIBLE.code,
        &[ErrorDetail::new("outcome", "incompatible")],
    )
    .expect("an incompatible outcome has a hint")
}

/// Asserts the start was refused as the spec says: data of `data` on a
/// runtime of `runtime`, at `verify`, before the journal and any spawn,
/// with the migration hint.
fn assert_refused(refused: &Refused, data: &str, runtime: &str) {
    let error = &refused.error;
    assert_eq!(
        error.code(),
        supervisor::DATA_INCOMPATIBLE.code,
        "{error:?}"
    );
    assert_eq!(error.detail("outcome"), Some("incompatible"));
    assert_eq!(error.detail("data"), Some(data));
    assert_eq!(error.detail("runtime"), Some(runtime));
    assert_eq!(error.hint(), Some(incompatible_hint()));
    let steps: Vec<(&str, StepState)> = refused
        .detail
        .steps
        .iter()
        .map(|step| (step.name.as_str(), step.state))
        .collect();
    assert_eq!(
        steps.last(),
        Some(&("verify", StepState::Failed)),
        "the start did not stop at verify: {steps:?}"
    );
    for later in ["journal", "render", "spawn"] {
        assert!(
            steps.iter().all(|(name, _)| *name != later),
            "the start reached {later}: {steps:?}"
        );
    }
}

/// The check refused `instance` on `target`: the code and details of the
/// error, and the migration hint over them.
fn assert_check_refuses(
    registry: &dyn Registry,
    site: &Site,
    instance: Uuid,
    target: RuntimeId,
    data: &str,
    runtime: &str,
) {
    let refusal = rebind::check(
        registry,
        &site.platform,
        &site.prepared,
        instance_id(instance),
        target,
    )
    .expect_err("data of one major never runs on a runtime of the other");
    assert_eq!(refusal.code().as_str(), supervisor::DATA_INCOMPATIBLE.code);
    let details: Vec<ErrorDetail> = refusal
        .details()
        .iter()
        .map(|(key, value)| ErrorDetail::new(key.as_str(), value.as_str()))
        .collect();
    let find = |name: &str| {
        details
            .iter()
            .find(|detail| detail.key() == name)
            .map(|detail| detail.value())
    };
    assert_eq!(find("outcome"), Some("incompatible"), "{details:?}");
    assert_eq!(find("data"), Some(data), "{details:?}");
    assert_eq!(find("runtime"), Some(runtime), "{details:?}");
    assert_eq!(
        supervisor::hint(refusal.code().as_str(), &details),
        Some(incompatible_hint())
    );
}

/// The rows of the registry the check could touch: the three instances and
/// the two runtimes, with the generation they were read at.
fn rows(registry: &dyn Registry, instances: [Uuid; 3], runtimes: [Uuid; 2]) -> Rows {
    let instances = instances
        .map(|id| {
            let read = registry
                .instance(instance_id(id))
                .expect("the instance reads");
            (
                read.value.expect("the instance exists"),
                read.generation.get(),
            )
        })
        .to_vec();
    let runtimes = runtimes
        .map(|id| {
            let read = registry
                .runtime(id.to_string().parse().expect("a runtime id"))
                .expect("the runtime reads");
            (
                read.value.expect("the runtime exists"),
                read.generation.get(),
            )
        })
        .to_vec();
    (instances, runtimes)
}

/// Waits until the daemon adopted the postmaster of `id` and the instance
/// is ready, and returns the summary.
async fn adopted_and_ready(api: &Api, id: Uuid) -> InstanceSummary {
    api.wait_instance(id, "adopted and ready", |summary| {
        summary
            .process
            .as_ref()
            .is_some_and(|process| process.adopted)
            && summary.readiness.state == InstanceReadinessState::Ready
    })
    .await
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs both managed PostgreSQL artifacts in the runtime cache"]
async fn another_major_is_refused_at_start_and_by_the_rebind_check() {
    let ev = evidence!(
        covers = ["BR-009", "AC-035", "AC-043", "REQ-043"],
        kind = Kind::Native,
        expected = "JOURNEY-03, the refusal: with pg17-test and pg18-dev stopped, the read-only rebind check of the core answers supervisor.data_incompatible (outcome incompatible, the right lines, the migration hint) for 17 data on the runtime 18 and for 18 data on the runtime 17, and Current for pg17-test on its own runtime, writing nothing (the same registry rows and generation, byte for byte the same manifests and folders); with the registry written by the test so each instance is bound to the other major, instance.start for both fails at verify, before the journal and any spawn, with the hint, leaves them stopped with no postmaster and their folders byte for byte unchanged, while pg17-dev is adopted again with the same process and reads its sentinel; bound back, all three run ready and isolated. The proof of AC-043 is partial: the core check on real data, while the user actions (method, CLI, GUI, controlled stop, backup guidance) belong to feature 58",
    );
    let Some(three) = three_postgresql_clusters(&ev).await else {
        return;
    };
    let ThreeClusters {
        site,
        daemon,
        api,
        two,
        pg18,
        made18,
        runtime18,
        dev_value,
        test_value,
        pg18_value,
    } = three;
    let version17 = two
        .dev
        .psql
        .scalar(&two.dev.password, "SHOW server_version");
    let version18 = pg18.psql.scalar(&pg18.password, "SHOW server_version");
    subjects(&ev, &version17, &version18);
    let values = [
        (&two.dev, dev_value.as_str()),
        (&two.test, test_value.as_str()),
        (&pg18, pg18_value.as_str()),
    ];
    api.start_ready(two.test.id).await;
    SentinelCluster::assert_isolated(&values);

    // pg17-test and pg18-dev stop; pg17-dev runs on.
    stop(&api, two.test.id).await;
    stop(&api, made18.id).await;
    let stopped = [two.test.id, made18.id];
    let mut trees: Vec<[Tree; 2]> = Vec::new();
    for id in stopped {
        trees.push(settled(&site, id).await);
    }
    let manifests = |site: &Site| -> Vec<Vec<u8>> {
        [two.dev.id, two.test.id, made18.id]
            .map(|id| std::fs::read(site.instance_paths(id).manifest).expect("a manifest reads"))
            .to_vec()
    };
    let manifests_before = manifests(&site);
    let dev_before = api.get(two.dev.id).await.expect("pg17-dev reads");
    let dev_process = dev_before
        .process
        .as_ref()
        .map(|process| (process.pid, process.started_at))
        .expect("pg17-dev runs");
    let test_data = site.instance_paths(two.test.id).data;
    let pg18_data = site.instance_paths(made18.id).data;

    // The daemon stops; the postmaster of pg17-dev stays.
    drop(api);
    daemon.shutdown().await;

    // AC-7: the check on real data, read only.
    let runtime17 = site.own_runtime();
    let runtime18_pair = runtime_pair(&runtime18, PG18);
    let ids = [two.dev.id, two.test.id, made18.id];
    let runtimes = [site.runtime_id, runtime18.id];
    {
        let registry = site.open_registry();
        let before = rows(&registry, ids, runtimes);
        assert_check_refuses(&registry, &site, two.test.id, runtime18_pair.0, "17", "18");
        assert_check_refuses(&registry, &site, made18.id, runtime17.0, "18", "17");
        let verdict = rebind::check(
            &registry,
            &site.platform,
            &site.prepared,
            instance_id(two.test.id),
            runtime17.0,
        )
        .expect("pg17-test may stay on its own runtime");
        assert_eq!(verdict, RebindVerdict::Current);
        assert_eq!(
            rows(&registry, ids, runtimes),
            before,
            "the check wrote to the registry"
        );
    }
    assert_eq!(manifests(&site), manifests_before);
    for (id, before) in stopped.iter().zip(&trees) {
        assert_eq!(&settled(&site, *id).await, before, "the check wrote files");
    }

    // AC-4: each instance is bound to the other major, with no daemon.
    site.rebind_instance(two.test.id, &runtime18_pair);
    site.rebind_instance(made18.id, &runtime17);
    assert_eq!(manifests(&site), manifests_before);
    for (id, before) in stopped.iter().zip(&trees) {
        assert_eq!(
            &settled(&site, *id).await,
            before,
            "the registry write touched files"
        );
    }
    let daemon = site.in_process_offline(|_| {}).await;
    let api = site.api().await;
    let dev_again = adopted_and_ready(&api, two.dev.id).await;
    assert_eq!(
        dev_again
            .process
            .as_ref()
            .map(|process| (process.pid, process.started_at)),
        Some(dev_process),
        "pg17-dev is not the same process"
    );
    SentinelCluster::assert_isolated(&[(&two.dev, dev_value.as_str())]);

    let test_refused = refused_start(&api, two.test.id).await;
    assert_refused(&test_refused, "17", "18");
    let pg18_refused = refused_start(&api, made18.id).await;
    assert_refused(&pg18_refused, "18", "17");
    for (id, data) in [(two.test.id, &test_data), (made18.id, &pg18_data)] {
        let summary = api.get(id).await.expect("the instance reads");
        assert_eq!(summary.lifecycle, InstanceLifecycle::Stopped, "{summary:?}");
        assert!(summary.process.is_none(), "{summary:?}");
        assert!(serving(data).is_empty());
        crate::phases::nothing_was_spawned(data);
    }
    assert_eq!(manifests(&site), manifests_before);
    for (id, before) in stopped.iter().zip(&trees) {
        assert_eq!(
            &settled(&site, *id).await,
            before,
            "the refused start touched files"
        );
    }

    // Bound back, everything runs again and nothing was read across.
    drop(api);
    daemon.shutdown().await;
    site.rebind_instance(two.test.id, &runtime17);
    site.rebind_instance(made18.id, &runtime18_pair);
    let daemon = site.in_process_offline(|_| {}).await;
    let api = site.api().await;
    adopted_and_ready(&api, two.dev.id).await;
    api.start_ready(two.test.id).await;
    api.start_ready(made18.id).await;
    SentinelCluster::assert_isolated(&values);
    assert_eq!(
        two.dev
            .psql
            .scalar(&two.dev.password, "SHOW server_version"),
        version17
    );
    assert_eq!(
        pg18.psql.scalar(&pg18.password, "SHOW server_version"),
        version18
    );

    stop(&api, two.test.id).await;
    stop(&api, made18.id).await;
    crate::harness::stop_and_shutdown(&api, daemon, two.dev.id).await;
    ev.pass_with(&format!(
        "PostgreSQL {version17} data was refused by runtime {version18} and the other way round, by rebind::check and by instance.start (verify, hint incompatible); registry rows, manifests, and folders were unchanged, pg17-dev was adopted with the same process, and after binding back the three clusters ran ready with isolated sentinels; AC-043 is proven in part (the core check on real data), the user actions belong to feature 58"
    ));
}
