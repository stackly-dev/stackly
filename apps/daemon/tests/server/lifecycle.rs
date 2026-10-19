//! The server's life (spec 0006 AC-11, AC-12, AC-18, AC-19): bind,
//! `starting` during the start slot, recovery mode and the retries before
//! it, the socket check, the stop order, and the context handlers
//! receive.

use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use stackly_ipc::{ConnectPolicy, IpcError, connect_verified};
use stackly_protocol::codes::{daemon, ipc, registry};
use stackly_protocol::daemon::{DaemonHealthStatus, SubsystemStatus, subsystem};
use stackly_protocol::events::ShutdownReason;
use stackly_protocol::handshake::ClientKind;
use stackly_protocol::{Call, Reply};
use stackly_storage::OpenOptions;
use stackly_testkit::evidence::{Kind, evidence};
use stacklyd::connection::recovery_refusal;
use stacklyd::dispatch::{HandlerFuture, Handlers, RequestContext, get_info};
use stacklyd::server::{self, DaemonConfig, StartError};
use stacklyd::state::DaemonCore;

use crate::common::{
    Hold, HoldInfo, Logs, PATIENCE, Raw, RegistryHolder, Site, client, client_as, error_of, request,
};

/// The line the start slot logs before each new try at a locked registry.
const LOCKED_RETRY: &str = "another process holds the registry; trying again";

/// A config whose registry open gives up on a held lock after 50 ms and
/// waits `wait` between tries.
fn locked_config(wait: Duration) -> DaemonConfig {
    let mut config = DaemonConfig::default();
    config.registry_open = OpenOptions::new().with_busy_timeout(Duration::from_millis(50));
    config.registry_locked_retries = vec![wait; 3];
    config
}

/// Writes a schema version newer than the app into the registry header, so
/// the next open reports `schema_newer_than_app`.
fn make_schema_newer(site: &Site) -> std::path::PathBuf {
    let db = site.prepared.layout().registry_db();
    let mut bytes = std::fs::read(&db).expect("the registry file can be read");
    bytes[60..64].copy_from_slice(&999_u32.to_be_bytes());
    std::fs::write(&db, &bytes).expect("the registry file can be written");
    db
}

fn status_of(health: &stackly_protocol::daemon::DaemonHealth, name: &str) -> SubsystemStatus {
    health
        .subsystems
        .iter()
        .find(|subsystem| subsystem.name == name)
        .map(|subsystem| subsystem.status)
        .unwrap_or_else(|| panic!("no subsystem {name}"))
}

#[tokio::test]
async fn health_reports_starting_while_the_start_slot_is_held_then_ready() {
    let site = Site::new();
    let (daemon, gate) = site.start_held().await;
    let (client, _) = client(&site).await;

    let Reply::DaemonGetHealth(health) = client.call(Call::DaemonGetHealth).await.unwrap() else {
        panic!("not a health reply");
    };
    assert_eq!(health.status, DaemonHealthStatus::Starting);
    assert_eq!(
        status_of(&health, subsystem::REGISTRY),
        SubsystemStatus::Unavailable
    );
    assert!(matches!(
        client.call(Call::DaemonGetInfo).await,
        Ok(Reply::DaemonGetInfo(_))
    ));

    gate.send_replace(true);
    daemon.started().await;

    let health = daemon.health();
    assert_eq!(health.status, DaemonHealthStatus::Ready, "{health:?}");
    assert_eq!(status_of(&health, subsystem::REGISTRY), SubsystemStatus::Ok);
    assert_eq!(status_of(&health, subsystem::IPC), SubsystemStatus::Ok);
    assert_eq!(health.recovery, None);
    assert_eq!(
        daemon.info().registry_schema,
        Some(stackly_storage::MigrationSet::builtin().latest())
    );
    assert_eq!(
        status_of(&health, subsystem::OPERATIONS),
        SubsystemStatus::NotBuilt
    );
    let names: Vec<_> = health.subsystems.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, subsystem::ALL);
}

