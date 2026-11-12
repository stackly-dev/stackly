//! NATS 2.15.1 behind the real `stacklyd` binary with the default probe
//! interval (spec 0023 AC-9 to AC-12): readiness that means the server
//! answers, identity and mode checks against foreign servers, adoption after
//! `SIGKILL` of the daemon, and the compatibility evidence of the provider.

use std::net::{IpAddr, Ipv4Addr, TcpListener, TcpStream};
use std::time::{Duration, Instant};

use serde_json::json;
use stackly_core::provider::probe::native::NativeProbe;
use stackly_core::provider::{NativeProbeSpec, ProbeSpec, ProbeTarget};
use stackly_core::secret::Redactor;
use stackly_core::supervisor::SupervisorConfig;
use stackly_core::supervisor::probe::{ProbeContext, ProbeOutcome, TcpConnect, run};
use stackly_domain::Port;
use stackly_protocol::events::SubscribeParams;
use stackly_protocol::instance::config::{ChangeEndpointsReply, GetConfigParams};
use stackly_protocol::instance::{
    InstanceDesired, InstanceHealthState, InstanceLifecycle, InstanceReadinessState,
    InstanceSummary, PortRequest,
};
use stackly_protocol::operation::OperationState;
use stackly_protocol::{Call, Reply};
use stackly_testkit::allocate_port;
use stackly_testkit::evidence::{Kind, evidence};
use uuid::Uuid;

