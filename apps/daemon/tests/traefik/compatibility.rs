//! The compatibility evidence of Traefik 3.7.15 behind the real `stacklyd`
//! binary (spec 0041 step 8, AC-11, AC-7): two instances with the same route
//! name and rule, a routing update, values the schema and Traefik refuse, a
//! roll back, and a clean stop, with `/etc/hosts` untouched throughout. A
//! check is added only after its assertions passed, and `compatibility.json`
//! is written only when every check passed. The test is native,
//! `#[ignore = "native: ..."]`, and ends as `BLOCKED` when the artifact is
//! not in the runtime cache.

use std::net::TcpStream;
use std::time::{Duration, Instant};

use serde_json::json;
use stackly_protocol::events::SubscribeParams;
use stackly_protocol::instance::config::{ConfigChangeRequest, ConfigRevisionOrigin};
use stackly_protocol::instance::{
    InstanceHealthState, InstanceLifecycle, InstanceReadinessState, InstanceSummary,
};
use stackly_protocol::operation::{OperationKind, OperationState};
use stackly_testkit::evidence::{Kind, evidence};
use uuid::Uuid;

use crate::config_calls::{Cfg, pid_of, settings_with};
use crate::harness::{
    Checks, RELOAD_PATIENCE, Serve, Site, alive, api_json, apply_until_end,
    assert_not_ready_before_ready, backend, create_with, drain_readiness, eventually, get,
    get_path, port_of, route, server_version, status_code,
};
use crate::support::Api;

/// The rule both instances share (spec 0041 AC-2).
const APP_RULE: &str = "Host(`app.localhost`)";
/// `timeouts.reloadSeconds` of the package: the bound of an answer.
const RELOAD_BOUND: Duration = Duration::from_secs(30);
/// The file AC-7 says Stackly never writes.
const HOSTS: &str = "/etc/hosts";

/// Waits until the config of `id` names `n` as its last known good.
async fn last_known_good(api: &Api, id: Uuid, n: u64) {
    Cfg { api }
        .wait_view(id, &format!("revision {n} as last known good"), |view| {
            view.last_known_good.map(|good| good.get()) == Some(n)
        })
        .await;
}

/// The PID, revision, routes file bytes, and answer for `app.localhost` of
/// `id`: what proves an instance was left alone.
async fn snapshot(site: &Site, api: &Api, id: Uuid) -> (u32, u64, Vec<u8>, (u16, String)) {
    let summary = api.get(id).await.expect("the instance can be read");
    let view = Cfg { api }.view(id).await;
    let (status, body) = get(port_of(&summary, "web"), "app.localhost");
    let file = site.instance_paths(id).config.join("dynamic/routes.yaml");
    (
        pid_of(&summary),
        view.revision.number.get(),
        std::fs::read(file).expect("the routes file reads"),
        (status_code(&status), body),
    )
}

/// Whether the router `name@file` is loaded on the `ping` port.
fn router_loaded(ping: u16, name: &str) -> bool {
    api_json(ping, "/api/http/routers?per_page=1000")
        .as_array()
        .is_some_and(|routers| {
            routers
                .iter()
                .any(|router| router["name"] == format!("{name}@file"))
        })
}

/// Stops `id` with a normal stop, never a force stop, and asserts that its
/// process and both listeners are gone and nothing is left behind.
async fn stop_clean(api: &Api, running: &InstanceSummary) {
    let id = running.id;
    let accepted = api
        .stop_when_free(id)
        .await
        .accepted
        .expect("a stop starts an operation");
    assert_eq!(accepted.operation.kind, OperationKind::StopInstance);
    let detail = api
        .wait_state(accepted.operation.id, OperationState::Succeeded)
        .await;
    assert_eq!(
        detail.operation.state,
        OperationState::Succeeded,
        "{detail:?}"
    );
    let pid = pid_of(running);
    eventually("the process to be gone", Duration::from_secs(10), || {
        !alive(pid)
    })
    .await;
    let stopped = api.get(id).await.expect("the instance can be read");
    assert_eq!(stopped.lifecycle, InstanceLifecycle::Stopped);
    assert!(stopped.process.is_none());
    assert!(stopped.last_failure.is_none(), "{stopped:#?}");
    assert!(stopped.leftover_processes.is_empty(), "{stopped:#?}");
    for endpoint in ["web", "ping"] {
        let port = port_of(running, endpoint);
        assert!(
            TcpStream::connect(("127.0.0.1", port)).is_err(),
            "nothing listens on {endpoint} {port} after the stop"
        );
    }
}

