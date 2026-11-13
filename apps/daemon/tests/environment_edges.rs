//! The environment methods at their edges, through the real daemon (spec
//! 0044): unreadable reasons and the 256 KiB bound, a local provider that
//! waits for approval or was revoked, a file name in another case, relinks
//! at the same time, the membership cache after a restart, a digest that
//! ignores a borrowed instance starting and stopping, and a secret sentinel
//! on the audit and operation surfaces.
#![cfg(target_os = "macos")]

mod common;
#[path = "supervisor/support.rs"]
mod support;

use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use stackly_core::catalog::{
    Catalog, CatalogEntry, EntryOrigin, ProviderEntry, TileColor, TileHint,
};
use stackly_core::provider::{NameCheck, ValidatedPackage, read_package};
use stackly_domain::{
    Environment, EnvironmentMember, InstanceId, Ownership, Registry as _, StoredEnvironment,
    WriteBatch, WriteOp,
};
use stackly_ipc::{CallOptions, Client};
use stackly_protocol::audit::ListAuditParams;
use stackly_protocol::catalog::ProviderDetail;
use stackly_protocol::env::{
    EnvGetParams, EnvOwnership, EnvPlan, EnvPlanParams, EnvSource, EnvValidateParams,
    EnvValidation, EnvironmentDetail, ImportEnvironmentParams, ManifestState, PlanAction,
    PlanRegistration, PlanTrust,
};
use stackly_protocol::instance::{InstanceOwnership, ListInstancesParams};
use stackly_protocol::operation::{ListOperationsParams, OperationState};
use stackly_protocol::provider::{
    ApprovalState, DecideProviderParams, ImportProviderParams, PreviewImportParams,
    ProviderTargetParams,
};
use stackly_protocol::{Call, ErrorBody, Reply};
use stackly_testkit::evidence::{Kind, evidence};
use stacklyd::server::DaemonHandle;
use uuid::Uuid;

use crate::support::{Api, Site, fast, key};

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

/// The fake provider and two official ones, as the daemon's catalog.
fn catalog() -> Catalog {
    let entries: Vec<_> = ["fake", "valkey", "nats"]
        .iter()
        .map(|name| entry(name))
        .collect();
    Catalog::for_tests(&entries)
}

async fn start(site: &Site) -> DaemonHandle {
    let mut config = fast();
    config.catalog = Some(stacklyd::catalog::CatalogOverride::Preloaded(catalog()));
    site.start_with(config).await
}

