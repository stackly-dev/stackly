//! `env.preview_down` and `env.down` through the real daemon (spec 0047
//! milestone 1, build step 5), with the fake provider only: PRD AC-022 (a
//! Down keeps an instance another environment holds, and the last Down
//! stops it), the precheck refusals, Down while Up runs, a stop that times
//! out, a daemon restart in the middle of a Down, a client that goes away,
//! Up and Down of two environments on one shared instance 50 times, and
//! that nothing is ever deleted. No edge stands for a real dependency
//! between databases.
#![cfg(target_os = "macos")]

mod common;
#[path = "supervisor/support.rs"]
mod support;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use stackly_core::catalog::{
    Catalog, CatalogEntry, EntryOrigin, ProviderEntry, TileColor, TileHint,
};
use stackly_core::provider::{NameCheck, ValidatedPackage, read_package};
use stackly_domain::{
    EnvironmentId, InstanceFilter, InstanceId, LeaseFilter, LeaseHolder, LeaseState, Registry as _,
};
use stackly_ipc::CallOptions;
use stackly_protocol::env::{
    EnvDownParams, EnvDownPreview, EnvDownReply, EnvGetParams, EnvPlanParams, EnvPreviewDownParams,
    EnvSource, EnvUpParams, EnvUpReply, EnvironmentDetail, UpPhase,
};
use stackly_protocol::instance::{InstanceLifecycle, InstanceReadinessState};
use stackly_protocol::operation::{OperationKind, OperationState};
use stackly_protocol::{Call, ErrorBody, Reply};
use stackly_testkit::evidence::{Kind, evidence};
use stacklyd::server::DaemonHandle;
use uuid::Uuid;

use crate::support::{Api, Site, fast, key, shutdown_server};

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

fn config() -> stacklyd::server::DaemonConfig {
    let mut config = fast();
    config.catalog = Some(stacklyd::catalog::CatalogOverride::Preloaded(catalog()));
    config
}

async fn start(site: &Site) -> DaemonHandle {
    site.unpin_fake_package();
    site.start_with(config()).await
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

/// A manifest of `name` with one dedicated fake service `db`.
fn dedicated(name: &str, settings: &str) -> String {
    format!(
        "apiVersion: stackly/v1alpha1\nkind: Environment\nmetadata:\n  name: {name}\nservices:\n  db:\n    provider: testing/fake\n    version: \"0.1\"\n{settings}"
    )
}

/// A manifest of `name` whose service `db` borrows `instance`.
fn borrowing(name: &str, instance: &str) -> String {
    format!(
        "apiVersion: stackly/v1alpha1\nkind: Environment\nmetadata:\n  name: {name}\nservices:\n  db:\n    provider: testing/fake\n    instanceRef: {instance}\n"
    )
}

async fn up(api: &Api, path: &Path) -> EnvUpReply {
    let planned = match api
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
    };
    let digest = planned
        .plan_digest
        .unwrap_or_else(|| panic!("the plan is applicable: {:?}", planned.blockers));
    match api
        .client
        .call_detailed(
            Call::EnvUp(EnvUpParams {
                source: EnvSource::File(path.display().to_string()),
                expected_plan_digest: digest,
                locked: false,
            }),
            CallOptions::default().with_idempotency_key(key()),
        )
        .await
        .expect("the up starts")
    {
        Reply::EnvUp(reply) => *reply,
        other => panic!("not an up reply: {other:?}"),
    }
}

async fn up_done(api: &Api, path: &Path) -> EnvUpReply {
    let reply = up(api, path).await;
    let done = api.wait_final(reply.operation.operation.id).await;
    assert_eq!(done.operation.state, OperationState::Succeeded, "{done:?}");
    reply
}

