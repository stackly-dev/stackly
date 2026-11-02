//! Stop, restart, reload, and force stop over the socket with the real
//! dummy server (spec 0026, AC-1, AC-2, AC-4, AC-6, AC-9 to AC-12): the
//! force reaches only the main process after a stop timed out, a restart
//! runs one new process, a reload keeps the PID, a provider without reload
//! is refused without an operation, and none of it needs the network.

use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use stackly_platform::{Clock, SystemClock};
use stackly_protocol::instance::{InstanceLastExit, InstanceLifecycle};
use stackly_protocol::operation::{ListOperationsParams, OperationState};
use stackly_protocol::{Call, Reply, Timestamp};
use stackly_testkit::evidence::{Kind, evidence};
use stackly_testkit::{FakeCodeSigning, FakeFetcher, SigningCall};
use stacklyd::log_sink::{LogSink, LogSinkConfig};
use stacklyd::map::StartFacts;
use stacklyd::server::{self, DaemonDeps};
use uuid::Uuid;

use crate::support::{Api, Site, alive, edited_fake_package, serving};

async fn operation_count(api: &Api) -> usize {
    match api
        .client
        .call(Call::OperationList(ListOperationsParams::default()))
        .await
    {
        Ok(Reply::OperationList(page)) => page.items.len(),
        other => panic!("operation.list: {other:?}"),
    }
}

/// A copy of the fake package whose stop gives up after 2 seconds.
fn short_stop_package() -> (stackly_testkit::TestRoot, std::path::PathBuf) {
    edited_fake_package(|dir| edit(dir, "stopSeconds: 10", "stopSeconds: 2"))
}

/// A copy of the fake package that cannot reload, like NATS.
fn no_reload_package() -> (stackly_testkit::TestRoot, std::path::PathBuf) {
    edited_fake_package(|dir| {
        edit(dir, "  reload: true", "  reload: false");
        edit(
            dir,
            "  reload:\n    render: [fakeConfig, fakeConf]\n    action: signal\n    signal: hup\n    verify: readiness\n",
            "",
        );
        edit(dir, "  reloadSeconds: 10\n", "");
        let schema = dir.join("schemas/settings.schema.json");
        let text = fs::read_to_string(&schema).expect("the schema reads");
        fs::write(
            &schema,
            text.replace("\"apply\": \"reload\"", "\"apply\": \"restart\""),
        )
        .expect("the schema writes");
    })
}

fn edit(dir: &Path, from: &str, to: &str) {
    let path = dir.join("provider.yaml");
    let text = fs::read_to_string(&path).expect("the manifest reads");
    assert!(text.contains(from), "provider.yaml has no {from:?}");
    fs::write(&path, text.replacen(from, to, 1)).expect("the manifest writes");
}

fn pid_of(summary: &stackly_protocol::instance::InstanceSummary) -> u32 {
    summary.process.as_ref().expect("a process runs").pid
}