/// A daemon whose catalog has no fake entry, so a create uses the seeded
/// pin and does not pin again.
async fn start_empty(site: &Site) -> DaemonHandle {
    let mut config = fast();
    config.catalog = Some(stacklyd::catalog::CatalogOverride::Preloaded(
        Catalog::for_tests(&[]),
    ));
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

fn header(name: &str) -> String {
    format!(
        "apiVersion: stackly/v1alpha1\nkind: Environment\nmetadata:\n  name: {name}\nservices:\n"
    )
}

/// A manifest named `name` with one fake service per alias.
fn fake_manifest(name: &str, aliases: &[&str]) -> String {
    let mut text = header(name);
    for alias in aliases {
        text.push_str(&format!(
            "  {alias}:\n    provider: testing/fake\n    version: \"0.1\"\n"
        ));
    }
    text
}

async fn validate(api: &Api, path: &Path) -> Result<EnvValidation, Box<ErrorBody>> {
    let params = EnvValidateParams {
        path: path.display().to_string(),
    };
    match api.client.call(Call::EnvValidate(params)).await? {
        Reply::EnvValidate(validation) => Ok(*validation),
        other => panic!("not a validation: {other:?}"),
    }
}

async fn plan_of(api: &Api, source: EnvSource) -> Result<EnvPlan, Box<ErrorBody>> {
    match api
        .client
        .call(Call::EnvPlan(EnvPlanParams { source }))
        .await?
    {
        Reply::EnvPlan(plan) => Ok(*plan),
        other => panic!("not a plan: {other:?}"),
    }
}

async fn plan(api: &Api, path: &Path) -> Result<EnvPlan, Box<ErrorBody>> {
    plan_of(api, EnvSource::File(path.display().to_string())).await
}

async fn import_with(
    client: &Client,
    path: &Path,
    digest: &str,
    relink: Option<Uuid>,
) -> Result<EnvironmentDetail, Box<ErrorBody>> {
    let params = ImportEnvironmentParams {
        path: path.display().to_string(),
        expected_manifest_sha256: digest.to_owned(),
        relink_environment_id: relink,
    };
    match client
        .call_detailed(
            Call::EnvImport(params),
            CallOptions::default().with_idempotency_key(key()),
        )
        .await?
    {
        Reply::EnvImport(detail) => Ok(*detail),
        other => panic!("not an import reply: {other:?}"),
    }
}

async fn digest(api: &Api, path: &Path) -> String {
    validate(api, path)
        .await
        .expect("validates")
        .manifest_sha256
        .expect("a readable file has a digest")
}

async fn import(api: &Api, path: &Path) -> Result<EnvironmentDetail, Box<ErrorBody>> {
    let digest = digest(api, path).await;
    import_with(&api.client, path, &digest, None).await
}

async fn get(
    api: &Api,
    id: Uuid,
    check_manifest: bool,
) -> Result<EnvironmentDetail, Box<ErrorBody>> {
    match api
        .client
        .call(Call::EnvGet(EnvGetParams { id, check_manifest }))
        .await?
    {
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

fn blocker_codes(plan: &EnvPlan) -> Vec<String> {
    plan.blockers
        .iter()
        .map(|b| b.error.code().to_owned())
        .collect()
}

/// The registry generation and the operation count.
fn counts(daemon: &DaemonHandle) -> (stackly_domain::Generation, usize) {
    let registry = daemon.core().registry().expect("the registry is open");
    let generation = registry.known_instance_ids().expect("reads").generation;
    let operations = registry
        .operations(stackly_domain::OperationQuery {
            active_only: false,
            resource: None,
            limit: 500,
            before: None,
        })
        .expect("reads")
        .value
        .items
        .len();
    (generation, operations)
}

// covers: AC-4 (spec 0044, Kode error di wire): a file above 256 KiB is
// `too_large`, one exactly 256 KiB reads, and a file macOS refuses is
// `permission_denied`, for validate and plan alike
#[tokio::test(flavor = "multi_thread")]
async fn a_file_over_256_kib_is_too_large_one_at_256_kib_reads_and_a_closed_file_is_denied() {
    const LIMIT: usize = 256 * 1024;
    let site = Site::new();
    let daemon = start(&site).await;
    let api = site.api().await;
    let dir = project(&site);
    let mut text = fake_manifest("big", &["api"]);
    let line = format!("# {}\n", "x".repeat(60));
    while text.len() + line.len() <= LIMIT {
        text.push_str(&line);
    }
    text.push_str(&"#".repeat(LIMIT - text.len() - 1));
    text.push('\n');
    assert_eq!(text.len(), LIMIT);
    let exact = write(&dir.join("exact.yaml"), &text);
    let over = write(&dir.join("over.yaml"), &format!("{text}#"));
    let closed = write(&dir.join("closed.yaml"), &fake_manifest("closed", &["api"]));
    std::fs::set_permissions(&closed, std::fs::Permissions::from_mode(0o000))
        .expect("the mode changes");

    let read = validate(&api, &exact).await.expect("256 KiB reads");
    assert!(read.valid, "{:?}", read.findings);
    assert!(plan(&api, &exact).await.expect("plans").applicable);
    for (path, reason) in [(&over, "too_large"), (&closed, "permission_denied")] {
        let refused = validate(&api, path).await.expect_err("refused");
        assert_eq!(refused.code(), "env.manifest_unreadable", "{path:?}");
        assert_eq!(
            detail(&refused, "reason").as_deref(),
            Some(reason),
            "{path:?}"
        );
        assert!(refused.hint().is_some());
        let planned = plan(&api, path).await.expect_err("refused");
        assert_eq!(planned.code(), "env.manifest_unreadable", "{path:?}");
        assert_eq!(
            detail(&planned, "reason").as_deref(),
            Some(reason),
            "{path:?}"
        );
        let digest = "0".repeat(64);
        let imported = import_with(&api.client, path, &digest, None)
            .await
            .expect_err("refused");
        assert_eq!(imported.code(), "env.manifest_unreadable", "{path:?}");
    }

    std::fs::set_permissions(&closed, std::fs::Permissions::from_mode(0o644))
        .expect("the mode changes back");
    drop(api);
    stop(daemon).await;
}

fn fixture_package(name: &str) -> PathBuf {
    repo()
        .join("crates/core/tests/fixtures/packages")
        .join(name)
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("the copy target can be created");
    for entry in std::fs::read_dir(from).expect("the folder can be listed") {
        let entry = entry.expect("the entry can be read");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("the entry has a type").is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).expect("the file can be copied");
        }
    }
}

async fn call(client: &Client, call: Call) -> Result<Reply, Box<ErrorBody>> {
    client
        .call_detailed(call, CallOptions::default().with_idempotency_key(key()))
        .await
}

async fn provider(client: &Client, name: &str) -> ProviderDetail {
    match client
        .call(Call::CatalogGetProvider(
            stackly_protocol::catalog::GetProviderParams {
                provider: name.to_owned(),
            },
        ))
        .await
        .expect("the provider reads")
    {
        Reply::CatalogGetProvider(detail) => *detail,
        other => panic!("not a provider: {other:?}"),
    }
}

// covers: AC-7 (spec 0044, spec 0037): through the socket, a local
// provider that waits for approval plans with `trust` `approval_required`
// and one blocker, a revoked one with `revoked` and one blocker, and no
// plan ever changes the approval or starts an operation
#[tokio::test(flavor = "multi_thread")]
async fn a_pending_or_revoked_local_provider_blocks_the_plan_and_the_plan_never_decides() {
    let ev = evidence!(
        covers = ["VAL-013"],
        kind = Kind::Fixture,
        expected = "env.plan of a manifest that uses an imported local provider shows trust approval_required with the one blocker provider.approval_required while it waits, trust revoked with the one blocker provider.revoked after a revoke, and never changes the approval or starts an operation",
    );
    let site = Site::new();
    let daemon = start(&site).await;
    let api = site.api().await;
    let package = site.root.path().join("packages/acme-demo");
    copy_dir(&fixture_package("acme-demo"), &package);
    let preview = match api
        .client
        .call(Call::ProviderPreviewImport(PreviewImportParams {
            path: package.display().to_string(),
        }))
        .await
        .expect("previews")
    {
        Reply::ProviderPreviewImport(preview) => *preview,
        other => panic!("not a preview: {other:?}"),
    };
    let package_digest = preview.package_sha256.expect("a digest");
    call(
        &api.client,
        Call::ProviderImport(ImportProviderParams {
            path: package.display().to_string(),
            expected_package_sha256: package_digest.clone(),
        }),
    )
    .await
    .expect("imports");
    let path = write(
        &project(&site).join("stackly.yaml"),
        &format!(
            "{}  demo:\n    provider: acme/demo\n    version: \"1.0\"\n",
            header("acme")
        ),
    );

    let before = counts(&daemon);
    let pending = plan(&api, &path).await.expect("plans");
    assert_eq!(blocker_codes(&pending), ["provider.approval_required"]);
    assert_eq!(pending.items[0].trust, Some(PlanTrust::ApprovalRequired));
    assert_eq!(pending.items[0].action, PlanAction::Create);
    assert!(!pending.applicable && pending.plan_digest.is_none());
    assert_eq!(counts(&daemon), before, "the plan writes nothing");
    assert_eq!(
        provider(&api.client, "acme/demo").await.summary.approval,
        Some(ApprovalState::Pending)
    );

    call(
        &api.client,
        Call::ProviderApprove(DecideProviderParams {
            provider: "acme/demo".to_owned(),
            expected_package_sha256: package_digest.clone(),
        }),
    )
    .await
    .expect("approves");
    let approved = plan(&api, &path).await.expect("plans");
    assert_eq!(approved.items[0].trust, Some(PlanTrust::Trusted));
    assert!(
        !blocker_codes(&approved)
            .iter()
            .any(|code| code.starts_with("provider.")),
        "{:?}",
        blocker_codes(&approved)
    );

    call(
        &api.client,
        Call::ProviderRevoke(ProviderTargetParams {
            provider: "acme/demo".to_owned(),
        }),
    )
    .await
    .expect("revokes");
    let before = counts(&daemon);
    let revoked = plan(&api, &path).await.expect("plans");
    assert_eq!(blocker_codes(&revoked), ["provider.revoked"]);
    assert_eq!(revoked.items[0].trust, Some(PlanTrust::Revoked));
    assert!(!revoked.applicable);
    assert_eq!(counts(&daemon), before, "the plan writes nothing");
    assert_eq!(
        provider(&api.client, "acme/demo").await.summary.approval,
        Some(ApprovalState::Revoked)
    );

    drop(api);
    stop(daemon).await;
    ev.pass();
}

// covers: AC-11 (spec 0044): the same file named in another case is the
// same location on APFS, so it is `registered`, never `env.name_taken`
#[tokio::test(flavor = "multi_thread")]
async fn a_file_name_in_another_case_is_the_registered_location() {
    let site = Site::new();
    let daemon = start(&site).await;
    let api = site.api().await;
    let dir = project(&site);
    let path = write(&dir.join("stackly.yaml"), &fake_manifest("shop", &["api"]));
    let imported = import(&api, &path).await.expect("imports");
    let upper = dir.join("Stackly.yaml");
    if !upper.exists() {
        // A case sensitive volume holds two names; nothing to prove here.
        drop(api);
        stop(daemon).await;
        return;
    }

    let planned = plan(&api, &upper).await.expect("plans");
    assert_eq!(planned.registration, Some(PlanRegistration::Registered));
    assert!(
        !blocker_codes(&planned).contains(&"env.name_taken".to_owned()),
        "{:?}",
        blocker_codes(&planned)
    );
    let again = import(&api, &upper).await.expect("imports");
    assert_eq!(again.summary.id, imported.summary.id);
    assert_eq!(again.summary.revision, imported.summary.revision);
    assert_eq!(
        again.summary.manifest_location,
        imported.summary.manifest_location
    );

    drop(api);
    stop(daemon).await;
}

// covers: AC-11, AC-12 (spec 0044): relinks of one environment at the same
// time leave one row with the same id at one of their files, and a plain
// import of the name at a third file is `env.name_taken` whatever ran first
#[tokio::test(flavor = "multi_thread")]
async fn relinks_at_the_same_time_keep_one_environment_and_a_plain_import_is_name_taken() {
    let site = Site::new();
    let daemon = start(&site).await;
    let api = site.api().await;
    let dir = project(&site);
    let text = fake_manifest("shop", &["api"]);
    let first = write(&dir.join("a/stackly.yaml"), &text);
    let imported = import(&api, &first).await.expect("imports");
    let id = imported.summary.id;
    let b = write(&dir.join("b/stackly.yaml"), &text);
    let c = write(&dir.join("c/stackly.yaml"), &text);
    let d = write(&dir.join("d/stackly.yaml"), &text);
    let digest = digest(&api, &b).await;
    let (one, two, three) = (site.api().await, site.api().await, site.api().await);

    let (relink_b, relink_c, plain_d) = tokio::join!(
        import_with(&one.client, &b, &digest, Some(id)),
        import_with(&two.client, &c, &digest, Some(id)),
        import_with(&three.client, &d, &digest, None),
    );

    let relink_b = relink_b.expect("the relink to b succeeds");
    let relink_c = relink_c.expect("the relink to c succeeds");
    assert_eq!(relink_b.summary.id, id);
    assert_eq!(relink_c.summary.id, id);
    let refused = plain_d.expect_err("a plain import of a taken name is refused");
    assert_eq!(refused.code(), "env.name_taken");
    assert_eq!(detail(&refused, "name").as_deref(), Some("shop"));
    let listed = match api.client.call(Call::EnvList).await.expect("lists") {
        Reply::EnvList(list) => list,
        other => panic!("not a list: {other:?}"),
    };
    assert_eq!(listed.items.len(), 1);
    assert_eq!(listed.items[0].id, id);
    let location = listed.items[0]
        .manifest_location
        .clone()
        .expect("a location");
    assert!(
        location.ends_with("b/stackly.yaml") || location.ends_with("c/stackly.yaml"),
        "{location}"
    );
    let now = get(&api, id, true).await.expect("gets");
    assert_eq!(now.manifest_state, Some(ManifestState::InSync));

    drop((one, two, three, api));
    stop(daemon).await;
}

/// Makes `instance` the dedicated member `alias` of `environment`, written
/// straight to the registry as spec 0046 will.
fn join(daemon: &DaemonHandle, environment: Uuid, alias: &str, instance: Uuid) {
    let registry = daemon.core().registry().expect("open");
    let stored = registry
        .environments()
        .expect("reads")
        .value
        .into_iter()
        .find(|env| env.id().as_uuid() == environment)
        .expect("the environment exists");
    let joined = Environment::restore(StoredEnvironment {
        id: stored.id(),
        name: stored.name().clone(),
        manifest_location: stored.manifest_location().cloned(),
        manifest_sha256: stored.manifest_sha256().copied(),
        declared: stored.declared().to_vec(),
        members: vec![EnvironmentMember::new(
            alias.parse().expect("an alias"),
            InstanceId::from_uuid(instance).expect("a v7 id"),
            Ownership::Dedicated,
        )],
        last_operation: None,
        last_up: None,
        updated_at: None,
        revision: stored.revision().next(),
    })
    .expect("a valid environment");
    registry
        .commit(WriteBatch::new(vec![WriteOp::UpdateEnvironment {
            environment: joined,
            expected: stored.revision(),
        }]))
        .expect("the member commits");
}

// covers: AC-15 (spec 0044): the membership cache loads when the daemon
// starts, so a member written before a restart shows `dedicated` and its
// environment on `instance.get`, `instance.list`, and `env.get` after it
#[tokio::test(flavor = "multi_thread")]
async fn membership_loads_again_when_the_daemon_restarts() {
    let site = Site::new();
    let first = start_empty(&site).await;
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
    join(&daemon, imported.summary.id, "db", instance);
    drop(api);
    stop(daemon).await;

    let daemon = start(&site).await;
    let api = site.api().await;

    let shown = api.get(instance).await.expect("gets");
    assert_eq!(shown.ownership, InstanceOwnership::Dedicated);
    assert_eq!(shown.environments.len(), 1);
    assert_eq!(shown.environments[0].environment_id, imported.summary.id);
    assert_eq!(shown.environments[0].name, "shop");
    assert_eq!(shown.environments[0].alias, "db");
    assert_eq!(shown.environments[0].ownership, EnvOwnership::Dedicated);
    let listed = api.list(ListInstancesParams::default()).await;
    let row = listed
        .items
        .iter()
        .find(|item| item.id == instance)
        .expect("listed");
    assert_eq!(row.environments, shown.environments);
    let detail = get(&api, imported.summary.id, false).await.expect("gets");
    assert_eq!(
        detail.services[0].instance.as_ref().map(|i| i.id),
        Some(instance)
    );
    let started = api.start_ready(instance).await;
    assert_eq!(started.ownership, InstanceOwnership::Dedicated);
    assert_eq!(started.environments, shown.environments);

    drop(api);
    stop(daemon).await;
}

// covers: AC-5 (spec 0044, Aksi plan 5): through the socket, the digest of
// a plan that borrows an instance stays the same while that instance
// starts and stops and takes its `auto` port, though the action changes
#[tokio::test(flavor = "multi_thread")]
async fn the_digest_ignores_a_borrowed_instance_starting_and_stopping() {
    let site = Site::new();
    let first = start_empty(&site).await;
    let instance = site
        .api()
        .await
        .create_ready("shared-fake", serde_json::Value::Null)
        .await;
    stop(first).await;
    let daemon = start(&site).await;
    let api = site.api().await;
    let path = write(
        &project(&site).join("stackly.yaml"),
        &format!(
            "{}  cache:\n    provider: testing/fake\n    instanceRef: shared-fake\n",
            header("borrow")
        ),
    );

    let stopped = plan(&api, &path).await.expect("plans");
    assert!(stopped.applicable, "{:?}", blocker_codes(&stopped));
    assert_eq!(stopped.items[0].action, PlanAction::Start);
    let digest = stopped.plan_digest.clone().expect("applicable");

    api.start_ready(instance).await;
    let running = plan(&api, &path).await.expect("plans");
    assert_eq!(running.items[0].action, PlanAction::Keep);
    assert_eq!(running.plan_digest.as_ref(), Some(&digest));

    let accepted = api
        .stop(instance)
        .await
        .expect("the stop is accepted")
        .accepted
        .expect("a stop begins an operation");
    api.wait_state(accepted.operation.id, OperationState::Succeeded)
        .await;
    let again = plan(&api, &path).await.expect("plans");
    assert_eq!(again.items[0].action, PlanAction::Start);
    assert_eq!(again.plan_digest.as_ref(), Some(&digest));

    drop(api);
    stop(daemon).await;
}

/// Every log line of this test binary, from a global subscriber.
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

// covers: AC-3 (spec 0044): a sentinel written into the file of a
// registered environment never reaches `env.get`, a registered plan, a
// refused import's operation and audit rows, the daemon log, or a file
#[tokio::test(flavor = "multi_thread")]
async fn a_sentinel_in_a_registered_manifest_reaches_no_reply_audit_or_log() {
    const SENTINEL: &str = "SENTINEL-registered-4b81d2";
    let logs = logs();
    let site = Site::new();
    let daemon = start(&site).await;
    let api = site.api().await;
    let path = write(
        &project(&site).join("stackly.yaml"),
        &fake_manifest("shop", &["api"]),
    );
    let imported = import(&api, &path).await.expect("imports");
    write(
        &path,
        &format!(
            "{}  api:\n    provider: testing/fake\n    version: \"0.1\"\n    secretRefs:\n      authToken: \"{SENTINEL}\"\n    settings:\n      authToken: \"{SENTINEL}\"\n      label: [\"{SENTINEL}\"]\n",
            header("shop")
        ),
    );

    let checked = get(&api, imported.summary.id, true).await.expect("gets");
    assert_eq!(checked.manifest_state, Some(ManifestState::Invalid));
    assert!(checked.problems.is_some_and(|n| u64::from(n) > 0));
    let planned = plan_of(&api, EnvSource::Registered(imported.summary.id))
        .await
        .expect("plans");
    assert!(!planned.applicable);
    let digest = digest(&api, &path).await;
    let refused = import_with(&api.client, &path, &digest, Some(imported.summary.id))
        .await
        .expect_err("an invalid manifest is not imported");
    assert_eq!(refused.code(), "env.manifest_invalid");
    let operations = match api
        .client
        .call(Call::OperationList(ListOperationsParams::default()))
        .await
        .expect("lists")
    {
        Reply::OperationList(page) => page,
        other => panic!("not an operation page: {other:?}"),
    };
    let audit = match api
        .client
        .call(Call::AuditList(ListAuditParams::default()))
        .await
        .expect("lists")
    {
        Reply::AuditList(page) => page,
        other => panic!("not an audit page: {other:?}"),
    };
    assert!(!audit.items.is_empty(), "the refused import is audited");

    for (what, text) in [
        ("env.get", serde_json::to_string(&checked).expect("encodes")),
        ("plan", serde_json::to_string(&planned).expect("encodes")),
        ("import", serde_json::to_string(&*refused).expect("encodes")),
        (
            "operation.list",
            serde_json::to_string(&operations).expect("encodes"),
        ),
        (
            "audit.list",
            serde_json::to_string(&audit).expect("encodes"),
        ),
    ] {
        assert!(
            !text.contains(SENTINEL),
            "the {what} reply holds the sentinel"
        );
    }
    drop(api);
    stop(daemon).await;
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

// covers: AC-13 (spec 0044): the delete itself refuses an active lease the
// guard did not look at (one the environment holds on an instance that is
// not its member), with `env.lease_active` and no instance items, and the
// environment stays
#[tokio::test(flavor = "multi_thread")]
async fn a_lease_the_guard_did_not_see_still_refuses_the_remove() {
    use stackly_domain::{Lease, LeaseHolder, LeaseId};
    use stackly_platform::{Clock as _, SystemClock};
    use stackly_protocol::env::RemoveEnvironmentParams;

    let site = Site::new();
    let first = start_empty(&site).await;
    let instance = site
        .api()
        .await
        .create_ready("loose-db", serde_json::Value::Null)
        .await;
    stop(first).await;
    let daemon = start(&site).await;
    let api = site.api().await;
    let path = write(
        &project(&site).join("stackly.yaml"),
        &fake_manifest("shop", &["db"]),
    );
    let imported = import(&api, &path).await.expect("imports");
    let registry = daemon.core().registry().expect("open");
    let env_id = registry
        .environments()
        .expect("reads")
        .value
        .into_iter()
        .find(|env| env.id().as_uuid() == imported.summary.id)
        .expect("stored")
        .id();
    let lease = Lease::new(
        LeaseId::generate(),
        InstanceId::from_uuid(instance).expect("a v7 id"),
        LeaseHolder::Environment(env_id),
        None,
        SystemClock.now(),
    )
    .expect("an environment lease");
    registry
        .commit(WriteBatch::new(vec![WriteOp::InsertLease(lease)]))
        .expect("the lease commits");

    let refused = match api
        .client
        .call_detailed(
            Call::EnvRemove(RemoveEnvironmentParams {
                id: imported.summary.id,
                expected_revision: imported.summary.revision,
            }),
            CallOptions::default().with_idempotency_key(key()),
        )
        .await
    {
        Err(error) => error,
        Ok(reply) => panic!("the remove went through: {reply:?}"),
    };

    assert_eq!(refused.code(), "env.lease_active");
    assert_eq!(detail(&refused, "count").as_deref(), Some("0"));
    assert!(refused.hint().is_some());
    assert!(
        get(&api, imported.summary.id, false).await.is_ok(),
        "the environment stays"
    );
    drop(api);
    stop(daemon).await;
}
