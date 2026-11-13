//! `env.up` of the ERP example manifest with the six official providers and
//! their real runtimes (spec 0046 AC-25, JOURNEY-04 the Up part, build step
//! 11). The runtimes come from the runtime cache of `cargo xtask` and are
//! installed before the Up, so nothing is downloaded: an artifact the plan
//! picks that is not in the cache ends the test as `BLOCKED`. Native:
//! `cargo test -p stacklyd --test env_up_native -- --ignored`.
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
use stackly_core::provider::{ArtifactStatus, NameCheck, ValidatedPackage, read_package};
use stackly_core::runtime::fixture::install_cached_artifact;
use stackly_ipc::CallOptions;
use stackly_protocol::env::{
    EnvGetParams, EnvPlan, EnvPlanParams, EnvSource, EnvUpParams, EnvUpReply, EnvironmentDetail,
    PlanAction, UpPhase, UpState,
};
use stackly_protocol::operation::{OperationDetail, OperationState};
use stackly_protocol::secret::{SecretInput, SecretMode, SecretText, SetSecretParams};
use stackly_protocol::{Call, Reply};
use stackly_testkit::evidence::{Kind, evidence};
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
/// How long six real runtimes may take to create, start, and be ready.
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
            "the up did not end: {detail:?}"
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

// covers: AC-25 (spec 0046)
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: brings up the six official runtimes from the xtask runtime cache"]
async fn the_erp_manifest_comes_up_from_the_runtime_cache_and_a_second_up_does_nothing() {
    let ev = evidence!(
        covers = ["REQ-021", "AC-021", "JOURNEY-04"],
        kind = Kind::Native,
        expected = "env.up of the ERP example manifest, with the six official runtimes installed from the xtask runtime cache and no download, creates and starts postgresql, mysql, nats, clickhouse, valkey, and traefik until all six are ready, env.get shows lastUp succeeded with each endpoint's bound port, and a second Up keeps all six and starts no child operation",
    );
    let site = Site::new();
    let path = manifest(&site);

    // The plan names the artifact of each runtime; install each from the cache.
    let daemon = start(&site).await;
    let api = site.api().await;
    let first = plan(&api, &path).await;
    stop(daemon).await;
    assert_eq!(first.items.len(), 6, "{first:?}");
    let cache = RuntimeCache::open();
    let mut missing = Vec::new();
    let mut installs = Vec::new();
    for item in &first.items {
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
            Some(file) => installs.push((package, runtime.artifact_id.clone(), file)),
            None => missing.push(runtime.artifact_id.clone()),
        }
    }
    if !missing.is_empty() {
        ev.blocked(
            &format!(
                "artifacts {} are not in the xtask runtime cache",
                missing.join(", ")
            ),
            OWNER,
        );
        return;
    }
    {
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
    }

    let daemon = start(&site).await;
    let api = site.api().await;
    let planned = plan(&api, &path).await;
    assert!(
        planned
            .items
            .iter()
            .all(|item| item.action == PlanAction::Create
                && item
                    .runtime
                    .as_ref()
                    .is_some_and(|runtime| !runtime.install)),
        "six creates with nothing to install: {planned:?}"
    );
    set_secrets(&api, &planned).await;

    let (_, reply) = up(&api, &path).await;
    let done = wait_final(&api, reply.operation.operation.id).await;
    let detail = get(&api, reply.environment_id).await;
    assert_eq!(
        done.operation.state,
        OperationState::Succeeded,
        "{done:?}\n{detail:?}"
    );
    let last = detail.last_up.as_ref().expect("lastUp is written");
    assert_eq!(last.state, UpState::Succeeded, "{last:?}");
    assert_eq!(detail.summary.ready_count.get(), 6, "{detail:?}");
    assert_eq!(detail.services.len(), 6);
    for service in &detail.services {
        assert!(service.not_ready.is_none(), "{service:?}");
        let instance = service.instance.as_ref().expect("each service has one");
        assert!(
            !instance.endpoints.is_empty()
                && instance
                    .endpoints
                    .iter()
                    .all(|endpoint| endpoint.port.is_some()),
            "{instance:?}"
        );
    }

    // A second Up keeps all six and starts no child.
    let (again, second) = up(&api, &path).await;
    assert!(
        again
            .items
            .iter()
            .all(|item| item.action == PlanAction::Keep),
        "{again:?}"
    );
    let done = wait_final(&api, second.operation.operation.id).await;
    assert_eq!(done.operation.state, OperationState::Succeeded, "{done:?}");
    let detail = get(&api, second.environment_id).await;
    let last = detail.last_up.expect("lastUp is written");
    assert_eq!(last.state, UpState::Succeeded);
    assert!(
        last.services
            .iter()
            .all(|service| service.phase == UpPhase::Ready && service.operation_id.is_none()),
        "{last:?}"
    );
    stop(daemon).await;
    ev.pass();
}