#[tokio::test]
async fn a_registry_in_recovery_keeps_the_daemon_serving_with_a_degraded_health() {
    let ev = evidence!(
        covers = ["REQ-048"],
        kind = Kind::Fixture,
        expected = "a registry newer than the app leaves stacklyd serving in recovery mode with the report in get_health, the file untouched, and no prune",
    );
    let site = Site::new();
    let first = site.start().await;
    first
        .shutdown(ShutdownReason::Requested, std::future::pending())
        .await;
    let db = make_schema_newer(&site);
    let before = std::fs::read(&db).unwrap();

    let logs = Logs::capture();
    let daemon = site.start().await;
    let (client, _) = client(&site).await;

    let health = daemon.health();
    assert_eq!(health.status, DaemonHealthStatus::Degraded);
    let recovery = health
        .recovery
        .clone()
        .expect("get_health carries the report");
    assert_eq!(recovery.reason, "schema_newer_than_app");
    assert_eq!(recovery.db_schema_version, Some(999));
    assert!(recovery.app_schema_version >= 1);
    assert!(
        std::path::Path::new(&recovery.report_path).is_file(),
        "{recovery:?}"
    );
    let Ok(stackly_protocol::Reply::DaemonGetHealth(served)) =
        client.call(Call::DaemonGetHealth).await
    else {
        panic!("not a health reply");
    };
    assert_eq!(served.recovery, Some(recovery));
    let Ok(stackly_protocol::Reply::DaemonGetInfo(info)) = client.call(Call::DaemonGetInfo).await
    else {
        panic!("not an info reply");
    };
    assert_eq!(info.registry_schema, None);
    let registry = health
        .subsystems
        .iter()
        .find(|subsystem| subsystem.name == subsystem::REGISTRY)
        .unwrap();
    assert_eq!(registry.status, SubsystemStatus::Unavailable);
    assert_eq!(
        registry.detail.as_deref(),
        Some("the registry needs recovery (schema_newer_than_app)")
    );
    for call in [
        Call::DaemonGetInfo,
        Call::DaemonGetHealth,
        Call::DaemonGetCapabilities,
    ] {
        let method = call.method();
        assert!(client.call(call).await.is_ok(), "{method}");
    }
    drop(client);
    daemon
        .shutdown(ShutdownReason::Requested, std::future::pending())
        .await;
    assert_eq!(
        std::fs::read(&db).unwrap(),
        before,
        "the registry file changed"
    );
    let lines = logs.lines();
    assert!(
        lines
            .iter()
            .any(|line| line["fields"]["registry_state"] == "recovery"),
        "{lines:?}"
    );
    assert!(
        lines.iter().all(|line| {
            line["fields"]["pruned_rows"].is_null()
                && line["fields"]["message"] != "registry history pruned"
        }),
        "a prune ran in recovery mode: {lines:?}"
    );
    ev.pass();
}

#[tokio::test]
async fn in_recovery_mode_a_method_refused_there_gets_recovery_mode_and_no_handler_runs() {
    let site = Site::new();
    let first = site.start().await;
    first
        .shutdown(ShutdownReason::Requested, std::future::pending())
        .await;
    make_schema_newer(&site);

    let daemon = site.start().await;

    let recovery = daemon
        .health()
        .recovery
        .expect("the daemon is in recovery mode");
    let error = recovery_refusal(daemon.core(), false).expect("a refused method is answered");
    assert!(error.is(&registry::RECOVERY_MODE));
    assert_eq!(error.detail("reason"), Some("schema_newer_than_app"));
    assert_eq!(
        error.detail("reportPath"),
        Some(recovery.report_path.as_str())
    );
    assert!(error.hint().is_some());
    assert_eq!(
        error.category(),
        stackly_protocol::ErrorCategory::Unavailable
    );
    assert!(!error.retryable());
    assert!(recovery_refusal(daemon.core(), true).is_none());
    daemon
        .shutdown(ShutdownReason::Requested, std::future::pending())
        .await;
}

#[tokio::test]
async fn outside_recovery_mode_no_method_is_refused_for_recovery() {
    let site = Site::new();
    let daemon = site.start().await;

    assert!(recovery_refusal(daemon.core(), false).is_none());
    assert!(recovery_refusal(daemon.core(), true).is_none());
    daemon
        .shutdown(ShutdownReason::Requested, std::future::pending())
        .await;
}