async fn wait_gone(pid: u32) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while alive(pid) {
        assert!(tokio::time::Instant::now() < deadline, "{pid} still runs");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

// covers: AC-2, AC-3, AC-4, AC-5
#[tokio::test(flavor = "multi_thread")]
async fn a_force_stop_after_a_stop_timeout_kills_the_main_process_and_reads_forced() {
    let ev = evidence!(
        covers = ["REQ-011", "AC-011", "AC-067"],
        kind = Kind::Fixture,
        expected = "a dummy server that ignores SIGTERM keeps a stop stopping past its 2 second budget; instance.force_stop then ends that one process, the instance reads stopped with lastExit forced, and a force on a running instance is refused without an operation",
    );
    let (_scratch, package) = short_stop_package();
    let site = Site::with_package(&package);
    let _daemon = site.start().await;
    let api = site.api().await;
    let id = api
        .create_ready("forced", json!({ "ignoreTerm": true }))
        .await;

    // A force on a running instance is refused, with no operation.
    let pid = pid_of(&api.start_ready(id).await);
    let before = operation_count(&api).await;
    let refused = api.force_stop(id).await.unwrap_err();
    assert_eq!(
        refused.code(),
        "supervisor.force_requires_stopping",
        "{refused:?}"
    );
    assert_eq!(operation_count(&api).await, before);
    assert!(alive(pid));
    // The stop gives up after 2 seconds: the dummy server ignores SIGTERM.
    assert_eq!(stuck_stopping_running(&api, id).await, pid);
    let summary = api.get(id).await.expect("instance.get");
    assert_eq!(summary.lifecycle, InstanceLifecycle::Stopping);
    assert!(alive(pid), "the term did not end it");

    let accepted = api
        .force_stop(id)
        .await
        .expect("the force is accepted")
        .accepted
        .expect("a force of a stopping instance begins an operation");
    api.wait_state(accepted.operation.id, OperationState::Succeeded)
        .await;
    wait_gone(pid).await;
    let summary = api.get(id).await.expect("instance.get");
    assert_eq!(summary.lifecycle, InstanceLifecycle::Stopped);
    assert_eq!(summary.last_exit, Some(InstanceLastExit::Forced));
    assert!(serving(&site.instance_paths(id).data).is_empty());

    // A force on a stopped instance has nothing to do.
    let again = api.force_stop(id).await.expect("answered");
    assert!(again.already_in_state && again.accepted.is_none());
    ev.pass();
}

// covers: AC-1, AC-5, AC-6
#[tokio::test(flavor = "multi_thread")]
async fn a_restart_runs_one_new_process_and_the_old_one_exits_clean() {
    let ev = evidence!(
        covers = ["REQ-011", "AC-011"],
        kind = Kind::Fixture,
        expected = "instance.restart on a ready instance stops the dummy server with SIGTERM, starts one new process with a new PID, and the summary reads running, desired running, lastExit clean",
    );
    let site = Site::new();
    let _daemon = site.start().await;
    let api = site.api().await;
    let id = api.create_ready("restarted", json!({})).await;
    let first = pid_of(&api.start_ready(id).await);

    let accepted = api
        .restart(id)
        .await
        .expect("the restart is accepted")
        .accepted
        .expect("a restart begins an operation");
    api.wait_state(accepted.operation.id, OperationState::Succeeded)
        .await;

    let summary = api.get(id).await.expect("instance.get");
    let second = pid_of(&summary);
    assert_ne!(second, first);
    assert!(!alive(first), "the old process is gone");
    assert_eq!(summary.lifecycle, InstanceLifecycle::Running);
    assert_eq!(summary.last_exit, Some(InstanceLastExit::Clean));
    assert_eq!(serving(&site.instance_paths(id).data), [second]);
    api.stop_when_free(id).await;
    ev.pass();
}

// covers: AC-7, AC-10, AC-11
#[tokio::test(flavor = "multi_thread")]
async fn a_reload_keeps_the_pid_and_a_stopped_instance_refuses_restart_and_reload() {
    let ev = evidence!(
        covers = ["BR-016", "AC-026"],
        kind = Kind::Fixture,
        expected = "the fake provider declares reload, so the summary carries capabilities.reload true and instance.reload keeps the same PID; restart and reload of a stopped instance are refused with supervisor.instance_not_running and no operation",
    );
    let site = Site::new();
    let _daemon = site.start().await;
    let api = site.api().await;
    let id = api.create_ready("reloaded", json!({})).await;

    let before = operation_count(&api).await;
    let restart = api.restart(id).await.unwrap_err();
    assert_eq!(
        restart.code(),
        "supervisor.instance_not_running",
        "{restart:?}"
    );
    let reload = api.reload(id).await.unwrap_err();
    assert_eq!(
        reload.code(),
        "supervisor.instance_not_running",
        "{reload:?}"
    );
    assert_eq!(operation_count(&api).await, before);

    let summary = api.start_ready(id).await;
    assert!(summary.capabilities.reload);
    let pid = pid_of(&summary);
    let accepted = api
        .reload(id)
        .await
        .expect("the reload is accepted")
        .accepted
        .expect("a reload begins an operation");
    api.wait_state(accepted.operation.id, OperationState::Succeeded)
        .await;
    let after = api.get(id).await.expect("instance.get");
    assert_eq!(pid_of(&after), pid, "a reload never runs the process again");
    api.stop_when_free(id).await;
    ev.pass();
}

// covers: AC-10, AC-11
#[tokio::test(flavor = "multi_thread")]
async fn a_provider_without_reload_is_refused_before_any_operation() {
    let ev = evidence!(
        covers = ["BR-016", "AC-026"],
        kind = Kind::Fixture,
        expected = "a fake package without the reload capability reads capabilities.reload false, and instance.reload on its running instance is refused with supervisor.capability_unsupported and no new operation",
    );
    let (_scratch, package) = no_reload_package();
    let site = Site::with_package(&package);
    let _daemon = site.start().await;
    let api = site.api().await;
    let id = api.create_ready("no-reload", json!({})).await;
    let summary = api.start_ready(id).await;
    assert!(!summary.capabilities.reload);

    let before = operation_count(&api).await;
    let refused = api.reload(id).await.unwrap_err();
    assert_eq!(
        refused.code(),
        "supervisor.capability_unsupported",
        "{refused:?}"
    );
    assert_eq!(operation_count(&api).await, before);
    api.stop_when_free(id).await;
    ev.pass();
}

// covers: AC-9
#[tokio::test(flavor = "multi_thread")]
async fn concurrent_starts_restarts_and_stops_from_two_clients_never_run_two_processes() {
    let ev = evidence!(
        covers = ["REQ-011"],
        kind = Kind::Fixture,
        expected = "two clients send start, restart, and stop at the same time ten times; at no sample are two dummy servers serving the data folder, and a stop while a restart runs answers resource.busy",
    );
    let site = Arc::new(Site::new());
    let _daemon = site.start().await;
    let first = site.api().await;
    let second = site.api().await;
    let id = first.create_ready("serialized", json!({})).await;
    first.start_ready(id).await;
    let data = site.instance_paths(id).data;

    let watching = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let most = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let sampler = {
        let (watching, most, data) = (Arc::clone(&watching), Arc::clone(&most), data.clone());
        tokio::task::spawn_blocking(move || {
            while watching.load(std::sync::atomic::Ordering::SeqCst) {
                let count = serving(&data).len();
                most.fetch_max(count, std::sync::atomic::Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(5));
            }
        })
    };
    let mut busy = 0;
    for _ in 0..10 {
        let (restart, stop, start) =
            tokio::join!(first.restart(id), second.stop(id), second.start(id));
        for answer in [&restart, &stop, &start] {
            if let Err(error) = answer
                && error.code() == "resource.busy"
            {
                busy += 1;
            }
        }
        for accepted in [restart, stop, start].into_iter().flatten() {
            if let Some(accepted) = accepted.accepted {
                first.wait_final(accepted.operation.id).await;
            }
        }
    }
    watching.store(false, std::sync::atomic::Ordering::SeqCst);
    sampler.await.expect("the sampler ends");

    assert!(
        most.load(std::sync::atomic::Ordering::SeqCst) <= 1,
        "two processes served the same data"
    );
    assert!(busy > 0, "a stop during a restart answers resource.busy");
    first.stop_when_free(id).await;
    ev.pass();
}

// covers: AC-12
#[tokio::test(flavor = "multi_thread")]
async fn stop_restart_reload_and_force_need_no_catalog_fetch_or_signature_check() {
    let ev = evidence!(
        covers = ["REQ-046", "AC-046"],
        kind = Kind::Fixture,
        expected = "with an offline fetcher and a recording code signing port, restart, reload, stop, and force stop of the fake instance succeed with zero catalog accesses, zero fetches, and zero signature assessments",
    );
    let (_scratch, package) = short_stop_package();
    let site = Site::with_package(&package);
    let fetcher = FakeFetcher::new();
    fetcher.go_offline();
    let signing = FakeCodeSigning::new();
    let platform = site
        .platform
        .clone()
        .with_fetcher(Arc::new(fetcher.clone()))
        .with_code_signing(Arc::new(signing.clone()));
    let sink = LogSink::start(Box::new(std::io::sink()), LogSinkConfig::DAEMON)
        .expect("the log writer starts");
    let facts = StartFacts::new(
        Uuid::now_v7(),
        Timestamp::from_jiff(SystemClock.now()),
        true,
        std::process::id(),
    );
    let daemon = server::start(
        DaemonDeps::new(platform, site.prepared.clone(), facts, Arc::new(sink))
            .with_config(crate::support::fast()),
    )
    .await
    .expect("the server starts");
    daemon.started().await;
    let api = site.api().await;
    let id = api.create_ready("offline", json!({})).await;
    let stubborn = api
        .create_ready("offline-stubborn", json!({ "ignoreTerm": true }))
        .await;
    api.start_ready(id).await;
    api.start_ready(stubborn).await;
    let before = daemon.core().catalog_accesses();

    for action in ["restart", "reload", "stop"] {
        let accepted = match action {
            "restart" => api.restart(id).await,
            "reload" => api.reload(id).await,
            _ => Ok(api.stop_when_free(id).await),
        }
        .expect("accepted")
        .accepted
        .expect("an operation");
        api.wait_state(accepted.operation.id, OperationState::Succeeded)
            .await;
    }
    let pid = stuck_stopping_running(&api, stubborn).await;
    let force = api
        .force_stop(stubborn)
        .await
        .expect("accepted")
        .accepted
        .expect("an operation");
    api.wait_state(force.operation.id, OperationState::Succeeded)
        .await;
    wait_gone(pid).await;

    assert_eq!(daemon.core().catalog_accesses(), before);
    assert!(fetcher.requests().is_empty(), "{:?}", fetcher.requests());
    assert!(
        signing
            .calls()
            .iter()
            .all(|call| !matches!(call, SigningCall::Assess(_))),
        "{:?}",
        signing.calls()
    );
    ev.pass();
}

/// Lets a stop of the running instance time out; returns its PID.
async fn stuck_stopping_running(api: &Api, id: Uuid) -> u32 {
    let pid = pid_of(&api.get(id).await.expect("instance.get"));
    let stop = api.stop_when_free(id).await.accepted.expect("an operation");
    let detail = api
        .wait_state(stop.operation.id, OperationState::Failed)
        .await;
    assert_eq!(Api::failure_code(&detail), "supervisor.stop_timeout");
    pid
}
