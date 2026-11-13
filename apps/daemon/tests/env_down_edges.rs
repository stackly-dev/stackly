//! Edges of `env.preview_down` and `env.down` through the real daemon
//! (spec 0047), next to `env_down`, with the fake provider only: three
//! environments on one instance, two Downs at once, Up of one environment
//! with Downs of two others, a stop set that changes between the preview
//! and the Down in both directions, a restart by you that keeps an instance
//! running, a cancel in the middle of a Down, a daemon restart after
//! `release`, every file byte for byte through a Down that succeeds, fails,
//! and is cancelled, a person's stop with the override, a lease that
//! appears between the dry run and the apply of a port change, the Stop All
//! refusal in both directions, the actions that need no override, and the
//! `stackly env down` binary killed in the middle of a Down. No edge stands
//! for a real dependency between databases.
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
use stackly_protocol::audit::{AuditPage, ListAuditParams};
use stackly_protocol::env::{
    EnvDownParams, EnvDownPreview, EnvDownReply, EnvGetParams, EnvPlanParams, EnvPreviewDownParams,
    EnvSource, EnvUpParams, EnvUpReply, EnvironmentDetail,
};
use stackly_protocol::instance::config::{
    ApplyConfigParams, ChangeEndpointsParams, ChangeEndpointsReply, ConfigChangeRequest,
    GetConfigParams, InstanceConfigView, RenameInstanceParams,
};
use stackly_protocol::instance::{
    EndpointRequest, InstanceLifecycle, InstanceReadinessState, InstanceSummary, PortRequest,
    SettingsDocument,
};
use stackly_protocol::operation::{
    CancelOperationParams, ListOperationsParams, OperationKind, OperationState, OperationSummary,
};
use stackly_protocol::stop_all::{StopAllParams, StopAllPreview, StopAllPreviewParams};
use stackly_protocol::{Call, ConfigRevisionNumber, ErrorBody, Reply};
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
fn dedicated(name: &str) -> String {
    format!(
        "apiVersion: stackly/v1alpha1\nkind: Environment\nmetadata:\n  name: {name}\nservices:\n  db:\n    provider: testing/fake\n    version: \"0.1\"\n"
    )
}

/// A manifest of `name` whose service `db` borrows `instance`.
fn borrowing(name: &str, instance: &str) -> String {
    format!(
        "apiVersion: stackly/v1alpha1\nkind: Environment\nmetadata:\n  name: {name}\nservices:\n  db:\n    provider: testing/fake\n    instanceRef: {instance}\n"
    )
}