#[tokio::test]
async fn a_registry_another_process_holds_is_tried_three_more_times_before_recovery() {
    let ev = evidence!(
        covers = ["REQ-048"],
        kind = Kind::Fixture,
        expected = "while another process holds the registry write lock, the start slot tries the open three more times, then enters recovery mode with reason registry_locked and prunes nothing",
    );
    let site = Site::new();
    let first = site.start().await;
    first
        .shutdown(ShutdownReason::Requested, std::future::pending())
        .await;
    let holder = RegistryHolder::lock(&site.prepared.layout().registry_db());

    let logs = Logs::capture();
    let daemon = site
        .start_configured(locked_config(Duration::from_millis(10)))
        .await;

    let retries = logs.with_message(LOCKED_RETRY);
    assert_eq!(retries.len(), 3, "{:?}", logs.lines());
    let health = daemon.health();
    assert_eq!(health.status, DaemonHealthStatus::Degraded);
    assert_eq!(
        health.recovery.map(|recovery| recovery.reason).as_deref(),
        Some("registry_locked")
    );
    assert!(
        logs.lines()
            .iter()
            .all(|line| line["fields"]["pruned_rows"].is_null()),
        "a prune ran in recovery mode"
    );
    daemon
        .shutdown(ShutdownReason::Requested, std::future::pending())
        .await;
    holder.release();
    ev.pass();
}

#[tokio::test(start_paused = true)]
async fn the_retries_at_a_held_registry_wait_one_two_then_four_seconds() {
    let ev = evidence!(
        covers = ["REQ-048"],
        kind = Kind::Fixture,
        expected = "with the default schedule, the start slot waits 1, 2, then 4 seconds between its tries at a held registry before recovery mode",
    );
    let site = Site::new();
    let first = site.start().await;
    first
        .shutdown(ShutdownReason::Requested, std::future::pending())
        .await;
    let holder = RegistryHolder::lock(&site.prepared.layout().registry_db());

    let logs = Logs::capture();
    let mut config = DaemonConfig::default();
    config.registry_open = OpenOptions::new().with_busy_timeout(Duration::from_millis(50));
    let begun = tokio::time::Instant::now();
    let daemon = site.start_configured(config).await;
    let waited = begun.elapsed();

    let waits: Vec<_> = logs
        .with_message(LOCKED_RETRY)
        .iter()
        .map(|line| line["fields"]["retry_in_seconds"].as_u64())
        .collect();
    assert_eq!(waits, [Some(1), Some(2), Some(4)], "{:?}", logs.lines());
    assert!(waited >= Duration::from_secs(7), "{waited:?}");
    assert_eq!(
        daemon
            .health()
            .recovery
            .map(|recovery| recovery.reason)
            .as_deref(),
        Some("registry_locked")
    );
    daemon
        .shutdown(ShutdownReason::Requested, std::future::pending())
        .await;
    holder.release();
    ev.pass();
}