async fn preview(api: &Api, id: Uuid) -> Result<EnvDownPreview, Box<ErrorBody>> {
    match api
        .client
        .call(Call::EnvPreviewDown(EnvPreviewDownParams { id }))
        .await?
    {
        Reply::EnvPreviewDown(preview) => Ok(*preview),
        other => panic!("not a preview: {other:?}"),
    }
}

async fn down_with(
    api: &Api,
    id: Uuid,
    expected: Vec<Uuid>,
    idempotency: String,
) -> Result<EnvDownReply, Box<ErrorBody>> {
    match api
        .client
        .call_detailed(
            Call::EnvDown(EnvDownParams {
                id,
                expected_stop_instance_ids: expected,
            }),
            CallOptions::default().with_idempotency_key(idempotency),
        )
        .await?
    {
        Reply::EnvDown(reply) => Ok(*reply),
        other => panic!("not a down reply: {other:?}"),
    }
}

/// Previews, then brings `id` down with the preview's stop set.
async fn down(api: &Api, id: Uuid) -> (EnvDownPreview, EnvDownReply) {
    let shown = preview(api, id).await.expect("the preview answers");
    let expected = shown.stop.iter().map(|item| item.instance_id).collect();
    let reply = down_with(api, id, expected, key())
        .await
        .expect("the down starts");
    assert_eq!(
        reply.operation.operation.kind,
        OperationKind::EnvironmentDown
    );
    (shown, reply)
}

async fn get(api: &Api, id: Uuid) -> EnvironmentDetail {
    match api
        .client
        .call(Call::EnvGet(EnvGetParams {
            id,
            check_manifest: false,
        }))
        .await
        .expect("the detail answers")
    {
        Reply::EnvGet(detail) => *detail,
        other => panic!("not a detail: {other:?}"),
    }
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

/// Rows that Down never adds or removes: instances, runtimes, members,
/// environments.
fn rows(daemon: &DaemonHandle) -> (usize, usize, usize, usize) {
    let registry = daemon.core().registry().expect("the registry is open");
    let environments = registry.environments().expect("reads").value;
    (
        registry
            .instances(InstanceFilter {
                include_retained: true,
            })
            .expect("reads")
            .value
            .len(),
        registry.runtimes().expect("reads").value.len(),
        environments.iter().map(|env| env.members().len()).sum(),
        environments.len(),
    )
}

/// Every file under the test root outside logs and `run/`, with its size.
fn tree(root: &Path) -> BTreeMap<PathBuf, u64> {
    let mut files = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let text = path.to_string_lossy();
            if text.contains("/logs") || text.contains("/run") || text.contains("/Logs") {
                continue;
            }
            let Ok(meta) = std::fs::symlink_metadata(&path) else {
                continue;
            };
            if meta.is_dir() {
                stack.push(path);
            } else if meta.is_file() {
                files.insert(path, meta.len());
            }
        }
    }
    files
}

/// Data and config folders of `instance`, by file.
fn data_files(site: &Site, instance: Uuid) -> BTreeMap<PathBuf, u64> {
    let paths = site.instance_paths(instance);
    let mut files = tree(&paths.data);
    files.extend(tree(&paths.config));
    files
}

