//! Edges of `env.up` through the real daemon (spec 0046), with the fake
//! provider only: a blocked plan and a plan edited after its review are
//! refused through the socket without a write, a member whose instance
//! holds a `needsSecret` blocker is refused `secret.needs_secret`, a
//! borrowed stopped instance is started and otherwise left as it was, two
//! environments borrow one instance at once, two manifests race for one
//! instance name, a client that disconnects leaves the Up running, a new
//! login session keeps the environment lease, and a restart or a cancel in
//! the create stage leaves only what the create owned. No edge stands for a
//! real dependency between databases.
#![cfg(target_os = "macos")]

mod common;
#[path = "supervisor/support.rs"]
mod support;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use stackly_core::catalog::{
    Catalog, CatalogEntry, EntryOrigin, ProviderEntry, TileColor, TileHint,
};
use stackly_core::provider::{NameCheck, ValidatedPackage, read_package};
use stackly_domain::{
    EnvironmentId, InstanceFilter, InstanceId, LeaseFilter, LeaseHolder, LeaseState,
    OperationQuery, Registry as _, SecretUser,
};
use stackly_ipc::CallOptions;
use stackly_platform::{Clock as _, LaunchContext, SystemClock};
use stackly_protocol::env::{
    EnvGetParams, EnvPlan, EnvPlanParams, EnvSource, EnvUpParams, EnvUpReply, EnvironmentDetail,
    UpPhase, UpStage, UpState,
};
use stackly_protocol::instance::{InstanceDesired, InstanceLifecycle, InstanceReadinessState};
use stackly_protocol::operation::{CancelOperationParams, OperationState};
use stackly_protocol::secret::{
    NeedsSecretReason, SecretInput, SecretMode, SecretText, SetSecretParams,
};
use stackly_protocol::{Call, ErrorBody, Reply, Timestamp};
use stackly_testkit::FakeLoginSessionInfo;
use stackly_testkit::evidence::{Kind, evidence};
use stacklyd::log_sink::{LogSink, LogSinkConfig};
use stacklyd::map::StartFacts;
use stacklyd::server::{self, DaemonDeps, DaemonHandle};
use uuid::Uuid;

use crate::support::{Site, fast, key, serving};

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

/// A daemon whose catalog holds the fake package.
async fn start(site: &Site) -> DaemonHandle {
    site.unpin_fake_package();
    site.start_with(config()).await
}

/// A daemon run by launchd that reads its login session from `session`,
/// as spec 0031's login tests start it.
async fn start_launchd(site: &Site, session: &FakeLoginSessionInfo) -> DaemonHandle {
    let log =
        LogSink::start(Box::new(std::io::sink()), LogSinkConfig::DAEMON).expect("the log starts");
    let facts = StartFacts::new(
        Uuid::now_v7(),
        Timestamp::from_jiff(SystemClock.now()),
        true,
        std::process::id(),
    );
    let platform = site
        .platform
        .clone()
        .with_login_session(Arc::new(session.clone()));
    let deps = DaemonDeps::new(platform, site.prepared.clone(), facts, Arc::new(log))
        .with_config(config())
        .with_launch(
            LaunchContext::Launchd {
                label: "com.stackly.test.envup".to_owned(),
            },
            None,
        );
    let handle = server::start(deps).await.expect("the server starts");
    handle.started().await;
    handle
}

async fn stop(daemon: DaemonHandle) {
    daemon
        .shutdown(
            stackly_protocol::events::ShutdownReason::Requested,
            std::future::pending(),
        )
        .await;
}

/// Writes `text` as `<dir>/stackly.yaml` under the test root.
fn write(site: &Site, dir: &str, text: &str) -> PathBuf {
    let dir = site.root.path().join(dir);
    std::fs::create_dir_all(&dir).expect("the project folder can be made");
    let path = dir.join("stackly.yaml");
    std::fs::write(&path, text).expect("the manifest writes");
    path
}

