//! The environment methods through the real daemon (spec 0044 milestone 1,
//! build step 5): `env.validate` and `env.plan` read the manifest and write
//! nothing; `env.import` and `env.remove` are registry only operations with
//! replay and audit; `env.list` and `env.get` read the registry and the
//! supervisor; the membership cache gives `InstanceSummary.ownership` and
//! `environments` the same value in `instance.get`, `instance.list`, and
//! the `instance.*` events.
#![cfg(target_os = "macos")]

mod common;
#[path = "supervisor/support.rs"]
mod support;

use std::io::Write;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::Duration;

use stackly_core::catalog::{
    Catalog, CatalogEntry, EntryOrigin, ProviderEntry, TileColor, TileHint,
};
use stackly_core::provider::{NameCheck, ValidatedPackage, read_package};
use stackly_domain::{
    AuditQuery, Environment, EnvironmentMember, InstanceId, Lease, LeaseHolder, LeaseId,
    OperationQuery, Ownership, Registry as _, StoredEnvironment, WriteBatch, WriteOp,
};
use stackly_ipc::CallOptions;
use stackly_platform::{Clock as _, SystemClock};
use stackly_protocol::env::{
    EnvGetParams, EnvOwnership, EnvPlan, EnvPlanParams, EnvSource, EnvValidateParams,
    EnvValidation, EnvironmentDetail, EnvironmentList, ImportEnvironmentParams, ManifestState,
    PlanAction, PlanStep, RemoveEnvironmentParams, RemovedEnvironment,
};
use stackly_protocol::events::{EventKind, SubscribeParams};
use stackly_protocol::instance::{InstanceOwnership, ListInstancesParams};
use stackly_protocol::json::DecodedFrame;
use stackly_protocol::{Call, ErrorBody, Reply, Revision};
use stackly_testkit::evidence::{Kind, evidence};
use stacklyd::server::DaemonHandle;
use uuid::Uuid;

use crate::support::{Site, fast, key};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn entry(name: &str) -> CatalogEntry {
    let dir = repo().join("providers").join(name);
    let files =
        read_package(&stackly_platform_macos::MacosFilesystem, &dir).expect("the package reads");
    let package =
        ValidatedPackage::from_files(files, NameCheck::FolderName).expect("the package validates");
    CatalogEntry::Provider(ProviderEntry {
        package: Arc::new(package),
        tile: TileHint {
            mark: "Te".to_owned(),
            tile: TileColor::Blue,
        },
        notes: Vec::new(),
        origin: EntryOrigin::Catalog,
    })
}

/// The fake provider and the six official ones, as the daemon's catalog.
fn catalog() -> Catalog {
    let names = [
        "fake",
        "postgresql",
        "mysql",
        "nats",
        "clickhouse",
        "valkey",
        "traefik",
    ];
    let entries: Vec<_> = names.iter().map(|name| entry(name)).collect();
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

/// Every log line of this test binary, from a global subscriber, so a
/// sentinel test can look for its value in what the daemon logged.
fn logs() -> Arc<Mutex<Vec<u8>>> {
    static LOGS: OnceLock<Arc<Mutex<Vec<u8>>>> = OnceLock::new();
    Arc::clone(LOGS.get_or_init(|| {
        let buffer = Arc::new(Mutex::new(Vec::new()));
        let writer = Arc::clone(&buffer);
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::TRACE)
            .with_ansi(false)
            .with_writer(move || Tee(Arc::clone(&writer)))
            .finish();
        let _ = tracing::subscriber::set_global_default(subscriber);
        buffer
    }))
}

struct Tee(Arc<Mutex<Vec<u8>>>);

impl Write for Tee {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn project(site: &Site) -> PathBuf {
    let dir = site.root.path().join("project");
    std::fs::create_dir_all(&dir).expect("the project folder can be made");
    dir
}

fn write(path: &Path, text: &str) -> PathBuf {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("the folder can be made");
    }
    std::fs::write(path, text).expect("the manifest writes");
    path.to_path_buf()
}

fn erp() -> String {
    std::fs::read_to_string(
        repo().join("crates/core/tests/fixtures/environments/erp-development.yaml"),
    )
    .expect("the ERP manifest reads")
}

fn graph() -> String {
    std::fs::read_to_string(repo().join("crates/core/tests/fixtures/environments/graph.yaml"))
        .expect("the graph manifest reads")
}

/// A manifest named `name` with one fake service per alias.
fn fake_manifest(name: &str, aliases: &[&str]) -> String {
    let mut text = format!(
        "apiVersion: stackly/v1alpha1\nkind: Environment\nmetadata:\n  name: {name}\n\nservices:\n"
    );
    for alias in aliases {
        text.push_str(&format!(
            "  {alias}:\n    provider: testing/fake\n    version: \"0.1\"\n"
        ));
    }
    text
}