/// Waits until `check` holds for the summary of `instance`.
async fn wait_summary(
    api: &Api,
    instance: Uuid,
    what: &str,
    check: impl Fn(&stackly_protocol::instance::InstanceSummary) -> bool,
) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    loop {
        let summary = api.get(instance).await.expect("the instance reads");
        if check(&summary) {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "{what}: {summary:?}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn instance_of(detail: &EnvironmentDetail) -> Uuid {
    detail.services[0]
        .instance
        .as_ref()
        .expect("the service has an instance")
        .id
}

// covers: AC-1, AC-2, AC-4, AC-5, AC-7, AC-12, AC-13 (spec 0047)
#[tokio::test(flavor = "multi_thread")]
async fn a_down_keeps_an_instance_another_environment_holds_and_the_last_down_stops_it() {
    let ev = evidence!(
        covers = ["AC-022", "REQ-022", "BR-014", "BR-015", "BR-006"],
        kind = Kind::Fixture,
        expected = "with shop creating shop-db and blog borrowing it, a stop without override is refused supervisor.stop_requires_override naming both environments, the preview of shop keeps shop-db for blog, Down of shop succeeds with nothing stopped, releases only shop's lease, and leaves the same PID ready; Down of blog then stops it; data, members, and rows stay",
    );
    let site = Site::new();
    let daemon = start(&site).await;
    let api = site.api().await;
    let shop = up_done(&api, &write(&site, "shop.yaml", &dedicated("shop", ""))).await;
    let blog = up_done(
        &api,
        &write(&site, "blog.yaml", &borrowing("blog", "shop-db")),
    )
    .await;
    let db = instance_of(&get(&api, shop.environment_id).await);
    let running = api.get(db).await.expect("the instance reads");
    let pid = running.process.as_ref().expect("a process").pid;
    wait_summary(&api, db, "both leases show", |summary| {
        summary.environments.len() == 2 && summary.environments.iter().all(|e| e.lease_active)
    })
    .await;
    let shown = get(&api, shop.environment_id).await;
    let lease = shown.lease.expect("the detail reads the lease");
    assert_eq!(u64::from(lease.active_count), 1);
    assert!(lease.held_since.is_some());

    // A person's stop names every environment that holds it.
    let refused = api.stop(db).await.expect_err("a stop needs the override");
    assert_eq!(refused.code(), "supervisor.stop_requires_override");
    let mut names: Vec<(Option<&str>, Option<&str>, Option<&str>)> = refused
        .items()
        .iter()
        .map(|item| {
            (
                item.field("name"),
                item.field("alias"),
                item.field("ownership"),
            )
        })
        .collect();
    names.sort_unstable();
    assert_eq!(
        names,
        [
            (Some("blog"), Some("db"), Some("shared")),
            (Some("shop"), Some("db"), Some("dedicated")),
        ]
    );
    assert!(refused.items()[0].field("environmentId").is_some());

    let tree_before = tree(site.root.path());
    let data_before = data_files(&site, db);
    let rows_before = rows(&daemon);
    let (seen, reply) = down(&api, shop.environment_id).await;
    assert!(seen.stop.is_empty(), "{seen:?}");
    assert_eq!(seen.keep.len(), 1);
    assert_eq!(seen.keep[0].instance.name, "shop-db");
    assert_eq!(seen.keep[0].environments[0].name, "blog");
    assert_eq!(u64::from(seen.lease_count), 1);
    assert!(!seen.cancels_up);
    let done = api.wait_final(reply.operation.operation.id).await;
    assert_eq!(done.operation.state, OperationState::Succeeded, "{done:?}");
    // shop-db was kept by the preview, so it is not in the stop set:
    // nothing was stopped and no child ran.
    let result = done.operation.result.as_ref().expect("a result");
    assert!(
        result.resources.is_empty() && result.partial.is_empty(),
        "{result:?}"
    );

    assert_eq!(active_leases(&daemon, db, shop.environment_id), 0);
    assert_eq!(active_leases(&daemon, db, blog.environment_id), 1);
    let after = api.get(db).await.expect("the instance reads");
    assert_eq!(
        after.process.as_ref().map(|p| p.pid),
        Some(pid),
        "the same PID"
    );
    assert_eq!(after.lifecycle, InstanceLifecycle::Running);
    assert_eq!(after.readiness.state, InstanceReadinessState::Ready);
    let shown = get(&api, shop.environment_id).await;
    assert_eq!(shown.lease.map(|l| u64::from(l.active_count)), Some(0));
    assert!(shown.last_up.is_none(), "Down clears last_up");
    wait_summary(&api, db, "shop's lease shows released", |summary| {
        summary
            .environments
            .iter()
            .any(|e| e.name == "shop" && !e.lease_active)
            && summary
                .environments
                .iter()
                .any(|e| e.name == "blog" && e.lease_active)
    })
    .await;

    // The last holder goes down: the instance stops, its data stays.
    let (seen, reply) = down(&api, blog.environment_id).await;
    assert_eq!(seen.stop.len(), 1);
    assert_eq!(
        seen.stop[0].ownership,
        Some(stackly_protocol::env::EnvOwnership::Shared)
    );
    let done = api.wait_final(reply.operation.operation.id).await;
    assert_eq!(done.operation.state, OperationState::Succeeded, "{done:?}");
    let stopped = api.get(db).await.expect("the instance reads");
    assert_eq!(stopped.lifecycle, InstanceLifecycle::Stopped);
    assert_eq!(data_files(&site, db), data_before, "data and config stay");
    assert_eq!(rows(&daemon), rows_before, "no row is added or removed");
    let tree_after = tree(site.root.path());
    for path in tree_before.keys() {
        assert!(tree_after.contains_key(path), "{} stays", path.display());
    }
    stop(daemon).await;
    ev.pass();
}

// covers: AC-1, AC-2 (spec 0047)
#[tokio::test(flavor = "multi_thread")]
async fn a_changed_stop_set_or_an_unknown_environment_is_refused_without_an_operation() {
    let ev = evidence!(
        covers = ["REQ-022"],
        kind = Kind::Fixture,
        expected = "env.down whose expectedStopInstanceIds differ from the preview is refused env.down_changed with a hint and no operation or registry change, an unknown environment is env.not_found for both methods, a repeated key replays the first reply, and Down of an environment with nothing running succeeds and releases its lease",
    );
    let site = Site::new();
    let daemon = start(&site).await;
    let api = site.api().await;
    let shop = up_done(&api, &write(&site, "shop.yaml", &dedicated("shop", ""))).await;
    let db = instance_of(&get(&api, shop.environment_id).await);
    let registry = daemon.core().registry().expect("the registry is open");
    let generation = registry.known_instance_ids().expect("reads").generation;

    let changed = down_with(&api, shop.environment_id, Vec::new(), key())
        .await
        .expect_err("another set is refused");
    assert_eq!(changed.code(), "env.down_changed");
    assert!(changed.hint().is_some());
    let extra = down_with(&api, shop.environment_id, vec![db, Uuid::now_v7()], key())
        .await
        .expect_err("an ID outside the set is refused");
    assert_eq!(extra.code(), "env.down_changed");
    assert_eq!(
        registry.known_instance_ids().expect("reads").generation,
        generation,
        "nothing was written"
    );
    assert_eq!(active_leases(&daemon, db, shop.environment_id), 1);

    let unknown = Uuid::now_v7();
    let missing = preview(&api, unknown)
        .await
        .expect_err("no such environment");
    assert_eq!(missing.code(), "env.not_found");
    let missing = down_with(&api, unknown, Vec::new(), key())
        .await
        .expect_err("no such environment");
    assert_eq!(missing.code(), "env.not_found");

    // The manifest is gone: Down never reads it.
    std::fs::remove_file(site.root.path().join("project/shop.yaml")).expect("removes");
    let once = key();
    let first = down_with(&api, shop.environment_id, vec![db], once.clone())
        .await
        .expect("the down starts");
    let again = down_with(&api, shop.environment_id, vec![db], once)
        .await
        .expect("the repeat replays");
    assert_eq!(again.operation.operation.id, first.operation.operation.id);
    assert!(again.operation.replayed);
    let done = api.wait_final(first.operation.operation.id).await;
    assert_eq!(done.operation.state, OperationState::Succeeded, "{done:?}");

    // Nothing runs and no lease is held: Down still succeeds.
    let (seen, reply) = down(&api, shop.environment_id).await;
    assert!(seen.stop.is_empty() && seen.keep.is_empty());
    assert_eq!(u64::from(seen.lease_count), 0);
    assert_eq!(seen.idle.len(), 1);
    let done = api.wait_final(reply.operation.operation.id).await;
    assert_eq!(done.operation.state, OperationState::Succeeded, "{done:?}");
    stop(daemon).await;
    ev.pass();
}

// covers: AC-3, AC-9 (spec 0047)
#[tokio::test(flavor = "multi_thread")]
async fn a_down_while_up_runs_cancels_the_up_and_a_new_up_is_busy() {
    let ev = evidence!(
        covers = ["REQ-022", "AC-073"],
        kind = Kind::Fixture,
        expected = "while an Up waits for a slow start, the preview says it cancels the Up; env.down cancels it, a new env.up is refused resource.busy, the Up ends cancelled, and the Down stops the instance and ends with last_up cleared and no active lease",
    );
    let site = Site::new();
    let daemon = start(&site).await;
    let api = site.api().await;
    let path = write(
        &site,
        "slow.yaml",
        &dedicated("slow", "    settings:\n      readyDelayMs: 20000\n"),
    );
    let first = up(&api, &path).await;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        if let Ok(Reply::EnvGet(detail)) = api
            .client
            .call(Call::EnvGet(EnvGetParams {
                id: first.environment_id,
                check_manifest: false,
            }))
            .await
            && detail
                .last_up
                .as_ref()
                .is_some_and(|last| last.services.iter().any(|s| s.phase == UpPhase::Starting))
        {
            break;
        }
        assert!(tokio::time::Instant::now() < deadline, "the Up starts db");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    let (seen, reply) = down(&api, first.environment_id).await;
    assert!(seen.cancels_up, "{seen:?}");
    let planned = match api
        .client
        .call(Call::EnvPlan(EnvPlanParams {
            source: EnvSource::File(path.display().to_string()),
            locked: false,
        }))
        .await
    {
        Ok(Reply::EnvPlan(plan)) => plan.plan_digest,
        other => panic!("not a plan: {other:?}"),
    };
    if let Some(digest) = planned {
        let busy = api
            .client
            .call_detailed(
                Call::EnvUp(EnvUpParams {
                    source: EnvSource::File(path.display().to_string()),
                    expected_plan_digest: digest,
                    locked: false,
                }),
                CallOptions::default().with_idempotency_key(key()),
            )
            .await
            .expect_err("an Up while Down is queued is refused");
        assert_eq!(busy.code(), "resource.busy");
    }
    let up_done = api.wait_final(first.operation.operation.id).await;
    assert_eq!(
        up_done.operation.state,
        OperationState::Cancelled,
        "{up_done:?}"
    );
    let done = api.wait_final(reply.operation.operation.id).await;
    assert_eq!(done.operation.state, OperationState::Succeeded, "{done:?}");
    let detail = get(&api, first.environment_id).await;
    assert!(detail.last_up.is_none(), "Down cleared last_up");
    assert_eq!(
        detail.lease.as_ref().map(|l| u64::from(l.active_count)),
        Some(0)
    );
    let db = instance_of(&detail);
    let stopped = api.get(db).await.expect("the instance reads");
    assert_eq!(stopped.lifecycle, InstanceLifecycle::Stopped);
    stop(daemon).await;
    ev.pass();
}

// covers: AC-8 (spec 0047)
#[tokio::test(flavor = "multi_thread")]
async fn a_stop_that_times_out_ends_the_down_incomplete_and_the_next_down_finishes() {
    let ev = evidence!(
        covers = ["REQ-022", "BR-006"],
        kind = Kind::Fixture,
        expected = "Down of an instance that ignores SIGTERM fails env.down_incomplete with one failed item supervisor.stop_timeout, the lease stays released and the data stays, and once the process exits a second Down stops it and succeeds",
    );
    let site = Site::new();
    let daemon = start(&site).await;
    let api = site.api().await;
    let shop = up_done(
        &api,
        &write(
            &site,
            "stubborn.yaml",
            &dedicated("stubborn", "    settings:\n      ignoreTerm: true\n"),
        ),
    )
    .await;
    let db = instance_of(&get(&api, shop.environment_id).await);
    let port = api.get(db).await.expect("reads").endpoints[0]
        .port
        .expect("a bound port");
    let data_before = data_files(&site, db);

    let (_, reply) = down(&api, shop.environment_id).await;
    let done = api.wait_final(reply.operation.operation.id).await;
    assert_eq!(done.operation.state, OperationState::Failed, "{done:?}");
    let error = &done.operation.failure.as_ref().expect("a failure").error;
    assert_eq!(error.code(), "env.down_incomplete");
    assert_eq!(error.detail("stopped"), Some("0"));
    assert_eq!(error.detail("total"), Some("1"));
    assert_eq!(error.items()[0].field("outcome"), Some("failed"));
    assert_eq!(
        error.items()[0].field("code"),
        Some("supervisor.stop_timeout")
    );
    assert!(error.items()[0].field("operationId").is_some());
    assert_eq!(
        error.hint(),
        Some("bring the environment down again, or stop the instances listed; nothing was deleted")
    );
    assert_eq!(active_leases(&daemon, db, shop.environment_id), 0);
    assert_eq!(data_files(&site, db), data_before);

    shutdown_server(port);
    let (seen, reply) = down(&api, shop.environment_id).await;
    let done = api.wait_final(reply.operation.operation.id).await;
    assert_eq!(
        done.operation.state,
        OperationState::Succeeded,
        "{seen:?} {done:?}"
    );
    wait_summary(&api, db, "it stops", |summary| {
        summary.lifecycle == InstanceLifecycle::Stopped
    })
    .await;
    stop(daemon).await;
    ev.pass();
}

// covers: AC-10, AC-12 (spec 0047)
#[tokio::test(flavor = "multi_thread")]
async fn a_daemon_restart_in_the_middle_of_a_down_closes_it_interrupted_with_the_leases_released() {
    let ev = evidence!(
        covers = ["REQ-022"],
        kind = Kind::Fixture,
        expected = "a daemon that stops while Down waits for a stop that hangs comes back with the Down failed interrupted, its child stop final, no environment lease active, and no active operation on the environment",
    );
    let site = Site::new();
    let daemon = start(&site).await;
    let api = site.api().await;
    let shop = up_done(
        &api,
        &write(
            &site,
            "stubborn.yaml",
            &dedicated("stubborn", "    settings:\n      ignoreTerm: true\n"),
        ),
    )
    .await;
    let db = instance_of(&get(&api, shop.environment_id).await);
    let (_, reply) = down(&api, shop.environment_id).await;
    wait_summary(&api, db, "the stop runs", |summary| {
        summary.lifecycle == InstanceLifecycle::Stopping
    })
    .await;
    drop(api);
    stop(daemon).await;

    let daemon = site.start_with(config()).await;
    let api = site.api().await;
    let done = api.wait_final(reply.operation.operation.id).await;
    assert_eq!(done.operation.state, OperationState::Failed, "{done:?}");
    assert_eq!(
        Api::failure_code(&done),
        "operation.interrupted",
        "a shutdown never records the Down as incomplete"
    );
    assert_eq!(active_leases(&daemon, db, shop.environment_id), 0);
    let detail = get(&api, shop.environment_id).await;
    assert_eq!(detail.summary.active_operation, None);
    stop(daemon).await;
    ev.pass();
}

// covers: AC-12 (spec 0047)
#[tokio::test(flavor = "multi_thread")]
async fn a_down_whose_client_goes_away_still_runs_to_the_end() {
    let ev = evidence!(
        covers = ["REQ-022"],
        kind = Kind::Fixture,
        expected = "a client that closes its connection right after env.down is accepted leaves the Down running: another client sees it succeed, the instance stopped, and the lease released",
    );
    let site = Site::new();
    let daemon = start(&site).await;
    let api = site.api().await;
    let shop = up_done(&api, &write(&site, "shop.yaml", &dedicated("shop", ""))).await;
    let db = instance_of(&get(&api, shop.environment_id).await);

    let gone = site.api().await;
    let (_, reply) = down(&gone, shop.environment_id).await;
    drop(gone);

    let done = api.wait_final(reply.operation.operation.id).await;
    assert_eq!(done.operation.state, OperationState::Succeeded, "{done:?}");
    assert_eq!(active_leases(&daemon, db, shop.environment_id), 0);
    let stopped = api.get(db).await.expect("reads");
    assert_eq!(stopped.lifecycle, InstanceLifecycle::Stopped);
    stop(daemon).await;
    ev.pass();
}

// covers: AC-5, AC-11 (spec 0047)
#[tokio::test(flavor = "multi_thread")]
async fn up_of_one_environment_and_down_of_another_on_a_shared_instance_agree_fifty_times() {
    let ev = evidence!(
        covers = ["AC-073", "BR-014"],
        kind = Kind::Fixture,
        expected = "50 times, Up of alpha and Down of beta at once, both borrowing one instance, end with exactly one active lease of alpha, none of beta, one instance, no member or lease row twice, and when the Up succeeds the instance runs ready",
    );
    let site = Site::new();
    let daemon = start(&site).await;
    let api = site.api().await;
    let shared = api.create_ready("shared", serde_json::Value::Null).await;
    let alpha_path = write(&site, "alpha.yaml", &borrowing("alpha", "shared"));
    let beta_path = write(&site, "beta.yaml", &borrowing("beta", "shared"));
    let beta = up_done(&api, &beta_path).await;
    let alpha = up_done(&api, &alpha_path).await;
    let (_, reset) = down(&api, alpha.environment_id).await;
    let _ = api.wait_final(reset.operation.operation.id).await;
    let rows_before = rows(&daemon);
    let other = site.api().await;

    for round in 0..50 {
        let shown = preview(&api, beta.environment_id).await.expect("previews");
        let expected = shown.stop.iter().map(|item| item.instance_id).collect();
        let (up_reply, down_reply) = tokio::join!(
            up(&other, &alpha_path),
            down_with(&api, beta.environment_id, expected, key()),
        );
        let up_end = api.wait_final(up_reply.operation.operation.id).await;
        let down_reply = down_reply.expect("the down starts");
        let down_end = api.wait_final(down_reply.operation.operation.id).await;

        assert_eq!(
            active_leases(&daemon, shared, alpha.environment_id),
            1,
            "round {round}"
        );
        assert_eq!(
            active_leases(&daemon, shared, beta.environment_id),
            0,
            "round {round}"
        );
        assert_eq!(rows(&daemon), rows_before, "round {round}");
        assert!(
            matches!(
                down_end.operation.state,
                OperationState::Succeeded | OperationState::Failed
            ),
            "round {round}: {down_end:?}"
        );
        if up_end.operation.state == OperationState::Succeeded {
            let now = api.get(shared).await.expect("reads");
            assert_eq!(now.lifecycle, InstanceLifecycle::Running, "round {round}");
        } else {
            assert_eq!(
                up_end.operation.state,
                OperationState::Failed,
                "round {round}: {up_end:?}"
            );
        }

        // Back to the start: beta holds it, alpha does not.
        let (_, back) = down(&api, alpha.environment_id).await;
        let _ = api.wait_final(back.operation.operation.id).await;
        let again = up(&api, &beta_path).await;
        let _ = api.wait_final(again.operation.operation.id).await;
        assert_eq!(
            active_leases(&daemon, shared, beta.environment_id),
            1,
            "round {round}"
        );
    }
    drop(other);
    stop(daemon).await;
    ev.pass();
}
