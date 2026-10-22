//! NATS 2.15.1 behind the real `stacklyd` binary with the default probe
//! interval (spec 0023 AC-9 to AC-12): readiness that means the server
//! answers, identity and mode checks against foreign servers, adoption after
//! `SIGKILL` of the daemon, and the compatibility evidence of the provider.

use std::net::{IpAddr, Ipv4Addr, TcpStream};
use std::time::{Duration, Instant};

use serde_json::json;
use stackly_core::provider::probe::native::NativeProbe;
use stackly_core::provider::{NativeProbeSpec, ProbeSpec, ProbeTarget};
use stackly_core::secret::Redactor;
use stackly_core::supervisor::SupervisorConfig;
use stackly_core::supervisor::probe::{ProbeContext, ProbeOutcome, TcpConnect, run};
use stackly_domain::Port;
use stackly_protocol::events::SubscribeParams;
use stackly_protocol::instance::{
    InstanceDesired, InstanceHealthState, InstanceLifecycle, InstanceReadinessState,
};
use stackly_testkit::evidence::{Kind, evidence};
use uuid::Uuid;

use crate::harness::{
    Checks, ContGuard, Foreign, Serve, Site, alive, assert_not_ready_before_ready, create_instance,
    drain_readiness, eventually, output_of, port_of, signal, stop_instance, varz,
};
use crate::support::Api;

/// The attempt timeout of the runner calls: the one the package declares.
const ATTEMPT: Duration = Duration::from_secs(2);
/// How long `SIGSTOP` may take to turn into `not_ready`: two failed attempts
/// of the default 10 second refresh, with the attempt timeout, and slack.
const HONEST: Duration = Duration::from_secs(40);

/// One attempt of the readiness probe against a server on `port`.
async fn probe(site: &Site, port: u16, server_name: &str, jetstream: bool) -> ProbeOutcome {
    let spec = ProbeSpec::Native(NativeProbeSpec::new(
        ProbeTarget::Ip {
            host: IpAddr::V4(Ipv4Addr::LOCALHOST),
            port: Port::new(port).expect("an allocated port is not 0"),
        },
        NativeProbe::NatsReadiness {
            server_name: server_name.to_owned(),
            jetstream,
        },
    ));
    let redactor = Redactor::new();
    let config = SupervisorConfig::default();
    run(
        &spec,
        ATTEMPT,
        ProbeContext {
            platform: &site.platform,
            redactor: &redactor,
            config: &config,
            native: &TcpConnect,
        },
    )
    .await
}

/// Asserts that `outcome` failed with `reason` and a detail holding `text`.
fn assert_failed(outcome: &ProbeOutcome, reason: &str, text: &str) {
    assert_eq!(outcome.reason().as_str(), reason, "{outcome:?}");
    let detail = outcome.detail().map(|detail| detail.as_str()).unwrap_or("");
    assert!(detail.contains(text), "detail `{detail}` lacks `{text}`");
}