async fn validate(api: &support::Api, path: &Path) -> Result<EnvValidation, Box<ErrorBody>> {
    let params = EnvValidateParams {
        path: path.display().to_string(),
    };
    match api.client.call(Call::EnvValidate(params)).await? {
        Reply::EnvValidate(validation) => Ok(*validation),
        other => panic!("not a validation: {other:?}"),
    }
}

async fn plan_reply(api: &support::Api, source: EnvSource) -> Result<Reply, Box<ErrorBody>> {
    api.client
        .call(Call::EnvPlan(EnvPlanParams { source }))
        .await
}

async fn plan(api: &support::Api, path: &Path) -> Result<EnvPlan, Box<ErrorBody>> {
    match plan_reply(api, EnvSource::File(path.display().to_string())).await? {
        Reply::EnvPlan(plan) => Ok(*plan),
        other => panic!("not a plan: {other:?}"),
    }
}

async fn import_with(
    api: &support::Api,
    path: &Path,
    digest: &str,
    relink: Option<Uuid>,
    idempotency: &str,
) -> Result<EnvironmentDetail, Box<ErrorBody>> {
    let params = ImportEnvironmentParams {
        path: path.display().to_string(),
        expected_manifest_sha256: digest.to_owned(),
        relink_environment_id: relink,
    };
    match api
        .client
        .call_detailed(
            Call::EnvImport(params),
            CallOptions::default().with_idempotency_key(idempotency),
        )
        .await?
    {
        Reply::EnvImport(detail) => Ok(*detail),
        other => panic!("not an import reply: {other:?}"),
    }
}

/// Plans `path` for its digest, then imports it with a fresh key.
async fn import(api: &support::Api, path: &Path) -> Result<EnvironmentDetail, Box<ErrorBody>> {
    let digest = validate(api, path)
        .await?
        .manifest_sha256
        .expect("a readable file has a digest");
    import_with(api, path, &digest, None, &key()).await
}

async fn remove(
    api: &support::Api,
    id: Uuid,
    revision: Revision,
    idempotency: &str,
) -> Result<RemovedEnvironment, Box<ErrorBody>> {
    let params = RemoveEnvironmentParams {
        id,
        expected_revision: revision,
    };
    match api
        .client
        .call_detailed(
            Call::EnvRemove(params),
            CallOptions::default().with_idempotency_key(idempotency),
        )
        .await?
    {
        Reply::EnvRemove(removed) => Ok(removed),
        other => panic!("not a remove reply: {other:?}"),
    }
}

async fn list(api: &support::Api) -> EnvironmentList {
    match api
        .client
        .call(Call::EnvList)
        .await
        .expect("the list answers")
    {
        Reply::EnvList(list) => list,
        other => panic!("not a list: {other:?}"),
    }
}

async fn get(
    api: &support::Api,
    id: Uuid,
    check_manifest: bool,
) -> Result<EnvironmentDetail, Box<ErrorBody>> {
    let params = EnvGetParams { id, check_manifest };
    match api.client.call(Call::EnvGet(params)).await? {
        Reply::EnvGet(detail) => Ok(*detail),
        other => panic!("not a detail: {other:?}"),
    }
}

fn detail(error: &ErrorBody, key: &str) -> Option<String> {
    error
        .details()
        .iter()
        .find(|detail| detail.key() == key)
        .map(|detail| detail.value().to_owned())
}

/// Every file and folder under `root` with its size and mtime, skipping
/// the project folder, the daemon's logs, its socket, and the registry
/// files, which change on their own.
fn tree(root: &Path) -> Vec<(String, u64, Option<std::time::SystemTime>)> {
    let mut found = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let relative = path
                .strip_prefix(root)
                .expect("under the root")
                .display()
                .to_string();
            if relative.starts_with("project")
                || relative.contains("logs")
                || relative.ends_with(".sock")
                || relative.contains("registry")
            {
                continue;
            }
            let meta = entry.metadata().expect("the entry has metadata");
            if meta.is_dir() {
                pending.push(path);
                found.push((relative, 0, None));
            } else {
                found.push((relative, meta.len(), meta.modified().ok()));
            }
        }
    }
    found.sort();
    found
}

/// The registry generation, the operation count, and the audit count.
fn counts(daemon: &DaemonHandle) -> (stackly_domain::Generation, usize, usize) {
    let registry = daemon.core().registry().expect("the registry is open");
    let generation = registry.known_instance_ids().expect("reads").generation;
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
    let audit = registry
        .audit_events(AuditQuery::page(500, None))
        .expect("reads")
        .value
        .items
        .len();
    (generation, operations, audit)
}