use crate::client::{Nats, loopback};
use crate::config_calls::Cfg;
use crate::harness::{
    Checks, ContGuard, Foreign, PASSWORD_A, PASSWORD_B, Serve, Site, USER, alive,
    assert_not_ready_before_ready, create_instance, create_instance_with_password, create_params,
    drain_readiness, eventually, listeners, output_of, port_of, signal, stop_instance, varz,
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

/// Signs in as `stackly` with `password` on the client port of `summary`.
async fn sign_in(summary: &InstanceSummary, password: &str) -> Nats {
    Nats::signed_in(loopback(port_of(summary, "client")), Some((USER, password))).await
}

/// Restarts `id` through `instance.restart` and returns the instance once
/// a new process is ready.
async fn restart_ready(api: &Api, id: Uuid) -> InstanceSummary {
    let before = api
        .get(id)
        .await
        .expect("the instance can be read")
        .process
        .expect("a running process")
        .pid;
    let accepted = api
        .restart(id)
        .await
        .expect("the restart is accepted")
        .accepted
        .expect("a restart starts an operation");
    api.wait_state(accepted.operation.id, OperationState::Succeeded)
        .await;
    api.wait_instance(id, "ready after the restart", |summary| {
        summary
            .process
            .as_ref()
            .is_some_and(|process| process.pid != before)
            && summary.readiness.state == InstanceReadinessState::Ready
    })
    .await
}

/// Asserts that `/varz` of `summary` names the instance and stores under
/// its data folder.
fn assert_own_server(site: &Site, id: Uuid, summary: &InstanceSummary) {
    let report = varz(port_of(summary, "monitoring"));
    assert_eq!(report["server_name"], summary.name, "{report}");
    let store_dir = report["jetstream"]["config"]["store_dir"]
        .as_str()
        .expect("/varz reports the JetStream store");
    let data = site.instance_paths(id).data.display().to_string();
    assert!(store_dir.starts_with(&data), "{store_dir} is under {data}");
}

/// The payloads of the messages 1 to `count` of the stream `name`.
async fn payloads(nats: &mut Nats, name: &str, count: u64) -> Vec<String> {
    let mut out = Vec::new();
    for seq in 1..=count {
        out.push(String::from_utf8_lossy(&nats.stream_get(name, seq).await).into_owned());
    }
    out
}

/// AC-3 through the daemon binary: the `stackly` user, a wrong password,
/// and a connection without credentials on the running instance `summary`.
async fn client_auth(summary: &InstanceSummary) {
    let client = loopback(port_of(summary, "client"));
    let mut app = sign_in(summary, PASSWORD_A).await;
    app.stream_create("AUTH", "auth.>", "file")
        .await
        .expect("the stackly user creates a stream");
    app.publish("auth.one", b"one")
        .await
        .expect("the stackly user publishes");
    let (_, wrong) = Nats::connect(client, Some((USER, PASSWORD_B))).await;
    assert!(
        wrong.starts_with("-ERR 'Authorization Violation'"),
        "a wrong password: {wrong}"
    );
    let mut anonymous = Nats::signed_in(client, None).await;
    for refused in [
        anonymous.publish_with_reply("auth.two", None, b"x").await,
        anonymous.subscribe(">").await.map(|_| ()),
        anonymous
            .publish_with_reply("$JS.API.STREAM.CREATE.X", None, b"{}")
            .await,
    ] {
        let error = refused.expect_err("the readiness user is refused");
        assert!(error.starts_with("-ERR 'Permissions Violation"), "{error}");
    }
    let info = anonymous
        .request_in("_INBOX.stackly", "$JS.API.INFO", b"")
        .await
        .expect("the readiness user asks $JS.API.INFO");
    let info: serde_json::Value = serde_json::from_slice(&info.payload).expect("JSON");
    assert_eq!(info["streams"], 0, "{info}");
    assert_eq!(app.stream_messages("AUTH").await, 1);
}

/// AC-6: two instances with JetStream, a stream of the same name in each,
/// their own server name and store, passwords that do not cross, and two
/// loopback listeners each. Returns both IDs.
async fn isolation(site: &Site, api: &Api) -> (Uuid, Uuid) {
    let a = create_instance_with_password(site, api, "iso-a", json!({}), PASSWORD_A).await;
    let b = create_instance_with_password(site, api, "iso-b", json!({}), PASSWORD_B).await;
    let a_running = api.start_ready(a).await;
    let b_running = api.start_ready(b).await;
    for (id, summary, password, prefix, count) in [
        (a, &a_running, PASSWORD_A, "a", 3),
        (b, &b_running, PASSWORD_B, "b", 5),
    ] {
        let mut nats = sign_in(summary, password).await;
        nats.stream_create("ORDERS", "orders.>", "file")
            .await
            .expect("a stream named ORDERS");
        for n in 1..=count {
            nats.publish("orders.new", format!("{prefix}-{n}").as_bytes())
                .await
                .expect("the publish is acked");
        }
        assert_own_server(site, id, summary);
        let pid = summary.process.as_ref().expect("a process").pid;
        let (listening, _) = listeners(pid);
        let mut expected = vec![
            format!("127.0.0.1:{}", port_of(summary, "client")),
            format!("127.0.0.1:{}", port_of(summary, "monitoring")),
        ];
        expected.sort();
        assert_eq!(
            listening, expected,
            "{} listens on its two ports",
            summary.name
        );
    }
    for (summary, foreign) in [(&a_running, PASSWORD_B), (&b_running, PASSWORD_A)] {
        let (_, refused) =
            Nats::connect(loopback(port_of(summary, "client")), Some((USER, foreign))).await;
        assert!(
            refused.starts_with("-ERR 'Authorization Violation'"),
            "{}: {refused}",
            summary.name
        );
    }
    (a, b)
}

/// AC-6 after a restart and AC-7: the streams of `a` and `b` survive
/// `instance.restart`, a file stream survives and a memory stream does not,
/// and after `SIGKILL` the file stream is still there. Returns the number
/// of messages the file stream held after the `SIGKILL`.
async fn persistence(site: &Site, api: &Api, a: Uuid, b: Uuid) -> u64 {
    let a_running = restart_ready(api, a).await;
    let b_running = restart_ready(api, b).await;
    for (id, summary, password, prefix, count) in [
        (a, &a_running, PASSWORD_A, "a", 3),
        (b, &b_running, PASSWORD_B, "b", 5),
    ] {
        let mut nats = sign_in(summary, password).await;
        assert_eq!(nats.stream_messages("ORDERS").await, count);
        let expected: Vec<String> = (1..=count).map(|n| format!("{prefix}-{n}")).collect();
        assert_eq!(payloads(&mut nats, "ORDERS", count).await, expected);
        assert_own_server(site, id, summary);
    }

    let mut nats = sign_in(&a_running, PASSWORD_A).await;
    nats.stream_create("F", "f.>", "file")
        .await
        .expect("a file stream");
    nats.stream_create("M", "m.>", "memory")
        .await
        .expect("a memory stream");
    for n in 1..=10 {
        nats.publish("f.x", format!("f-{n}").as_bytes())
            .await
            .expect("acked");
        nats.publish("m.x", format!("m-{n}").as_bytes())
            .await
            .expect("acked");
    }
    drop(nats);
    let restarted = restart_ready(api, a).await;
    let mut nats = sign_in(&restarted, PASSWORD_A).await;
    assert_eq!(nats.stream_messages("F").await, 10);
    let gone = nats
        .stream_info("M")
        .await
        .expect_err("a memory stream does not survive a restart");
    assert!(gone.to_string().contains("stream not found"), "{gone}");
    drop(nats);

    let killed = restarted.process.as_ref().expect("a process").pid;
    signal(killed, "KILL");
    let deadline = Instant::now() + Duration::from_secs(60);
    let back = loop {
        let summary = api.get(a).await.expect("the instance can be read");
        if summary
            .process
            .as_ref()
            .is_some_and(|process| process.pid != killed)
            && summary.readiness.state == InstanceReadinessState::Ready
        {
            break summary;
        }
        if summary.lifecycle == InstanceLifecycle::Failed {
            break api.start_ready(a).await;
        }
        assert!(
            Instant::now() < deadline,
            "not ready again after SIGKILL: {summary:#?}"
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    };
    let mut nats = sign_in(&back, PASSWORD_A).await;
    nats.stream_messages("F").await
}

/// AC-8: a listener of the test holds the fixed monitoring port, so the
/// start fails at the `endpoints` stage, nothing changes, and after the
/// port moves to the suggestion the instance is ready there.
async fn monitoring_conflict(site: &Site, api: &Api) {
    let held = allocate_port().expect("a free port can be allocated");
    let mut params = create_params(site, "mon", json!({}));
    for endpoint in &mut params.endpoints {
        if endpoint.name == "monitoring" {
            endpoint.port = PortRequest::Fixed(held);
        }
    }
    let accepted = api
        .create(params)
        .await
        .expect("the create is accepted")
        .accepted
        .expect("a create starts an operation");
    let id: Uuid = accepted
        .operation
        .resource
        .as_ref()
        .map(|resource| resource.id.parse().expect("an instance id"))
        .expect("the operation names the instance");
    api.wait_state(accepted.operation.id, OperationState::Succeeded)
        .await;

    let holder = TcpListener::bind((Ipv4Addr::LOCALHOST, held)).expect("the test holds the port");
    let operation = api
        .start(id)
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
    assert_eq!(error.detail("endpoint"), Some("monitoring"));
    let held_text = held.to_string();
    assert_eq!(error.detail("port"), Some(held_text.as_str()));
    let suggested: u16 = error
        .detail("suggestedPort")
        .unwrap_or_else(|| panic!("a free port is suggested: {error:?}"))
        .parse()
        .expect("a port number");
    assert!(suggested > held, "{suggested} comes after {held}");

    let refused = api.get(id).await.expect("the instance can be read");
    assert_eq!(refused.lifecycle, InstanceLifecycle::Stopped);
    assert!(refused.process.is_none(), "no nats-server was spawned");
    assert!(
        refused.last_failure.is_none(),
        "a refusal before the journal"
    );
    assert_eq!(port_of(&refused, "monitoring"), held);
    drop(
        TcpStream::connect(("127.0.0.1", held))
            .expect("the outside listener still accepts a connection"),
    );
    drop(holder);

    let base = match api
        .client
        .call(Call::InstanceConfigGet(GetConfigParams { instance_id: id }))
        .await
        .expect("the config can be read")
    {
        Reply::InstanceConfigGet(view) => view.revision.number.get(),
        other => panic!("not a config reply: {other:?}"),
    };
    let reply = Cfg { api }
        .change_endpoints(id, base, "monitoring", PortRequest::Fixed(suggested))
        .await
        .expect("the suggested port is accepted");
    if let ChangeEndpointsReply::Applied(applied) = reply
        && let Some(accepted) = applied.accepted
    {
        api.wait_state(accepted.operation.id, OperationState::Succeeded)
            .await;
    }
    let running = api.start_ready(id).await;
    assert_eq!(port_of(&running, "monitoring"), suggested);
    assert_eq!(varz(suggested)["server_name"], "mon");
    stop_instance(api, id).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed NATS 2.15.1 artifact in the runtime cache"]
async fn nats_compatibility() {
    let ev = evidence!(
        covers = [
            "BR-025", "REQ-029", "REQ-036", "AC-029", "REQ-012", "REQ-013", "AC-036", "AC-077",
            "AC-010"
        ],
        kind = Kind::Native,
        expected = "behind the stacklyd binary with the default probe interval, a NATS 2.15.1 instance is created and ready through nats-readiness, a SIGSTOP turns readiness to not_ready with reason timeout while the port still accepts and the process stays running, SIGCONT makes it ready again, foreign servers give identity_mismatch, jetstream_disabled, jetstream_unexpected, and auth_required while matching ones succeed, an instance with JetStream off is ready without a jetstream folder, a SIGKILL of the daemon leaves the server running and the next daemon adopts the same process holding instance.lock, the stackly user signs in with its password while a wrong password and a connection without credentials are refused, two instances with a file stream ORDERS each keep only their own 3 and 5 messages, server name, store, password, and two loopback listeners across a restart, a file stream keeps its 10 messages across a restart while a memory stream is gone and the file stream is still there after SIGKILL, a held fixed monitoring port fails the start at endpoints with supervisor.endpoint_in_use and a suggestedPort while the instance stays stopped and the outside listener keeps working, the instance is ready on the suggested port, and a stop leaves no process",
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

    // client password
    client_auth(&adopted).await;
    checks.passed("client_auth");

    // two instances side by side
    let (a, b) = isolation(&site, &api).await;
    checks.passed("isolation");

    // streams across restart and SIGKILL
    let after_kill = persistence(&site, &api, a, b).await;
    checks.passed("persistence");
    for other in [a, b] {
        let pid = api
            .get(other)
            .await
            .expect("the instance can be read")
            .process
            .expect("a running process")
            .pid;
        assert_stopped_clean(&api, other, pid).await;
    }

    // a fixed monitoring port held by another process
    monitoring_conflict(&site, &api).await;
    checks.passed("monitoring_conflict");

    // stop
    assert_stopped_clean(&api, id, process.pid).await;
    assert_stopped_clean(&api, nojs, nojs_process.pid).await;
    second.stop().expect("the daemon stops");
    checks.passed("stop");

    checks.write(&ev, &site);
    ev.pass_with(&format!(
        "NATS {version}: ready through nats-readiness, not_ready with reason timeout under SIGSTOP and ready again after SIGCONT, foreign servers refused, adopted after SIGKILL, client password enforced, two instances isolated across a restart, file stream kept and memory stream gone after a restart, file stream F held {after_kill} of 10 messages after SIGKILL of nats-server, monitoring port conflict refused at endpoints and ready on the suggested port, and stopped with no process left"
    ));
}
