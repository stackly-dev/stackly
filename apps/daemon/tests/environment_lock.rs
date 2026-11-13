//! `env.lock`, the local override file, and `env.export` through the real
//! daemon with what spec 0045 asks its tests to plant (spec 0045, the gaps
//! the checker listed): a lock written from a root that holds secret
//! sentinels, a running instance, and an override with a path under the
//! home holds none of them; a lock that can't be written leaves what was
//! there; since `env.up` exists (spec 0046), a storage override reaches the
//! instance Up creates, Up writes `lock_sha256` from the plan, and
//! `env.export` prints the real ports of the members Up started, never a
//! secret, a path, or a PID, in the reply, on the stdout of `stackly`, or
//! in the daemon's log.
#![cfg(target_os = "macos")]

mod common;
#[path = "supervisor/support.rs"]
mod support;

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use stackly_core::catalog::{
    Catalog, CatalogEntry, EntryOrigin, ProviderEntry, TileColor, TileHint,
};
use stackly_core::provider::{NameCheck, ValidatedPackage, read_package};
use stackly_domain::{InstanceId, InstancePathKind, Registry as _};
use stackly_ipc::CallOptions;
use stackly_protocol::env::{
    EnvExport, EnvExportParams, EnvGetParams, EnvLockParams, EnvLockResult, EnvPlan, EnvPlanParams,
    EnvSource, EnvUpParams, EnvUpReply, EnvironmentDetail, ExportFormat, ExportSkipReason,
    LockRefresh, LockState, OverrideState,
};
use stackly_protocol::operation::OperationState;
use stackly_protocol::secret::{SecretInput, SecretMode, SecretText, SetSecretParams};
use stackly_protocol::settings::{LocationKind, LocationTarget, SetLocationParams};
use stackly_protocol::{Call, ErrorBody, Reply};
use stackly_testkit::cargo_bin;
use stackly_testkit::evidence::{Kind, evidence};
use stacklyd::server::DaemonHandle;
use uuid::Uuid;

use crate::support::{Site, fast, key, serving};

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

/// The fake provider and the six official ones.
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

/// A daemon on [`catalog`]. The seed pinned the fake package with a
/// test's permissions, so it goes first and create pins it again.
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