// covers: AC-4, AC-6 (spec 0044)
#[tokio::test(flavor = "multi_thread")]
async fn validate_reads_the_file_answers_every_finding_and_writes_nothing() {
    let ev = evidence!(
        covers = ["REQ-020", "VAL-013"],
        kind = Kind::Fixture,
        expected = "env.validate reads a stackly.yaml through the daemon, answers valid with its services and digest or every finding, refuses an unreadable path with env.manifest_unreadable, and writes no file, registry row, operation, or audit row",
    );
    let site = Site::new();
    let daemon = start(&site).await;
    let api = site.api().await;
    let dir = project(&site);
    let erp_path = write(&dir.join("stackly.yaml"), &erp());
    let cyclic = write(
        &dir.join("cyclic.yaml"),
        "apiVersion: stackly/v1alpha1\nkind: Environment\nmetadata:\n  name: cyclic\nservices:\n  a:\n    provider: testing/fake\n    version: \"0.1\"\n    dependsOn: [{service: b}]\n  b:\n    provider: testing/fake\n    version: \"0.1\"\n    dependsOn: [{service: a}]\n  c:\n    provider: nobody/nothing\n",
    );
    symlink(&erp_path, dir.join("link.yaml")).expect("the link can be made");
    let before_counts = counts(&daemon);
    let before_tree = tree(site.root.path());

    let valid = validate(&api, &erp_path).await.expect("validates");
    assert!(valid.valid, "{:?}", valid.findings);
    assert_eq!(valid.name.as_deref(), Some("erp-development"));
    assert_eq!(valid.services.len(), 6);
    assert_eq!(valid.file, erp_path.display().to_string());
    assert_eq!(valid.supported_api_versions, ["stackly/v1alpha1"]);
    assert_eq!(valid.manifest_sha256.as_ref().map(String::len), Some(64));
    assert_eq!(u64::from(valid.finding_count), 0);

    let invalid = validate(&api, &cyclic)
        .await
        .expect("an invalid file still answers");
    assert!(!invalid.valid);
    let codes: Vec<_> = invalid.findings.iter().map(|f| f.code.as_str()).collect();
    assert!(codes.contains(&"dependency_cycle"), "{codes:?}");
    assert!(codes.contains(&"unknown_provider"), "{codes:?}");
    assert_eq!(
        u64::from(invalid.finding_count),
        invalid.findings.len() as u64
    );

    for (path, reason) in [
        (dir.join("missing.yaml"), "not_found"),
        (dir.join("link.yaml"), "not_a_file"),
        (dir.clone(), "not_a_file"),
    ] {
        let error = validate(&api, &path).await.expect_err("refused");
        assert_eq!(error.code(), "env.manifest_unreadable", "{path:?}");
        assert_eq!(
            detail(&error, "reason").as_deref(),
            Some(reason),
            "{path:?}"
        );
        assert!(error.hint().is_some(), "every env code has a hint");
    }

    assert_eq!(counts(&daemon), before_counts, "nothing is written");
    assert_eq!(tree(site.root.path()), before_tree, "no file changes");
    drop(api);
    stop(daemon).await;
    ev.pass();
}

// covers: AC-5, AC-6, AC-25 (spec 0044, the socket half)
#[tokio::test(flavor = "multi_thread")]
async fn a_plan_reads_this_mac_and_starts_no_operation() {
    let ev = evidence!(
        covers = ["AC-020", "REQ-020"],
        kind = Kind::Fixture,
        expected = "env.plan of the ERP manifest on a root without official runtimes answers six install, six create, and six start steps in manifest order, the graph manifest starts db, cache, api, web, and neither plan writes a file, a registry row, an operation, or an audit row, nor creates an instance",
    );
    let site = Site::new();
    let daemon = start(&site).await;
    let api = site.api().await;
    let dir = project(&site);
    let erp_path = write(&dir.join("stackly.yaml"), &erp());
    let graph_path = write(&dir.join("graph.yaml"), &graph());
    let before_counts = counts(&daemon);
    let before_tree = tree(site.root.path());

    let planned = plan(&api, &erp_path).await.expect("plans");
    assert!(planned.validation.valid);
    let aliases: Vec<_> = planned
        .items
        .iter()
        .map(|item| item.alias.as_str())
        .collect();
    assert_eq!(
        aliases,
        [
            "postgres",
            "mysql",
            "nats",
            "clickhouse",
            "valkey",
            "traefik"
        ]
    );
    let steps = |step: PlanStep| {
        planned
            .items
            .iter()
            .filter(|item| item.steps.contains(&step))
            .count()
    };
    assert_eq!(steps(PlanStep::InstallRuntime), 6);
    assert_eq!(steps(PlanStep::CreateInstance), 6);
    assert_eq!(steps(PlanStep::StartInstance), 6);
    assert!(
        planned
            .items
            .iter()
            .all(|item| item.action == PlanAction::Create)
    );
    assert!(planned.items.iter().all(|item| item.trust.is_some()));
    assert_eq!(
        planned.registration,
        Some(stackly_protocol::env::PlanRegistration::New)
    );
    // A port of this Mac may be taken by something else; then that is the
    // only kind of blocker and there is no digest.
    if planned.applicable {
        assert_eq!(planned.plan_digest.as_ref().map(String::len), Some(64));
    } else {
        assert!(planned.plan_digest.is_none());
        for blocker in &planned.blockers {
            assert_eq!(
                blocker.error.code(),
                "supervisor.endpoint_in_use",
                "{blocker:?}"
            );
        }
    }

    let graph_plan = plan(&api, &graph_path).await.expect("plans");
    assert_eq!(graph_plan.order, ["db", "cache", "api", "web"]);
    let web = graph_plan
        .items
        .iter()
        .find(|item| item.alias == "web")
        .expect("web is planned");
    assert_eq!(web.waits_for, ["api"]);

    assert_eq!(counts(&daemon), before_counts, "a plan writes nothing");
    assert_eq!(
        tree(site.root.path()),
        before_tree,
        "a plan changes no file"
    );
    let registry = daemon.core().registry().expect("open");
    assert_eq!(
        registry
            .instances(stackly_domain::InstanceFilter::default())
            .expect("reads")
            .value
            .len(),
        0,
        "a plan creates no instance"
    );
    drop(api);
    stop(daemon).await;
    ev.pass();
}