// covers: AC-11, AC-7 (spec 0041)
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed Traefik 3.7.15 artifact in the runtime cache"]
async fn traefik_compatibility() {
    let ev = evidence!(
        covers = ["BR-025", "REQ-039", "AC-039"],
        kind = Kind::Native,
        expected = "behind the stacklyd binary a Traefik 3.7.15 instance is created, started, and ready only after its router loaded, healthy with /ping 200, and serves its route; a second instance with the same route name and rule is ready from its own backend; a routing update on the first answers within reloadSeconds with the same pid while the second is untouched; schema refusals write nothing, a rule Traefik rejects fails the apply and keeps the last known good, an explicit roll back makes the route answer again; both stop with a normal stop and leave no process or listener; /etc/hosts keeps its bytes",
    );
    let Some(site) = Site::open(&ev, Serve::Binary) else {
        return;
    };
    let hosts_before = std::fs::read(HOSTS).expect("/etc/hosts reads");
    let mut checks = Checks::default();
    let alpha = backend("alpha");
    let bravo = backend("bravo");
    let charlie = backend("charlie");

    let daemon = site.spawn();
    let api = site.binary_api(&daemon).await;
    let cfg = Cfg { api: &api };
    let (_, mut events) = api
        .client
        .subscribe(SubscribeParams::default())
        .await
        .expect("the subscription is accepted");

    // init
    let a = create_with(
        &site,
        &api,
        "edge-a",
        json!({ "routes": [route("api", APP_RULE, alpha)] }),
    )
    .await;
    let created = api.get(a).await.expect("the instance can be read");
    assert_eq!(created.lifecycle, InstanceLifecycle::Stopped);
    assert_eq!(created.initialized, Some(true));
    checks.passed("init");

    // start
    let a_running = api.start_ready(a).await;
    assert_eq!(a_running.lifecycle, InstanceLifecycle::Running);
    let (a_web, a_ping) = (port_of(&a_running, "web"), port_of(&a_running, "ping"));
    let a_pid = pid_of(&a_running);
    let version = server_version(a_ping);
    site.subject(&ev, &version);
    checks.passed("start");

    // readiness: never ready before the router it asked for was loaded
    let seen = drain_readiness(&mut events, a).await;
    assert_not_ready_before_ready(&seen);
    assert!(router_loaded(a_ping, "api"), "the api router is loaded");
    checks.passed("readiness");

    // health
    // The binary daemon probes every 10 s, so health may read `unknown` for
    // a moment after `ready`; the package has no health block, so the
    // default of spec 0010 makes it `healthy`.
    let summary = api
        .wait_instance(a, "healthy", |summary| {
            summary.health.state == InstanceHealthState::Healthy
        })
        .await;
    assert_eq!(summary.readiness.state, InstanceReadinessState::Ready);
    assert_eq!(status_code(&get_path(a_ping, "127.0.0.1", "/ping").0), 200);
    checks.passed("health");

    // http
    let (status, body) = get(a_web, "app.localhost");
    assert_eq!((status_code(&status), body.as_str()), (200, "alpha"));
    checks.passed("http");

    // second_instance
    let b = create_with(
        &site,
        &api,
        "edge-b",
        json!({ "routes": [route("api", APP_RULE, bravo)] }),
    )
    .await;
    let b_running = api.start_ready(b).await;
    assert_ne!(pid_of(&b_running), a_pid);
    assert_ne!(port_of(&b_running, "web"), a_web);
    assert_ne!(port_of(&b_running, "ping"), a_ping);
    assert_eq!(get(port_of(&b_running, "web"), "app.localhost").1, "bravo");
    let a_still = api.get(a).await.expect("instance A");
    assert_eq!(a_still.readiness.state, InstanceReadinessState::Ready);
    assert_eq!(pid_of(&a_still), a_pid);
    last_known_good(&api, a, 1).await;
    last_known_good(&api, b, 1).await;
    let b_before = snapshot(&site, &api, b).await;
    checks.passed("second_instance");

    // routing_update: the sequence of AC-2
    let view = cfg.view(a).await;
    let changed_at = Instant::now();
    let applied = apply_until_end(
        &api,
        a,
        1,
        settings_with(
            &view,
            json!({ "routes": [route("api", APP_RULE, charlie)] }),
        ),
    )
    .await;
    assert_eq!(
        applied.operation.state,
        OperationState::Succeeded,
        "{applied:?}"
    );
    eventually(
        "instance A answers from its new backend",
        RELOAD_BOUND.saturating_sub(changed_at.elapsed()),
        || get(a_web, "app.localhost").1 == "charlie",
    )
    .await;
    let reloaded_in = changed_at.elapsed();
    assert_eq!(
        pid_of(&api.get(a).await.expect("instance A")),
        a_pid,
        "a reload, not a restart"
    );
    last_known_good(&api, a, 2).await;
    assert_eq!(snapshot(&site, &api, b).await, b_before);
    checks.passed("routing_update");

    // invalid_config: refused by the schema (AC-3, AC-15), then by Traefik
    let view = cfg.view(a).await;
    let before = snapshot(&site, &api, a).await;
    let history = view.history.len();
    let refused_by_schema = [
        (
            json!([
                route("api", APP_RULE, charlie),
                route("api", "Host(`b.localhost`)", charlie)
            ]),
            "settings.routes[1].name",
        ),
        (
            json!([{ "name": "api", "rule": APP_RULE, "backendUrl": "http://127.0.0.1:9/%zz" }]),
            "settings.routes[0].backendUrl",
        ),
    ];
    for (routes, path) in refused_by_schema {
        let change = settings_with(&view, json!({ "routes": routes }));
        let verdict = cfg.validate(a, 2, change.clone()).await;
        assert!(!verdict.valid, "{path}: {verdict:#?}");
        let paths: Vec<&str> = verdict.field_errors.iter().map(|e| e.path()).collect();
        assert!(paths.contains(&path), "{paths:?}");
        let refused = cfg
            .apply(a, 2, change)
            .await
            .expect_err("the apply is refused");
        assert_eq!(refused.code(), "instance.settings_invalid", "{path}");
    }
    let after = cfg.view(a).await;
    assert_eq!(after.revision.number.get(), 2, "no revision written");
    assert_eq!(after.history.len(), history);
    assert_eq!(snapshot(&site, &api, a).await, before);
    let good = backend("good");
    let failed = apply_until_end(
        &api,
        a,
        2,
        settings_with(
            &view,
            json!({ "routes": [
                route("good", "Host(`good.localhost`)", good),
                route("broken", "Host(", charlie),
            ] }),
        ),
    )
    .await;
    assert_eq!(failed.operation.state, OperationState::Failed, "{failed:?}");
    let failure = format!("{:?}", failed.operation.failure);
    assert!(failure.contains("supervisor.reload_failed"), "{failure}");
    let not_ready = api
        .wait_instance(a, "not ready after the rejected rule", |summary| {
            summary.readiness.state == InstanceReadinessState::NotReady
        })
        .await;
    assert_eq!(not_ready.readiness.reason, "routes_mismatch");
    assert!(
        not_ready
            .readiness
            .detail
            .as_deref()
            .is_some_and(|detail| detail.contains("broken")),
        "{not_ready:#?}"
    );
    assert_eq!(pid_of(&not_ready), a_pid);
    let failed_view = cfg.view(a).await;
    assert_eq!(failed_view.revision.number.get(), 3);
    assert!(failed_view.revision.ready_at.is_none());
    assert_eq!(failed_view.last_known_good.map(|good| good.get()), Some(2));
    eventually("the accepted route of revision 3", RELOAD_PATIENCE, || {
        get(a_web, "good.localhost").1 == "good"
    })
    .await;
    checks.passed("invalid_config");

    // rollback
    let rolled = apply_until_end(&api, a, 3, ConfigChangeRequest::Rollback).await;
    assert_eq!(
        rolled.operation.state,
        OperationState::Succeeded,
        "{rolled:?}"
    );
    let back = api
        .wait_instance(a, "ready after the roll back", |summary| {
            summary.readiness.state == InstanceReadinessState::Ready
        })
        .await;
    assert_eq!(pid_of(&back), a_pid, "a roll back is a reload too");
    let rolled_view = cfg.view(a).await;
    assert_eq!(rolled_view.revision.number.get(), 4);
    assert_eq!(rolled_view.revision.origin, ConfigRevisionOrigin::Rollback);
    assert_eq!(rolled_view.revision.source_number.map(|n| n.get()), Some(2));
    eventually("the route answers again", RELOAD_PATIENCE, || {
        get(a_web, "app.localhost").1 == "charlie"
    })
    .await;
    last_known_good(&api, a, 4).await;
    assert_eq!(snapshot(&site, &api, b).await, b_before);
    checks.passed("rollback");

    // stop
    stop_clean(&api, &a_running).await;
    stop_clean(&api, &b_running).await;
    drop((api, events));
    daemon.stop().expect("the daemon stops");
    checks.passed("stop");

    assert_eq!(
        std::fs::read(HOSTS).expect("/etc/hosts reads"),
        hosts_before,
        "/etc/hosts keeps its bytes"
    );
    checks.write(&ev, &site);
    ev.pass_with(&format!(
        "Traefik {version}: two instances served their own backend, A answered its new backend {reloaded_in:?} after the apply, refusals wrote nothing, the rejected rule kept revision 2 as last known good and the roll back made revision 4 ready, both stopped clean, /etc/hosts unchanged"
    ));
}