/// A manifest named `name` with the services in `services` (YAML under
/// `services:`, indented two spaces).
fn manifest(name: &str, services: &str) -> String {
    format!(
        "apiVersion: stackly/v1alpha1\nkind: Environment\nmetadata:\n  name: {name}\nservices:\n{services}"
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

fn digest_of(planned: &EnvPlan) -> String {
    planned
        .plan_digest
        .clone()
        .unwrap_or_else(|| panic!("the plan is applicable: {:?}", planned.blockers))
}

/// Plans `path`, then brings it up with the plan's digest.
async fn up(api: &support::Api, path: &Path) -> EnvUpReply {
    let digest = digest_of(&plan(api, path).await);
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

/// Waits until `lastUp` of the environment shows `alias` in `phase`.
async fn wait_phase(api: &support::Api, environment: Uuid, alias: &str, phase: UpPhase) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        if let Ok(detail) = try_get(api, environment).await
            && detail.last_up.as_ref().is_some_and(|last| {
                last.services
                    .iter()
                    .any(|service| service.alias == alias && service.phase == phase)
            })
        {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "{alias} never reached {phase:?}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// What a refused request must leave as it was: the registry generation
/// and the number of operations, instances, environments, runtimes, and
/// leases.
fn counts(daemon: &DaemonHandle) -> (stackly_domain::Generation, [usize; 5]) {
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
    let environments = registry.environments().expect("reads").value.len();
    let runtimes = registry.runtimes().expect("reads").value.len();
    let leases: usize = ids
        .value
        .iter()
        .map(|id| {
            registry
                .leases(*id, LeaseFilter::All)
                .expect("reads")
                .value
                .len()
        })
        .sum();
    (
        ids.generation,
        [operations, ids.value.len(), environments, runtimes, leases],
    )
}

/// The active leases of `instance`, by holder.
fn active_holders(daemon: &DaemonHandle, instance: Uuid) -> Vec<LeaseHolder> {
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

fn environment_id(id: Uuid) -> EnvironmentId {
    EnvironmentId::from_uuid(id).expect("an environment ID")
}

/// Every file under `dir`, with its bytes, by relative path.
fn tree(dir: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn walk(base: &Path, dir: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries {
            let path = entry.expect("the folder reads").path();
            if path.is_dir() {
                walk(base, &path, out);
            } else {
                let relative = path
                    .strip_prefix(base)
                    .expect("under the base")
                    .to_path_buf();
                out.insert(relative, std::fs::read(&path).expect("the file reads"));
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, dir, &mut out);
    out
}

/// The endpoints as requested: name, transport, address, and port
/// request. The port a start binds for an `auto` request is what the
/// start observes, not a change of the endpoint.
fn requested(endpoints: &[stackly_domain::Endpoint]) -> Vec<String> {
    endpoints
        .iter()
        .map(|endpoint| match endpoint {
            stackly_domain::Endpoint::Ip(ip) => format!(
                "{} {:?} {} {:?}",
                ip.name(),
                ip.transport(),
                ip.address(),
                ip.request()
            ),
            other => format!("{other:?}"),
        })
        .collect()
}

/// Everything of an instance an Up must never change (AC-5): the row
/// without desired, lifecycle, readiness, health, last exit, and revision,
/// the active config revision, the secrets it uses, and its `config/`
/// tree.
fn untouchable(site: &Site, daemon: &DaemonHandle, id: Uuid) -> String {
    let registry = daemon.core().registry().expect("the registry is open");
    let instance_id = InstanceId::from_uuid(id).expect("an instance ID");
    let row = registry
        .instance(instance_id)
        .expect("reads")
        .value
        .expect("the instance exists");
    // The start marks a revision ready (`ready_at`); what it holds stays.
    let config = registry
        .active_config_revision(instance_id)
        .expect("reads")
        .value
        .map(|record| {
            format!(
                "{:?}",
                (
                    record.id,
                    record.number,
                    record.values,
                    record.endpoints,
                    record.previous,
                    record.status,
                    record.origin,
                )
            )
        });
    let secrets = registry
        .secret_uses_by(SecretUser::Instance(instance_id))
        .expect("reads")
        .value;
    format!(
        "{:?} {:?}",
        (
            row.name(),
            row.display_name(),
            row.provider_revision(),
            row.software(),
            row.runtime_id(),
            row.data_line(),
            row.data_version(),
            row.data_initialized_at(),
            requested(row.endpoints()),
            row.startup(),
        ),
        (
            row.capabilities(),
            row.created_by(),
            config,
            secrets,
            tree(&site.instance_paths(id).config),
        ),
    )
}

/// Asserts two snapshots are equal and shows where they first differ.
fn assert_same(after: &str, before: &str) {
    if after == before {
        return;
    }
    let at = after
        .chars()
        .zip(before.chars())
        .take_while(|(a, b)| a == b)
        .count();
    let from = at.saturating_sub(80);
    let window = |text: &str| text.chars().skip(from).take(240).collect::<String>();
    panic!(
        "the instance changed at {at}:\n after: {}\nbefore: {}",
        window(after),
        window(before)
    );
}

// covers: AC-1 (spec 0046), `env.plan_blocked` through the socket
#[tokio::test(flavor = "multi_thread")]
async fn a_blocked_plan_is_refused_through_the_socket_with_its_blockers_and_writes_nothing() {
    let ev = evidence!(
        covers = ["REQ-021"],
        kind = Kind::Fixture,
        expected = "env.up of a manifest whose instanceRef names no instance answers env.plan_blocked with one blocker item per plan blocker, and moves neither the registry generation nor the count of operations, instances, environments, runtimes, or leases",
    );
    let site = Site::new();
    let daemon = start(&site).await;
    let api = site.api().await;
    let path = write(
        &site,
        "blocked",
        &manifest(
            "blocked",
            "  cache:\n    provider: testing/fake\n    instanceRef: nowhere-cache\n  api:\n    provider: testing/fake\n    version: \"0.1\"\n    dependsOn: [{service: cache}]\n",
        ),
    );
    let planned = plan(&api, &path).await;
    assert!(!planned.applicable, "{planned:?}");
    assert!(!planned.blockers.is_empty());
    let before = counts(&daemon);

    let refused = up_with(&api, &path, &"0".repeat(64))
        .await
        .expect_err("a blocked plan is refused");

    assert_eq!(refused.code(), "env.plan_blocked", "{refused:?}");
    assert_eq!(refused.items().len(), planned.blockers.len(), "{refused:?}");
    let codes: Vec<&str> = refused
        .items()
        .iter()
        .filter_map(|item| item.field("code"))
        .collect();
    let expected: Vec<&str> = planned
        .blockers
        .iter()
        .map(|blocker| blocker.error.code())
        .collect();
    assert_eq!(codes, expected);
    assert_eq!(counts(&daemon), before, "nothing was written");
    stop(daemon).await;
    ev.pass();
}

// covers: AC-1, AC-19 (spec 0046), a plan that changed between review and Up
#[tokio::test(flavor = "multi_thread")]
async fn a_manifest_edited_after_its_review_is_refused_with_plan_changed_and_writes_nothing() {
    let ev = evidence!(
        covers = ["REQ-021"],
        kind = Kind::Fixture,
        expected = "env.up with the digest of a plan the client reviewed before the manifest was edited answers env.plan_changed and writes nothing; the digest of the new plan brings it up",
    );
    let site = Site::new();
    let daemon = start(&site).await;
    let api = site.api().await;
    let services = "  db:\n    provider: testing/fake\n    version: \"0.1\"\n";
    let path = write(&site, "edited", &manifest("edited", services));
    let reviewed = digest_of(&plan(&api, &path).await);
    std::fs::write(
        &path,
        manifest(
            "edited",
            &format!("{services}  cache:\n    provider: testing/fake\n    version: \"0.1\"\n"),
        ),
    )
    .expect("the manifest is edited");
    let before = counts(&daemon);

    let refused = up_with(&api, &path, &reviewed)
        .await
        .expect_err("the old digest is refused");

    assert_eq!(refused.code(), "env.plan_changed", "{refused:?}");
    assert_eq!(counts(&daemon), before, "nothing was written");
    let fresh = up(&api, &path).await;
    let done = api.wait_final(fresh.operation.operation.id).await;
    assert_eq!(done.operation.state, OperationState::Succeeded, "{done:?}");
    assert_eq!(get(&api, fresh.environment_id).await.services.len(), 2);
    stop(daemon).await;
    ev.pass();
}

/// Sets `reference` to a fixture value.
async fn set_secret(api: &support::Api, reference: &str) {
    let reply = api
        .client
        .call(Call::SecretSet(SetSecretParams {
            reference: reference.to_owned(),
            input: SecretInput::Value(SecretText::new("stackly-test-fixture-token".to_owned())),
            mode: SecretMode::Upsert,
            expected_revision: None,
            confirm_stored_only: false,
        }))
        .await;
    assert!(reply.is_ok(), "{reply:?}");
}

// covers: AC-2 (spec 0046), an `instanceRef` whose instance holds a `needsSecret` blocker
#[tokio::test(flavor = "multi_thread")]
async fn a_borrowed_instance_with_a_needs_secret_blocker_is_refused_with_its_reference() {
    let ev = evidence!(
        covers = ["REQ-021"],
        kind = Kind::Fixture,
        expected = "after the start of an instance met a locked store and left a needsSecret blocker, env.up of a manifest that borrows it through instanceRef answers secret.needs_secret with the blocker's reference and reason store_locked, and writes nothing",
    );
    let site = Site::new();
    site.unpin_fake_package();
    // The store retry of spec 0031 clears a `store_locked` blocker of an
    // instance whose desired state is `stopped`; keep it out of the way.
    let mut slow_retry = config();
    slow_retry.supervisor.login_secret_retry_interval = Duration::from_secs(3_600);
    let daemon = site.start_with(slow_retry).await;
    let api = site.api().await;
    let held = api.create_ready("held-cache", json!({})).await;
    site.secrets.lock();
    let start = api.start(held).await.expect("the start is accepted");
    let start = api
        .wait_final(start.accepted.expect("a start operation").operation.id)
        .await;
    assert_eq!(start.operation.state, OperationState::Failed, "{start:?}");
    let blocked = api
        .wait_instance(held, "blocked", |summary| summary.blocker.is_some())
        .await;
    let path = write(
        &site,
        "held",
        &manifest(
            "held",
            "  cache:\n    provider: testing/fake\n    instanceRef: held-cache\n",
        ),
    );
    let before = counts(&daemon);

    let planned = plan(&api, &path).await;
    let digest = planned.plan_digest.unwrap_or_else(|| "0".repeat(64));
    let refused = up_with(&api, &path, &digest)
        .await
        .expect_err("the blocked instance is refused");

    assert_eq!(refused.code(), "secret.needs_secret", "{refused:?}");
    assert_eq!(refused.items().len(), 1, "{refused:?}");
    let reference = refused.items()[0]
        .field("reference")
        .expect("the item names its reference");
    assert!(
        reference.starts_with(&format!("local:instance/{held}/")),
        "{refused:?} {blocked:?}"
    );
    assert_eq!(refused.items()[0].field("reason"), Some("store_locked"));
    assert_eq!(counts(&daemon), before, "nothing was written");
    site.secrets.unlock();
    stop(daemon).await;
    ev.pass();
}

// covers: AC-2, AC-10, AC-21 (spec 0046), `lastUp.references` keeps the child's reason
#[tokio::test(flavor = "multi_thread")]
async fn a_create_that_meets_a_locked_store_records_store_locked_and_no_generate_in_last_up() {
    let site = Site::new();
    let daemon = start(&site).await;
    let api = site.api().await;
    set_secret(&api, "local:shut/token").await;
    let path = write(
        &site,
        "shut",
        &manifest(
            "shut",
            "  db:\n    provider: testing/fake\n    version: \"0.1\"\n    secretRefs:\n      authToken: \"local:shut/token\"\n",
        ),
    );
    site.secrets.lock();

    let reply = up(&api, &path).await;
    let done = api.wait_final(reply.operation.operation.id).await;

    assert_eq!(done.operation.state, OperationState::Failed, "{done:?}");
    let detail = get(&api, reply.environment_id).await;
    let last = detail.last_up.expect("lastUp is written");
    let db = last
        .services
        .iter()
        .find(|service| service.alias == "db")
        .expect("the report names db");
    assert_eq!(db.phase, UpPhase::NeedsSecret, "{db:?}");
    assert_eq!(db.references.len(), 1);
    assert_eq!(db.references[0].reason, NeedsSecretReason::StoreLocked);
    assert!(
        !db.references[0].generate_allowed,
        "a secretRefs reference is never generated"
    );
    site.secrets.unlock();
    stop(daemon).await;
}

// covers: AC-5, AC-13 (spec 0046), a borrowed stopped instance
#[tokio::test(flavor = "multi_thread")]
async fn a_borrowed_stopped_instance_is_started_and_keeps_its_config_ports_secrets_and_startup() {
    let ev = evidence!(
        covers = ["VAL-014"],
        kind = Kind::Fixture,
        expected = "env.up of a manifest that borrows a stopped instance through instanceRef starts it through start_instance with no manual lease, and leaves its name, provider pin, runtime, data, endpoints and ports, startup policy, active config revision, secret uses, and config folder byte for byte as they were",
    );
    let site = Site::new();
    let daemon = start(&site).await;
    let api = site.api().await;
    let shared = api
        .create_ready("lent-cache", json!({"greeting": "kept"}))
        .await;
    // Its first start renders its config; start and stop it once by hand
    // so the snapshot holds what a start of it renders.
    api.start_ready(shared).await;
    api.stop_when_free(shared).await;
    api.wait_instance(shared, "stopped", |summary| {
        summary.lifecycle == InstanceLifecycle::Stopped
    })
    .await;
    let before = untouchable(&site, &daemon, shared);
    let path = write(
        &site,
        "lend",
        &manifest(
            "lend",
            "  cache:\n    provider: testing/fake\n    instanceRef: lent-cache\n",
        ),
    );

    let reply = up(&api, &path).await;
    let done = api.wait_final(reply.operation.operation.id).await;

    assert_eq!(done.operation.state, OperationState::Succeeded, "{done:?}");
    let summary = api.get(shared).await.expect("the instance reads");
    assert_eq!(summary.lifecycle, InstanceLifecycle::Running);
    assert_eq!(summary.desired, InstanceDesired::Running);
    assert_same(&untouchable(&site, &daemon, shared), &before);
    assert_eq!(
        active_holders(&daemon, shared),
        [LeaseHolder::Environment(environment_id(
            reply.environment_id
        ))],
        "one environment lease and no manual lease"
    );
    let last = get(&api, reply.environment_id)
        .await
        .last_up
        .expect("lastUp is written");
    assert!(
        last.services[0].operation_id.is_some(),
        "a start_instance child started it"
    );
    stop(daemon).await;
    ev.pass();
}

// covers: AC-14, AC-5 (spec 0046), two environments borrow one instance at once
#[tokio::test(flavor = "multi_thread")]
async fn two_environments_that_borrow_one_stopped_instance_at_once_get_two_leases_and_one_process()
{
    let ev = evidence!(
        covers = ["AC-073"],
        kind = Kind::Fixture,
        expected = "two env.up runs of two environments that borrow the same stopped instance, sent at once, both succeed with exactly two active environment leases on it, one serving process, and a restart count of 0",
    );
    let site = Site::new();
    let daemon = start(&site).await;
    let api = site.api().await;
    let shared = api.create_ready("both-cache", json!({})).await;
    let borrow = "  cache:\n    provider: testing/fake\n    instanceRef: both-cache\n";
    let left = write(&site, "left", &manifest("left", borrow));
    let right = write(&site, "right", &manifest("right", borrow));
    let left_digest = digest_of(&plan(&api, &left).await);
    let right_digest = digest_of(&plan(&api, &right).await);

    let (a, b) = tokio::join!(
        up_with(&api, &left, &left_digest),
        up_with(&api, &right, &right_digest)
    );
    let (a, b) = (a.expect("left starts"), b.expect("right starts"));
    let done_a = api.wait_final(a.operation.operation.id).await;
    let done_b = api.wait_final(b.operation.operation.id).await;

    assert_eq!(
        done_a.operation.state,
        OperationState::Succeeded,
        "{done_a:?}"
    );
    assert_eq!(
        done_b.operation.state,
        OperationState::Succeeded,
        "{done_b:?}"
    );
    let mut holders = active_holders(&daemon, shared);
    holders.sort_by_key(|holder| format!("{holder:?}"));
    let mut expected = vec![
        LeaseHolder::Environment(environment_id(a.environment_id)),
        LeaseHolder::Environment(environment_id(b.environment_id)),
    ];
    expected.sort_by_key(|holder| format!("{holder:?}"));
    assert_eq!(holders, expected);
    let summary = api.get(shared).await.expect("the instance reads");
    assert_eq!(summary.restart_total.get(), 0);
    assert_eq!(
        serving(&site.instance_paths(shared).data).len(),
        1,
        "one process serves it"
    );
    stop(daemon).await;
    ev.pass();
}

// covers: AC-14, AC-10, AC-8 (spec 0046), two manifests race for one instance name
#[tokio::test(flavor = "multi_thread")]
async fn two_manifests_that_create_one_instance_name_at_once_end_with_exactly_one_instance() {
    let ev = evidence!(
        covers = ["AC-073", "AC-021"],
        kind = Kind::Fixture,
        expected = "two env.up runs of two manifests whose db both name the instance race-db, sent at once, leave exactly one instance race-db; the winner succeeds and the loser fails env.up_incomplete with db failed at create (instance.name_taken or resource.busy) and its dependent skipped without a start, unless its precheck already saw the winner's plan and refused env.plan_changed",
    );
    let site = Site::new();
    let daemon = start(&site).await;
    let api = site.api().await;
    let services = "  db:\n    provider: testing/fake\n    version: \"0.1\"\n    instanceName: race-db\n    settings:\n      initDelayMs: 1500\n  api:\n    provider: testing/fake\n    version: \"0.1\"\n    dependsOn: [{service: db}]\n";
    let one = write(&site, "one", &manifest("race-one", services));
    let two = write(&site, "two", &manifest("race-two", services));
    let one_digest = digest_of(&plan(&api, &one).await);
    let two_digest = digest_of(&plan(&api, &two).await);

    let (a, b) = tokio::join!(
        up_with(&api, &one, &one_digest),
        up_with(&api, &two, &two_digest)
    );

    let mut outcomes = Vec::new();
    for reply in [a, b] {
        match reply {
            Ok(reply) => {
                let done = api.wait_final(reply.operation.operation.id).await;
                outcomes.push(Some((reply, done)));
            }
            Err(error) => {
                assert_eq!(error.code(), "env.plan_changed", "{error:?}");
                outcomes.push(None);
            }
        }
    }
    let registry = daemon.core().registry().expect("the registry is open");
    let named: Vec<_> = registry
        .instances(InstanceFilter {
            include_retained: true,
        })
        .expect("reads")
        .value
        .into_iter()
        .filter(|instance| instance.name().as_str() == "race-db")
        .collect();
    assert_eq!(named.len(), 1, "exactly one instance race-db");
    let succeeded = outcomes
        .iter()
        .flatten()
        .filter(|(_, done)| done.operation.state == OperationState::Succeeded)
        .count();
    assert_eq!(succeeded, 1, "{outcomes:?}");
    for (reply, done) in outcomes.iter().flatten() {
        if done.operation.state == OperationState::Succeeded {
            continue;
        }
        assert_eq!(done.operation.state, OperationState::Failed, "{done:?}");
        let last = get(&api, reply.environment_id)
            .await
            .last_up
            .expect("lastUp is written");
        let db = last
            .services
            .iter()
            .find(|service| service.alias == "db")
            .expect("the report names db");
        assert_eq!(db.phase, UpPhase::Failed, "{db:?}");
        assert_eq!(db.stage, Some(UpStage::Create));
        assert!(
            matches!(
                db.code.as_deref(),
                Some("instance.name_taken" | "resource.busy")
            ),
            "{db:?}"
        );
        let dependent = last
            .services
            .iter()
            .find(|service| service.alias == "api")
            .expect("the report names api");
        assert_eq!(dependent.phase, UpPhase::Skipped, "{dependent:?}");
        assert_eq!(dependent.operation_id, None);
    }
    stop(daemon).await;
    ev.pass();
}

// covers: AC-13 (spec 0046), a client that disconnects in the middle of an Up
#[tokio::test(flavor = "multi_thread")]
async fn a_client_that_disconnects_in_the_middle_of_an_up_leaves_it_running_to_success() {
    let site = Site::new();
    let daemon = start(&site).await;
    let sender = site.api().await;
    let path = write(
        &site,
        "gone",
        &manifest(
            "gone",
            "  db:\n    provider: testing/fake\n    version: \"0.1\"\n    settings:\n      readyDelayMs: 1500\n",
        ),
    );
    let reply = up(&sender, &path).await;
    wait_phase(&sender, reply.environment_id, "db", UpPhase::Starting).await;

    drop(sender);
    let api = site.api().await;
    let done = api.wait_final(reply.operation.operation.id).await;

    assert_eq!(done.operation.state, OperationState::Succeeded, "{done:?}");
    let detail = get(&api, reply.environment_id).await;
    let db = detail.services[0]
        .instance
        .as_ref()
        .expect("db has an instance")
        .id;
    assert_eq!(
        active_holders(&daemon, db),
        [LeaseHolder::Environment(environment_id(
            reply.environment_id
        ))]
    );
    let summary = api.get(db).await.expect("the instance reads");
    assert_eq!(summary.desired, InstanceDesired::Running);
    stop(daemon).await;
}

// covers: AC-13 (spec 0046), a new login session (spec 0031 rotation `new`)
#[tokio::test(flavor = "multi_thread")]
async fn a_new_login_session_keeps_the_environment_lease_and_the_desired_state() {
    let site = Site::new();
    site.unpin_fake_package();
    let session = FakeLoginSessionInfo::new();
    let first = start_launchd(&site, &session).await;
    let api = site.api().await;
    let path = write(
        &site,
        "login",
        &manifest(
            "login",
            "  db:\n    provider: testing/fake\n    version: \"0.1\"\n",
        ),
    );
    let reply = up(&api, &path).await;
    let done = api.wait_final(reply.operation.operation.id).await;
    assert_eq!(done.operation.state, OperationState::Succeeded, "{done:?}");
    let db = get(&api, reply.environment_id).await.services[0]
        .instance
        .as_ref()
        .expect("db has an instance")
        .id;
    let holders = vec![LeaseHolder::Environment(environment_id(
        reply.environment_id,
    ))];
    assert_eq!(active_holders(&first, db), holders);
    drop(api);
    stop(first).await;

    session.set_audit_session(100_002);
    let second = start_launchd(&site, &session).await;
    let api = site.api().await;

    assert_eq!(
        active_holders(&second, db),
        holders,
        "the rotation never releases an environment lease"
    );
    let summary = api
        .wait_instance(db, "running and ready in the new session", |summary| {
            summary.readiness.state == InstanceReadinessState::Ready
        })
        .await;
    assert_eq!(summary.desired, InstanceDesired::Running);
    assert_eq!(active_holders(&second, db), holders);
    drop(api);
    stop(second).await;
}

// covers: AC-15 (spec 0046), a daemon restart in the create stage
#[tokio::test(flavor = "multi_thread")]
async fn a_daemon_restart_in_the_create_stage_closes_the_up_interrupted_and_the_next_up_finishes() {
    let ev = evidence!(
        covers = ["REQ-021"],
        kind = Kind::Fixture,
        expected = "a daemon that stops while an Up waits for a slow create comes back with the Up failed operation.interrupted, lastUp interrupted, no operation left queued or running, and the next env.up creates and starts every service",
    );
    let site = Site::new();
    let daemon = start(&site).await;
    let api = site.api().await;
    let slow = manifest(
        "midcreate",
        "  db:\n    provider: testing/fake\n    version: \"0.1\"\n    settings:\n      initDelayMs: 20000\n  api:\n    provider: testing/fake\n    version: \"0.1\"\n    dependsOn: [{service: db}]\n",
    );
    let path = write(&site, "midcreate", &slow);
    let reply = up(&api, &path).await;
    wait_phase(&api, reply.environment_id, "db", UpPhase::Creating).await;
    drop(api);
    stop(daemon).await;

    let daemon = site.start_with(config()).await;
    let api = site.api().await;
    let done = api.wait_final(reply.operation.operation.id).await;

    assert_eq!(done.operation.state, OperationState::Failed, "{done:?}");
    assert_eq!(support::Api::failure_code(&done), "operation.interrupted");
    let detail = get(&api, reply.environment_id).await;
    assert_eq!(
        detail.last_up.as_ref().map(|last| last.state),
        Some(UpState::Interrupted)
    );
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        let registry = daemon.core().registry().expect("the registry is open");
        let active = registry
            .operations(OperationQuery {
                active_only: true,
                resource: None,
                limit: 500,
                before: None,
            })
            .expect("reads")
            .value
            .items;
        if active.is_empty() {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "an operation stays active: {active:?}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    std::fs::write(&path, slow.replace("initDelayMs: 20000", "initDelayMs: 0"))
        .expect("the manifest is edited");
    let next = up(&api, &path).await;
    assert_eq!(next.environment_id, reply.environment_id);
    let done = api.wait_final(next.operation.operation.id).await;
    assert_eq!(done.operation.state, OperationState::Succeeded, "{done:?}");
    let detail = get(&api, next.environment_id).await;
    assert!(
        detail
            .services
            .iter()
            .all(|service| service.not_ready.is_none()),
        "{detail:?}"
    );
    stop(daemon).await;
    ev.pass();
}

// covers: AC-7, AC-11, AC-12 (spec 0046), a cancel in the create stage
#[tokio::test(flavor = "multi_thread")]
async fn a_cancel_in_the_create_stage_compensates_only_what_that_create_made() {
    let ev = evidence!(
        covers = ["REQ-021", "BR-023"],
        kind = Kind::Fixture,
        expected = "operation.cancel on an Up while its create runs ends it cancelled with lastUp cancelled, removes the instance, member, and lease that create made, and leaves a user instance with a similar name, its files, and every runtime as they were",
    );
    let site = Site::new();
    let daemon = start(&site).await;
    let api = site.api().await;
    let user = api.create_ready("halt-db-own", json!({})).await;
    let user_row = untouchable(&site, &daemon, user);
    let user_data = tree(&site.instance_paths(user).data);
    let runtimes = daemon
        .core()
        .registry()
        .expect("the registry is open")
        .runtimes()
        .expect("reads")
        .value;
    let path = write(
        &site,
        "halt",
        &manifest(
            "halt",
            "  db:\n    provider: testing/fake\n    version: \"0.1\"\n    instanceName: halt-db\n    settings:\n      initDelayMs: 20000\n",
        ),
    );
    let reply = up(&api, &path).await;
    wait_phase(&api, reply.environment_id, "db", UpPhase::Creating).await;

    let cancelled = api
        .client
        .call(Call::OperationCancel(CancelOperationParams {
            operation_id: reply.operation.operation.id,
        }))
        .await;
    assert!(cancelled.is_ok(), "{cancelled:?}");
    let done = api.wait_final(reply.operation.operation.id).await;

    assert_eq!(done.operation.state, OperationState::Cancelled, "{done:?}");
    let detail = get(&api, reply.environment_id).await;
    assert_eq!(
        detail.last_up.as_ref().map(|last| last.state),
        Some(UpState::Cancelled)
    );
    assert!(detail.services[0].instance.is_none(), "{detail:?}");
    let child = detail.last_up.expect("lastUp is written").services[0]
        .operation_id
        .expect("the create child is recorded");
    let child = api.wait_final(child).await;
    assert_eq!(
        child.operation.state,
        OperationState::Cancelled,
        "{child:?}"
    );
    let registry = daemon.core().registry().expect("the registry is open");
    let names: Vec<String> = registry
        .instances(InstanceFilter {
            include_retained: true,
        })
        .expect("reads")
        .value
        .iter()
        .map(|instance| instance.name().to_string())
        .collect();
    assert_eq!(names, ["halt-db-own"], "the cancelled create left nothing");
    let environment = registry
        .environment(environment_id(reply.environment_id))
        .expect("reads")
        .value
        .expect("the environment stays");
    assert!(environment.members().is_empty());
    assert_same(&untouchable(&site, &daemon, user), &user_row);
    assert_eq!(tree(&site.instance_paths(user).data), user_data);
    assert_eq!(registry.runtimes().expect("reads").value, runtimes);
    stop(daemon).await;
    ev.pass();
}