// covers: AC-6 (spec 0044, *Anggaran frame*)
#[tokio::test(flavor = "multi_thread")]
async fn the_reply_of_a_plan_of_32_services_stays_under_900_kib() {
    let site = Site::new();
    let daemon = start(&site).await;
    let api = site.api().await;
    let aliases: Vec<String> = (0..32).map(|n| format!("svc{n}")).collect();
    let refs: Vec<&str> = aliases.iter().map(String::as_str).collect();
    let path = write(
        &project(&site).join("wide.yaml"),
        &fake_manifest("wide", &refs),
    );

    let reply = plan_reply(&api, EnvSource::File(path.display().to_string()))
        .await
        .expect("plans");
    let Reply::EnvPlan(planned) = &reply else {
        panic!("not a plan");
    };
    assert_eq!(planned.items.len(), 32);
    let length = serde_json::to_vec(&reply).expect("encodes").len();
    assert!(length < 900 * 1024, "the plan frame is {length} bytes");
    drop(api);
    stop(daemon).await;
}

// covers: AC-3 (spec 0044, through the daemon)
#[tokio::test(flavor = "multi_thread")]
async fn a_secret_sentinel_in_the_manifest_never_leaves_the_file() {
    const SENTINEL: &str = "SENTINEL-envfile-7f3a9c";
    let logs = logs();
    let site = Site::new();
    let daemon = start(&site).await;
    let api = site.api().await;
    let path = write(
        &project(&site).join("stackly.yaml"),
        &format!(
            "apiVersion: stackly/v1alpha1\nkind: Environment\nmetadata:\n  name: leaky\nservices:\n  one:\n    provider: testing/fake\n    version: \"0.1\"\n    secretRefs:\n      authToken: \"{SENTINEL}\"\n  two:\n    provider: testing/fake\n    version: \"0.1\"\n    settings:\n      authToken: \"{SENTINEL}\"\n      label: {{nested: \"{SENTINEL}\"}}\n    endpoints:\n      http: {{port: \"{SENTINEL}\"}}\n  three:\n    provider: testing/fake\n    version: \"0.1\"\n    secretRefs:\n      authToken: \"local:instance/{SENTINEL}\"\n"
        ),
    );

    let validation = validate(&api, &path).await.expect("validates");
    assert!(!validation.valid);
    let codes: Vec<_> = validation
        .findings
        .iter()
        .map(|f| f.code.as_str())
        .collect();
    assert!(codes.contains(&"secret_literal"), "{codes:?}");
    assert!(codes.contains(&"secret_in_settings"), "{codes:?}");
    let planned = plan(&api, &path).await.expect("plans");
    assert!(!planned.applicable);
    let digest = validation.manifest_sha256.clone().expect("a digest");
    let refused = import_with(&api, &path, &digest, None, &key())
        .await
        .expect_err("an invalid manifest is not imported");
    assert_eq!(refused.code(), "env.manifest_invalid");

    for (what, text) in [
        (
            "validate",
            serde_json::to_string(&validation).expect("encodes"),
        ),
        ("plan", serde_json::to_string(&planned).expect("encodes")),
        ("import", serde_json::to_string(&*refused).expect("encodes")),
    ] {
        assert!(
            !text.contains(SENTINEL),
            "the {what} reply holds the sentinel"
        );
    }
    stop(daemon).await;
    drop(api);
    let logged =
        String::from_utf8_lossy(&logs.lock().unwrap_or_else(PoisonError::into_inner)).into_owned();
    assert!(!logged.contains(SENTINEL), "the daemon logged the sentinel");
    let mut pending = vec![site.root.path().to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(&dir).expect("lists").flatten() {
            let file = entry.path();
            if file.starts_with(site.root.path().join("project")) {
                continue;
            }
            if entry.file_type().expect("a type").is_dir() {
                pending.push(file);
            } else if let Ok(bytes) = std::fs::read(&file) {
                assert!(
                    !String::from_utf8_lossy(&bytes).contains(SENTINEL),
                    "{file:?} holds the sentinel"
                );
            }
        }
    }
}