#[tokio::test]
async fn a_registry_lock_released_during_the_retries_opens_ready_and_prunes_once() {
    let site = Site::new();
    let first = site.start().await;
    first
        .shutdown(ShutdownReason::Requested, std::future::pending())
        .await;
    let holder = RegistryHolder::lock(&site.prepared.layout().registry_db());

    let logs = Logs::capture();
    let mut config = locked_config(Duration::from_millis(300));
    let (gate, held) = tokio::sync::watch::channel(false);
    config.start_gate = Some(held);
    let daemon = server::start(site.deps().with_config(config))
        .await
        .expect("the server starts");
    gate.send_replace(true);
    let deadline = Instant::now() + PATIENCE;
    while logs.with_message(LOCKED_RETRY).is_empty() {
        assert!(Instant::now() < deadline, "the start slot never retried");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    holder.release();
    daemon.started().await;

    let health = daemon.health();
    assert_eq!(health.status, DaemonHealthStatus::Ready, "{health:?}");
    assert_eq!(health.recovery, None);
    let retries = logs.with_message(LOCKED_RETRY).len();
    assert!((1..=3).contains(&retries), "{retries} retries");
    let ready = logs.with_message("registry ready");
    assert_eq!(ready.len(), 1, "{:?}", logs.lines());
    assert!(ready[0]["fields"]["pruned_rows"].is_number(), "{ready:?}");
    daemon
        .shutdown(ShutdownReason::Requested, std::future::pending())
        .await;
}

#[tokio::test]
async fn a_file_at_the_socket_path_stops_the_start_and_stays() {
    let site = Site::new();
    std::fs::write(site.socket(), b"keep").unwrap();

    let error = server::start(site.deps()).await.unwrap_err();

    assert!(
        matches!(error, StartError::Bind(IpcError::EndpointOccupied { .. })),
        "{error}"
    );
    assert_eq!(error.code().as_deref(), Some(ipc::ENDPOINT_OCCUPIED.code));
    assert_eq!(std::fs::read(site.socket()).unwrap(), b"keep");
}

#[tokio::test]
async fn a_stale_socket_is_replaced_and_the_final_path_is_only_ever_mode_0600() {
    let ev = evidence!(
        covers = ["REQ-047"],
        kind = Kind::Fixture,
        expected = "a stale socket at the path is replaced, and polling lstat during the start never sees the final path with a mode other than 0600",
    );
    let site = Site::new();
    drop(std::os::unix::net::UnixListener::bind(site.socket()).unwrap());
    assert!(site.socket().exists());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let watcher = {
        let socket = site.socket();
        let seen = Arc::clone(&seen);
        std::thread::spawn(move || {
            let until = Instant::now() + Duration::from_secs(2);
            while Instant::now() < until {
                if let Ok(metadata) = std::fs::symlink_metadata(&socket) {
                    seen.lock()
                        .unwrap()
                        .push((metadata.ino(), metadata.mode() & 0o7777));
                }
            }
        })
    };

    let daemon = site.start().await;
    let (client, _) = client(&site).await;
    drop(client);
    watcher.join().unwrap();

    let final_inode = std::fs::symlink_metadata(site.socket()).unwrap().ino();
    let seen = seen.lock().unwrap();
    let modes: Vec<_> = seen
        .iter()
        .filter(|(inode, _)| *inode == final_inode)
        .map(|(_, mode)| *mode)
        .collect();
    assert!(!modes.is_empty());
    assert!(modes.iter().all(|mode| *mode == 0o600), "{modes:?}");
    let mode = std::fs::metadata(site.socket())
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600);
    drop(daemon);
    ev.pass();
}

#[tokio::test]
async fn a_socket_and_runtime_root_removed_while_serving_come_back_at_the_next_check() {
    let site = Site::new();
    let mut config = DaemonConfig::default();
    config.socket_check_interval = Duration::from_millis(100);
    let daemon = server::start(site.deps().with_config(config))
        .await
        .unwrap();
    daemon.started().await;
    let runtime = site.prepared.layout().roots().runtime().to_path_buf();

    std::fs::remove_dir_all(&runtime).unwrap();

    let deadline = Instant::now() + PATIENCE;
    loop {
        if connect_verified(&site.socket(), ConnectPolicy::same_user(site.uid()))
            .await
            .is_ok()
        {
            break;
        }
        assert!(Instant::now() < deadline, "the socket did not come back");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(daemon.health().counters.socket_rebinds.get(), 1);
    let mode = std::fs::metadata(&runtime).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o700);
    assert_eq!(daemon.health().status, DaemonHealthStatus::Ready);
}

