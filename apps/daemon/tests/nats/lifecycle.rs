//! The happy path of a NATS instance through the daemon in process (spec
//! 0023 AC-8).

use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use stackly_protocol::events::SubscribeParams;
use stackly_protocol::instance::{
    InstanceDesired, InstanceHealthState, InstanceLifecycle, InstanceReadinessState,
};
use stackly_testkit::evidence::{Kind, evidence};

use crate::harness::{
    Serve, Site, alive, assert_not_ready_before_ready, create_instance, drain_readiness,
    eventually, listeners, port_of, stop_instance, varz,
};
use crate::support::LogRecorder;

/// How long the test watches the spool after the instance is ready.
const QUIET: Duration = Duration::from_secs(60);

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed NATS 2.15.1 artifact in the runtime cache"]
async fn nats_lifecycle_in_process() {
    let ev = evidence!(
        covers = ["REQ-029", "REQ-036", "AC-029"],
        kind = Kind::Native,
        expected = "a NATS 2.15.1 instance with both endpoints on Auto in a data root with a space and Unicode in its name is created with default settings, starts, shows not_ready before ready through nats-readiness, shows lifecycle, desired, readiness, health, and process apart, listens only on its client and monitoring endpoints on loopback, reports its name, store folder, and limits on /varz, sends its log lines to the sink without one line per probe attempt, stops with no process left, and is ready again after the next start",
    );
    let Some(site) = Site::open(&ev, Serve::InProcess) else {
        return;
    };
    let recorder = Arc::new(LogRecorder::default());
    let daemon = site
        .in_process(|config| {
            config
                .extra_log_sinks
                .push(Arc::clone(&recorder) as Arc<dyn stackly_core::supervisor::LogSink>);
        })
        .await;
    let api = site.api().await;
    let (_, mut events) = api
        .client
        .subscribe(SubscribeParams::default())
        .await
        .expect("the subscription is accepted");

    let name = "lifecycle";
    let id = create_instance(&site, &api, name, json!({})).await;
    let created = api.get(id).await.expect("the instance can be read");
    assert_eq!(created.lifecycle, InstanceLifecycle::Stopped);
    assert_eq!(created.initialized, Some(true));
    let paths = site.instance_paths(id);
    assert!(paths.config.is_dir(), "the init creates the config folder");
    assert!(
        !paths.config.join("nats.conf").exists(),
        "nats.conf is written by the first start, not by the init (spec 0023 AC-15)"
    );

    let running = api.start_ready(id).await;
    assert_eq!(running.lifecycle, InstanceLifecycle::Running);
    assert_eq!(running.desired, InstanceDesired::Running);
    assert_eq!(running.readiness.state, InstanceReadinessState::Ready);
    assert_eq!(running.readiness.reason, "probe_ok");
    let process = running.process.clone().expect("a running process");
    assert!(!process.adopted, "{running:?}");
    api.wait_instance(id, "healthy", |summary| {
        summary.health.state == InstanceHealthState::Healthy
    })
    .await;
    let seen = drain_readiness(&mut events, id).await;
    assert_not_ready_before_ready(&seen);
    let pinned = std::fs::read_to_string(
        site.prepared
            .layout()
            .provider_revision_dir(&site.revision_key)
            .join("provider.yaml"),
    )
    .expect("the pinned package can be read");
    assert!(pinned.contains("name: nats-readiness"), "{pinned}");

    let client = port_of(&running, "client");
    let monitoring = port_of(&running, "monitoring");
    let (listening, udp) = listeners(process.pid);
    let mut expected = vec![
        format!("127.0.0.1:{client}"),
        format!("127.0.0.1:{monitoring}"),
    ];
    expected.sort();
    assert_eq!(listening, expected, "only the two endpoints listen");
    assert_eq!(udp, 0, "no UDP socket");

    let conf = std::fs::read_to_string(paths.config.join("nats.conf"))
        .expect("nats.conf is rendered before the first spawn");
    assert!(
        conf.contains(&format!("listen: \"127.0.0.1:{client}\"\n")),
        "listen names the effective client port {client}: {conf}"
    );
    assert!(
        conf.contains(&format!("http: \"127.0.0.1:{monitoring}\"\n")),
        "http names the effective monitoring port {monitoring}: {conf}"
    );
    assert!(
        conf.contains("Test Root Ünï cödé"),
        "store_dir keeps the space and the Unicode: {conf}"
    );

    let report = varz(monitoring);
    assert_eq!(report["server_name"], name);
    let version = report["version"]
        .as_str()
        .expect("/varz reports the version")
        .to_owned();
    assert_eq!(version, "2.15.1");
    site.subject(&ev, &version);
    let store_dir = report["jetstream"]["config"]["store_dir"]
        .as_str()
        .expect("/varz reports the JetStream store");
    let data = paths.data.display().to_string();
    assert!(
        store_dir.starts_with(&data),
        "store_dir {store_dir} is under {data}"
    );
    assert_eq!(
        report["jetstream"]["config"]["max_memory"],
        1_073_741_824u64
    );
    assert_eq!(
        report["jetstream"]["config"]["max_storage"],
        10_737_418_240u64
    );
    assert!(paths.data.join("jetstream").is_dir(), "no jetstream folder");

    let ready_lines = recorder.text_of(id);
    assert!(
        ready_lines
            .iter()
            .any(|line| line.contains("Server is ready")),
        "{ready_lines:#?}"
    );
    let before = recorder.lines_of(id);
    tokio::time::sleep(QUIET).await;
    let grown = recorder.lines_of(id) - before;
    assert!(
        grown < 3,
        "{grown} log lines in {QUIET:?}: a line per probe attempt would be about 300"
    );

    stop_instance(&api, id).await;
    eventually("the process to be gone", Duration::from_secs(10), || {
        !alive(process.pid)
    })
    .await;
    let stopped = api.get(id).await.expect("the instance can be read");
    assert_eq!(stopped.lifecycle, InstanceLifecycle::Stopped);
    assert!(stopped.process.is_none());
    assert!(stopped.last_failure.is_none(), "{stopped:#?}");
    assert!(stopped.leftover_processes.is_empty(), "{stopped:#?}");

    let again = api.start_ready(id).await;
    assert_eq!(again.readiness.state, InstanceReadinessState::Ready);
    assert_ne!(again.process.expect("a process").pid, process.pid);

    stop_instance(&api, id).await;
    daemon.shutdown().await;
    ev.pass_with(&format!(
        "NATS {version} ran twice with the Unicode data root, was ready through nats-readiness, kept the spool quiet for {QUIET:?} ({grown} new lines), and stopped"
    ));
}
