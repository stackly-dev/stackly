//! The happy path of a MySQL instance through the daemon (spec 0042 AC-13):
//! create with a generated root password, init, start to ready, the
//! connection string, a row kept over a stop and a start, a restart with a
//! new process, a config edit that restarts, the binary log setting, and
//! stop.

use std::time::Duration;

use serde_json::json;
use stackly_protocol::instance::ConnectionAvailability;
use stackly_protocol::instance::{InstanceDesired, InstanceLifecycle, InstanceReadinessState};
use stackly_protocol::operation::OperationState;
use stackly_testkit::evidence::{Evidence, Kind, evidence};
use uuid::Uuid;

use crate::config_calls::{Cfg, pid_of, settings_with};
use crate::harness::{
    MY84, My, Site, Via, binlog_files, create_generated, create_with, stop_and_shutdown,
};
use crate::support::Api;

/// Starts `id` and returns every readiness state seen until it is ready.
async fn start_watching_readiness(api: &Api, id: Uuid) -> Vec<InstanceReadinessState> {
    let accepted = api
        .start(id)
        .await
        .expect("the start is accepted")
        .accepted
        .expect("a start starts an operation");
    let mut seen = Vec::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(120);
    loop {
        let summary = api.get(id).await.expect("the instance can be read");
        if summary.lifecycle == InstanceLifecycle::Running
            && seen.last() != Some(&summary.readiness.state)
        {
            seen.push(summary.readiness.state);
        }
        if summary.readiness.state == InstanceReadinessState::Ready {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "not ready in time: {summary:?}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    api.wait_state(accepted.operation.id, OperationState::Succeeded)
        .await;
    seen
}

async fn is_created_started_served_restarted_and_stopped_through_the_daemon(my: My, ev: Evidence) {
    let Some(site) = Site::open(&ev, my) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let cfg = Cfg { api: &api };

    let made = create_generated(&site, &api, &format!("my{}", my.line.replace('.', ""))).await;
    let paths = site.instance_paths(made.id);
    let manifest: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(&paths.manifest).expect("the manifest is written"),
    )
    .expect("the manifest is JSON");
    assert_eq!(manifest["data"]["initializedLine"], my.line);
    assert_eq!(manifest["runtime"]["line"], my.line);
    let created = api.get(made.id).await.expect("the instance can be read");
    assert_eq!(created.lifecycle, InstanceLifecycle::Stopped);
    assert_eq!(created.initialized, Some(true));

    let seen = start_watching_readiness(&api, made.id).await;
    assert_eq!(
        seen.first(),
        Some(&InstanceReadinessState::NotReady),
        "running starts not ready: {seen:?}"
    );
    assert_eq!(seen.last(), Some(&InstanceReadinessState::Ready));
    let running = api.get(made.id).await.expect("the instance can be read");
    assert_eq!(running.lifecycle, InstanceLifecycle::Running);
    assert_eq!(running.desired, InstanceDesired::Running);
    let first_pid = pid_of(&running);

    let mysql = site.mysql(made.id, made.port);
    let version = mysql.scalar(&made.password, "SELECT VERSION()");
    assert_eq!(version, my.version);
    site.subject(&ev, &version);
    for via in [Via::Socket, Via::Tcp] {
        let ran = mysql.run(via, "root", Some(&made.password), "SELECT 1");
        assert!(ran.ok, "{via:?}: {ran:?}");
    }
    assert_eq!(mysql.scalar(&made.password, "SELECT @@log_bin"), "0");

    let detail = api.detail(made.id).await.expect("the detail can be read");
    let [uri] = detail.connections.as_slice() else {
        panic!("one connection entry: {:?}", detail.connections);
    };
    assert_eq!(uri.name, "uri");
    assert_eq!(uri.state, ConnectionAvailability::Available);
    let masked = uri.masked.as_deref().expect("an available entry is masked");
    assert!(
        masked.starts_with("mysql://root:")
            && masked.ends_with(&format!("@127.0.0.1:{}/", made.port)),
        "{masked}"
    );
    assert!(!masked.contains(&made.password), "{masked}");

    let wrote = mysql.admin(
        &made.password,
        "CREATE DATABASE kept; CREATE TABLE kept.notes (note text); INSERT INTO kept.notes VALUES ('before the stop')",
    );
    assert!(wrote.ok, "{wrote:?}");
    let accepted = api
        .stop_when_free(made.id)
        .await
        .accepted
        .expect("a stop starts an operation");
    api.wait_state(accepted.operation.id, OperationState::Succeeded)
        .await;
    let stopped = api.get(made.id).await.expect("the instance can be read");
    assert_eq!(stopped.lifecycle, InstanceLifecycle::Stopped);
    assert!(stopped.process.is_none());

    let running = api.start_ready(made.id).await;
    assert_eq!(
        mysql.scalar(&made.password, "SELECT note FROM kept.notes"),
        "before the stop"
    );
    let second_pid = pid_of(&running);
    assert_ne!(second_pid, first_pid);

    let accepted = api
        .restart(made.id)
        .await
        .expect("the restart is accepted")
        .accepted
        .expect("a restart starts an operation");
    api.wait_state(accepted.operation.id, OperationState::Succeeded)
        .await;
    let restarted = api
        .wait_instance(made.id, "ready after the restart", |summary| {
            summary.readiness.state == InstanceReadinessState::Ready
        })
        .await;
    let third_pid = pid_of(&restarted);
    assert_ne!(third_pid, second_pid);

    // maxConnections through the config edit of spec 0029: a restart.
    let view = cfg
        .wait_view(made.id, "revision 1 as last known good", |view| {
            view.last_known_good.map(|n| n.get()) == Some(1)
        })
        .await;
    let change = settings_with(&view, json!({ "maxConnections": 77 }));
    cfg.apply_done(made.id, view.revision.number.get(), change)
        .await;
    let edited = api
        .wait_instance(made.id, "a new mysqld on the new settings", |summary| {
            summary
                .process
                .as_ref()
                .is_some_and(|process| process.pid != third_pid)
                && summary.readiness.state == InstanceReadinessState::Ready
        })
        .await;
    assert_ne!(pid_of(&edited), third_pid);
    assert_eq!(
        mysql.scalar(&made.password, "SELECT @@max_connections"),
        "77"
    );
    assert_eq!(
        mysql.scalar(&made.password, "SELECT note FROM kept.notes"),
        "before the stop"
    );

    stop_and_shutdown(&api, daemon, made.id).await;
    ev.pass_with(&format!(
        "MySQL {version} created with a generated password, ready after not_ready, kept its row, restarted on pids {first_pid}, {second_pid}, {third_pid}, and took max_connections 77"
    ));
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed MySQL 8.4 artifact in the runtime cache"]
async fn mysql_8_4_is_created_started_served_restarted_and_stopped_through_the_daemon() {
    let my = MY84;
    let ev = evidence!(
        covers = ["REQ-055"],
        kind = Kind::Native,
        expected = &format!(
            "a MySQL {} instance is created with a generated rootPassword, initialized with data.initializedLine {}, starts running not_ready then ready, shows one available uri connection whose masked value is an absolute URI, keeps a row over a stop and a start, gets a new pid on restart, takes maxConnections 77 through the config edit and a restart, and stops",
            my.line, my.line
        ),
    );
    is_created_started_served_restarted_and_stopped_through_the_daemon(my, ev).await;
}

async fn writes_a_binary_log_only_when_the_setting_is_on(my: My, ev: Evidence) {
    let Some(site) = Site::open(&ev, my) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let password = crate::harness::new_password();
    let made = create_with(
        &site,
        &api,
        "binlog",
        &password,
        json!({ "binaryLog": true }),
    )
    .await;
    let paths = site.instance_paths(made.id);
    assert!(
        !binlog_files(&paths.data),
        "init writes no binary log even with the setting on"
    );
    api.start_ready(made.id).await;
    let mysql = site.mysql(made.id, made.port);
    let version = mysql.scalar(&made.password, "SELECT VERSION()");
    site.subject(&ev, &version);
    assert_eq!(mysql.scalar(&made.password, "SELECT @@log_bin"), "1");
    assert!(binlog_files(&paths.data), "the server writes binlog.NNNNNN");
    stop_and_shutdown(&api, daemon, made.id).await;
    ev.pass_with("@@log_bin is 1 and binlog files exist with binaryLog true");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed MySQL 8.4 artifact in the runtime cache"]
async fn mysql_8_4_writes_a_binary_log_only_when_the_setting_is_on() {
    let ev = evidence!(
        covers = ["REQ-055"],
        kind = Kind::Native,
        expected = "with binaryLog true the data folder holds no binlog file after init, and the running server has @@log_bin 1 and writes binlog.NNNNNN",
    );
    writes_a_binary_log_only_when_the_setting_is_on(MY84, ev).await;
}