#[tokio::test]
async fn the_stop_unlinks_first_tells_subscribers_refuses_new_requests_and_drains() {
    let ev = evidence!(
        covers = ["NFR-004"],
        kind = Kind::Fixture,
        expected = "on stop with a held request: a new client gets ipc.endpoint_missing at once, a subscriber gets daemon.shutting_down, a new request gets daemon.shutting_down, and the stop ends within 7 seconds",
    );
    let site = Site::new();
    let hold = Hold::new();
    let daemon = site.start_with(HoldInfo(hold.clone())).await;
    let mut subscriber = Raw::ready(&site.socket()).await;
    subscriber
        .send(&request(
            "sub",
            "events.subscribe",
            Some(serde_json::json!({})),
        ))
        .await;
    assert_eq!(subscriber.answer("sub").await["type"], "response");
    let mut busy = Raw::ready(&site.socket()).await;
    busy.send(&request("held", "daemon.get_info", None)).await;
    hold.wait_entered(1).await;
    let socket = site.socket();

    let started = Instant::now();
    let stop = tokio::spawn(daemon.shutdown(ShutdownReason::Signal, std::future::pending()));

    let deadline = Instant::now() + PATIENCE;
    while socket.exists() {
        assert!(Instant::now() < deadline, "the socket was not removed");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let refused = connect_verified(&socket, ConnectPolicy::same_user(site.uid()))
        .await
        .unwrap_err();
    assert!(
        refused.to_error_body().is(&ipc::ENDPOINT_MISSING),
        "{refused}"
    );
    let event = subscriber.recv().await.expect("the shutdown event arrives");
    assert_eq!(event["content"]["kind"]["type"], "daemon.shutting_down");
    assert_eq!(event["content"]["kind"]["content"]["reason"], "signal");
    busy.send(&request("late", "daemon.get_health", None)).await;
    let error = error_of(&busy.answer("late").await);
    assert!(error.is(&daemon::SHUTTING_DOWN));

    let stopped = stop.await.unwrap();
    assert!(!stopped.forced);
    assert!(
        started.elapsed() < Duration::from_secs(7),
        "{:?}",
        started.elapsed()
    );
    subscriber.expect_end().await;
    hold.release();
    ev.pass();
}

#[tokio::test]
async fn a_second_stop_request_skips_the_drain() {
    let site = Site::new();
    let hold = Hold::new();
    let daemon = site.start_with(HoldInfo(hold.clone())).await;
    let mut busy = Raw::ready(&site.socket()).await;
    busy.send(&request("held", "daemon.get_info", None)).await;
    hold.wait_entered(1).await;

    let started = Instant::now();
    let stopped = daemon
        .shutdown(
            ShutdownReason::Signal,
            tokio::time::sleep(Duration::from_millis(200)),
        )
        .await;

    assert!(stopped.forced);
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "{:?}",
        started.elapsed()
    );
    hold.release();
}

/// Records the context `daemon.get_info` receives.
#[derive(Clone, Default)]
struct RecordContext(Arc<Mutex<Option<RequestContext>>>);

impl Handlers for RecordContext {
    fn get_info<'a>(
        &'a self,
        core: &'a DaemonCore,
        context: &'a RequestContext,
    ) -> HandlerFuture<'a, stackly_protocol::daemon::DaemonInfo> {
        *self
            .0
            .lock()
            .expect("recorded request context lock is not poisoned") = Some(context.clone());
        get_info(core)
    }
}

#[tokio::test]
async fn handlers_receive_the_peer_and_the_client_of_their_connection() {
    let site = Site::new();
    let record = RecordContext::default();
    let logs = Logs::capture();
    let _daemon = site.start_with(record.clone()).await;

    // A client from before `interactive`: it never sends the flag.
    let (older, _) = client(&site).await;
    older.call(Call::DaemonGetInfo).await.unwrap();
    let context = record.0.lock().unwrap().clone().expect("the handler ran");
    assert_eq!(context.peer.uid, site.uid());
    assert_eq!(context.peer.pid, Some(std::process::id()));
    assert_eq!(context.client.kind, ClientKind::Cli);
    assert_eq!(context.client.interactive, None);
    assert!(!context.client.is_interactive());
    assert!(context.connection_id >= 1);
    assert!(!context.request_id.is_empty());
    let older_request = context.request_id.clone();

    // The GUI always says it can show a dialog.
    let (gui, _) = client_as(&site, ClientKind::Gui, Some(true)).await;
    gui.call(Call::DaemonGetInfo).await.unwrap();
    let context = record.0.lock().unwrap().clone().expect("the handler ran");
    assert_eq!(context.client.kind, ClientKind::Gui);
    assert!(context.client.is_interactive());
    let gui_request = context.request_id.clone();

    let lines = logs.with_message("request");
    let line_of = |request_id: &str| {
        lines
            .iter()
            .find(|line| {
                line["fields"]["request_id"] == request_id
                    && line["fields"]["method"] == "daemon.get_info"
            })
            .unwrap_or_else(|| panic!("no request line for {request_id}: {lines:?}"))
            .clone()
    };
    let older_line = line_of(&older_request);
    assert_eq!(older_line["fields"]["client_kind"], "cli");
    assert_eq!(older_line["fields"]["interactive"], false);
    let gui_line = line_of(&gui_request);
    assert_eq!(gui_line["fields"]["client_kind"], "gui");
    assert_eq!(gui_line["fields"]["interactive"], true);

    let handshakes = logs.with_message("handshake accepted");
    let flags: Vec<_> = handshakes
        .iter()
        .map(|line| {
            (
                line["fields"]["client_kind"].clone(),
                line["fields"]["interactive"].clone(),
            )
        })
        .collect();
    assert!(
        flags.contains(&(serde_json::json!("cli"), serde_json::json!(false))),
        "{flags:?}"
    );
    assert!(
        flags.contains(&(serde_json::json!("gui"), serde_json::json!(true))),
        "{flags:?}"
    );
    let handshake_requests: Vec<_> = lines
        .iter()
        .filter(|line| line["fields"]["method"] == "daemon.handshake")
        .map(|line| line["fields"]["interactive"].clone())
        .collect();
    assert_eq!(handshake_requests.len(), 2, "{lines:?}");
}

