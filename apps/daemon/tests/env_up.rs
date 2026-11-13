//! `env.up` through the real daemon (spec 0046 milestone 2, build step 6),
//! with the fake provider only: the precheck refuses without a mutation, a
//! chain comes up in dependency order with leases and a report, a failed
//! create skips its dependents and the Up ends `env.up_incomplete`, a
//! second Up of the same environment is busy, a cancel records the rest
//! `cancelled`, and a borrowed instance gets one shared lease and is left
//! as it was. No edge stands for a real dependency between databases.
#![cfg(target_os = "macos")]

mod common;
#[path = "supervisor/support.rs"]
mod support;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use stackly_core::catalog::{
    Catalog, CatalogEntry, EntryOrigin, ProviderEntry, TileColor, TileHint,
};
use stackly_core::provider::{NameCheck, ValidatedPackage, read_package};
use stackly_domain::{
    EnvironmentId, InstanceId, LeaseFilter, LeaseHolder, LeaseState, OperationQuery, Registry as _,
};
use stackly_ipc::CallOptions;
use stackly_protocol::env::{
    EnvGetParams, EnvPlan, EnvPlanParams, EnvSource, EnvUpParams, EnvUpReply, EnvironmentDetail,
    NotReadyReason, UpPhase, UpStage, UpState,
};
use stackly_protocol::operation::{CancelOperationParams, OperationKind, OperationState};
use stackly_protocol::{Call, ErrorBody, Reply};
use stackly_testkit::evidence::{Kind, evidence};
use stacklyd::server::DaemonHandle;
use uuid::Uuid;

use crate::support::{Site, fast, key};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn catalog() -> Catalog {
    let dir = repo().join("providers").join("fake");
    let files =
        read_package(&stackly_platform_macos::MacosFilesystem, &dir).expect("the package reads");
    let package =
        ValidatedPackage::from_files(files, NameCheck::FolderName).expect("the package validates");
    Catalog::for_tests(&[CatalogEntry::Provider(ProviderEntry {
        package: Arc::new(package),
        tile: TileHint {
            mark: "Te".to_owned(),
            tile: TileColor::Blue,
        },
        notes: Vec::new(),
        origin: EntryOrigin::Catalog,
    })])
}

/// A daemon whose catalog holds the fake package. The seed pinned it with
/// a test's permissions, so it goes first and create pins it again.
async fn start(site: &Site) -> DaemonHandle {
    site.unpin_fake_package();
    let mut config = fast();
    config.catalog = Some(stacklyd::catalog::CatalogOverride::Preloaded(catalog()));
    site.start_with(config).await
}

async fn stop(daemon: DaemonHandle) {
    daemon
        .shutdown(
            stackly_protocol::events::ShutdownReason::Requested,
            std::future::pending(),
        )
        .await;
}

fn write(site: &Site, name: &str, text: &str) -> PathBuf {
    let dir = site.root.path().join("project");
    std::fs::create_dir_all(&dir).expect("the project folder can be made");
    let path = dir.join(name);
    std::fs::write(&path, text).expect("the manifest writes");
    path
}

/// A chain on the fake provider: `api` waits for `db`, `web` for `api`.
/// `api_extra` goes under the settings of `api`.
fn chain(name: &str, api_settings: &str) -> String {
    format!(
        "apiVersion: stackly/v1alpha1\nkind: Environment\nmetadata:\n  name: {name}\nservices:\n  db:\n    provider: testing/fake\n    version: \"0.1\"\n  api:\n    provider: testing/fake\n    version: \"0.1\"\n    dependsOn: [{{service: db}}]\n{api_settings}  web:\n    provider: testing/fake\n    version: \"0.1\"\n    dependsOn: [{{service: api}}]\n"
    )
}

async fn plan(api: &support::Api, path: &Path) -> EnvPlan {
    match api
        .client
        .call(Call::EnvPlan(EnvPlanParams {
            source: EnvSource::File(path.display().to_string()),
            locked: false,
        }))
        .await
        .expect("the plan answers")
    {
        Reply::EnvPlan(plan) => *plan,
        other => panic!("not a plan: {other:?}"),
    }
}

async fn up_with(
    api: &support::Api,
    path: &Path,
    digest: &str,
) -> Result<EnvUpReply, Box<ErrorBody>> {
    let params = EnvUpParams {
        source: EnvSource::File(path.display().to_string()),
        expected_plan_digest: digest.to_owned(),
        locked: false,
    };
    match api
        .client
        .call_detailed(
            Call::EnvUp(params),
            CallOptions::default().with_idempotency_key(key()),
        )
        .await?
    {
        Reply::EnvUp(reply) => Ok(*reply),
        other => panic!("not an up reply: {other:?}"),
    }
}

