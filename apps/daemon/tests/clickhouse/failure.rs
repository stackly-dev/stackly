//! When the server that answers is not the instance (spec 0043 AC-7,
//! AC-12): the readiness probe of the instance fails against a foreign
//! ClickHouse with another instance ID, a port held by a foreign server
//! refuses the start before the spawn, and a data folder held by a foreign
//! server fails the start with exit 76 while Stackly leaves that server and
//! its `status` file alone.

use std::collections::BTreeMap;
use std::net::{IpAddr, Ipv4Addr};
use std::time::Duration;

use serde_json::json;
use stackly_core::provider::{PlanContext, ProbeSpec, ResolvedEndpoint, RuntimeContext};
use stackly_core::secret::Redactor;
use stackly_core::supervisor::SupervisorConfig;
use stackly_core::supervisor::probe::{ProbeContext, ProbeOutcome, TcpConnect, run};
use stackly_domain::Port;
use stackly_protocol::instance::InstanceLifecycle;
use stackly_protocol::operation::OperationState;
use stackly_testkit::evidence::{Kind, evidence};
use uuid::Uuid;

use crate::harness::{
    Foreign, ForeignOptions, Made, Site, VERSION, alive, create_from, create_instance,
    create_params, http_get, instance_id, new_password, package, pid_of, port_of,
    stop_and_shutdown, stop_instance, wait_log,
};
use crate::support::Api;

/// The readiness probe of the run plan of `made`, with the native endpoint
/// on `native`, the way the supervisor builds it.
fn readiness_probe(site: &Site, made: &Made, http: u16, native: u16) -> (ProbeSpec, Duration) {
    let package = package(&site.platform);
    let loopback = IpAddr::V4(Ipv4Addr::LOCALHOST);
    let endpoint = |port: u16| ResolvedEndpoint::Ip {
        host: loopback,
        port: Some(Port::new(port).expect("a test port is not zero")),
    };
    let context = PlanContext {
        instance_id: instance_id(made.id),
        instance_name: "probe".parse().expect("a valid name"),
        paths: site.instance_paths(made.id),
        runtime: RuntimeContext {
            root: site.runtime_dir.clone(),
            version: VERSION.parse().expect("a valid version"),
            line: "26.8".parse().expect("a valid line"),
        },
        endpoints: BTreeMap::from([
            ("http".parse().expect("an endpoint name"), endpoint(http)),
            (
                "native".parse().expect("an endpoint name"),
                endpoint(native),
            ),
        ]),
        settings: package
            .settings_schema()
            .validate(&json!({
                "defaultPassword": format!("local:instance/{}/default-password", made.id),
            }))
            .expect("the settings are valid"),
    };
    let plan = package.run_plan(&context).expect("the run plan builds");
    (plan.readiness.probe, plan.readiness.attempt_timeout)
}

