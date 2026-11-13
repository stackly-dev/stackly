//! `env.down` of the ERP example manifest with the six official providers and
//! their real runtimes (spec 0047 AC-26, JOURNEY-04 the Down part, build step
//! 10): Up, then Down, then Up again. The runtimes come from the runtime cache
//! of `cargo xtask` and are installed before the Up, so nothing is
//! downloaded: an artifact the plan picks that is not in the cache ends the
//! test as `BLOCKED`. Native:
//! `cargo test -p stacklyd --test env_down_native -- --ignored`.
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
use stackly_core::provider::{ArtifactStatus, NameCheck, ValidatedPackage, read_package};
use stackly_core::runtime::fixture::install_cached_artifact;
use stackly_ipc::CallOptions;
use stackly_protocol::env::{
    EnvDownParams, EnvDownPreview, EnvDownReply, EnvGetParams, EnvPlan, EnvPlanParams,
    EnvPreviewDownParams, EnvSource, EnvUpParams, EnvUpReply, EnvironmentDetail, PlanAction,
    UpState,
};
use stackly_protocol::instance::{InstanceLifecycle, InstanceReadinessState};
use stackly_protocol::operation::{OperationDetail, OperationState};
use stackly_protocol::secret::{SecretInput, SecretMode, SecretText, SetSecretParams};
use stackly_protocol::{Call, Reply};
use stackly_testkit::evidence::{Kind, Source, evidence};
use stackly_testkit::{RuntimeCache, allocate_port};
use stacklyd::server::DaemonHandle;
use uuid::Uuid;

use crate::support::{Api, Site, fast, key};

/// Who can fill the runtime cache (spec 0002).
const OWNER: &str = "user";
/// The six official packages of the ERP manifest, by folder.
const PROVIDERS: [&str; 6] = [
    "postgresql",
    "mysql",
    "nats",
    "clickhouse",
    "valkey",
    "traefik",
];
/// The ports the ERP manifest names; each one is replaced by a free port.
const PORTS: [u16; 9] = [5432, 3306, 4222, 8222, 8123, 9000, 6379, 8000, 8082];
/// How long six real runtimes may take to create, start, stop, and be ready.
const UP_PATIENCE: Duration = Duration::from_secs(600);

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn package(name: &str) -> ValidatedPackage {
    let dir = repo().join("providers").join(name);
    let files =
        read_package(&stackly_platform_macos::MacosFilesystem, &dir).expect("the package reads");
    ValidatedPackage::from_files(files, NameCheck::FolderName).expect("the package validates")
}

/// The six official packages as the daemon's catalog.
fn catalog() -> Catalog {
    let entries: Vec<_> = PROVIDERS
        .iter()
        .map(|name| {
            CatalogEntry::Provider(ProviderEntry {
                package: Arc::new(package(name)),
                tile: TileHint {
                    mark: "Te".to_owned(),
                    tile: TileColor::Blue,
                },
                notes: Vec::new(),
                origin: EntryOrigin::Catalog,
            })
        })
        .collect();
    Catalog::for_tests(&entries)
}