/// Plans `path`, then brings it up with the plan's digest.
async fn up(api: &support::Api, path: &Path) -> EnvUpReply {
    let planned = plan(api, path).await;
    let digest = planned
        .plan_digest
        .unwrap_or_else(|| panic!("the plan is applicable: {:?}", planned.blockers));
    up_with(api, path, &digest).await.expect("the up starts")
}

async fn get(api: &support::Api, id: Uuid) -> EnvironmentDetail {
    try_get(api, id).await.expect("the detail answers")
}

/// `env.get`, which answers `env.not_found` until `accept` committed.
async fn try_get(api: &support::Api, id: Uuid) -> Result<EnvironmentDetail, Box<ErrorBody>> {
    match api
        .client
        .call(Call::EnvGet(EnvGetParams {
            id,
            check_manifest: false,
        }))
        .await?
    {
        Reply::EnvGet(detail) => Ok(*detail),
        other => panic!("not a detail: {other:?}"),
    }
}

/// The registry generation and the number of operations and instances.
fn counts(daemon: &DaemonHandle) -> (stackly_domain::Generation, usize, usize) {
    let registry = daemon.core().registry().expect("the registry is open");
    let ids = registry.known_instance_ids().expect("reads");
    let operations = registry
        .operations(OperationQuery {
            active_only: false,
            resource: None,
            limit: 500,
            before: None,
        })
        .expect("reads")
        .value
        .items
        .len();
    (ids.generation, operations, ids.value.len())
}

fn active_leases(daemon: &DaemonHandle, instance: Uuid, environment: Uuid) -> usize {
    let registry = daemon.core().registry().expect("the registry is open");
    let instance = InstanceId::from_uuid(instance).expect("an instance ID");
    let environment = EnvironmentId::from_uuid(environment).expect("an environment ID");
    registry
        .leases(instance, LeaseFilter::All)
        .expect("reads")
        .value
        .iter()
        .filter(|lease| {
            lease.holder() == LeaseHolder::Environment(environment)
                && lease.state() == LeaseState::Active
        })
        .count()
}

// covers: AC-1, AC-2 (spec 0046)
#[tokio::test(flavor = "multi_thread")]
async fn a_refused_up_starts_no_operation_and_writes_nothing() {
    let ev = evidence!(
        covers = ["REQ-021"],
        kind = Kind::Fixture,
        expected = "env.up refuses an invalid manifest with env.manifest_invalid, a digest the client did not review with env.plan_changed, and a secretRefs reference without a value with secret.needs_secret naming it, and none of them moves the registry generation, adds an operation, or creates an instance",
    );
    let site = Site::new();
    let daemon = start(&site).await;
    let api = site.api().await;
    let invalid = write(
        &site,
        "invalid.yaml",
        "apiVersion: stackly/v1alpha1\nkind: Environment\n",
    );
    let good = write(&site, "stackly.yaml", &chain("refused", ""));
    let secret = write(
        &site,
        "secret.yaml",
        "apiVersion: stackly/v1alpha1\nkind: Environment\nmetadata:\n  name: secretive\nservices:\n  db:\n    provider: testing/fake\n    version: \"0.1\"\n    secretRefs:\n      authToken: \"local:secretive/token\"\n",
    );
    let before = counts(&daemon);

    let invalid_error = up_with(&api, &invalid, &"0".repeat(64))
        .await
        .expect_err("an invalid manifest is refused");
    assert_eq!(invalid_error.code(), "env.manifest_invalid");

    let changed = up_with(&api, &good, &"0".repeat(64))
        .await
        .expect_err("another digest is refused");
    assert_eq!(changed.code(), "env.plan_changed");
    assert!(changed.hint().is_some(), "{changed:?}");

    let secret_plan = plan(&api, &secret).await;
    let digest = secret_plan.plan_digest.unwrap_or_else(|| "0".repeat(64));
    let needs = up_with(&api, &secret, &digest)
        .await
        .expect_err("a reference without a value is refused");
    assert_eq!(needs.code(), "secret.needs_secret", "{needs:?}");
    assert_eq!(
        needs.items()[0].field("reference"),
        Some("local:secretive/token")
    );

    assert_eq!(counts(&daemon), before, "nothing was written");
    stop(daemon).await;
    ev.pass();
}