// covers: AC-11, AC-12 (spec 0044)
#[tokio::test(flavor = "multi_thread")]
async fn an_import_registers_the_file_once_and_a_repeated_key_runs_no_second_operation() {
    let ev = evidence!(
        covers = ["REQ-020"],
        kind = Kind::Fixture,
        expected = "env.import registers a valid stackly.yaml as one import_environment operation with an audit row, the same file again keeps the revision, the same key runs no second operation, a changed digest is env.manifest_changed, an invalid file is env.manifest_invalid, and a path through a symlinked folder is the same location",
    );
    let site = Site::new();
    let daemon = start(&site).await;
    let api = site.api().await;
    let dir = project(&site);
    let path = write(&dir.join("stackly.yaml"), &fake_manifest("shop", &["db"]));
    let digest = validate(&api, &path)
        .await
        .expect("validates")
        .manifest_sha256
        .expect("a digest");
    let (_, operations_before, audit_before) = counts(&daemon);

    let first_key = key();
    let imported = import_with(&api, &path, &digest, None, &first_key)
        .await
        .expect("imports");
    assert_eq!(imported.summary.name, "shop");
    assert_eq!(u64::from(imported.summary.revision), 1);
    assert_eq!(u64::from(imported.summary.service_count), 1);
    assert_eq!(
        imported.summary.manifest_location.as_deref(),
        Some(
            std::fs::canonicalize(&path)
                .expect("canonical")
                .display()
                .to_string()
                .as_str()
        )
    );
    assert_eq!(
        imported.summary.manifest_sha256.as_deref(),
        Some(digest.as_str())
    );
    assert!(imported.manifest_state.is_none());
    let (_, operations, audit) = counts(&daemon);
    assert_eq!(operations, operations_before + 1);
    assert_eq!(audit, audit_before + 1, "the import is audited");

    // The same key: the first answer, no second operation.
    let again = import_with(&api, &path, &digest, None, &first_key)
        .await
        .expect("replays");
    assert_eq!(again.summary.id, imported.summary.id);
    assert_eq!(counts(&daemon).1, operations);

    // The same file through a symlinked folder, with a new key: one more
    // operation, the row unchanged.
    let linked = site.root.path().join("linked");
    symlink(&dir, &linked).expect("the folder link can be made");
    let same = import_with(&api, &linked.join("stackly.yaml"), &digest, None, &key())
        .await
        .expect("imports again");
    assert_eq!(same.summary.id, imported.summary.id);
    assert_eq!(
        u64::from(same.summary.revision),
        1,
        "an unchanged import keeps the revision"
    );
    assert_eq!(
        counts(&daemon).1,
        operations + 1,
        "it is still an operation"
    );

    // A changed file: the old digest is refused, the new one updates.
    write(&path, &fake_manifest("shop", &["db", "cache"]));
    let changed = import_with(&api, &path, &digest, None, &key())
        .await
        .expect_err("the digest changed");
    assert_eq!(changed.code(), "env.manifest_changed");
    let updated = import(&api, &path).await.expect("imports the new bytes");
    assert_eq!(u64::from(updated.summary.revision), 2);
    assert_eq!(u64::from(updated.summary.service_count), 2);

    // An invalid file.
    let broken = write(
        &dir.join("broken.yaml"),
        "apiVersion: stackly/v1alpha1\nkind: Nope\n",
    );
    let invalid = import(&api, &broken).await.expect_err("invalid");
    assert_eq!(invalid.code(), "env.manifest_invalid");
    assert!(detail(&invalid, "errors").is_some());
    assert!(invalid.hint().is_some());

    let rows = daemon
        .core()
        .registry()
        .expect("open")
        .environments()
        .expect("reads")
        .value;
    assert_eq!(rows.len(), 1);
    drop(api);
    stop(daemon).await;
    ev.pass();
}