impl std::io::Write for Tee {
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

/// What the daemon logged from byte `from` on.
fn logged_since(from: usize) -> String {
    let logs = logs();
    let bytes = logs.lock().unwrap_or_else(PoisonError::into_inner);
    String::from_utf8_lossy(&bytes[from.min(bytes.len())..]).into_owned()
}

fn log_len() -> usize {
    logs().lock().unwrap_or_else(PoisonError::into_inner).len()
}

fn project(site: &Site) -> PathBuf {
    let dir = site.root.path().join("project");
    std::fs::create_dir_all(&dir).expect("the project folder can be made");
    dir
}

fn write(path: &Path, text: &str) -> PathBuf {
    std::fs::write(path, text).expect("the file writes");
    path.to_path_buf()
}

fn erp() -> String {
    std::fs::read_to_string(
        repo().join("crates/core/tests/fixtures/environments/erp-development.yaml"),
    )
    .expect("the ERP manifest reads")
}

async fn set_secret(api: &support::Api, reference: &str, value: &str) {
    let reply = api
        .client
        .call(Call::SecretSet(SetSecretParams {
            reference: reference.to_owned(),
            input: SecretInput::Value(SecretText::new(value.to_owned())),
            mode: SecretMode::Upsert,
            expected_revision: None,
            confirm_stored_only: false,
        }))
        .await;
    assert!(reply.is_ok(), "{reference}: {reply:?}");
}

async fn lock_call(
    api: &support::Api,
    path: &Path,
    refresh: LockRefresh,
) -> Result<EnvLockResult, Box<ErrorBody>> {
    let params = EnvLockParams {
        source: EnvSource::File(path.display().to_string()),
        refresh,
    };
    match api.client.call(Call::EnvLock(params)).await? {
        Reply::EnvLock(result) => Ok(*result),
        other => panic!("not a lock reply: {other:?}"),
    }
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

/// Plans `path`, brings it up with the plan's digest, and waits for the
/// Up to succeed.
async fn up(api: &support::Api, path: &Path) -> (EnvPlan, EnvUpReply) {
    let planned = plan(api, path).await;
    let digest = planned
        .plan_digest
        .clone()
        .unwrap_or_else(|| panic!("the plan is applicable: {:#?}", planned.blockers));
    let params = EnvUpParams {
        source: EnvSource::File(path.display().to_string()),
        expected_plan_digest: digest,
        locked: false,
    };
    let reply = match api
        .client
        .call_detailed(
            Call::EnvUp(params),
            CallOptions::default().with_idempotency_key(key()),
        )
        .await
        .expect("the up starts")
    {
        Reply::EnvUp(reply) => *reply,
        other => panic!("not an up reply: {other:?}"),
    };
    let done = api.wait_final(reply.operation.operation.id).await;
    assert_eq!(done.operation.state, OperationState::Succeeded, "{done:?}");
    (planned, reply)
}

async fn get(api: &support::Api, id: Uuid, check_manifest: bool) -> EnvironmentDetail {
    match api
        .client
        .call(Call::EnvGet(EnvGetParams { id, check_manifest }))
        .await
        .expect("the detail answers")
    {
        Reply::EnvGet(detail) => *detail,
        other => panic!("not a detail: {other:?}"),
    }
}

async fn export_call(api: &support::Api, source: EnvSource, format: ExportFormat) -> EnvExport {
    match api
        .client
        .call(Call::EnvExport(EnvExportParams { source, format }))
        .await
        .expect("the export answers")
    {
        Reply::EnvExport(export) => *export,
        other => panic!("not an export: {other:?}"),
    }
}

fn mode(path: &Path) -> u32 {
    std::fs::metadata(path)
        .expect("the file exists")
        .permissions()
        .mode()
        & 0o777
}

/// Whether `text` holds `pid` as a whole number, outside the 64 hex
/// digests a lock carries.
fn holds_pid(text: &str, pid: u32) -> bool {
    let pid = pid.to_string();
    text.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|word| word.len() != 64)
        .any(|word| word == pid)
}

// covers: AC-1, AC-2 (spec 0045): the lock test the AC asks for, with
// every secret of the manifest stored as a sentinel, an instance process
// running, and an override with a path under the home
#[tokio::test(flavor = "multi_thread")]
async fn a_lock_of_a_root_with_secret_sentinels_a_running_process_and_a_home_override_holds_none_of_them()
 {
    let ev = evidence!(
        covers = ["REQ-023", "BR-018", "AC-080"],
        kind = Kind::Fixture,
        expected = "spec 0045 AC-1: env.lock of the ERP manifest from a root that stores a sentinel value for each of its five secretRefs, runs a fake instance, and holds a local override with a path under the home writes a stackly.lock with mode 0600 that holds no sentinel, reference, path under the home, override, PID of the daemon or of the instance process, and the daemon logs no sentinel",
    );
    let from = log_len();
    let site = Site::new();
    let daemon = start(&site).await;
    let api = site.api().await;
    let dir = project(&site);
    let path = write(&dir.join("stackly.yaml"), &erp());
    let references = [
        "local:erp/postgres/admin",
        "local:erp/mysql/root",
        "local:erp/nats/client",
        "local:erp/clickhouse/default",
        "local:erp/valkey/password",
    ];
    let sentinels: Vec<String> = (0..references.len())
        .map(|at| format!("stackly-lock-sentinel-{at}-{}", Uuid::now_v7().simple()))
        .collect();
    for (reference, value) in references.iter().zip(&sentinels) {
        set_secret(&api, reference, value).await;
    }
    let home = site.root.path().display().to_string();
    write(
        &dir.join(".stackly.local.yaml"),
        &format!(
            "apiVersion: stackly/v1alpha1\nkind: EnvironmentLocal\nservices:\n  postgres:\n    endpoints:\n      client: {{port: 15999}}\n    storage:\n      dataLocation: {home}\n"
        ),
    );
    let instance = api
        .create_ready("lock-witness", serde_json::Value::Null)
        .await;
    api.start_ready(instance).await;
    let data = api.detail(instance).await.expect("the detail").data_path;
    let pids = serving(Path::new(&data));
    assert!(!pids.is_empty(), "the instance process runs");

    let result = lock_call(&api, &path, LockRefresh::None)
        .await
        .expect("locks");
    assert!(result.blockers.is_empty(), "{:#?}", result.blockers);
    assert!(result.written);
    let lock_path = dir.join("stackly.lock");
    assert_eq!(mode(&lock_path), 0o600, "write_file_atomic writes 0600");
    let text = std::fs::read_to_string(&lock_path).expect("the lock reads");
    let reply = serde_json::to_string(&result).expect("encodes");
    for value in &sentinels {
        assert!(!text.contains(value.as_str()), "a sentinel is in the lock");
        assert!(
            !reply.contains(value.as_str()),
            "a sentinel is in the reply"
        );
    }
    for reference in references {
        assert!(!text.contains(reference), "{reference} is in the lock");
        assert!(!text.contains("local:"), "a reference is in the lock");
    }
    for absent in [home.as_str(), "15999", "dataLocation", "/Users/", "~"] {
        assert!(!text.contains(absent), "{absent} is in the lock");
    }
    for pid in pids.iter().copied().chain([std::process::id()]) {
        assert!(!holds_pid(&text, pid), "the PID {pid} is in the lock");
    }
    // Only the fields of *Schema `stackly.lock`*.
    let fields: std::collections::BTreeSet<&str> = text
        .lines()
        .filter(|line| !line.starts_with('#'))
        .filter_map(|line| line.trim_start().trim_start_matches("- ").split_once(':'))
        .map(|(key, _)| key)
        .filter(|key| {
            ![
                "postgres",
                "mysql",
                "nats",
                "clickhouse",
                "valkey",
                "traefik",
            ]
            .contains(key)
        })
        .collect();
    assert_eq!(
        fields.into_iter().collect::<Vec<_>>(),
        [
            "apiVersion",
            "artifacts",
            "id",
            "kind",
            "line",
            "provider",
            "providerManifestSha256",
            "providerRevision",
            "selector",
            "services",
            "sha256",
            "target",
            "version",
        ]
    );
    drop(api);
    stop(daemon).await;
    let logged = logged_since(from);
    for value in &sentinels {
        assert!(
            !logged.contains(value.as_str()),
            "the daemon logged a sentinel"
        );
    }
    ev.pass();
}

// covers: AC-2, AC-3 (spec 0045): a lock that can't be written answers
// `env.lock_unwritable` and leaves the file that was there: a folder
// without write access, and a symlink at the lock path, which is never
// followed to its target
#[tokio::test(flavor = "multi_thread")]
async fn a_lock_that_cannot_be_written_answers_lock_unwritable_and_leaves_what_was_there() {
    let site = Site::new();
    let daemon = start(&site).await;
    let api = site.api().await;
    let dir = project(&site);
    let path = write(&dir.join("stackly.yaml"), &erp());
    lock_call(&api, &path, LockRefresh::None)
        .await
        .expect("locks");
    let lock_path = dir.join("stackly.lock");
    let first = std::fs::read(&lock_path).expect("the lock reads");

    // A seventh service changes the lock; the folder takes no new file.
    let with_cache = erp().replace(
        "services:\n",
        "services:\n  cache:\n    provider: official/valkey\n    endpoints:\n      client: {port: 16380}\n",
    );
    write(&path, &with_cache);
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o500)).expect("read only");
    let refused = lock_call(&api, &path, LockRefresh::None).await;
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).expect("writable");
    let refused = refused.expect_err("the folder takes no file");
    assert_eq!(refused.code(), "env.lock_unwritable");
    let reason = refused
        .details()
        .iter()
        .find(|detail| detail.key() == "reason")
        .map(|detail| detail.value().to_owned());
    assert_eq!(reason.as_deref(), Some("permission_denied"));
    assert_eq!(
        std::fs::read(&lock_path).expect("reads"),
        first,
        "unchanged"
    );
    let leftovers: Vec<String> = std::fs::read_dir(&dir)
        .expect("lists")
        .map(|e| {
            e.expect("an entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .filter(|name| name != "stackly.yaml" && name != "stackly.lock")
        .collect();
    assert!(leftovers.is_empty(), "no temporary file: {leftovers:?}");

    // A symlink at the lock path: refused, the target keeps its bytes.
    let target = dir.join("team.lock");
    std::fs::rename(&lock_path, &target).expect("moves");
    std::os::unix::fs::symlink(&target, &lock_path).expect("links");
    for refresh in [LockRefresh::None, LockRefresh::All] {
        let refused = lock_call(&api, &path, refresh)
            .await
            .expect_err("a symlink is not a file");
        assert_eq!(refused.code(), "env.lock_unwritable");
        assert_eq!(
            refused
                .details()
                .iter()
                .find(|detail| detail.key() == "reason")
                .map(|detail| detail.value().to_owned())
                .as_deref(),
            Some("not_a_file")
        );
    }
    assert_eq!(std::fs::read(&target).expect("reads"), first);
    assert!(
        std::fs::symlink_metadata(&lock_path)
            .expect("the link")
            .file_type()
            .is_symlink()
    );
    // The plan reads the symlink as a lock that can't be read.
    let planned = plan(&api, &path).await;
    assert_eq!(planned.lock.state, LockState::Unreadable);
    assert!(
        planned
            .blockers
            .iter()
            .any(|b| b.error.code() == "env.lock_unreadable"),
        "{:#?}",
        planned.blockers
    );
    drop(api);
    stop(daemon).await;
}

/// Two fake services, each with its own secret reference.
fn shop(name: &str) -> String {
    format!(
        "apiVersion: stackly/v1alpha1\nkind: Environment\nmetadata:\n  name: {name}\nservices:\n  db:\n    provider: testing/fake\n    version: \"0.1\"\n    secretRefs:\n      authToken: \"local:{name}/db-token\"\n  api:\n    provider: testing/fake\n    version: \"0.1\"\n    dependsOn: [{{service: db}}]\n    secretRefs:\n      authToken: \"local:{name}/api-token\"\n"
    )
}

// covers: AC-11 (spec 0045, with spec 0046 Up): a storage override that
// names a registered data location reaches the instance Up creates: its
// data folder is in that location and its logs stay at the default, the
// service without an override keeps the default, and the manifest file,
// its digest, and the lock file never change
#[tokio::test(flavor = "multi_thread")]
async fn up_creates_the_member_in_the_data_location_the_override_names_and_keeps_its_logs() {
    let site = Site::new();
    let daemon = start(&site).await;
    let api = site.api().await;
    let disk = site.root.path().join("disk");
    std::fs::create_dir_all(&disk).expect("the folder");
    std::fs::set_permissions(&disk, std::fs::Permissions::from_mode(0o700)).expect("private");
    let set = |target: LocationTarget| {
        let api = &api;
        async move {
            let revision = match api.client.call(Call::SettingsGet).await {
                Ok(Reply::SettingsGet(view)) => view.revision,
                other => panic!("not settings: {other:?}"),
            };
            match api
                .client
                .call_detailed(
                    Call::StorageSetLocation(SetLocationParams {
                        kind: LocationKind::Data,
                        target,
                        expected_revision: revision,
                    }),
                    CallOptions::default().with_idempotency_key(key()),
                )
                .await
                .expect("the location is set")
            {
                Reply::StorageSetLocation(view) => *view,
                other => panic!("not a location: {other:?}"),
            }
        }
    };
    // The only way to register a data location today is to make it the
    // default; the built in one goes back to being the default.
    let chosen = set(LocationTarget::Path(disk.display().to_string())).await;
    let built_in = set(LocationTarget::BuiltIn).await;
    assert!(built_in.built_in && built_in.is_default);

    for name in ["db", "api"] {
        set_secret(
            &api,
            &format!("local:fast/{name}-token"),
            "stackly-test-storage-token",
        )
        .await;
    }
    let dir = project(&site);
    let path = write(&dir.join("stackly.yaml"), &shop("fast"));
    write(
        &dir.join("stackly.lock"),
        "apiVersion: \"stackly/v1alpha1\"\nkind: \"EnvironmentLock\"\nservices: {}\n",
    );
    let canonical = std::fs::canonicalize(&disk).expect("canonical");
    write(
        &dir.join(".stackly.local.yaml"),
        &format!(
            "apiVersion: stackly/v1alpha1\nkind: EnvironmentLocal\nservices:\n  db:\n    storage:\n      dataLocation: {}\n",
            canonical.display()
        ),
    );
    let manifest_before = std::fs::read(&path).expect("reads");
    let lock_before = std::fs::read(dir.join("stackly.lock")).expect("reads");

    let (planned, reply) = up(&api, &path).await;
    let db_item = planned
        .items
        .iter()
        .find(|item| item.alias == "db")
        .expect("db");
    assert_eq!(
        db_item.storage.as_ref().map(|s| s.location_id),
        Some(chosen.id)
    );
    let local = planned.local.as_ref().expect("the override file");
    assert!(
        local
            .overrides
            .iter()
            .all(|o| o.state == OverrideState::Applied)
    );

    let detail = get(&api, reply.environment_id, true).await;
    let registry = daemon.core().registry().expect("open");
    for service in &detail.services {
        let instance = service.instance.as_ref().expect("a member");
        let data = api.detail(instance.id).await.expect("the detail").data_path;
        let paths = registry
            .instance_paths(InstanceId::from_uuid(instance.id).expect("an id"))
            .expect("reads")
            .value
            .expect("the paths");
        let location = |kind: InstancePathKind| {
            paths
                .iter()
                .find(|entry| entry.kind() == kind)
                .and_then(|entry| entry.location())
                .map(|id| id.as_uuid())
        };
        if service.alias == "db" {
            assert!(
                data.starts_with(&chosen.path),
                "db data in the override: {data} under {}",
                chosen.path
            );
            assert_eq!(location(InstancePathKind::Data), Some(chosen.id));
        } else {
            assert!(!data.starts_with(&chosen.path), "api keeps the default");
            assert_ne!(location(InstancePathKind::Data), Some(chosen.id));
        }
        assert_ne!(
            location(InstancePathKind::Logs),
            Some(chosen.id),
            "{}: the logs stay at the default",
            service.alias
        );
    }
    assert_eq!(std::fs::read(&path).expect("reads"), manifest_before);
    assert_eq!(
        std::fs::read(dir.join("stackly.lock")).expect("reads"),
        lock_before
    );
    assert_eq!(
        detail.summary.manifest_sha256.as_deref(),
        planned.validation.manifest_sha256.as_deref(),
        "the digest of the team's file"
    );
    stop(daemon).await;
}

// covers: AC-12 (spec 0045, with spec 0046 Up): Up writes
// `environments.lock_sha256` from `EnvPlan.lock.sha256`: empty without a
// lock, the digest of a valid lock, and empty again once the lock is gone;
// `env.lock` itself never writes the column
#[tokio::test(flavor = "multi_thread")]
async fn up_writes_the_lock_digest_of_its_plan_and_none_without_a_valid_lock() {
    let site = Site::new();
    let daemon = start(&site).await;
    let api = site.api().await;
    for name in ["db", "api"] {
        set_secret(
            &api,
            &format!("local:digest/{name}-token"),
            "stackly-test-digest-token",
        )
        .await;
    }
    let dir = project(&site);
    let path = write(&dir.join("stackly.yaml"), &shop("digest"));

    let (planned, reply) = up(&api, &path).await;
    assert_eq!(planned.lock.state, LockState::Absent);
    let id = reply.environment_id;
    assert_eq!(get(&api, id, false).await.summary.lock_sha256, None);

    // A valid lock with an entry only for an alias the manifest lacks:
    // every service resolves from the catalog, and the plan has its digest.
    let lock_text = std::fs::read_to_string(
        repo().join("crates/core/tests/fixtures/environments/erp-development.lock"),
    )
    .expect("the golden lock reads");
    write(&dir.join("stackly.lock"), &lock_text);
    let (planned, _) = up(&api, &path).await;
    assert_eq!(planned.lock.state, LockState::Stale);
    let digest = planned
        .lock
        .sha256
        .clone()
        .expect("a valid lock has a digest");
    assert_eq!(
        digest,
        stackly_core::environment::manifest_digest(lock_text.as_bytes()).to_string(),
        "the SHA-256 of the stackly.lock bytes"
    );
    let summary = get(&api, id, false).await.summary;
    assert_eq!(summary.lock_sha256.as_deref(), Some(digest.as_str()));

    // env.lock writes the file, never the column.
    let erp_dir = site.root.path().join("erp");
    std::fs::create_dir_all(&erp_dir).expect("a folder");
    let erp_path = write(&erp_dir.join("stackly.yaml"), &erp());
    lock_call(&api, &erp_path, LockRefresh::All)
        .await
        .expect("locks");
    assert_eq!(
        get(&api, id, false).await.summary.lock_sha256.as_deref(),
        Some(digest.as_str())
    );

    std::fs::remove_file(dir.join("stackly.lock")).expect("removes");
    let (planned, _) = up(&api, &path).await;
    assert_eq!(planned.lock.sha256, None);
    assert_eq!(
        get(&api, id, false).await.summary.lock_sha256,
        None,
        "an Up without a lock writes none"
    );
    stop(daemon).await;
}

/// `stackly <args>` against the daemon of `site`, with a clean
/// environment; returns stdout, stderr, and the exit code.
fn stackly(site: &Site, args: &[&str]) -> (String, String, Option<i32>) {
    let output = Command::new(cargo_bin("stackly-cli", "stackly"))
        .args(args)
        .env("STACKLY_ROOT", site.root.path())
        .env("TZ", "UTC")
        .env_remove("NO_COLOR")
        .env_remove("CLICOLOR")
        .env_remove("CLICOLOR_FORCE")
        .env_remove("STACKLY_NO_INPUT")
        .env_remove("CI")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("stackly runs");
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
        output.status.code(),
    )
}

// covers: AC-14, AC-15, AC-17 (spec 0045, with spec 0046 Up): the export
// test the AC asks for: after Up starts two fake members, each with a
// secret sentinel, env.export prints the real port of each member in both
// formats, from the file and from the registered environment, and neither
// the reply, the stdout of `stackly env export`, nor what the daemon
// logged while exporting holds a sentinel, a reference, a path under the
// home, or the PID of an instance process
#[tokio::test(flavor = "multi_thread")]
async fn export_after_up_prints_the_real_ports_and_never_a_sentinel_a_home_path_or_a_pid() {
    let ev = evidence!(
        covers = ["BR-018", "AC-080"],
        kind = Kind::Fixture,
        expected = "spec 0045 AC-15: after env.up brings up two fake members, each with its own secret sentinel, env.export answers HOST, PORT, and ADDRESS of each member's bound port in dotenv and shell, from the file and by name, the text of stackly env export on stdout is the same text, and no reply, stdout, or daemon log line written during the export holds a sentinel, a reference, a path under the home, or the PID of an instance process",
    );
    let site = Site::for_cli();
    let daemon = start(&site).await;
    let api = site.api().await;
    let sentinels: Vec<(String, String)> = ["db", "api"]
        .iter()
        .map(|alias| {
            (
                format!("local:shop/{alias}-token"),
                format!(
                    "stackly-export-sentinel-{alias}-{}",
                    Uuid::now_v7().simple()
                ),
            )
        })
        .collect();
    for (reference, value) in &sentinels {
        set_secret(&api, reference, value).await;
    }
    let dir = project(&site);
    let path = write(&dir.join("stackly.yaml"), &shop("shop"));
    let (_, reply) = up(&api, &path).await;
    let detail = get(&api, reply.environment_id, false).await;

    let mut expected = Vec::new();
    let mut pids = Vec::new();
    for alias in ["db", "api"] {
        let service = detail
            .services
            .iter()
            .find(|service| service.alias == alias)
            .expect("the service");
        let instance = service.instance.as_ref().expect("a member");
        let http = instance
            .endpoints
            .iter()
            .find(|endpoint| endpoint.name == "http")
            .expect("the http endpoint");
        let port = http.port.expect("Up bound a port");
        let host = http.host.clone().expect("a host");
        let upper = alias.to_uppercase();
        expected.push((format!("STACKLY_{upper}_HTTP_HOST"), host.clone()));
        expected.push((format!("STACKLY_{upper}_HTTP_PORT"), port.to_string()));
        expected.push((
            format!("STACKLY_{upper}_HTTP_ADDRESS"),
            format!("{host}:{port}"),
        ));
        let data = api.detail(instance.id).await.expect("the detail").data_path;
        let serving = serving(Path::new(&data));
        assert!(!serving.is_empty(), "{alias} runs");
        pids.extend(serving);
    }

    let from = log_len();
    let home = site.root.path().display().to_string();
    let check = |what: &str, text: &str| {
        for (reference, value) in &sentinels {
            assert!(!text.contains(value.as_str()), "{what}: a sentinel");
            assert!(!text.contains(reference.as_str()), "{what}: a reference");
        }
        assert!(!text.contains(&home), "{what}: a path under the home");
        assert!(!text.contains("/Users/"), "{what}: a path");
        for pid in &pids {
            assert!(!holds_pid(text, *pid), "{what}: the PID {pid}");
        }
    };
    let registered = EnvSource::Registered(reply.environment_id);
    let file = EnvSource::File(path.display().to_string());
    let mut texts = Vec::new();
    for source in [registered, file] {
        for format in [ExportFormat::Dotenv, ExportFormat::Shell] {
            let export = export_call(&api, source.clone(), format).await;
            let pairs: Vec<(String, String)> = export
                .variables
                .iter()
                .map(|v| (v.name.clone(), v.value.clone()))
                .collect();
            assert_eq!(pairs, expected, "{format:?}");
            assert!(
                export.variables.iter().all(|v| !v.may_change),
                "running members keep their ports"
            );
            assert!(
                export
                    .skipped
                    .iter()
                    .all(|s| s.reason != ExportSkipReason::Redacted),
                "{:#?}",
                export.skipped
            );
            let lines: Vec<String> = expected
                .iter()
                .map(|(name, value)| match format {
                    ExportFormat::Dotenv => format!("{name}={value}"),
                    _ => format!("export {name}='{value}'"),
                })
                .collect();
            assert_eq!(export.text, format!("{}\n", lines.join("\n")));
            check(
                "the reply",
                &serde_json::to_string(&export).expect("encodes"),
            );
            texts.push((format, export.text));
        }
    }

    // The stdout of `stackly env export` is exactly that text.
    let file_arg = path.display().to_string();
    for (format, args) in [
        (
            ExportFormat::Dotenv,
            vec!["env", "export", "--file", file_arg.as_str()],
        ),
        (
            ExportFormat::Shell,
            vec!["env", "export", "shop", "--format", "shell"],
        ),
    ] {
        let (stdout, stderr, code) = stackly(&site, &args);
        assert_eq!(code, Some(0), "{stderr}");
        let text = &texts
            .iter()
            .find(|(f, _)| *f == format)
            .expect("the text")
            .1;
        assert_eq!(&stdout, text, "{args:?}");
        check("stdout", &stdout);
        check("stderr", &stderr);
    }
    let during = logged_since(from);
    check("the log of the export", &during);
    for (name, _) in &expected {
        assert!(
            !during.contains(&format!("{name}=")),
            "a variable was logged"
        );
    }
    drop(api);
    stop(daemon).await;
    let whole = logged_since(0);
    for (_, value) in &sentinels {
        assert!(
            !whole.contains(value.as_str()),
            "the daemon logged a sentinel"
        );
    }
    ev.pass();
}

/// `env.up` with `locked` true and the digest of the plan without it.
async fn up_locked(api: &support::Api, path: &Path, digest: &str) -> Box<ErrorBody> {
    let params = EnvUpParams {
        source: EnvSource::File(path.display().to_string()),
        expected_plan_digest: digest.to_owned(),
        locked: true,
    };
    match api
        .client
        .call_detailed(
            Call::EnvUp(params),
            CallOptions::default().with_idempotency_key(key()),
        )
        .await
    {
        Err(error) => error,
        Ok(reply) => panic!("a locked Up without a matching lock started: {reply:?}"),
    }
}

/// The number of instance and environment rows, and the operations.
fn rows(daemon: &DaemonHandle) -> (usize, usize, usize) {
    let registry = daemon.core().registry().expect("open");
    let instances = registry.known_instance_ids().expect("reads").value.len();
    let environments = registry.environments().expect("reads").value.len();
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
    (instances, environments, operations)
}

// covers: AC-6 (spec 0045, with spec 0046 Up): `env.up` with `locked` true
// stops before an operation exists: without a lock (`env.lock_missing`)
// and with a lock that does not cover a service (`env.lock_stale` per
// alias), and no instance, environment, or operation row is written
#[tokio::test(flavor = "multi_thread")]
async fn a_locked_up_without_a_matching_lock_writes_no_row() {
    let ev = evidence!(
        covers = ["AC-023", "REQ-023"],
        kind = Kind::Fixture,
        expected = "spec 0045 AC-6: env.up with locked true refuses with env.plan_blocked naming env.lock_missing when there is no lock and env.lock_stale for each alias of a lock that covers none of them, and the registry keeps zero instance, environment, and operation rows",
    );
    let site = Site::new();
    let daemon = start(&site).await;
    let api = site.api().await;
    for name in ["db", "api"] {
        set_secret(
            &api,
            &format!("local:strict/{name}-token"),
            "stackly-test-strict-token",
        )
        .await;
    }
    let dir = project(&site);
    let path = write(&dir.join("stackly.yaml"), &shop("strict"));
    let open = plan(&api, &path).await;
    let digest = open.plan_digest.clone().expect("the open plan applies");
    let before = rows(&daemon);
    assert_eq!((before.0, before.1), (0, 0), "an empty registry");

    let codes = |error: &ErrorBody| -> Vec<String> {
        error
            .items()
            .iter()
            .filter_map(|item| item.field("code").map(str::to_owned))
            .collect()
    };
    let missing = up_locked(&api, &path, &digest).await;
    assert_eq!(missing.code(), "env.plan_blocked", "{missing:?}");
    assert_eq!(codes(&missing), ["env.lock_missing"], "{missing:?}");
    assert_eq!(rows(&daemon), before, "nothing was written");

    write(
        &dir.join("stackly.lock"),
        "apiVersion: \"stackly/v1alpha1\"\nkind: \"EnvironmentLock\"\nservices: {}\n",
    );
    let stale = up_locked(&api, &path, &digest).await;
    assert_eq!(stale.code(), "env.plan_blocked", "{stale:?}");
    assert_eq!(
        codes(&stale),
        ["env.lock_stale", "env.lock_stale"],
        "one env.lock_stale per alias: {stale:?}"
    );
    let services: std::collections::BTreeSet<&str> = stale
        .items()
        .iter()
        .filter_map(|item| item.field("service"))
        .collect();
    assert_eq!(services.into_iter().collect::<Vec<_>>(), ["api", "db"]);
    assert_eq!(rows(&daemon), before, "nothing was written");
    stop(daemon).await;
    ev.pass();
}