// covers: AC-3, AC-4, AC-7, AC-8, AC-13, AC-16, AC-24 (spec 0046)
#[tokio::test(flavor = "multi_thread")]
async fn up_brings_a_chain_up_in_order_with_leases_and_a_report() {
    let ev = evidence!(
        covers = ["REQ-021", "AC-021"],
        kind = Kind::Fixture,
        expected = "env.up of db, api waiting for db, and web waiting for api registers the environment, creates three dedicated members each with one active environment lease, starts db before api and api before web, succeeds, and env.get shows lastUp succeeded with every service ready and each endpoint's bound port, and no active operation; each child names the Up as its parent",
    );
    let site = Site::new();
    let daemon = start(&site).await;
    let api = site.api().await;
    let path = write(&site, "stackly.yaml", &chain("ordered", ""));

    let reply = up(&api, &path).await;
    assert_eq!(reply.name, "ordered");
    assert_eq!(reply.operation.operation.kind, OperationKind::EnvironmentUp);
    let done = api.wait_final(reply.operation.operation.id).await;
    assert_eq!(done.operation.state, OperationState::Succeeded, "{done:?}");

    let detail = get(&api, reply.environment_id).await;
    assert_eq!(detail.summary.active_operation, None);
    let last = detail.last_up.expect("lastUp is written");
    assert_eq!(last.state, UpState::Succeeded);
    assert!(last.services.iter().all(|s| s.phase == UpPhase::Ready));
    assert_eq!(detail.services.len(), 3);
    let mut started = Vec::new();
    for service in &detail.services {
        assert!(service.not_ready.is_none(), "{service:?}");
        let instance = service.instance.as_ref().expect("each service has one");
        assert_eq!(
            active_leases(&daemon, instance.id, reply.environment_id),
            1,
            "{}",
            service.alias
        );
        assert!(
            instance
                .endpoints
                .iter()
                .all(|endpoint| endpoint.port.is_some()),
            "{instance:?}"
        );
        let report = last
            .services
            .iter()
            .find(|entry| entry.alias == service.alias)
            .expect("the report names it");
        let child = api
            .operation(report.operation_id.expect("a child started it"))
            .await;
        assert_eq!(child.operation.parent, Some(reply.operation.operation.id));
        started.push((
            child.operation.started_at.expect("the child ran"),
            service.alias.clone(),
        ));
    }
    started.sort();
    let order: Vec<&str> = started.iter().map(|(_, alias)| alias.as_str()).collect();
    assert_eq!(order, ["db", "api", "web"]);

    let stats = match api.client.call(Call::OperationGetStats).await {
        Ok(Reply::OperationGetStats(stats)) => stats,
        other => panic!("not stats: {other:?}"),
    };
    assert_eq!(stats.max_coordinators, 4);
    stop(daemon).await;
    ev.pass();
}

// covers: AC-7, AC-8, AC-10, AC-12, AC-16 (spec 0046)
#[tokio::test(flavor = "multi_thread")]
async fn a_failed_create_skips_its_dependents_and_ends_the_up_incomplete() {
    let ev = evidence!(
        covers = ["REQ-021", "AC-021", "BR-023"],
        kind = Kind::Fixture,
        expected = "env.up where the create of api fails at init leaves db ready, ends api failed at create and web skipped waiting for api, fails with env.up_incomplete (ready 1 of 3), leaves no instance or member of api behind, and env.get explains api with up_failed and web with dependency_not_ready",
    );
    let site = Site::new();
    let daemon = start(&site).await;
    let api = site.api().await;
    let path = write(
        &site,
        "stackly.yaml",
        &chain("partial", "    settings:\n      failInit: true\n"),
    );

    let reply = up(&api, &path).await;
    let done = api.wait_final(reply.operation.operation.id).await;
    assert_eq!(done.operation.state, OperationState::Failed);
    let failure = done
        .operation
        .failure
        .as_ref()
        .map(|failure| &failure.error)
        .expect("the Up stored its error");
    assert_eq!(failure.code(), "env.up_incomplete");
    assert!(
        failure.hint().is_some(),
        "operation.get carries the env hint (AC-10): {failure:?}"
    );
    assert_eq!(failure.detail("ready"), Some("1"));
    assert_eq!(failure.detail("total"), Some("3"));
    assert_eq!(done.operation.stage.as_deref(), Some("create"));

    let detail = get(&api, reply.environment_id).await;
    let last = detail.last_up.as_ref().expect("lastUp is written");
    assert_eq!(last.state, UpState::Failed);
    let phase = |alias: &str| {
        last.services
            .iter()
            .find(|service| service.alias == alias)
            .expect("the report names it")
    };
    assert_eq!(phase("db").phase, UpPhase::Ready);
    assert_eq!(phase("api").phase, UpPhase::Failed);
    assert_eq!(phase("api").stage, Some(UpStage::Create));
    assert_eq!(phase("web").phase, UpPhase::Skipped);
    let reason = |alias: &str| {
        detail
            .services
            .iter()
            .find(|service| service.alias == alias)
            .and_then(|service| service.not_ready.as_ref())
            .map(|not_ready| not_ready.reason)
    };
    assert_eq!(reason("api"), Some(NotReadyReason::UpFailed));
    assert_eq!(reason("web"), Some(NotReadyReason::DependencyNotReady));
    let api_service = detail
        .services
        .iter()
        .find(|service| service.alias == "api")
        .expect("api is a service");
    assert!(api_service.instance.is_none(), "the create compensated");
    stop(daemon).await;
    ev.pass();
}