// covers: AC-11 (spec 0044)
#[tokio::test(flavor = "multi_thread")]
async fn a_name_at_another_location_is_taken_until_a_relink_moves_it() {
    let site = Site::new();
    let daemon = start(&site).await;
    let api = site.api().await;
    let dir = project(&site);
    let first = write(&dir.join("a/stackly.yaml"), &fake_manifest("shop", &["db"]));
    let second = write(&dir.join("b/stackly.yaml"), &fake_manifest("shop", &["db"]));
    let imported = import(&api, &first).await.expect("imports");

    let planned = plan(&api, &second).await.expect("plans");
    assert_eq!(
        planned.registration,
        Some(stackly_protocol::env::PlanRegistration::NameTaken)
    );
    assert!(
        planned
            .blockers
            .iter()
            .any(|b| b.error.code() == "env.name_taken")
    );
    let taken = import(&api, &second).await.expect_err("the name is taken");
    assert_eq!(taken.code(), "env.name_taken");
    assert_eq!(detail(&taken, "name").as_deref(), Some("shop"));
    assert!(detail(&taken, "location").is_some());

    // A relink to a file with another name is refused.
    let other = write(
        &dir.join("c/stackly.yaml"),
        &fake_manifest("other", &["db"]),
    );
    let digest = validate(&api, &other)
        .await
        .expect("ok")
        .manifest_sha256
        .expect("d");
    let mismatch = import_with(&api, &other, &digest, Some(imported.summary.id), &key())
        .await
        .expect_err("another name");
    assert_eq!(mismatch.code(), "env.relink_mismatch");
    assert_eq!(detail(&mismatch, "declared").as_deref(), Some("other"));

    // The relink moves the location and keeps the ID.
    let digest = validate(&api, &second)
        .await
        .expect("ok")
        .manifest_sha256
        .expect("d");
    let moved = import_with(&api, &second, &digest, Some(imported.summary.id), &key())
        .await
        .expect("relinks");
    assert_eq!(moved.summary.id, imported.summary.id);
    assert!(
        moved
            .summary
            .manifest_location
            .as_deref()
            .is_some_and(|location| location.ends_with("b/stackly.yaml"))
    );

    // A file that now declares another name takes its location from the
    // environment that held it, which then shows `missing`.
    write(&second, &fake_manifest("renamed", &["db"]));
    let renamed = import(&api, &second).await.expect("imports the new name");
    assert_eq!(renamed.summary.name, "renamed");
    let old = get(&api, imported.summary.id, true)
        .await
        .expect("still there");
    assert!(old.summary.manifest_location.is_none());
    assert_eq!(old.manifest_state, Some(ManifestState::Missing));
    drop(api);
    stop(daemon).await;
}

// covers: AC-12 (spec 0044, env.limit_reached)
#[tokio::test(flavor = "multi_thread")]
async fn the_65th_environment_is_refused() {
    let site = Site::new();
    let daemon = start(&site).await;
    let api = site.api().await;
    let dir = project(&site);
    for n in 0..64 {
        let path = write(
            &dir.join(format!("e{n}.yaml")),
            &fake_manifest(&format!("env{n}"), &["db"]),
        );
        import(&api, &path).await.expect("imports");
    }
    let path = write(
        &dir.join("last.yaml"),
        &fake_manifest("one-too-many", &["db"]),
    );
    let refused = import(&api, &path).await.expect_err("the limit is reached");
    assert_eq!(refused.code(), "env.limit_reached");
    assert_eq!(detail(&refused, "limit").as_deref(), Some("64"));
    assert_eq!(list(&api).await.items.len(), 64);
    drop(api);
    stop(daemon).await;
}

/// Makes the instance `instance` the dedicated member `alias` of the
/// environment `environment`, with an active environment lease, written
/// straight to the registry as spec 0046 will.
fn join(daemon: &DaemonHandle, environment: Uuid, alias: &str, instance: Uuid) -> LeaseId {
    let registry = daemon.core().registry().expect("open");
    let stored = registry
        .environments()
        .expect("reads")
        .value
        .into_iter()
        .find(|env| env.id().as_uuid() == environment)
        .expect("the environment exists");
    let instance = InstanceId::from_uuid(instance).expect("a v7 id");
    let joined = Environment::restore(StoredEnvironment {
        id: stored.id(),
        name: stored.name().clone(),
        manifest_location: stored.manifest_location().cloned(),
        manifest_sha256: stored.manifest_sha256().copied(),
        declared: stored.declared().to_vec(),
        members: vec![EnvironmentMember::new(
            alias.parse().expect("an alias"),
            instance,
            Ownership::Dedicated,
        )],
        last_operation: None,
        last_up: None,
        updated_at: None,
        revision: stored.revision().next(),
    })
    .expect("a valid environment");
    let lease_id = LeaseId::generate();
    let lease = Lease::new(
        lease_id,
        instance,
        LeaseHolder::Environment(stored.id()),
        None,
        SystemClock.now(),
    )
    .expect("an environment lease");
    registry
        .commit(WriteBatch::new(vec![
            WriteOp::UpdateEnvironment {
                environment: joined,
                expected: stored.revision(),
            },
            WriteOp::InsertLease(lease),
        ]))
        .expect("the member commits");
    lease_id
}

