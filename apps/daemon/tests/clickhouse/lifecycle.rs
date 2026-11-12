//! The happy path of a ClickHouse instance through the daemon in process
//! (spec 0043 AC-5, AC-7, AC-9, AC-11): create, start with `not_ready`
//! before `ready` through the identity query, health on `/ping`, the two
//! connections and open actions of `instance.get_detail`, a stop with
//! `SIGTERM` and exit 0, and a second start on the same data. Also the
//! process contract: one foreground server, no child, the instance lock on
//! fd 3, and the same program and PID until the stop.

use std::time::Duration;

use serde_json::json;
use stackly_protocol::events::SubscribeParams;
use stackly_protocol::instance::{
    ConnectionAvailability, InstanceDesired, InstanceHealthState, InstanceLifecycle,
    InstanceReadinessState,
};
use stackly_testkit::evidence::{Kind, evidence};

use crate::harness::{
    Site, VERSION, alive, assert_not_ready_before_ready, assert_two_listeners, children,
    command_line, create_instance, drain_readiness, eventually, http_get, output_of, pid_of,
    port_of, stop_instance, wait_log,
};

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed ClickHouse 26.8.30.8 artifact in the runtime cache"]
async fn clickhouse_lifecycle_in_process() {
    let ev = evidence!(
        covers = ["REQ-037"],
        kind = Kind::Native,
        expected = "a ClickHouse 26.8.30.8 instance with both endpoints on Auto in a data root with a space and Unicode in its name is created, starts, shows not_ready before ready through the identity query, turns healthy on /ping, shows the http and native connections masked and the play and client open actions, stops on SIGTERM with exit 0 and no process left, and starts again on the same data with the row it wrote still there",
    );
    let Some(site) = Site::open(&ev) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let (_, mut events) = api
        .client
        .subscribe(SubscribeParams::default())
        .await
        .expect("the subscription is accepted");

    let made = create_instance(&site, &api, "lifecycle", json!({})).await;
    let created = api.get(made.id).await.expect("the instance can be read");
    assert_eq!(created.lifecycle, InstanceLifecycle::Stopped);
    assert_eq!(created.initialized, Some(true));
    let paths = site.instance_paths(made.id);
    assert!(
        !paths.config.join("config.xml").exists(),
        "config.xml is written by the first start, not by the init"
    );

    let running = api.start_ready(made.id).await;
    assert_eq!(running.lifecycle, InstanceLifecycle::Running);
    assert_eq!(running.desired, InstanceDesired::Running);
    assert_eq!(running.readiness.state, InstanceReadinessState::Ready);
    assert_eq!(running.readiness.reason, "probe_ok");
    let seen = drain_readiness(&mut events, made.id).await;
    assert_not_ready_before_ready(&seen);
    api.wait_instance(made.id, "healthy", |summary| {
        summary.health.state == InstanceHealthState::Healthy
    })
    .await;

    let http = port_of(&running, "http");
    let native = port_of(&running, "native");
    let pid = pid_of(&running);
    assert_two_listeners(pid, http, native);
    assert_eq!(http_get(http, "/ping"), (200, "Ok.\n".to_owned()));
    let version = site.version(http, &made.password);
    assert_eq!(version, VERSION);
    site.subject(&ev, &version);
    assert_eq!(
        site.sql_ok(
            native,
            "default",
            Some(&made.password),
            "SELECT getMacro('stackly_instance_id')"
        ),
        made.id.to_string()
    );
    site.exec(
        native,
        "default",
        Some(&made.password),
        "CREATE TABLE default.kept (id UInt64) ENGINE = MergeTree ORDER BY id",
    );
    site.exec(
        native,
        "default",
        Some(&made.password),
        "INSERT INTO default.kept VALUES (42)",
    );
    let ready_lines = wait_log(
        &api,
        made.id,
        "Ready for connections",
        Duration::from_secs(10),
    )
    .await;
    assert!(!ready_lines.is_empty());

    let detail = api.detail(made.id).await.expect("the detail can be read");
    let names: Vec<&str> = detail
        .connections
        .iter()
        .map(|entry| entry.name.as_str())
        .collect();
    assert_eq!(names, ["http", "native"], "http is the main connection");
    for entry in &detail.connections {
        assert_eq!(entry.state, ConnectionAvailability::Available, "{entry:?}");
        let masked = entry.masked.as_deref().expect("a masked text");
        assert!(!masked.contains(&made.password), "{masked}");
        assert!(masked.contains("default:****@127.0.0.1:"), "{masked}");
    }
    assert_eq!(
        detail.connections[0].masked.as_deref(),
        Some(format!("http://default:****@127.0.0.1:{http}/").as_str())
    );
    assert_eq!(
        detail.connections[1].masked.as_deref(),
        Some(format!("clickhouse://default:****@127.0.0.1:{native}/default").as_str())
    );
    let actions: Vec<&str> = detail
        .open_actions
        .iter()
        .map(|action| action.name.as_str())
        .collect();
    assert_eq!(actions, ["play", "client"]);
    assert_eq!(
        detail.open_actions[0].url.as_deref(),
        Some(format!("http://127.0.0.1:{http}/play").as_str())
    );
    let shown = format!("{:?}", detail.open_actions);
    assert!(!shown.contains(&made.password), "{shown}");
    assert!(shown.contains("--ask-password"), "{shown}");

    stop_instance(&api, made.id).await;
    eventually("the process to be gone", Duration::from_secs(60), || {
        !alive(pid)
    })
    .await;
    let stopped = api.get(made.id).await.expect("the instance can be read");
    assert_eq!(stopped.lifecycle, InstanceLifecycle::Stopped);
    assert!(stopped.process.is_none());
    assert!(stopped.last_failure.is_none(), "{stopped:#?}");
    assert!(stopped.leftover_processes.is_empty(), "{stopped:#?}");
    assert!(
        !paths.data.join("status").exists(),
        "a clean SIGTERM stop removes the status file"
    );
    for port in [http, native] {
        assert!(
            std::net::TcpStream::connect(("127.0.0.1", port)).is_err(),
            "port {port} is closed after the stop"
        );
    }
    assert!(
        !site.users_xml(made.id).exists(),
        "a clean stop removes users.xml from the secrets folder"
    );

    let again = api.start_ready(made.id).await;
    assert_ne!(pid_of(&again), pid);
    assert_eq!(
        site.sql_ok(
            port_of(&again, "native"),
            "default",
            Some(&made.password),
            "SELECT id FROM default.kept"
        ),
        "42",
        "the row written before the stop is still there"
    );
    stop_instance(&api, made.id).await;
    let table = output_of("/bin/ps", &["-ax", "-o", "pid=,args="]);
    assert!(
        !table.contains(&paths.config.join("config.xml").display().to_string()),
        "no clickhouse server of the instance is left: {table}"
    );
    daemon.shutdown().await;
    ev.pass_with(&format!(
        "ClickHouse {version} was not_ready then ready, healthy, listened on 127.0.0.1:{http} and 127.0.0.1:{native} only, showed masked http and native connections, stopped cleanly, and kept its row across a second start"
    ));
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed ClickHouse 26.8.30.8 artifact in the runtime cache"]
async fn clickhouse_runs_as_one_foreground_process() {
    let ev = evidence!(
        covers = ["REQ-037"],
        kind = Kind::Native,
        expected = "the spawned ClickHouse process is the server itself with the watchdog off: no child process, the instance lock on fd 3, the runtime binary as its program, and the same PID and argv from ready until the stop",
    );
    let Some(site) = Site::open(&ev) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let made = create_instance(&site, &api, "foreground", json!({})).await;
    let running = api.start_ready(made.id).await;
    let pid = pid_of(&running);
    site.subject(
        &ev,
        &site.version(port_of(&running, "http"), &made.password),
    );

    assert!(
        children(pid).is_empty(),
        "no watchdog child: {:?}",
        children(pid)
    );
    let fd3 = output_of(
        "/usr/sbin/lsof",
        &["-nP", "-a", "-p", &pid.to_string(), "-d", "3"],
    );
    assert!(fd3.contains("instance.lock"), "{fd3}");
    let txt = output_of(
        "/usr/sbin/lsof",
        &["-nP", "-a", "-p", &pid.to_string(), "-d", "txt", "-Fn"],
    );
    let program = txt
        .lines()
        .find_map(|line| line.strip_prefix('n'))
        .expect("lsof shows the program");
    // lsof writes the non-ASCII bytes of the data root as `\xNN`, so the
    // runtime folder named by the artifact digest identifies the binary.
    assert!(
        program.ends_with(&format!(
            "/runtimes/clickhouse/{VERSION}/macos-aarch64/{}/clickhouse",
            site.artifact_sha256
        )),
        "the first txt entry is the runtime binary: {program}"
    );
    let argv = command_line(pid);
    assert!(argv.contains("server"), "{argv}");
    assert!(
        argv.contains("--config-file="),
        "the server keeps its argv: {argv}"
    );

    tokio::time::sleep(Duration::from_secs(5)).await;
    let later = api.get(made.id).await.expect("the instance can be read");
    assert_eq!(pid_of(&later), pid, "the PID stays the same");
    assert!(children(pid).is_empty(), "still no child");
    assert_eq!(command_line(pid), argv, "the program does not change");

    stop_instance(&api, made.id).await;
    eventually("the process to be gone", Duration::from_secs(60), || {
        !alive(pid)
    })
    .await;
    daemon.shutdown().await;
    ev.pass_with(&format!(
        "PID {pid} ran {program} with no child and instance.lock on fd 3 until the stop"
    ));
}