// covers: AC-3, AC-11, AC-16 (spec 0046)
#[tokio::test(flavor = "multi_thread")]
async fn a_second_up_is_busy_and_a_cancel_records_the_rest_cancelled() {
    let ev = evidence!(
        covers = ["REQ-021"],
        kind = Kind::Fixture,
        expected = "while an Up whose db is slow to become ready runs, a second env.up of the same environment is refused resource.busy naming the first, env.get shows the Up as the active operation with lastUp running, and operation.cancel ends it cancelled with lastUp cancelled and the members and leases kept",
    );
    let site = Site::new();
    let daemon = start(&site).await;
    let api = site.api().await;
    let manifest = "apiVersion: stackly/v1alpha1\nkind: Environment\nmetadata:\n  name: slow\nservices:\n  db:\n    provider: testing/fake\n    version: \"0.1\"\n    settings:\n      readyDelayMs: 20000\n  api:\n    provider: testing/fake\n    version: \"0.1\"\n    dependsOn: [{service: db}]\n";
    let path = write(&site, "stackly.yaml", manifest);

    let planned = plan(&api, &path).await;
    let digest = planned.plan_digest.expect("an applicable plan");
    let first = up_with(&api, &path, &digest).await.expect("the up starts");
    let busy = up_with(&api, &path, &digest)
        .await
        .expect_err("a second Up is refused");
    assert_eq!(busy.code(), "resource.busy");

    // Wait until the slow start runs.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        let Ok(detail) = try_get(&api, first.environment_id).await else {
            tokio::time::sleep(Duration::from_millis(50)).await;
            continue;
        };
        let starting = detail.last_up.as_ref().is_some_and(|last| {
            last.state == UpState::Running
                && last
                    .services
                    .iter()
                    .any(|service| service.alias == "db" && service.phase == UpPhase::Starting)
        });
        if starting {
            assert_eq!(
                detail.summary.active_operation.map(|active| active.id),
                Some(first.operation.operation.id)
            );
            break;
        }
        assert!(tokio::time::Instant::now() < deadline, "{detail:?}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let cancelled = api
        .client
        .call(Call::OperationCancel(CancelOperationParams {
            operation_id: first.operation.operation.id,
        }))
        .await;
    assert!(cancelled.is_ok(), "{cancelled:?}");
    let done = api.wait_final(first.operation.operation.id).await;
    assert_eq!(done.operation.state, OperationState::Cancelled, "{done:?}");

    let detail = get(&api, first.environment_id).await;
    let last = detail.last_up.expect("lastUp is written");
    assert_eq!(last.state, UpState::Cancelled);
    assert!(
        last.services
            .iter()
            .all(|service| service.phase == UpPhase::Cancelled),
        "{last:?}"
    );
    for service in &detail.services {
        let instance = service.instance.as_ref().expect("the members stay");
        assert_eq!(active_leases(&daemon, instance.id, first.environment_id), 1);
    }
    stop(daemon).await;
    ev.pass();
}