/// A manifest of `name` with `db` and `web`, where `web` waits for `db`
/// and ignores `SIGTERM`, so its stop runs until the stop timeout.
fn chain(name: &str) -> String {
    format!(
        "apiVersion: stackly/v1alpha1\nkind: Environment\nmetadata:\n  name: {name}\nservices:\n  db:\n    provider: testing/fake\n    version: \"0.1\"\n  web:\n    provider: testing/fake\n    version: \"0.1\"\n    dependsOn:\n      - {{service: db}}\n    settings:\n      ignoreTerm: true\n"
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

async fn preview(api: &Api, id: Uuid) -> EnvDownPreview {
    match api
        .client
        .call(Call::EnvPreviewDown(EnvPreviewDownParams { id }))
        .await
        .expect("the preview answers")
    {
        Reply::EnvPreviewDown(preview) => *preview,
        other => panic!("not a preview: {other:?}"),
    }
}

fn stop_set(shown: &EnvDownPreview) -> Vec<Uuid> {
    shown.stop.iter().map(|item| item.instance_id).collect()
}

async fn down_with(
    api: &Api,
    id: Uuid,
    expected: Vec<Uuid>,
) -> Result<EnvDownReply, Box<ErrorBody>> {
    match api
        .client
        .call_detailed(
            Call::EnvDown(EnvDownParams {
                id,
                expected_stop_instance_ids: expected,
            }),
            CallOptions::default().with_idempotency_key(key()),
        )
        .await?
    {
        Reply::EnvDown(reply) => Ok(*reply),
        other => panic!("not a down reply: {other:?}"),
    }
}

/// Previews, then brings `id` down with the preview's stop set.
async fn down(api: &Api, id: Uuid) -> (EnvDownPreview, EnvDownReply) {
    let shown = preview(api, id).await;
    let reply = down_with(api, id, stop_set(&shown))
        .await
        .expect("the down starts");
    (shown, reply)
}

/// Brings `id` down and waits until the Down ended.
async fn down_done(api: &Api, id: Uuid) -> OperationSummary {
    let (_, reply) = down(api, id).await;
    api.wait_final(reply.operation.operation.id).await.operation
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

fn instance_of(detail: &EnvironmentDetail, alias: &str) -> Uuid {
    detail
        .services
        .iter()
        .find(|service| service.alias == alias)
        .and_then(|service| service.instance.as_ref())
        .unwrap_or_else(|| panic!("the service {alias} has an instance"))
        .id
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

/// Every active lease of `instance`, by holder kind: environment, manual,
/// startup.
fn holders(daemon: &DaemonHandle, instance: Uuid) -> Vec<LeaseHolder> {
    let registry = daemon.core().registry().expect("the registry is open");
    let instance = InstanceId::from_uuid(instance).expect("an instance ID");
    registry
        .leases(instance, LeaseFilter::All)
        .expect("reads")
        .value
        .iter()
        .filter(|lease| lease.state() == LeaseState::Active)
        .map(|lease| lease.holder())
        .collect()
}

/// Rows Down never adds or removes: instances, runtimes, members,
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

/// Every file under `root` with its bytes, outside logs, `run/`, and the
/// registry database Down writes by design (the lease and environment rows
/// it changes are checked row by row).
fn bytes(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut files = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let text = path.to_string_lossy().into_owned();
            let name = entry.file_name().to_string_lossy().into_owned();
            if text.contains("/logs") || text.contains("/run") || text.contains("/Logs") {
                continue;
            }
            if name.starts_with("registry") || name.ends_with("-wal") || name.ends_with("-shm") {
                continue;
            }
            let Ok(meta) = std::fs::symlink_metadata(&path) else {
                continue;
            };
            if meta.is_dir() {
                stack.push(path);
            } else if meta.is_file()
                && let Ok(content) = std::fs::read(&path)
            {
                files.insert(path, content);
            }
        }
    }
    files
}

/// Asserts that every file of `before` is still there with the same bytes.
fn same_bytes(before: &BTreeMap<PathBuf, Vec<u8>>, after: &BTreeMap<PathBuf, Vec<u8>>, what: &str) {
    for (path, content) in before {
        let now = after
            .get(path)
            .unwrap_or_else(|| panic!("{what}: {} is gone", path.display()));
        assert!(
            now == content,
            "{what}: {} changed ({} bytes, then {})",
            path.display(),
            content.len(),
            now.len()
        );
    }
}

/// The active config revision of `instance`.
async fn config_of(api: &Api, instance: Uuid) -> InstanceConfigView {
    match api
        .client
        .call(Call::InstanceConfigGet(GetConfigParams {
            instance_id: instance,
        }))
        .await
        .expect("the config reads")
    {
        Reply::InstanceConfigGet(view) => *view,
        other => panic!("not a config reply: {other:?}"),
    }
}

/// The active config revision number of each of `instances`.
async fn revisions(api: &Api, instances: &[Uuid]) -> Vec<String> {
    let mut numbers = Vec::new();
    for id in instances {
        numbers.push(format!("{:?}", config_of(api, *id).await.revision.number));
    }
    numbers
}

async fn wait_summary(
    api: &Api,
    instance: Uuid,
    what: &str,
    check: impl Fn(&InstanceSummary) -> bool,
) -> InstanceSummary {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        let summary = api.get(instance).await.expect("the instance reads");
        if check(&summary) {
            return summary;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "{what}: {summary:?}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn operations(api: &Api) -> Vec<OperationSummary> {
    match api
        .client
        .call(Call::OperationList(ListOperationsParams {
            limit: Some(200),
            ..ListOperationsParams::default()
        }))
        .await
        .expect("operation.list answers")
    {
        Reply::OperationList(page) => page.items,
        other => panic!("not an operation page: {other:?}"),
    }
}

/// The `stop_instance` operations of `instance`.
async fn stops_of(api: &Api, instance: Uuid) -> Vec<OperationSummary> {
    let id = instance.to_string();
    operations(api)
        .await
        .into_iter()
        .filter(|op| {
            op.kind == OperationKind::StopInstance
                && op.resource.as_ref().is_some_and(|r| r.id == id)
        })
        .collect()
}

async fn cancel(api: &Api, operation: Uuid) {
    let cancelled = api
        .client
        .call(Call::OperationCancel(CancelOperationParams {
            operation_id: operation,
        }))
        .await;
    assert!(cancelled.is_ok(), "{cancelled:?}");
}

/// The `(name, outcome, code)` of every item of `env.down_incomplete`.
fn incomplete(done: &OperationSummary) -> Vec<(String, String, String)> {
    let error = &done.failure.as_ref().expect("a failure").error;
    assert_eq!(error.code(), "env.down_incomplete", "{done:?}");
    error
        .items()
        .iter()
        .map(|item| {
            assert_eq!(item.kind(), "instance");
            (
                item.field("name").unwrap_or_default().to_owned(),
                item.field("outcome").unwrap_or_default().to_owned(),
                item.field("code").unwrap_or_default().to_owned(),
            )
        })
        .collect()
}

fn port_of(summary: &InstanceSummary) -> u16 {
    summary.endpoints[0].port.expect("a bound port")
}

// covers: AC-1, AC-4, AC-5, AC-12 (spec 0047), PRD AC-022
#[tokio::test(flavor = "multi_thread")]
async fn three_environments_on_one_instance_keep_it_running_until_the_last_down() {
    let ev = evidence!(
        covers = ["AC-022", "REQ-022", "BR-014"],
        kind = Kind::Fixture,
        expected = "shop creates shop-db and blog and wiki borrow it: Down of shop keeps it for blog and wiki, Down of blog keeps it for wiki, each with the same PID ready and only the down environment's lease released, env.get lease and leaseActive follow each step, and Down of wiki stops it",
    );
    let site = Site::new();
    let daemon = start(&site).await;
    let api = site.api().await;
    let shop = up_done(&api, &write(&site, "shop.yaml", &dedicated("shop"))).await;
    let blog = up_done(
        &api,
        &write(&site, "blog.yaml", &borrowing("blog", "shop-db")),
    )
    .await;
    let wiki = up_done(
        &api,
        &write(&site, "wiki.yaml", &borrowing("wiki", "shop-db")),
    )
    .await;
    let db = instance_of(&get(&api, shop.environment_id).await, "db");
    let pid = api
        .get(db)
        .await
        .expect("reads")
        .process
        .expect("a process")
        .pid;
    let held_since = get(&api, blog.environment_id)
        .await
        .lease
        .and_then(|lease| lease.held_since);
    assert!(held_since.is_some(), "blog holds a lease");

    let shown = preview(&api, shop.environment_id).await;
    assert!(shown.stop.is_empty());
    let mut keepers: Vec<String> = shown.keep[0]
        .environments
        .iter()
        .map(|e| e.name.clone())
        .collect();
    keepers.sort_unstable();
    assert_eq!(keepers, ["blog", "wiki"]);
    let done = down_done(&api, shop.environment_id).await;
    assert_eq!(done.state, OperationState::Succeeded, "{done:?}");
    assert_eq!(active_leases(&daemon, db, shop.environment_id), 0);
    assert_eq!(active_leases(&daemon, db, blog.environment_id), 1);
    assert_eq!(active_leases(&daemon, db, wiki.environment_id), 1);
    let detail = get(&api, shop.environment_id).await;
    let lease = detail.lease.expect("a lease line");
    assert_eq!(u64::from(lease.active_count), 0);
    assert!(lease.held_since.is_none(), "no lease, no since");
    assert_eq!(
        get(&api, blog.environment_id)
            .await
            .lease
            .and_then(|lease| lease.held_since),
        held_since,
        "another environment's Down never moves heldSince"
    );

    let shown = preview(&api, blog.environment_id).await;
    assert_eq!(shown.keep[0].environments.len(), 1);
    assert_eq!(shown.keep[0].environments[0].name, "wiki");
    let done = down_done(&api, blog.environment_id).await;
    assert_eq!(done.state, OperationState::Succeeded, "{done:?}");
    let after = wait_summary(&api, db, "only wiki's lease shows active", |s| {
        s.environments
            .iter()
            .all(|e| e.lease_active == (e.name == "wiki"))
    })
    .await;
    assert_eq!(after.process.as_ref().map(|p| p.pid), Some(pid));
    assert_eq!(after.readiness.state, InstanceReadinessState::Ready);
    assert_eq!(after.environments.len(), 3, "membership stays");

    let shown = preview(&api, wiki.environment_id).await;
    assert_eq!(stop_set(&shown), [db]);
    let done = down_done(&api, wiki.environment_id).await;
    assert_eq!(done.state, OperationState::Succeeded, "{done:?}");
    let stopped = wait_summary(&api, db, "no lease shows active", |s| {
        s.environments.iter().all(|e| !e.lease_active)
    })
    .await;
    assert_eq!(stopped.lifecycle, InstanceLifecycle::Stopped);
    assert_eq!(stops_of(&api, db).await.len(), 1, "one stop in all");
    stop(daemon).await;
    ev.pass();
}

// covers: AC-8, AC-11 (spec 0047)
#[tokio::test(flavor = "multi_thread")]
async fn two_downs_at_once_on_a_shared_instance_never_both_succeed_while_it_runs_unwanted() {
    let ev = evidence!(
        covers = ["AC-073", "BR-014", "REQ-022"],
        kind = Kind::Fixture,
        expected = "10 times, Down of shop and Down of blog at once, each previewing while the other still held shop-db, stop it at most once (never here, since neither confirmed it), at least one ends env.down_incomplete with shop-db left_running env.down_changed, both leases end released, rows stay, and the next Down stops it",
    );
    let site = Site::new();
    let daemon = start(&site).await;
    let api = site.api().await;
    let other = site.api().await;
    let shop_path = write(&site, "shop.yaml", &dedicated("shop"));
    let blog_path = write(&site, "blog.yaml", &borrowing("blog", "shop-db"));
    let shop = up_done(&api, &shop_path).await;
    let blog = up_done(&api, &blog_path).await;
    let db = instance_of(&get(&api, shop.environment_id).await, "db");
    let rows_before = rows(&daemon);

    for round in 0..10 {
        let shop_seen = preview(&api, shop.environment_id).await;
        let blog_seen = preview(&api, blog.environment_id).await;
        assert!(shop_seen.stop.is_empty() && blog_seen.stop.is_empty());
        let stops_before = stops_of(&api, db).await.len();
        let (a, b) = tokio::join!(
            down_with(&api, shop.environment_id, Vec::new()),
            down_with(&other, blog.environment_id, Vec::new()),
        );
        let a = api
            .wait_final(a.expect("shop's down starts").operation.operation.id)
            .await
            .operation;
        let b = api
            .wait_final(b.expect("blog's down starts").operation.operation.id)
            .await
            .operation;

        assert_eq!(
            active_leases(&daemon, db, shop.environment_id),
            0,
            "round {round}"
        );
        assert_eq!(
            active_leases(&daemon, db, blog.environment_id),
            0,
            "round {round}"
        );
        assert_eq!(rows(&daemon), rows_before, "round {round}");
        assert!(
            stops_of(&api, db).await.len() - stops_before <= 1,
            "round {round}: at most one stop"
        );
        let incomplete_downs: Vec<_> = [&a, &b]
            .into_iter()
            .filter(|done| done.state == OperationState::Failed)
            .collect();
        assert!(
            !incomplete_downs.is_empty(),
            "round {round}: never both succeed silently: {a:?} {b:?}"
        );
        for done in incomplete_downs {
            assert_eq!(
                incomplete(done),
                [(
                    "shop-db".to_owned(),
                    "left_running".to_owned(),
                    "env.down_changed".to_owned()
                )],
                "round {round}"
            );
        }
        let running = api.get(db).await.expect("reads");
        assert_eq!(
            running.lifecycle,
            InstanceLifecycle::Running,
            "round {round}"
        );

        // Down once more stops it; Up both again for the next round.
        let again = preview(&api, shop.environment_id).await;
        assert_eq!(stop_set(&again), [db], "round {round}");
        let done = down_done(&api, shop.environment_id).await;
        assert_eq!(
            done.state,
            OperationState::Succeeded,
            "round {round}: {done:?}"
        );
        assert_eq!(
            api.get(db).await.expect("reads").lifecycle,
            InstanceLifecycle::Stopped,
            "round {round}"
        );
        up_done(&api, &shop_path).await;
        up_done(&api, &blog_path).await;
    }
    drop(other);
    stop(daemon).await;
    ev.pass();
}

// covers: AC-5, AC-11 (spec 0047): three environments at once
#[tokio::test(flavor = "multi_thread")]
async fn up_of_one_environment_with_downs_of_two_others_on_a_shared_instance_agree() {
    let ev = evidence!(
        covers = ["AC-073", "BR-014"],
        kind = Kind::Fixture,
        expected = "15 times, Up of alpha with Down of beta and Down of gamma at once, all three on shop-db, end with exactly one active lease of alpha, none of beta or gamma, one instance, rows that stay, and shop-db running and ready whenever the Up succeeds",
    );
    let site = Site::new();
    let daemon = start(&site).await;
    let api = site.api().await;
    let a_client = site.api().await;
    let b_client = site.api().await;
    let shop_path = write(&site, "shop.yaml", &dedicated("shop"));
    let alpha_path = write(&site, "alpha.yaml", &borrowing("alpha", "shop-db"));
    let beta_path = write(&site, "beta.yaml", &borrowing("beta", "shop-db"));
    let gamma_path = write(&site, "gamma.yaml", &borrowing("gamma", "shop-db"));
    let shop = up_done(&api, &shop_path).await;
    let db = instance_of(&get(&api, shop.environment_id).await, "db");
    let alpha = up_done(&api, &alpha_path).await;
    let beta = up_done(&api, &beta_path).await;
    let gamma = up_done(&api, &gamma_path).await;
    // Only the three borrowers hold it from here on.
    assert_eq!(
        down_done(&api, shop.environment_id).await.state,
        OperationState::Succeeded
    );
    assert_eq!(
        down_done(&api, alpha.environment_id).await.state,
        OperationState::Succeeded
    );
    let rows_before = rows(&daemon);

    for round in 0..15 {
        let beta_set = stop_set(&preview(&api, beta.environment_id).await);
        let gamma_set = stop_set(&preview(&api, gamma.environment_id).await);
        let (up_reply, b, g) = tokio::join!(
            up(&a_client, &alpha_path),
            down_with(&b_client, beta.environment_id, beta_set),
            down_with(&api, gamma.environment_id, gamma_set),
        );
        let up_end = api
            .wait_final(up_reply.operation.operation.id)
            .await
            .operation;
        for reply in [b, g] {
            let end = api
                .wait_final(reply.expect("the down starts").operation.operation.id)
                .await
                .operation;
            assert!(
                matches!(
                    end.state,
                    OperationState::Succeeded | OperationState::Failed
                ),
                "round {round}: {end:?}"
            );
        }

        assert_eq!(
            active_leases(&daemon, db, alpha.environment_id),
            1,
            "round {round}"
        );
        assert_eq!(
            active_leases(&daemon, db, beta.environment_id),
            0,
            "round {round}"
        );
        assert_eq!(
            active_leases(&daemon, db, gamma.environment_id),
            0,
            "round {round}"
        );
        assert_eq!(rows(&daemon), rows_before, "round {round}");
        if up_end.state == OperationState::Succeeded {
            let now = api.get(db).await.expect("reads");
            assert_eq!(now.lifecycle, InstanceLifecycle::Running, "round {round}");
            assert_eq!(
                now.readiness.state,
                InstanceReadinessState::Ready,
                "round {round}"
            );
        } else {
            assert_eq!(
                up_end.state,
                OperationState::Failed,
                "round {round}: {up_end:?}"
            );
        }

        // Back to the start: beta and gamma hold it, alpha does not.
        up_done(&api, &beta_path).await;
        up_done(&api, &gamma_path).await;
        let back = down_done(&api, alpha.environment_id).await;
        assert!(
            matches!(
                back.state,
                OperationState::Succeeded | OperationState::Failed
            ),
            "round {round}: {back:?}"
        );
    }
    stop(daemon).await;
    ev.pass();
}

// covers: AC-2, AC-5 (spec 0047): `env.down_changed` both ways, and an
// instance kept by the manual lease your own restart writes
#[tokio::test(flavor = "multi_thread")]
async fn a_stop_set_that_changed_after_the_preview_is_refused_in_both_directions() {
    let ev = evidence!(
        covers = ["REQ-022", "BR-014"],
        kind = Kind::Fixture,
        expected = "a preview that kept shop-db for blog is refused env.down_changed once blog went down and shop-db became stoppable; a preview that would stop it is refused env.down_changed once your restart wrote a manual lease; the fresh preview keeps it as started by you and the Down leaves it running",
    );
    let site = Site::new();
    let daemon = start(&site).await;
    let api = site.api().await;
    let shop = up_done(&api, &write(&site, "shop.yaml", &dedicated("shop"))).await;
    let blog = up_done(
        &api,
        &write(&site, "blog.yaml", &borrowing("blog", "shop-db")),
    )
    .await;
    let db = instance_of(&get(&api, shop.environment_id).await, "db");

    // keep → stop: the preview kept it for blog; blog then goes down.
    let stale = preview(&api, shop.environment_id).await;
    assert!(stale.stop.is_empty());
    let blog_down = down_done(&api, blog.environment_id).await;
    assert_eq!(blog_down.state, OperationState::Succeeded, "{blog_down:?}");
    assert_eq!(
        api.get(db).await.expect("reads").lifecycle,
        InstanceLifecycle::Running,
        "blog's Down kept it for shop"
    );
    let changed = down_with(&api, shop.environment_id, stop_set(&stale))
        .await
        .expect_err("the instance became stoppable");
    assert_eq!(changed.code(), "env.down_changed");
    assert_eq!(
        changed.hint(),
        Some("review what will stop, then bring it down again")
    );

    // stop → keep: your restart writes a manual lease.
    let stale = preview(&api, shop.environment_id).await;
    assert_eq!(stop_set(&stale), [db]);
    let restart = api.restart(db).await.expect("a restart needs no override");
    let restarted = api
        .wait_final(restart.accepted.expect("an operation").operation.id)
        .await;
    assert_eq!(
        restarted.operation.state,
        OperationState::Succeeded,
        "{restarted:?}"
    );
    assert!(
        holders(&daemon, db).contains(&LeaseHolder::Manual),
        "the restart wrote a manual lease"
    );
    let changed = down_with(&api, shop.environment_id, stop_set(&stale))
        .await
        .expect_err("the instance now keeps running");
    assert_eq!(changed.code(), "env.down_changed");
    assert_eq!(
        active_leases(&daemon, db, shop.environment_id),
        1,
        "nothing was released"
    );

    let fresh = preview(&api, shop.environment_id).await;
    assert!(fresh.stop.is_empty());
    assert_eq!(fresh.keep.len(), 1);
    assert_eq!(
        fresh.keep[0].demand,
        [stackly_protocol::stop_all::DemandSourceKind::Manual]
    );
    let pid = api
        .get(db)
        .await
        .expect("reads")
        .process
        .expect("a process")
        .pid;
    let done = down_done(&api, shop.environment_id).await;
    assert_eq!(done.state, OperationState::Succeeded, "{done:?}");
    let after = api.get(db).await.expect("reads");
    assert_eq!(
        after.process.map(|p| p.pid),
        Some(pid),
        "started by you, so it runs"
    );
    assert_eq!(active_leases(&daemon, db, shop.environment_id), 0);
    stop(daemon).await;
    ev.pass();
}

// covers: AC-6, AC-7, AC-9 (spec 0047)
#[tokio::test(flavor = "multi_thread")]
async fn a_cancel_in_the_middle_of_a_down_waits_for_the_running_stop_and_cancels_the_rest() {
    let ev = evidence!(
        covers = ["REQ-022", "BR-006"],
        kind = Kind::Fixture,
        expected = "Down of a chain whose web ignores SIGTERM, cancelled while web stops: web's stop runs to its timeout and is never cancelled, db is never stopped, the Down ends cancelled at stage stop with every lease released, and every file outside logs, run, and the registry keeps its bytes, as do the config revisions; once web exits, the next Down stops db and succeeds",
    );
    let site = Site::new();
    let daemon = start(&site).await;
    let api = site.api().await;
    let shop = up_done(&api, &write(&site, "shop.yaml", &chain("shop"))).await;
    let detail = get(&api, shop.environment_id).await;
    let (db, web) = (instance_of(&detail, "db"), instance_of(&detail, "web"));
    let web_port = port_of(&api.get(web).await.expect("reads"));
    let files_before = bytes(site.root.path());
    let revisions_before = revisions(&api, &[db, web]).await;
    let rows_before = rows(&daemon);

    let (shown, reply) = down(&api, shop.environment_id).await;
    assert_eq!(shown.stop.len(), 2);
    wait_summary(&api, web, "web stops first", |s| {
        s.lifecycle == InstanceLifecycle::Stopping
    })
    .await;
    assert_eq!(
        api.get(db).await.expect("reads").lifecycle,
        InstanceLifecycle::Running,
        "db waits for its dependent"
    );
    cancel(&api, reply.operation.operation.id).await;
    let done = api.wait_final(reply.operation.operation.id).await.operation;

    assert_eq!(done.state, OperationState::Cancelled, "{done:?}");
    assert_eq!(done.stage.as_deref(), Some("stop"), "release had run");
    let web_stops = stops_of(&api, web).await;
    assert_eq!(web_stops.len(), 1);
    assert_eq!(
        web_stops[0].state,
        OperationState::Failed,
        "it ran to its timeout"
    );
    assert!(stops_of(&api, db).await.is_empty(), "db never got a stop");
    assert_eq!(
        api.get(db).await.expect("reads").lifecycle,
        InstanceLifecycle::Running
    );
    assert_eq!(active_leases(&daemon, db, shop.environment_id), 0);
    assert_eq!(active_leases(&daemon, web, shop.environment_id), 0);
    same_bytes(&files_before, &bytes(site.root.path()), "a cancelled Down");
    assert_eq!(revisions(&api, &[db, web]).await, revisions_before);
    assert_eq!(rows(&daemon), rows_before);

    shutdown_server(web_port);
    wait_summary(&api, web, "web exits", |s| {
        s.lifecycle == InstanceLifecycle::Stopped
    })
    .await;
    let done = down_done(&api, shop.environment_id).await;
    assert_eq!(done.state, OperationState::Succeeded, "{done:?}");
    assert_eq!(
        api.get(db).await.expect("reads").lifecycle,
        InstanceLifecycle::Stopped
    );
    same_bytes(
        &files_before,
        &bytes(site.root.path()),
        "a Down that succeeds",
    );
    stop(daemon).await;
    ev.pass();
}

// covers: AC-7, AC-8, AC-10 (spec 0047)
#[tokio::test(flavor = "multi_thread")]
async fn a_daemon_restart_after_release_leaves_the_rest_running_without_a_lease_and_the_next_down_stops_it()
 {
    let ev = evidence!(
        covers = ["REQ-022", "BR-006"],
        kind = Kind::Fixture,
        expected = "a daemon that stops while Down of a chain waits on web's stop comes back with the Down failed interrupted, no stop operation left queued or running, db still running with no lease and never stopped by itself, every file keeping its bytes; once web exits, the next Down stops db and succeeds; a Down that fails env.down_incomplete keeps every byte too",
    );
    let site = Site::new();
    let daemon = start(&site).await;
    let api = site.api().await;
    let shop = up_done(&api, &write(&site, "shop.yaml", &chain("shop"))).await;
    let detail = get(&api, shop.environment_id).await;
    let (db, web) = (instance_of(&detail, "db"), instance_of(&detail, "web"));
    let web_port = port_of(&api.get(web).await.expect("reads"));
    let files_before = bytes(site.root.path());
    let revisions_before = revisions(&api, &[db, web]).await;

    let (_, reply) = down(&api, shop.environment_id).await;
    wait_summary(&api, web, "web stops", |s| {
        s.lifecycle == InstanceLifecycle::Stopping
    })
    .await;
    drop(api);
    stop(daemon).await;

    let daemon = site.start_with(config()).await;
    let api = site.api().await;
    let done = api.wait_final(reply.operation.operation.id).await;
    assert_eq!(done.operation.state, OperationState::Failed, "{done:?}");
    assert_eq!(Api::failure_code(&done), "operation.interrupted");
    // Every child stop of the Down is final; a stop the daemon begins on
    // its own for web, whose desired is already `stopped`, is no child.
    let down_id = reply.operation.operation.id;
    for id in [db, web] {
        let children: Vec<OperationSummary> = stops_of(&api, id)
            .await
            .into_iter()
            .filter(|op| op.parent == Some(down_id))
            .collect();
        assert!(
            children.iter().all(|op| op.state == OperationState::Failed
                && op.failure.as_ref().map(|f| f.reason.as_str()) == Some("interrupted")),
            "no child stop is left to run later: {children:?}"
        );
    }
    assert!(stops_of(&api, db).await.is_empty(), "db never got a stop");
    assert_eq!(active_leases(&daemon, db, shop.environment_id), 0);
    let db_now = wait_summary(&api, db, "db runs on after the restart", |s| {
        s.lifecycle == InstanceLifecycle::Running
    })
    .await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(
        api.get(db).await.expect("reads").process.map(|p| p.pid),
        db_now.process.map(|p| p.pid),
        "nothing stops it by itself"
    );
    same_bytes(
        &files_before,
        &bytes(site.root.path()),
        "an interrupted Down",
    );

    // While web still ignores SIGTERM, a Down fails incomplete.
    let shown = preview(&api, shop.environment_id).await;
    assert!(stop_set(&shown).contains(&db), "{shown:?}");
    let failed = down_done(&api, shop.environment_id).await;
    if failed.state == OperationState::Failed {
        let items = incomplete(&failed);
        assert!(
            items.iter().all(|(name, _, _)| name == "shop-web"),
            "{items:?}"
        );
    }
    same_bytes(&files_before, &bytes(site.root.path()), "a Down that fails");

    shutdown_server(web_port);
    wait_summary(&api, web, "web exits", |s| {
        s.lifecycle == InstanceLifecycle::Stopped
    })
    .await;
    if api.get(db).await.expect("reads").lifecycle != InstanceLifecycle::Stopped {
        let done = down_done(&api, shop.environment_id).await;
        assert_eq!(done.state, OperationState::Succeeded, "{done:?}");
    }
    assert_eq!(
        api.get(db).await.expect("reads").lifecycle,
        InstanceLifecycle::Stopped
    );
    assert_eq!(revisions(&api, &[db, web]).await, revisions_before);
    same_bytes(&files_before, &bytes(site.root.path()), "the last Down");
    stop(daemon).await;
    ev.pass();
}

/// Every audit detail value named `key` of the rows of `instance`.
async fn audit_values(api: &Api, instance: Uuid, key: &str) -> Vec<String> {
    let page: AuditPage = match api
        .client
        .call(Call::AuditList(ListAuditParams {
            instance_id: Some(instance),
            ..ListAuditParams::default()
        }))
        .await
        .expect("audit.list answers")
    {
        Reply::AuditList(page) => page,
        other => panic!("not an audit.list reply: {other:?}"),
    };
    page.items
        .iter()
        .flat_map(|row| row.details.iter())
        .filter(|detail| detail.key == key)
        .map(|detail| detail.value.clone())
        .collect()
}

async fn stop_overriding(api: &Api, id: Uuid) -> Uuid {
    match api
        .client
        .call_detailed(
            Call::InstanceStop(stackly_protocol::instance::StopInstanceParams {
                instance_id: id,
                expected_revision: None,
                override_leases: true,
            }),
            CallOptions::default().with_idempotency_key(key()),
        )
        .await
        .expect("a stop with the override is accepted")
    {
        Reply::InstanceStop(accepted) => {
            accepted
                .accepted
                .expect("a stop begins an operation")
                .operation
                .id
        }
        other => panic!("not a stop reply: {other:?}"),
    }
}

// covers: AC-13 (spec 0047), BR-015
#[tokio::test(flavor = "multi_thread")]
async fn a_persons_stop_with_the_override_keeps_every_lease_and_the_next_up_starts_it_again() {
    let ev = evidence!(
        covers = ["BR-015", "REQ-022"],
        kind = Kind::Fixture,
        expected = "instance.stop with overrideLeases on shop-db, which shop and blog hold, stops it with desired stopped, keeps both leases active and leaseActive true, names both environments in the audit, leaves shop not ready; Up of shop then starts it again and Down of shop keeps it for blog",
    );
    let site = Site::new();
    let daemon = start(&site).await;
    let api = site.api().await;
    let shop_path = write(&site, "shop.yaml", &dedicated("shop"));
    let shop = up_done(&api, &shop_path).await;
    let blog = up_done(
        &api,
        &write(&site, "blog.yaml", &borrowing("blog", "shop-db")),
    )
    .await;
    let db = instance_of(&get(&api, shop.environment_id).await, "db");

    let operation = stop_overriding(&api, db).await;
    let done = api.wait_final(operation).await;
    assert_eq!(done.operation.state, OperationState::Succeeded, "{done:?}");

    let stopped = api.get(db).await.expect("reads");
    assert_eq!(stopped.lifecycle, InstanceLifecycle::Stopped);
    assert_eq!(active_leases(&daemon, db, shop.environment_id), 1);
    assert_eq!(active_leases(&daemon, db, blog.environment_id), 1);
    assert!(
        stopped.environments.iter().all(|e| e.lease_active),
        "{stopped:?}"
    );
    let mut named = audit_values(&api, db, "overriddenEnvironments").await;
    named.sort_unstable();
    assert_eq!(named.len(), 1, "{named:?}");
    assert!(
        named[0].contains("shop") && named[0].contains("blog"),
        "{named:?}"
    );
    let shop_detail = get(&api, shop.environment_id).await;
    assert_eq!(
        shop_detail.lease.map(|l| u64::from(l.active_count)),
        Some(1),
        "the override never releases a lease"
    );

    up_done(&api, &shop_path).await;
    let again = api.get(db).await.expect("reads");
    assert_eq!(again.lifecycle, InstanceLifecycle::Running);
    let kept = down_done(&api, shop.environment_id).await;
    assert_eq!(kept.state, OperationState::Succeeded, "{kept:?}");
    assert_eq!(
        api.get(db).await.expect("reads").lifecycle,
        InstanceLifecycle::Running,
        "blog still holds it"
    );
    stop(daemon).await;
    ev.pass();
}

async fn change_port(
    api: &Api,
    id: Uuid,
    base: u64,
    port: u16,
    dry_run: bool,
) -> Result<ChangeEndpointsReply, Box<ErrorBody>> {
    match api
        .client
        .call_detailed(
            Call::InstanceChangeEndpoints(ChangeEndpointsParams {
                instance_id: id,
                base_revision: ConfigRevisionNumber::new(base).expect("a revision number"),
                endpoints: vec![EndpointRequest {
                    name: "http".to_owned(),
                    port: PortRequest::Fixed(port),
                }],
                dry_run,
                override_leases: false,
            }),
            CallOptions::default().with_idempotency_key(key()),
        )
        .await?
    {
        Reply::InstanceChangeEndpoints(reply) => Ok(*reply),
        other => panic!("not a change endpoints reply: {other:?}"),
    }
}

fn revision_number(view: &InstanceConfigView) -> u64 {
    u64::from(view.revision.number)
}

// covers: AC-15 (spec 0047)
#[tokio::test(flavor = "multi_thread")]
async fn a_lease_that_appears_between_the_dry_run_and_the_apply_refuses_the_port_change() {
    let ev = evidence!(
        covers = ["BR-015"],
        kind = Kind::Fixture,
        expected = "the dry run of a port change on solo, which nothing holds, lists no lease; blog then borrows solo; the apply without the override is refused instance.endpoints_require_override naming blog, and the port stays",
    );
    let site = Site::new();
    let daemon = start(&site).await;
    let api = site.api().await;
    let solo = api.create_ready("solo", serde_json::Value::Null).await;
    api.start_ready(solo).await;
    let base = revision_number(&config_of(&api, solo).await);
    let before = port_of(&api.get(solo).await.expect("reads"));
    let wanted = stackly_testkit::allocate_port().expect("a free port");

    let dry = change_port(&api, solo, base, wanted, true)
        .await
        .expect("a dry run is never refused");
    let validation = match &dry {
        ChangeEndpointsReply::DryRun(validation) => validation,
        other => panic!("not a dry run reply: {other:?}"),
    };
    assert!(validation.leases.is_empty(), "nothing holds solo yet");

    up_done(&api, &write(&site, "blog.yaml", &borrowing("blog", "solo"))).await;
    let refused = change_port(&api, solo, base, wanted, false)
        .await
        .expect_err("the lease that appeared refuses it");
    assert_eq!(refused.code(), "instance.endpoints_require_override");
    assert_eq!(refused.items().len(), 1);
    assert_eq!(refused.items()[0].field("name"), Some("blog"));
    assert_eq!(refused.items()[0].field("ownership"), Some("shared"));
    assert_eq!(port_of(&api.get(solo).await.expect("reads")), before);
    drop(daemon);
    ev.pass();
}

async fn preview_stop_all(api: &Api, override_leases: bool) -> StopAllPreview {
    match api
        .client
        .call(Call::InstancePreviewStopAll(StopAllPreviewParams {
            override_leases,
        }))
        .await
        .expect("the preview answers")
    {
        Reply::InstancePreviewStopAll(preview) => *preview,
        other => panic!("not a preview reply: {other:?}"),
    }
}

async fn stop_all(
    api: &Api,
    expected: Vec<Uuid>,
    override_leases: bool,
) -> Result<(), Box<ErrorBody>> {
    match api
        .client
        .call_detailed(
            Call::InstanceStopAll(StopAllParams {
                expected_instance_ids: Some(expected),
                override_leases,
            }),
            CallOptions::default().with_idempotency_key(key()),
        )
        .await?
    {
        Reply::InstanceStopAll(_) => Ok(()),
        other => panic!("not a stop_all reply: {other:?}"),
    }
}

// covers: AC-16 (spec 0047): `expectedInstanceIds` across the two modes
#[tokio::test(flavor = "multi_thread")]
async fn stop_all_refuses_a_set_shown_with_the_other_override_mode_in_both_directions() {
    let ev = evidence!(
        covers = ["BR-015", "REQ-027"],
        kind = Kind::Fixture,
        expected = "with shop-db held by shop and solo held by nobody, the set shown without the override sent with it, and the set shown with the override sent without it, are both refused instance.stop_all_changed and start no operation; both instances keep running and the lease stays",
    );
    let site = Site::new();
    let daemon = start(&site).await;
    let api = site.api().await;
    let shop = up_done(&api, &write(&site, "shop.yaml", &dedicated("shop"))).await;
    let db = instance_of(&get(&api, shop.environment_id).await, "db");
    let solo = api.create_ready("solo", serde_json::Value::Null).await;
    api.start_ready(solo).await;
    let ids = |preview: &StopAllPreview| -> Vec<Uuid> {
        preview.items.iter().map(|item| item.instance_id).collect()
    };
    let plain = preview_stop_all(&api, false).await;
    let overriding = preview_stop_all(&api, true).await;
    assert_eq!(ids(&plain), [solo]);
    assert_eq!(ids(&overriding).len(), 2);
    let before = operations(&api).await.len();

    let refused = stop_all(&api, ids(&plain), true)
        .await
        .expect_err("the override would stop shop-db too");
    assert_eq!(refused.code(), "instance.stop_all_changed");
    let refused = stop_all(&api, ids(&overriding), false)
        .await
        .expect_err("without the override shop-db is skipped");
    assert_eq!(refused.code(), "instance.stop_all_changed");

    assert_eq!(operations(&api).await.len(), before, "no operation started");
    for id in [db, solo] {
        assert_eq!(
            api.get(id).await.expect("reads").lifecycle,
            InstanceLifecycle::Running
        );
    }
    assert_eq!(active_leases(&daemon, db, shop.environment_id), 1);
    stop(daemon).await;
    ev.pass();
}

async fn rename(api: &Api, id: Uuid, name: &str) -> Uuid {
    match api
        .client
        .call_detailed(
            Call::InstanceRename(RenameInstanceParams {
                instance_id: id,
                name: name.to_owned(),
                expected_revision: None,
            }),
            CallOptions::default().with_idempotency_key(key()),
        )
        .await
        .expect("a rename needs no override")
    {
        Reply::InstanceRename(accepted) => {
            accepted
                .accepted
                .expect("a rename begins an operation")
                .operation
                .id
        }
        other => panic!("not a rename reply: {other:?}"),
    }
}

async fn apply_greeting(api: &Api, id: Uuid, greeting: &str) -> Uuid {
    let view = config_of(api, id).await;
    let mut map = view.revision.values.as_map().clone();
    map.insert("greeting".to_owned(), serde_json::json!(greeting));
    match api
        .client
        .call_detailed(
            Call::InstanceConfigApply(ApplyConfigParams {
                instance_id: id,
                base_revision: view.revision.number,
                change: ConfigChangeRequest::Settings(SettingsDocument::new(map)),
                expected_revision: None,
            }),
            CallOptions::default().with_idempotency_key(key()),
        )
        .await
        .expect("a settings change needs no override")
    {
        Reply::InstanceConfigApply(accepted) => {
            accepted
                .accepted
                .expect("a change begins an operation")
                .operation
                .id
        }
        other => panic!("not an apply reply: {other:?}"),
    }
}

// covers: AC-14 (spec 0047)
#[tokio::test(flavor = "multi_thread")]
async fn restart_reload_a_settings_apply_and_a_rename_of_a_leased_instance_need_no_override() {
    let ev = evidence!(
        covers = ["BR-015"],
        kind = Kind::Fixture,
        expected = "on shop-db, which shop holds, instance.restart, instance.reload, instance.config.apply of a reload setting, and instance.rename each succeed with no override field, the endpoint keeps its port, and shop's lease stays active",
    );
    let site = Site::new();
    let daemon = start(&site).await;
    let api = site.api().await;
    let shop = up_done(&api, &write(&site, "shop.yaml", &dedicated("shop"))).await;
    let db = instance_of(&get(&api, shop.environment_id).await, "db");
    let port = port_of(&api.get(db).await.expect("reads"));

    let restart = api.restart(db).await.expect("a restart needs no override");
    let done = api
        .wait_final(restart.accepted.expect("an operation").operation.id)
        .await;
    assert_eq!(done.operation.state, OperationState::Succeeded, "{done:?}");
    let reload = api.reload(db).await.expect("a reload needs no override");
    let done = api
        .wait_final(reload.accepted.expect("an operation").operation.id)
        .await;
    assert_eq!(done.operation.state, OperationState::Succeeded, "{done:?}");
    let apply = apply_greeting(&api, db, "hello from a leased instance").await;
    let done = api.wait_final(apply).await;
    assert_eq!(done.operation.state, OperationState::Succeeded, "{done:?}");
    let renamed = rename(&api, db, "shop-db-renamed").await;
    let done = api.wait_final(renamed).await;
    assert_eq!(done.operation.state, OperationState::Succeeded, "{done:?}");

    let now = api.get(db).await.expect("reads");
    assert_eq!(now.name, "shop-db-renamed");
    assert_eq!(now.lifecycle, InstanceLifecycle::Running);
    assert_eq!(port_of(&now), port, "the same port");
    assert_eq!(active_leases(&daemon, db, shop.environment_id), 1);
    stop(daemon).await;
    ev.pass();
}

/// Runs `stackly` for the CLI site with a clean environment, without
/// waiting.
fn spawn_cli(site: &Site, args: &[&str]) -> std::process::Child {
    std::process::Command::new(stackly_testkit::cargo_bin("stackly-cli", "stackly"))
        .current_dir(site.root.path())
        .args(args)
        .env("STACKLY_ROOT", site.root.path())
        .env("NO_COLOR", "1")
        .env("TERM", "dumb")
        .env_remove("STACKLY_NO_INPUT")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("stackly runs")
}

// covers: AC-12 (spec 0047): the lease belongs to the environment, not the
// connection, so closing the CLI in the middle of a Down changes nothing
#[tokio::test(flavor = "multi_thread")]
async fn killing_stackly_env_down_in_the_middle_leaves_the_down_running_to_its_end() {
    let ev = evidence!(
        covers = ["REQ-022", "REQ-028"],
        kind = Kind::Fixture,
        expected = "stackly env down shop --wait, killed with SIGKILL while web's stop runs, leaves the Down running: it ends on its own, shop holds no lease, and env.get reads the same lease line from another client",
    );
    let site = Site::for_cli();
    let daemon = start(&site).await;
    let api = site.api().await;
    let shop = up_done(&api, &write(&site, "shop.yaml", &chain("shop"))).await;
    let web = instance_of(&get(&api, shop.environment_id).await, "web");
    let web_port = port_of(&api.get(web).await.expect("reads"));

    let mut cli = spawn_cli(&site, &["env", "down", "shop", "--wait"]);
    wait_summary(&api, web, "the Down reached web", |s| {
        s.lifecycle == InstanceLifecycle::Stopping
    })
    .await;
    let running: Vec<OperationSummary> = operations(&api)
        .await
        .into_iter()
        .filter(|op| {
            op.kind == OperationKind::EnvironmentDown
                && !matches!(
                    op.state,
                    OperationState::Succeeded | OperationState::Failed | OperationState::Cancelled
                )
        })
        .collect();
    assert_eq!(running.len(), 1, "the CLI started one Down");
    cli.kill().expect("the CLI can be killed");
    let _ = cli.wait();
    shutdown_server(web_port);

    let done = api.wait_final(running[0].id).await;
    assert_eq!(done.operation.state, OperationState::Succeeded, "{done:?}");
    assert_eq!(active_leases(&daemon, web, shop.environment_id), 0);
    let other = site.api().await;
    assert_eq!(
        get(&other, shop.environment_id)
            .await
            .lease
            .map(|l| u64::from(l.active_count)),
        Some(0)
    );
    stop(daemon).await;
    ev.pass();
}