async fn start(site: &Site) -> DaemonHandle {
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

/// The ERP example manifest with every port replaced by a free one, so a
/// database that already runs on this Mac does not block the plan.
fn manifest(site: &Site) -> PathBuf {
    let mut text = std::fs::read_to_string(
        repo().join("crates/core/tests/fixtures/environments/erp-development.yaml"),
    )
    .expect("the ERP manifest reads");
    for port in PORTS {
        let free = allocate_port().expect("a free port");
        text = text.replace(&format!("port: {port}}}"), &format!("port: {free}}}"));
    }
    let dir = site.root.path().join("erp");
    std::fs::create_dir_all(&dir).expect("the project folder can be made");
    let path = dir.join("stackly.yaml");
    std::fs::write(&path, text).expect("the manifest writes");
    path
}

async fn plan(api: &Api, path: &Path) -> EnvPlan {
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

/// Plans `path`, then brings it up with the plan's digest.
async fn up(api: &Api, path: &Path) -> (EnvPlan, EnvUpReply) {
    let planned = plan(api, path).await;
    let digest = planned
        .plan_digest
        .clone()
        .unwrap_or_else(|| panic!("the plan is applicable: {:?}", planned.blockers));
    let reply = match api
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
    };
    (planned, reply)
}

async fn wait_final(api: &Api, id: Uuid) -> OperationDetail {
    let deadline = tokio::time::Instant::now() + UP_PATIENCE;
    loop {
        let detail = api.operation(id).await;
        if matches!(
            detail.operation.state,
            OperationState::Succeeded
                | OperationState::Failed
                | OperationState::Cancelled
                | OperationState::NeedsRecovery
        ) {
            return detail;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the operation did not end: {detail:?}"
        );
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
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

/// Stores a fixture value for every `local:` reference of the plan.
async fn set_secrets(api: &Api, planned: &EnvPlan) {
    for item in &planned.items {
        for secret in &item.secrets {
            let Some(reference) = &secret.reference else {
                continue;
            };
            let value = format!("stackly-test-erp-{}", item.alias);
            let reply = api
                .client
                .call(Call::SecretSet(SetSecretParams {
                    reference: reference.clone(),
                    input: SecretInput::Value(SecretText::new(value)),
                    mode: SecretMode::Upsert,
                    expected_revision: None,
                    confirm_stored_only: false,
                }))
                .await;
            assert!(reply.is_ok(), "the secret {reference} is stored: {reply:?}");
        }
    }
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

/// Previews `id`, then brings it down with the preview's stop set.
async fn down(api: &Api, id: Uuid) -> (EnvDownPreview, EnvDownReply) {
    let shown = preview(api, id).await;
    let reply = match api
        .client
        .call_detailed(
            Call::EnvDown(EnvDownParams {
                id,
                expected_stop_instance_ids: shown
                    .stop
                    .iter()
                    .map(|item| item.instance_id)
                    .collect(),
            }),
            CallOptions::default().with_idempotency_key(key()),
        )
        .await
        .expect("the down starts")
    {
        Reply::EnvDown(reply) => *reply,
        other => panic!("not a down reply: {other:?}"),
    };
    (shown, reply)
}

/// Files a database writes only while it runs, which a normal stop removes.
fn is_run_file(path: &Path) -> bool {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default();
    name.ends_with(".pid") || name.ends_with(".lock") || name.ends_with(".sock")
}

/// How many files the data folder of `instance` holds, outside the files of
/// a running process.
fn data_count(site: &Site, instance: Uuid) -> usize {
    let paths = site.instance_paths(instance);
    let mut count = 0;
    let mut stack = vec![paths.data.clone()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(meta) = std::fs::symlink_metadata(&path) else {
                continue;
            };
            if meta.is_dir() {
                stack.push(path);
            } else if meta.is_file() && !is_run_file(&path) {
                count += 1;
            }
        }
    }
    count
}

/// Installs the runtime of every create in `planned` from the xtask runtime
/// cache, or names the artifacts the cache does not hold.
fn install_from_cache(
    site: &Site,
    planned: &EnvPlan,
    ev: &stackly_testkit::evidence::Evidence,
) -> Result<(), Vec<String>> {
    let cache = RuntimeCache::open();
    let mut missing = Vec::new();
    let mut installs = Vec::new();
    for item in &planned.items {
        let runtime = item.runtime.as_ref().expect("a create names its runtime");
        let name = item
            .provider
            .strip_prefix("official/")
            .expect("an official provider");
        let package = package(name);
        let size = package
            .versions()
            .artifacts()
            .iter()
            .find(|artifact| artifact.id == runtime.artifact_id)
            .and_then(|artifact| match &artifact.status {
                ArtifactStatus::Available { size, .. } => Some(*size),
                _ => None,
            })
            .unwrap_or_else(|| panic!("versions.json has {} available", runtime.artifact_id));
        match cache
            .as_ref()
            .and_then(|cache| cache.entry(&runtime.artifact_sha256, size))
        {
            Some(file) => {
                ev.subject(
                    name,
                    &runtime.version,
                    Some(&runtime.artifact_sha256),
                    Source::Managed,
                );
                installs.push((package, runtime.artifact_id.clone(), file));
            }
            None => missing.push(runtime.artifact_id.clone()),
        }
    }
    if !missing.is_empty() {
        return Err(missing);
    }
    let registry = site.open_registry();
    for (package, artifact, file) in &installs {
        install_cached_artifact(
            &site.platform,
            &site.prepared,
            &registry,
            package,
            artifact,
            file,
        )
        .unwrap_or_else(|error| panic!("{artifact} installs from the cache: {error:?}"));
    }
    Ok(())
}

/// The instance of each service of `detail`, by alias.
fn instances(detail: &EnvironmentDetail) -> BTreeMap<String, Uuid> {
    detail
        .services
        .iter()
        .map(|service| {
            let instance = service.instance.as_ref().expect("each service has one");
            (service.alias.clone(), instance.id)
        })
        .collect()
}

// covers: AC-26 (spec 0047)
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: brings the six official runtimes up and down from the xtask runtime cache"]
async fn the_erp_manifest_goes_down_keeping_its_data_and_comes_up_again_with_it() {
    let ev = evidence!(
        covers = ["REQ-022", "AC-022"],
        kind = Kind::Native,
        expected = "after env.up of the ERP example manifest with the six official runtimes from the xtask runtime cache, env.preview_down lists all six to stop and none to keep, env.down succeeds and leaves all six stopped with their data folders in place and the same number of data files, the environment holds no active lease, and a second Up brings all six back to ready on the same instances",
    );
    let site = Site::new();
    let path = manifest(&site);

    // The plan names the artifact of each runtime; install each from the cache.
    let daemon = start(&site).await;
    let api = site.api().await;
    let first = plan(&api, &path).await;
    stop(daemon).await;
    assert_eq!(first.items.len(), 6, "{first:?}");
    if let Err(missing) = install_from_cache(&site, &first, &ev) {
        ev.blocked(
            &format!(
                "artifacts {} are not in the xtask runtime cache",
                missing.join(", ")
            ),
            OWNER,
        );
        return;
    }

    let daemon = start(&site).await;
    let api = site.api().await;
    let planned = plan(&api, &path).await;
    set_secrets(&api, &planned).await;
    let (_, reply) = up(&api, &path).await;
    let done = wait_final(&api, reply.operation.operation.id).await;
    let environment = reply.environment_id;
    let detail = get(&api, environment).await;
    assert_eq!(
        done.operation.state,
        OperationState::Succeeded,
        "{done:?}\n{detail:?}"
    );
    assert_eq!(detail.summary.ready_count.get(), 6, "{detail:?}");
    let members = instances(&detail);
    assert_eq!(members.len(), 6);
    let lease = detail.lease.as_ref().expect("the detail carries the lease");
    assert_eq!(lease.active_count.get(), 6, "{lease:?}");
    let before: BTreeMap<String, usize> = members
        .iter()
        .map(|(alias, id)| (alias.clone(), data_count(&site, *id)))
        .collect();
    // NATS and Traefik keep no data files; the databases do.
    assert!(
        before.values().sum::<usize>() > 0,
        "the data folders hold files: {before:?}"
    );

    // Down: the preview stops all six and keeps none, then each one stops.
    let (shown, reply) = down(&api, environment).await;
    assert_eq!(shown.stop.len(), 6, "{shown:?}");
    assert!(
        shown.keep.is_empty() && shown.blocked.is_empty(),
        "{shown:?}"
    );
    assert_eq!(shown.lease_count.get(), 6);
    let done = wait_final(&api, reply.operation.operation.id).await;
    assert_eq!(done.operation.state, OperationState::Succeeded, "{done:?}");
    let mut observed = Vec::new();
    for (alias, id) in &members {
        let summary = api.get(*id).await.expect("the instance reads");
        assert_eq!(
            summary.lifecycle,
            InstanceLifecycle::Stopped,
            "{alias}: {summary:?}"
        );
        let paths = site.instance_paths(*id);
        assert!(paths.data.is_dir(), "{alias}: the data folder stays");
        let after = data_count(&site, *id);
        assert_eq!(after, before[alias], "{alias}: the data files stay");
        observed.push(format!("{alias} stopped with {after} data files"));
    }
    let detail = get(&api, environment).await;
    let lease = detail.lease.as_ref().expect("the detail carries the lease");
    assert_eq!(lease.active_count.get(), 0, "{lease:?}");
    assert!(detail.last_up.is_none(), "Down clears lastUp: {detail:?}");

    // A second Up starts the same six instances on the same data.
    let (again, second) = up(&api, &path).await;
    assert!(
        again
            .items
            .iter()
            .all(|item| item.action != PlanAction::Create),
        "the second Up creates nothing: {again:?}"
    );
    let done = wait_final(&api, second.operation.operation.id).await;
    let detail = get(&api, environment).await;
    assert_eq!(
        done.operation.state,
        OperationState::Succeeded,
        "{done:?}\n{detail:?}"
    );
    assert_eq!(
        detail.last_up.as_ref().map(|up| up.state),
        Some(UpState::Succeeded)
    );
    assert_eq!(instances(&detail), members, "the same instances come back");
    for (alias, id) in &members {
        let summary = api.get(*id).await.expect("the instance reads");
        assert_eq!(
            summary.readiness.state,
            InstanceReadinessState::Ready,
            "{alias}: {summary:?}"
        );
    }

    // Leave nothing running.
    let (_, reply) = down(&api, environment).await;
    let done = wait_final(&api, reply.operation.operation.id).await;
    assert_eq!(done.operation.state, OperationState::Succeeded, "{done:?}");
    stop(daemon).await;
    ev.pass_with(&format!(
        "Down stopped 6 of 6 and kept none: {}; the lease count went 6 to 0; the second Up brought the same six back to ready",
        observed.join("; ")
    ));
}