// covers: AC-5, AC-9, AC-14 (spec 0046)
#[tokio::test(flavor = "multi_thread")]
async fn a_borrowed_instance_gets_one_shared_lease_and_is_left_as_it_was() {
    let ev = evidence!(
        covers = ["VAL-014", "AC-073"],
        kind = Kind::Fixture,
        expected = "two env.up runs of a manifest whose cache borrows a running instance through instanceRef add one shared member and exactly one active lease, start no child for it, and leave its provider pin, runtime, endpoints, desired state, restart count, and process as they were",
    );
    let site = Site::new();
    let daemon = start(&site).await;
    let api = site.api().await;
    let shared = api
        .create_ready("shared-cache", serde_json::Value::Null)
        .await;
    let before = api.start_ready(shared).await;
    let manifest = "apiVersion: stackly/v1alpha1\nkind: Environment\nmetadata:\n  name: borrow\nservices:\n  cache:\n    provider: testing/fake\n    instanceRef: shared-cache\n  api:\n    provider: testing/fake\n    version: \"0.1\"\n    dependsOn: [{service: cache}]\n";
    let path = write(&site, "stackly.yaml", manifest);

    let first = up(&api, &path).await;
    let done = api.wait_final(first.operation.operation.id).await;
    assert_eq!(done.operation.state, OperationState::Succeeded, "{done:?}");
    let second = up(&api, &path).await;
    let done = api.wait_final(second.operation.operation.id).await;
    assert_eq!(done.operation.state, OperationState::Succeeded, "{done:?}");

    assert_eq!(active_leases(&daemon, shared, first.environment_id), 1);
    let detail = get(&api, first.environment_id).await;
    let last = detail.last_up.expect("lastUp is written");
    let cache = last
        .services
        .iter()
        .find(|service| service.alias == "cache")
        .expect("the report names cache");
    assert_eq!(cache.phase, UpPhase::Ready);
    assert_eq!(cache.operation_id, None, "no child for a running instance");
    let after = api.get(shared).await.expect("the instance reads");
    assert_eq!(after.provider, before.provider);
    assert_eq!(after.runtime_id, before.runtime_id);
    assert_eq!(after.endpoints, before.endpoints);
    assert_eq!(after.desired, before.desired);
    assert_eq!(after.restart_total, before.restart_total);
    assert_eq!(
        after.process.as_ref().map(|process| process.pid),
        before.process.as_ref().map(|process| process.pid),
        "Up never restarts a running instance"
    );
    stop(daemon).await;
    ev.pass();
}

// covers: AC-13, AC-15, AC-16 (spec 0046)
#[tokio::test(flavor = "multi_thread")]
async fn a_daemon_restart_in_the_middle_of_an_up_shows_it_interrupted_and_keeps_the_leases() {
    let ev = evidence!(
        covers = ["REQ-021", "AC-073"],
        kind = Kind::Fixture,
        expected = "a daemon that stops while an Up waits for a slow start comes back with the Up closed failed, env.get showing lastUp interrupted with no active operation, and every member's environment lease still active",
    );
    let site = Site::new();
    let daemon = start(&site).await;
    let api = site.api().await;
    let manifest = "apiVersion: stackly/v1alpha1\nkind: Environment\nmetadata:\n  name: restarted\nservices:\n  db:\n    provider: testing/fake\n    version: \"0.1\"\n    settings:\n      readyDelayMs: 20000\n";
    let path = write(&site, "stackly.yaml", manifest);
    let reply = up(&api, &path).await;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    let instance = loop {
        let Ok(detail) = try_get(&api, reply.environment_id).await else {
            tokio::time::sleep(Duration::from_millis(50)).await;
            continue;
        };
        let running = detail.last_up.as_ref().is_some_and(|last| {
            last.services
                .iter()
                .any(|service| service.phase == UpPhase::Starting)
        });
        if running && let Some(instance) = detail.services[0].instance.as_ref() {
            break instance.id;
        }
        assert!(tokio::time::Instant::now() < deadline, "{detail:?}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    drop(api);
    stop(daemon).await;

    let mut config = fast();
    config.catalog = Some(stacklyd::catalog::CatalogOverride::Preloaded(catalog()));
    let daemon = site.start_with(config).await;
    let api = site.api().await;
    let done = api.wait_final(reply.operation.operation.id).await;
    assert_eq!(done.operation.state, OperationState::Failed, "{done:?}");
    let detail = get(&api, reply.environment_id).await;
    assert_eq!(detail.summary.active_operation, None);
    assert_eq!(
        detail.last_up.map(|last| last.state),
        Some(UpState::Interrupted)
    );
    assert_eq!(active_leases(&daemon, instance, reply.environment_id), 1);
    stop(daemon).await;
    ev.pass();
}