/// Waits up to [`PATIENCE`] until `ready` holds.
async fn eventually(what: &str, mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + PATIENCE;
    while !ready() {
        assert!(Instant::now() < deadline, "{what} did not happen in time");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

/// A folder made unreadable for one test, readable again on drop so the
/// test root can still be removed after a failed assert.
struct Unreadable(std::path::PathBuf);

impl Unreadable {
    fn new(dir: std::path::PathBuf) -> Self {
        std::fs::create_dir_all(&dir).expect("the folder can be created");
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o000))
            .expect("the folder's mode can be changed");
        Self(dir)
    }
}

impl Drop for Unreadable {
    fn drop(&mut self) {
        let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o700));
    }
}

#[tokio::test]
async fn the_registry_history_is_pruned_again_at_every_interval() {
    let ev = evidence!(
        covers = ["REQ-048"],
        kind = Kind::Fixture,
        expected = "after the prune at start, the prune task prunes the registry again at every prune interval",
    );
    let site = Site::new();
    let logs = Logs::capture();
    let mut config = DaemonConfig::default();
    config.prune_interval = Duration::from_millis(20);

    let daemon = site.start_configured(config).await;

    eventually("two periodic prunes", || {
        logs.with_message("registry history pruned").len() >= 2
    })
    .await;
    let pruned = logs.with_message("registry history pruned");
    assert!(pruned[0]["fields"]["pruned_rows"].is_number(), "{pruned:?}");
    assert_eq!(logs.with_message("registry ready").len(), 1);
    daemon
        .shutdown(ShutdownReason::Requested, std::future::pending())
        .await;
    ev.pass();
}

#[tokio::test]
async fn a_registry_that_cannot_be_opened_fails_the_daemon_with_exit_1() {
    let ev = evidence!(
        covers = ["REQ-048"],
        kind = Kind::Fixture,
        expected = "when the start slot cannot open the registry at all, the daemon reports the fatal exit code 1, health shows the registry unavailable, and the usual stop still runs",
    );
    let site = Site::new();
    let unreadable = Unreadable::new(site.prepared.layout().registry_dir());
    let logs = Logs::capture();

    let daemon = site.start().await;
    let code = tokio::time::timeout(PATIENCE, daemon.fatal())
        .await
        .expect("the start slot reports the failure");
    drop(unreadable);

    assert_eq!(code, 1);
    assert_eq!(
        status_of(&daemon.health(), subsystem::REGISTRY),
        SubsystemStatus::Unavailable
    );
    assert_eq!(
        logs.with_message("cannot open the registry").len(),
        1,
        "{:?}",
        logs.lines()
    );
    daemon
        .shutdown(ShutdownReason::Requested, std::future::pending())
        .await;
    assert!(!site.socket().exists(), "the stop removes the socket");
    ev.pass();
}