fn release(daemon: &DaemonHandle, instance: Uuid, lease: LeaseId) {
    let registry = daemon.core().registry().expect("open");
    let instance = InstanceId::from_uuid(instance).expect("a v7 id");
    let mut found = registry
        .leases(instance, stackly_domain::LeaseFilter::All)
        .expect("reads")
        .value
        .into_iter()
        .find(|row| row.id() == lease)
        .expect("the lease exists");
    found.release(SystemClock.now()).expect("releases");
    registry
        .commit(WriteBatch::new(vec![WriteOp::UpdateLease(found)]))
        .expect("the release commits");
}

// covers: AC-13, AC-14, AC-15 (spec 0044)
#[tokio::test(flavor = "multi_thread")]
async fn membership_shows_on_every_summary_and_remove_waits_for_the_lease() {
    let ev = evidence!(
        covers = ["REQ-020"],
        kind = Kind::Fixture,
        expected = "a dedicated member shows ownership dedicated and its environment in instance.get, instance.list, and instance.state_changed alike; env.list and env.get count it; env.remove refuses an active lease with env.lease_active naming the instance, then removes the row and leaves the instance manual",
    );
    let site = Site::new();
    // The instance is created from the seeded pin, by a daemon whose
    // catalog has no fake entry, so create does not pin again.
    let mut empty = fast();
    empty.catalog = Some(stacklyd::catalog::CatalogOverride::Preloaded(
        Catalog::for_tests(&[]),
    ));
    let first = site.start_with(empty).await;
    let instance = site
        .api()
        .await
        .create_ready("shop-db", serde_json::Value::Null)
        .await;
    stop(first).await;
    let daemon = start(&site).await;
    let api = site.api().await;
    let path = write(
        &project(&site).join("stackly.yaml"),
        &fake_manifest("shop", &["db"]),
    );
    let imported = import(&api, &path).await.expect("imports");
    assert_eq!(
        imported.services[0].not_ready.as_ref().map(|n| n.reason),
        Some(stackly_protocol::env::NotReadyReason::NotCreated)
    );
    let lease = join(&daemon, imported.summary.id, "db", instance);
    // The cache loads again after every import, also an unchanged one.
    import(&api, &path).await.expect("imports again");

    let shown = api.get(instance).await.expect("gets");
    assert_eq!(shown.ownership, InstanceOwnership::Dedicated);
    assert_eq!(shown.environments.len(), 1);
    assert_eq!(shown.environments[0].name, "shop");
    assert_eq!(shown.environments[0].alias, "db");
    assert_eq!(shown.environments[0].ownership, EnvOwnership::Dedicated);
    let listed = api.list(ListInstancesParams::default()).await;
    let row = listed
        .items
        .iter()
        .find(|item| item.id == instance)
        .expect("listed");
    assert_eq!(row.ownership, shown.ownership);
    assert_eq!(row.environments, shown.environments);

    let (_, mut events) = api
        .client
        .subscribe(SubscribeParams::default())
        .await
        .expect("subscribes");
    api.start_ready(instance).await;
    let mut seen = None;
    while let Ok(Ok(Some(frame))) =
        tokio::time::timeout(Duration::from_secs(5), events.next()).await
    {
        if let DecodedFrame::Event(event) = frame
            && let EventKind::InstanceStateChanged(changed) = event.kind
            && changed.instance.id == instance
        {
            seen = Some(changed.instance);
            break;
        }
    }
    let seen = seen.expect("a state change arrives");
    assert_eq!(seen.ownership, shown.ownership);
    assert_eq!(seen.environments, shown.environments);

    let summary = list(&api)
        .await
        .items
        .into_iter()
        .find(|item| item.id == imported.summary.id)
        .expect("listed");
    assert_eq!(u64::from(summary.service_count), 1);
    let detail_now = get(&api, imported.summary.id, false).await.expect("gets");
    assert_eq!(
        detail_now.services[0].instance.as_ref().map(|i| i.id),
        Some(instance)
    );

    let stale = remove(&api, imported.summary.id, Revision::saturating(1), &key())
        .await
        .expect_err("an old revision");
    assert_eq!(stale.code(), "resource.revision_mismatch");
    let leased = remove(
        &api,
        imported.summary.id,
        detail_now.summary.revision,
        &key(),
    )
    .await
    .expect_err("the lease is active");
    assert_eq!(leased.code(), "env.lease_active");
    assert_eq!(detail(&leased, "count").as_deref(), Some("1"));
    let item = leased.items().first().expect("one item");
    assert!(
        item.fields()
            .iter()
            .any(|field| field.value() == instance.to_string())
    );
    assert!(item.fields().iter().any(|field| field.value() == "shop-db"));

    release(&daemon, instance, lease);
    let removed_key = key();
    let removed = remove(
        &api,
        imported.summary.id,
        detail_now.summary.revision,
        &removed_key,
    )
    .await
    .expect("removes");
    assert_eq!(removed.name, "shop");
    let replayed = remove(
        &api,
        imported.summary.id,
        detail_now.summary.revision,
        &removed_key,
    )
    .await
    .expect("the same key answers again");
    assert_eq!(replayed, removed);
    let gone = get(&api, imported.summary.id, false)
        .await
        .expect_err("gone");
    assert_eq!(gone.code(), "env.not_found");
    let kept = api.get(instance).await.expect("the instance stays");
    assert_eq!(kept.ownership, InstanceOwnership::Manual);
    assert!(kept.environments.is_empty());
    assert!(path.exists(), "the manifest stays");

    api.stop(instance).await.expect("stops");
    drop(api);
    stop(daemon).await;
    ev.pass();
}