/// Calls the probe until it succeeds, for a server whose JetStream needs a
/// moment after its port listens.
async fn eventually_ok(site: &Site, port: u16, server_name: &str, jetstream: bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let outcome = probe(site, port, server_name, jetstream).await;
        if outcome.is_ok() {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "the probe never succeeded: {outcome:?}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Waits until the instance reads `ready` and returns its process.
async fn ready_process(api: &Api, id: Uuid) -> stackly_protocol::instance::InstanceProcess {
    api.wait_instance(id, "ready", |summary| {
        summary.readiness.state == InstanceReadinessState::Ready
    })
    .await
    .process
    .expect("a ready instance has a process")
}

/// Asserts that the stop of `id` ended its process and left nothing behind.
async fn assert_stopped_clean(api: &Api, id: Uuid, pid: u32) {
    stop_instance(api, id).await;
    eventually("the process to be gone", Duration::from_secs(10), || {
        !alive(pid)
    })
    .await;
    let stopped = api.get(id).await.expect("the instance can be read");
    assert_eq!(stopped.lifecycle, InstanceLifecycle::Stopped);
    assert!(stopped.process.is_none());
    assert!(stopped.last_failure.is_none(), "{stopped:#?}");
    assert!(stopped.leftover_processes.is_empty(), "{stopped:#?}");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed NATS 2.15.1 artifact in the runtime cache"]
async fn nats_compatibility() {
    let ev = evidence!(
        covers = [
            "BR-025", "REQ-029", "REQ-036", "AC-029", "REQ-012", "REQ-013"
        ],
        kind = Kind::Native,
        expected = "behind the stacklyd binary with the default probe interval, a NATS 2.15.1 instance is created and ready through nats-readiness, a SIGSTOP turns readiness to not_ready with reason timeout while the port still accepts and the process stays running, SIGCONT makes it ready again, foreign servers give identity_mismatch, jetstream_disabled, jetstream_unexpected, and auth_required while matching ones succeed, an instance with JetStream off is ready without a jetstream folder, a SIGKILL of the daemon leaves the server running and the next daemon adopts the same process holding instance.lock, and a stop leaves no process",
    );
    let Some(site) = Site::open(&ev, Serve::Binary) else {
        return;
    };
    let mut checks = Checks::default();

    // init and start, readiness
    let first = site.spawn();
    let api = site.binary_api(&first).await;
    let (_, mut events) = api
        .client
        .subscribe(SubscribeParams::default())
        .await
        .expect("the subscription is accepted");
    let name = "golden";
    let id = create_instance(&site, &api, name, json!({})).await;
    let created = api.get(id).await.expect("the instance can be read");
    assert_eq!(created.lifecycle, InstanceLifecycle::Stopped);
    assert_eq!(created.initialized, Some(true));
    checks.passed("init");

    let running = api.start_ready(id).await;
    assert_eq!(running.lifecycle, InstanceLifecycle::Running);
    assert_eq!(running.desired, InstanceDesired::Running);
    assert_eq!(running.readiness.state, InstanceReadinessState::Ready);
    let process = running.process.clone().expect("a running process");
    assert!(!process.adopted);
    let client = port_of(&running, "client");
    let monitoring = port_of(&running, "monitoring");
    let version = varz(monitoring)["version"]
        .as_str()
        .expect("/varz reports the version")
        .to_owned();
    site.subject(&ev, &version);
    checks.passed("start");

    let seen = drain_readiness(&mut events, id).await;
    assert_not_ready_before_ready(&seen);
    checks.passed("readiness");

    // honest readiness: a stopped server still accepts TCP, but never answers
    {
        let _cont = ContGuard(process.pid);
        signal(process.pid, "STOP");
        assert!(
            TcpStream::connect(("127.0.0.1", client)).is_ok(),
            "the kernel accepts a connection for a stopped server"
        );
        let started = Instant::now();
        let not_ready = loop {
            let summary = api.get(id).await.expect("the instance can be read");
            if summary.readiness.state == InstanceReadinessState::NotReady {
                break summary;
            }
            assert!(
                started.elapsed() < HONEST,
                "still ready {HONEST:?} after SIGSTOP: {summary:#?}"
            );
            tokio::time::sleep(Duration::from_millis(200)).await;
        };
        assert_eq!(not_ready.readiness.reason, "timeout", "{not_ready:#?}");
        let detail = not_ready.readiness.detail.clone().unwrap_or_default();
        assert!(detail.contains("no INFO from the server"), "{detail}");
        assert_ne!(not_ready.health.state, InstanceHealthState::Healthy);
        assert_eq!(not_ready.lifecycle, InstanceLifecycle::Running);
        let kept = not_ready.process.clone().expect("the process is kept");
        assert_eq!(kept.pid, process.pid, "no restart, no stop");
        assert_eq!(kept.started_at, process.started_at);
        assert!(
            TcpStream::connect(("127.0.0.1", client)).is_ok(),
            "the port still accepts"
        );
        signal(process.pid, "CONT");
        api.wait_instance(id, "ready again", |summary| {
            summary.readiness.state == InstanceReadinessState::Ready
        })
        .await;
    }
    checks.passed("honest_readiness");

    // identity and mode against foreign servers
    let bin = site.server_bin();
    {
        let foreign = Foreign::start(&bin, "foreign", &[]);
        assert_failed(
            &probe(&site, foreign.port, "golden", false).await,
            "identity_mismatch",
            "server_name is \"foreign\", expected \"golden\"",
        );
        let secured = Foreign::start(&bin, "golden", &["--user", "u", "--pass", "p"]);
        assert_failed(
            &probe(&site, secured.port, "golden", false).await,
            "auth_required",
            "the server requires authentication",
        );
        let plain = Foreign::start(&bin, "golden", &[]);
        eventually_ok(&site, plain.port, "golden", false).await;
    }
    checks.passed("identity_check");
    {
        let plain = Foreign::start(&bin, "golden", &[]);
        assert_failed(
            &probe(&site, plain.port, "golden", true).await,
            "jetstream_disabled",
            "JetStream is off",
        );
        let store = site.root.path().join("foreign-js/store");
        let store = store.display().to_string();
        let with_js = Foreign::start(&bin, "golden", &["-js", "-sd", &store]);
        assert_failed(
            &probe(&site, with_js.port, "golden", false).await,
            "jetstream_unexpected",
            "JetStream is on",
        );
        eventually_ok(&site, with_js.port, "golden", true).await;
    }
    let nojs = create_instance(&site, &api, "nojs", json!({ "jetstream": false })).await;
    let nojs_running = api.start_ready(nojs).await;
    let nojs_process = nojs_running.process.clone().expect("a running process");
    let nojs_report = varz(port_of(&nojs_running, "monitoring"));
    assert!(
        nojs_report["jetstream"]["config"].is_null(),
        "/varz of a server without JetStream has a config: {}",
        nojs_report["jetstream"]
    );
    assert!(
        !site.instance_paths(nojs).data.join("jetstream").exists(),
        "an instance without JetStream made a jetstream folder"
    );
    checks.passed("jetstream_mode");

    // adoption: the server outlives a daemon killed with SIGKILL
    drop((api, events));
    first.kill().expect("the daemon can be killed");
    assert!(alive(process.pid), "the server outlives the daemon");
    assert!(alive(nojs_process.pid), "the second server outlives it too");
    let second = site.spawn();
    let api = site.binary_api(&second).await;
    let adopted = api
        .wait_instance(id, "adopted and ready", |summary| {
            summary
                .process
                .as_ref()
                .is_some_and(|process| process.adopted)
                && summary.readiness.state == InstanceReadinessState::Ready
        })
        .await;
    let adopted_process = adopted.process.clone().expect("the process is adopted");
    assert_eq!(adopted_process.pid, process.pid);
    assert_eq!(adopted_process.started_at, process.started_at);
    assert_eq!(adopted.lifecycle, InstanceLifecycle::Running);
    let held = output_of(
        "/usr/sbin/lsof",
        &["-a", "-p", &process.pid.to_string(), "-d", "3", "-n", "-P"],
    );
    assert!(held.contains("instance.lock"), "fd 3 of the server: {held}");
    assert_eq!(ready_process(&api, nojs).await.pid, nojs_process.pid);
    checks.passed("adoption");

    // stop
    assert_stopped_clean(&api, id, process.pid).await;
    assert_stopped_clean(&api, nojs, nojs_process.pid).await;
    second.stop().expect("the daemon stops");
    checks.passed("stop");

    checks.write(&ev, &site);
    ev.pass_with(&format!(
        "NATS {version}: ready through nats-readiness, not_ready with reason timeout under SIGSTOP and ready again after SIGCONT, foreign servers refused, adopted after SIGKILL, and stopped with no process left"
    ));
}