#[tokio::test]
async fn a_file_at_the_socket_path_makes_ipc_unavailable_until_the_path_is_free() {
    let ev = evidence!(
        covers = ["REQ-047", "NFR-004"],
        kind = Kind::Fixture,
        expected = "a regular file in place of the socket makes subsystem ipc unavailable and the daemon degraded at the next check, and once the file is gone the socket is bound again and clients connect",
    );
    let site = Site::new();
    let mut config = DaemonConfig::default();
    config.socket_check_interval = Duration::from_millis(50);
    let daemon = site.start_configured(config).await;
    std::fs::remove_file(site.socket()).unwrap();
    std::fs::write(site.socket(), b"not a socket").unwrap();

    eventually("ipc unavailable", || {
        status_of(&daemon.health(), subsystem::IPC) == SubsystemStatus::Unavailable
    })
    .await;
    let health = daemon.health();
    assert_eq!(health.status, DaemonHealthStatus::Degraded);
    assert_eq!(health.counters.socket_rebinds.get(), 0);
    assert!(
        std::fs::read(site.socket()).unwrap() == b"not a socket",
        "the daemon never removes a file it did not make"
    );

    std::fs::remove_file(site.socket()).unwrap();
    eventually("ipc ok again", || {
        status_of(&daemon.health(), subsystem::IPC) == SubsystemStatus::Ok
    })
    .await;
    let health = daemon.health();
    assert_eq!(health.status, DaemonHealthStatus::Ready);
    assert_eq!(health.counters.socket_rebinds.get(), 1);
    let (client, _) = client(&site).await;
    assert!(client.call(Call::DaemonGetInfo).await.is_ok());
    drop(client);
    daemon
        .shutdown(ShutdownReason::Requested, std::future::pending())
        .await;
    ev.pass();
}

#[tokio::test]
async fn a_socket_that_replaced_ours_is_replaced_back_at_the_next_check() {
    let site = Site::new();
    let logs = Logs::capture();
    let mut config = DaemonConfig::default();
    config.socket_check_interval = Duration::from_millis(50);
    let daemon = site.start_configured(config).await;
    std::fs::remove_file(site.socket()).unwrap();
    let other = std::os::unix::net::UnixListener::bind(site.socket()).unwrap();
    other.set_nonblocking(true).unwrap();

    eventually("a rebind", || {
        daemon.health().counters.socket_rebinds.get() == 1
    })
    .await;
    let (client, _) = client(&site).await;
    assert!(client.call(Call::DaemonGetInfo).await.is_ok());

    assert!(other.accept().is_err(), "the other socket got the client");
    let rebound = logs.with_message("socket_rebound: the socket was bound again");
    assert_eq!(rebound.len(), 1, "{:?}", logs.lines());
    assert_eq!(rebound[0]["fields"]["was"], "replaced");
    drop(client);
    daemon
        .shutdown(ShutdownReason::Requested, std::future::pending())
        .await;
}

#[tokio::test]
async fn a_peer_of_another_uid_is_refused_counted_and_logged_with_its_uid_and_pid() {
    let ev = evidence!(
        covers = ["REQ-047", "AC-047"],
        kind = Kind::Fixture,
        expected = "with peers expected to have another uid, the daemon refuses a real client with ipc.peer_rejected before any request, counts it in connectionsRejected, and logs the peer's uid and pid",
    );
    let site = Site::new();
    let logs = Logs::capture();
    let mut config = DaemonConfig::default();
    config.peer_uid = Some(site.uid() + 1);
    let daemon = site.start_configured(config).await;

    let mut raw = Raw::connect(&site.socket()).await;
    let frame = raw.recv().await.expect("the refusal arrives");

    let error = error_of(&frame);
    assert!(error.is(&ipc::PEER_REJECTED), "{frame}");
    assert!(frame["content"]["requestId"].is_null(), "{frame}");
    raw.expect_end().await;
    eventually("the refusal is counted", || {
        daemon.health().counters.connections_rejected.get() == 1
    })
    .await;
    assert_eq!(daemon.health().counters.connections_open.get(), 0);
    let refused = logs.with_message(
        "a peer of another user, or one whose credentials cannot be read, was refused",
    );
    assert_eq!(refused.len(), 1, "{:?}", logs.lines());
    assert_eq!(refused[0]["fields"]["peer_uid"], site.uid());
    assert_eq!(refused[0]["fields"]["peer_pid"], std::process::id());
    daemon
        .shutdown(ShutdownReason::Requested, std::future::pending())
        .await;
    ev.pass();
}