/// One attempt of `probe` through the supervisor's probe runner.
async fn attempt(site: &Site, probe: &ProbeSpec, timeout: Duration) -> ProbeOutcome {
    let redactor = Redactor::new();
    let config = SupervisorConfig::default();
    run(
        probe,
        timeout,
        ProbeContext {
            platform: &site.platform,
            redactor: &redactor,
            config: &config,
            native: &TcpConnect,
        },
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed ClickHouse 26.8.30.8 artifact in the runtime cache"]
async fn the_readiness_probe_fails_against_a_clickhouse_with_another_id() {
    let ev = evidence!(
        covers = ["REQ-037"],
        kind = Kind::Native,
        expected = "the exec readiness probe of an instance's run plan, run through the supervisor's probe runner, succeeds against the instance's own server and against a foreign server whose macro holds the same ID, and fails against a foreign server with a stackly_readiness user and another instance ID",
    );
    let Some(site) = Site::open(&ev) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let made = create_instance(&site, &api, "identity", json!({})).await;
    let running = api.start_ready(made.id).await;
    let http = port_of(&running, "http");
    let native = port_of(&running, "native");
    site.subject(&ev, &site.version(http, &made.password));

    let (own, timeout) = readiness_probe(&site, &made, http, native);
    let outcome = attempt(&site, &own, timeout).await;
    assert!(outcome.is_ok(), "the instance's own server: {outcome:?}");

    let other_id = Uuid::now_v7().to_string();
    let other = Foreign::start(
        &site,
        &other_id,
        &ForeignOptions {
            wait: true,
            ..ForeignOptions::default()
        },
    );
    let (probe, timeout) = readiness_probe(&site, &made, other.http, other.native);
    let outcome = attempt(&site, &probe, timeout).await;
    assert!(
        !outcome.is_ok(),
        "a server with another ID is never ready: {outcome:?}"
    );

    let twin = Foreign::start(
        &site,
        &made.id.to_string(),
        &ForeignOptions {
            wait: true,
            ..ForeignOptions::default()
        },
    );
    let (probe, timeout) = readiness_probe(&site, &made, twin.http, twin.native);
    let twin_outcome = attempt(&site, &probe, timeout).await;
    assert!(
        twin_outcome.is_ok(),
        "the macro is what the probe checks: {twin_outcome:?}"
    );

    drop(twin);
    drop(other);
    stop_and_shutdown(&api, daemon, made.id).await;
    ev.pass_with(&format!(
        "the probe passed on the instance and on a server with its ID, and failed with `{}` against another ID",
        outcome.reason().as_str()
    ));
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed ClickHouse 26.8.30.8 artifact in the runtime cache"]
async fn a_port_in_use_refuses_the_start_and_leaves_the_foreign_process() {
    let ev = evidence!(
        covers = ["REQ-037"],
        kind = Kind::Native,
        expected = "a foreign ClickHouse server on the fixed native port of a stopped instance makes its start fail at the endpoints stage with supervisor.endpoint_in_use and a suggested port before any spawn, with no process of the instance and the foreign server still running and answering",
    );
    let Some(site) = Site::open(&ev) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let held_port = stackly_testkit::allocate_port().expect("a free port can be allocated");
    let made = create_from(
        &api,
        create_params(&site, "busy", &new_password(), Some(held_port), json!({})),
    )
    .await;
    let foreign = Foreign::start(
        &site,
        "foreign",
        &ForeignOptions {
            native: Some(held_port),
            wait: true,
            ..ForeignOptions::default()
        },
    );

    let operation = api
        .start(made.id)
        .await
        .expect("the start is accepted")
        .accepted
        .expect("a start begins an operation")
        .operation
        .id;
    let detail = api.wait_final(operation).await;
    assert_eq!(detail.operation.state, OperationState::Failed, "{detail:?}");
    assert_eq!(detail.operation.stage.as_deref(), Some("endpoints"));
    assert_eq!(Api::failure_code(&detail), "supervisor.endpoint_in_use");
    let error = &detail.operation.failure.as_ref().expect("a failure").error;
    assert_eq!(error.detail("endpoint"), Some("native"));
    let held = foreign.native.to_string();
    assert_eq!(error.detail("port"), Some(held.as_str()));
    assert!(
        error.detail("suggestedPort").is_some(),
        "a free port is suggested: {error:?}"
    );
    let refused = api.get(made.id).await.expect("the instance can be read");
    assert_eq!(refused.lifecycle, InstanceLifecycle::Stopped);
    assert!(
        refused.process.is_none(),
        "no clickhouse server was spawned"
    );
    assert_eq!(port_of(&refused, "native"), foreign.native, "no repair");
    assert!(alive(foreign.pid), "the foreign server is never signalled");
    assert_eq!(http_get(foreign.http, "/ping").0, 200);
    let version = crate::harness::http_query(foreign.http, "default", "", "SELECT version()").1;
    site.subject(&ev, version.trim());

    drop(foreign);
    daemon.shutdown().await;
    ev.pass_with(&format!(
        "the start failed at endpoints with supervisor.endpoint_in_use on port {held} and the foreign server kept running"
    ));
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed ClickHouse 26.8.30.8 artifact in the runtime cache"]
async fn a_data_folder_in_use_fails_the_start_and_is_left_alone() {
    let ev = evidence!(
        covers = ["REQ-037"],
        kind = Kind::Native,
        expected = "a foreign ClickHouse server on its own ports serving the data folder of a stopped instance makes the instance start fail: lastFailure is process_exited with exit code 76, and once the restart budget is spent the instance is failed with crash_loop and exit code 76, with a Cannot lock file line in the instance log, and the foreign server and the status file left alone",
    );
    let Some(site) = Site::open(&ev) else {
        return;
    };
    // A longer pause between respawns, so the process_exited failure of
    // one attempt is seen before the next attempt starts.
    let daemon = site
        .in_process(|config| {
            config.supervisor.restart_base_delay = Duration::from_secs(1);
            config.supervisor.restart_max_delay = Duration::from_secs(2);
        })
        .await;
    let api = site.api().await;
    let made = create_instance(&site, &api, "locked", json!({})).await;
    let first = api.start_ready(made.id).await;
    site.subject(&ev, &site.version(port_of(&first, "http"), &made.password));
    let first_pid = pid_of(&first);
    stop_instance(&api, made.id).await;
    crate::harness::eventually(
        "the instance server to be gone",
        Duration::from_secs(60),
        || !alive(first_pid),
    )
    .await;
    let data = site.instance_paths(made.id).data;
    let foreign = Foreign::start(
        &site,
        "foreign",
        &ForeignOptions {
            data: Some(&data),
            wait: true,
            ..ForeignOptions::default()
        },
    );
    let status = data.join("status");
    assert!(
        status.is_file(),
        "the foreign server holds {}",
        status.display()
    );

    let operation = api
        .start(made.id)
        .await
        .expect("the start is accepted")
        .accepted
        .expect("a start begins an operation")
        .operation
        .id;
    let crashed = api
        .wait_instance(made.id, "a process_exited failure", |summary| {
            summary
                .last_failure
                .as_ref()
                .is_some_and(|failure| failure.code == "process_exited")
        })
        .await;
    let first = crashed.last_failure.as_ref().expect("a last failure");
    assert_eq!(first.exit_code, Some(76), "{first:#?}");
    let detail = api.wait_final(operation).await;
    assert_eq!(detail.operation.state, OperationState::Failed, "{detail:?}");
    let failed = api
        .wait_instance(made.id, "failed with a spent budget", |summary| {
            summary.lifecycle == InstanceLifecycle::Failed && summary.process.is_none()
        })
        .await;
    let failure = failed.last_failure.as_ref().expect("a last failure");
    assert_eq!(failure.code, "crash_loop", "{failure:#?}");
    assert_eq!(failure.exit_code, Some(76), "{failure:#?}");
    let lines = wait_log(&api, made.id, "Cannot lock file", Duration::from_secs(10)).await;
    assert!(
        lines.iter().any(|line| line.contains("status")),
        "{lines:#?}"
    );
    assert!(alive(foreign.pid), "the foreign server is never signalled");
    assert!(status.is_file(), "Stackly never removes the status file");
    assert_eq!(http_get(foreign.http, "/ping").0, 200);

    drop(foreign);
    daemon.shutdown().await;
    ev.pass_with(&format!(
        "the start failed with process_exited and exit 76, ended {} after the restart budget, logged `{}`, and left the foreign server and status alone",
        failure.code, lines[0]
    ));
}
