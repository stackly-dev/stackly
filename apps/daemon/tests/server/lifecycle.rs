//! The server's life (spec 0006 AC-11, AC-12, AC-18 in part): bind,
//! `starting` during the start slot, recovery mode, the socket check, the
//! stop order, and the context handlers receive.

use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use stackly_ipc::{ConnectPolicy, IpcError, connect_verified};
use stackly_protocol::codes::{daemon, ipc};
use stackly_protocol::daemon::{HealthStatus, SubsystemStatus, subsystem};
use stackly_protocol::events::ShutdownReason;
use stackly_protocol::{Call, Reply};
use stackly_testkit::evidence::{Kind, evidence};
use stacklyd::dispatch::{HandlerFuture, Handlers, RequestContext, get_info};
use stacklyd::server::{self, DaemonConfig, StartError};
use stacklyd::state::DaemonCore;

use crate::common::{Hold, HoldInfo, PATIENCE, Raw, Site, client, error_of, request};

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
    assert_eq!(health.status, HealthStatus::Starting);
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
    assert_eq!(health.status, HealthStatus::Ready, "{health:?}");
    assert_eq!(status_of(&health, subsystem::REGISTRY), SubsystemStatus::Ok);
    assert_eq!(status_of(&health, subsystem::IPC), SubsystemStatus::Ok);
    assert_eq!(
        status_of(&health, subsystem::OPERATIONS),
        SubsystemStatus::NotBuilt
    );
    let names: Vec<_> = health.subsystems.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, subsystem::ALL);
}

#[tokio::test]
async fn a_registry_in_recovery_keeps_the_daemon_serving_with_a_degraded_health() {
    let site = Site::new();
    let first = site.start().await;
    first
        .shutdown(ShutdownReason::Requested, std::future::pending())
        .await;
    let db = site.prepared.layout().registry_db();
    let mut bytes = std::fs::read(&db).unwrap();
    bytes[60..64].copy_from_slice(&999_u32.to_be_bytes());
    std::fs::write(&db, &bytes).unwrap();
    let before = std::fs::read(&db).unwrap();

    let daemon = site.start().await;
    let (client, _) = client(&site).await;

    let health = daemon.health();
    assert_eq!(health.status, HealthStatus::Degraded);
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
    assert_eq!(daemon.health().status, HealthStatus::Ready);
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
        *self.0.lock().unwrap() = Some(context.clone());
        get_info(core)
    }
}

#[tokio::test]
async fn handlers_receive_the_peer_and_the_client_of_their_connection() {
    let site = Site::new();
    let record = RecordContext::default();
    let _daemon = site.start_with(record.clone()).await;
    let (client, _) = client(&site).await;

    client.call(Call::DaemonGetInfo).await.unwrap();

    let context = record.0.lock().unwrap().clone().expect("the handler ran");
    assert_eq!(context.peer.uid, site.uid());
    assert_eq!(context.peer.pid, Some(std::process::id()));
    assert_eq!(
        context.client.kind,
        stackly_protocol::handshake::ClientKind::Cli
    );
    assert!(context.connection_id >= 1);
    assert!(!context.request_id.is_empty());
}