// covers: AC-14 (spec 0044, *State manifest*)
#[tokio::test(flavor = "multi_thread")]
async fn get_reads_the_manifest_only_when_asked() {
    let site = Site::new();
    let daemon = start(&site).await;
    let api = site.api().await;
    let path = write(
        &project(&site).join("stackly.yaml"),
        &fake_manifest("shop", &["db"]),
    );
    let imported = import(&api, &path).await.expect("imports");
    let id = imported.summary.id;

    let synced = get(&api, id, true).await.expect("gets");
    assert_eq!(synced.manifest_state, Some(ManifestState::InSync));
    assert_eq!(synced.problems.map(u64::from), Some(0));
    assert_eq!(
        synced.summary.status,
        stackly_protocol::env::EnvironmentStatus::Down
    );

    write(&path, &fake_manifest("shop", &["db", "cache"]));
    assert_eq!(
        get(&api, id, true).await.expect("gets").manifest_state,
        Some(ManifestState::Changed)
    );
    write(&path, &fake_manifest("elsewhere", &["db"]));
    assert_eq!(
        get(&api, id, true).await.expect("gets").manifest_state,
        Some(ManifestState::NameChanged)
    );
    write(&path, "apiVersion: stackly/v1alpha1\nkind: Nope\n");
    let invalid = get(&api, id, true).await.expect("gets");
    assert_eq!(invalid.manifest_state, Some(ManifestState::Invalid));
    assert!(invalid.problems.map(u64::from).is_some_and(|n| n > 0));
    std::fs::remove_file(&path).expect("removes");
    let unchecked = get(&api, id, false).await.expect("gets");
    assert!(unchecked.manifest_state.is_none() && unchecked.problems.is_none());
    assert_eq!(
        get(&api, id, true).await.expect("gets").manifest_state,
        Some(ManifestState::Missing)
    );
    let registered = plan_reply(&api, EnvSource::Registered(id))
        .await
        .expect_err("the file is gone");
    assert_eq!(registered.code(), "env.manifest_unreadable");

    let unknown = get(&api, Uuid::now_v7(), false).await.expect_err("unknown");
    assert_eq!(unknown.code(), "env.not_found");
    assert_eq!(
        list(&api).await.items.len(),
        1,
        "the list never reads the file"
    );
    drop(api);
    stop(daemon).await;
}

// covers: AC-14 (spec 0044)
#[tokio::test(flavor = "multi_thread")]
async fn updated_at_is_the_row_stamp_so_an_unchanged_import_never_moves_it() {
    let site = Site::new();
    let daemon = start(&site).await;
    let api = site.api().await;
    let dir = project(&site);
    let path = write(&dir.join("stackly.yaml"), &fake_manifest("shop", &["db"]));
    let imported = import(&api, &path).await.expect("imports");
    let first = imported.summary.updated_at;
    let stored = daemon
        .core()
        .registry()
        .expect("open")
        .environment(
            stackly_domain::EnvironmentId::from_uuid(imported.summary.id).expect("a v7 ID"),
        )
        .expect("reads")
        .value
        .expect("the row")
        .updated_at()
        .expect("a stored row has a stamp");
    assert_eq!(
        first,
        stackly_protocol::wire::Timestamp::from_jiff(stored),
        "the summary carries the row's own stamp"
    );

    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    let same = import(&api, &path).await.expect("imports again");
    assert_eq!(same.summary.updated_at, first, "an unchanged import");
    assert_eq!(list(&api).await.items[0].updated_at, first);
    assert_eq!(
        get(&api, imported.summary.id, false)
            .await
            .expect("reads")
            .summary
            .updated_at,
        first
    );

    write(&path, &fake_manifest("shop", &["db", "cache"]));
    let changed = import(&api, &path).await.expect("imports the change");
    assert_ne!(changed.summary.updated_at, first, "a changed row moves it");
    drop(api);
    stop(daemon).await;
}
