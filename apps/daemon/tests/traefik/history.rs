//! The last known good and instance isolation across a restart, on real
//! Traefik 3.7.15 instances (spec 0041 `/test`): an apply that only moves
//! backends never makes a revision with a `disabled` route the last known
//! good, so `instance.config.rollback` keeps a target (the High finding of
//! verify milestones 1 and 2); an apply on an instance that is already
//! `not_ready` is characterized as it stands (spec 0026 AC-10); and two
//! instances with the same route name and rule stay apart after each one
//! restarts. Every test is native, `#[ignore = "native: ..."]`, and ends as
//! `BLOCKED` when the artifact is not in the runtime cache.

use std::time::Duration;

use serde_json::json;
use stackly_protocol::instance::InstanceReadinessState;
use stackly_protocol::instance::config::{ConfigChangeRequest, ConfigRevisionOrigin};
use stackly_protocol::operation::OperationState;
use stackly_testkit::evidence::{Kind, evidence};
use uuid::Uuid;

use crate::config_calls::{Cfg, pid_of, settings_with};
use crate::harness::{
    RELOAD_PATIENCE, Serve, Site, alive, api_json, apply_until_end, backend, create_with,
    eventually, get, port_of, route, server_version, status_code, stop_instance,
};
use crate::routing::{APP_RULE, by_name, last_known_good, snapshot};
use crate::support::Api;

/// The rule of the second route of the backend only test.
const SIDE_RULE: &str = "Host(`side.localhost`)";

/// The rule of the third route of the backend only test.
const SIX_RULE: &str = "Host(`six.localhost`)";

/// The `status` of every `@file` router and service Traefik loaded, as
/// `(kind, name, status)`, sorted.
fn loaded_status(ping: u16) -> Vec<(&'static str, String, String)> {
    let mut seen = Vec::new();
    for (kind, path) in [
        ("router", "/api/http/routers?per_page=1000"),
        ("service", "/api/http/services?per_page=1000"),
    ] {
        for (name, entry) in by_name(&api_json(ping, path)) {
            if name.ends_with("@file") {
                let status = entry["status"].as_str().unwrap_or_default().to_owned();
                seen.push((kind, name, status));
            }
        }
    }
    seen.sort();
    seen
}

/// Whether every `@file` router and service Traefik loaded is `enabled`.
fn all_enabled(ping: u16) -> bool {
    loaded_status(ping)
        .iter()
        .all(|(_, _, status)| status == "enabled")
}

/// Restarts `id` and waits until it is ready again.
async fn restart_ready(api: &Api, id: Uuid) -> stackly_protocol::instance::InstanceSummary {
    let accepted = api
        .restart(id)
        .await
        .expect("the restart is accepted")
        .accepted
        .expect("a restart begins an operation");
    api.wait_state(accepted.operation.id, OperationState::Succeeded)
        .await;
    api.wait_instance(id, "ready after the restart", |summary| {
        summary.readiness.state == InstanceReadinessState::Ready
    })
    .await
}

// covers: AC-15, AC-4 (spec 0041)
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed Traefik 3.7.15 artifact in the runtime cache"]
async fn a_backend_only_apply_never_makes_a_disabled_route_last_known_good_and_roll_back_keeps_a_target()
 {
    let ev = evidence!(
        covers = ["REQ-039", "AC-039", "AC-015"],
        kind = Kind::Native,
        expected = "a backend only apply to 127.0.0.1, localhost, and [::1] with a trailing slash succeeds and becomes the last known good only with every router and service enabled; a later rule Traefik rejects keeps that revision as the last known good, and the roll back writes a new revision from it, ready with the same pid, answering from the moved backend",
    );
    let Some(site) = Site::open(&ev, Serve::InProcess) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let cfg = Cfg { api: &api };
    let first = backend("first");
    let second = backend("second");

    let id = create_with(
        &site,
        &api,
        "edge-lkg",
        json!({ "routes": [
            route("api", APP_RULE, first),
            route("side", SIDE_RULE, first),
            route("six", SIX_RULE, first),
        ] }),
    )
    .await;
    let running = api.start_ready(id).await;
    let (web, ping) = (port_of(&running, "web"), port_of(&running, "ping"));
    site.subject(&ev, &server_version(ping));
    let pid = pid_of(&running);
    last_known_good(&api, id, 1).await;
    assert!(all_enabled(ping), "{:?}", loaded_status(ping));

    // Revision 2 moves only the backends, to edge forms of AC-15 with a
    // trailing slash; `api` keeps the IPv4 host so its answer is checked.
    let moved = [
        format!("http://127.0.0.1:{second}/"),
        format!("http://localhost:{second}/"),
        format!("http://[::1]:{second}/"),
    ];
    let routes_to = |side_rule: &str| {
        json!({ "routes": [
            { "name": "api", "rule": APP_RULE, "backendUrl": moved[0] },
            { "name": "side", "rule": side_rule, "backendUrl": moved[1] },
            { "name": "six", "rule": SIX_RULE, "backendUrl": moved[2] },
        ] })
    };
    let view = cfg.view(id).await;
    let applied = apply_until_end(&api, id, 1, settings_with(&view, routes_to(SIDE_RULE))).await;
    assert_eq!(
        applied.operation.state,
        OperationState::Succeeded,
        "{applied:?}"
    );
    eventually(
        "the three services carry the moved urls",
        Duration::from_secs(30),
        || {
            let services = by_name(&api_json(ping, "/api/http/services?per_page=1000"));
            ["api@file", "side@file", "six@file"]
                .iter()
                .zip(&moved)
                .all(|(name, url)| {
                    services
                        .get(*name)
                        .is_some_and(|s| s["loadBalancer"]["servers"][0]["url"] == **url)
                })
        },
    )
    .await;
    last_known_good(&api, id, 2).await;
    assert!(
        all_enabled(ping),
        "revision 2 is the last known good with every route enabled: {:?}",
        loaded_status(ping)
    );
    eventually(
        "api answers from the moved backend",
        RELOAD_PATIENCE,
        || get(web, "app.localhost").1 == "second",
    )
    .await;

    // Revision 3 breaks one rule: the last known good stays 2.
    let view = cfg.view(id).await;
    let failed = apply_until_end(&api, id, 2, settings_with(&view, routes_to("Host("))).await;
    assert_eq!(failed.operation.state, OperationState::Failed, "{failed:?}");
    let after_fail = cfg.view(id).await;
    assert_eq!(after_fail.revision.number.get(), 3);
    assert_eq!(after_fail.revision.ready_at, None);
    assert_eq!(after_fail.last_known_good.map(|n| n.get()), Some(2));

    // The roll back has a target, and it is the backend only revision.
    let rolled = apply_until_end(&api, id, 3, ConfigChangeRequest::Rollback).await;
    assert_eq!(
        rolled.operation.state,
        OperationState::Succeeded,
        "{rolled:?}"
    );
    let back = api
        .wait_instance(id, "ready after the roll back", |summary| {
            summary.readiness.state == InstanceReadinessState::Ready
        })
        .await;
    assert_eq!(pid_of(&back), pid, "a roll back is a reload");
    let rolled_view = cfg.view(id).await;
    assert_eq!(rolled_view.revision.number.get(), 4);
    assert_eq!(rolled_view.revision.origin, ConfigRevisionOrigin::Rollback);
    assert_eq!(rolled_view.revision.source_number.map(|n| n.get()), Some(2));
    last_known_good(&api, id, 4).await;
    eventually("every route is enabled again", RELOAD_PATIENCE, || {
        all_enabled(ping)
    })
    .await;
    assert_eq!(get(web, "app.localhost").1, "second");

    stop_instance(&api, id).await;
    assert!(!alive(pid));
    daemon.shutdown().await;
    ev.pass_with(&format!(
        "revision 2 moved the three backends and became the last known good with every route enabled; the roll back from failed revision 3 made revision 4 ready with traefik {pid} unchanged"
    ));
}

// covers: AC-4 (spec 0041)
//
// A characterization test, not a requirement: spec 0026 AC-10 verifies a
// reload of an instance that is not ready when its action ends, so an
// apply on an instance that is already `not_ready` (here, because of a
// rule Traefik rejected) ends `succeeded` even though the revision still
// holds that rule. The test locks the safe part, which the spec does
// promise: the revision never gets `ready_at`, the last known good stays
// where it was, and the roll back still reaches it. If spec 0026 AC-10
// changes so this apply fails, update the first assertion only.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed Traefik 3.7.15 artifact in the runtime cache"]
async fn an_apply_on_an_instance_already_not_ready_keeps_the_last_known_good_and_gets_no_ready_at()
{
    let ev = evidence!(
        covers = ["REQ-039", "AC-039"],
        kind = Kind::Native,
        expected = "on an instance not_ready after a rule Traefik rejected, a further apply that keeps the rejected rule ends succeeded (spec 0026 AC-10), its revision gets no ready_at, the last known good stays revision 1, readiness stays routes_mismatch, and the roll back still writes a ready revision from revision 1",
    );
    let Some(site) = Site::open(&ev, Serve::InProcess) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let cfg = Cfg { api: &api };
    let alpha = backend("alpha");
    let bravo = backend("bravo");
    let charlie = backend("charlie");

    let id = create_with(
        &site,
        &api,
        "edge-stale",
        json!({ "routes": [route("api", APP_RULE, alpha)] }),
    )
    .await;
    let running = api.start_ready(id).await;
    let (web, ping) = (port_of(&running, "web"), port_of(&running, "ping"));
    site.subject(&ev, &server_version(ping));
    let pid = pid_of(&running);
    last_known_good(&api, id, 1).await;

    // Revision 2: a good route and a rule Traefik rejects.
    let view = cfg.view(id).await;
    let failed = apply_until_end(
        &api,
        id,
        1,
        settings_with(
            &view,
            json!({ "routes": [
                route("good", "Host(`good.localhost`)", bravo),
                route("broken", "Host(", bravo),
            ] }),
        ),
    )
    .await;
    assert_eq!(failed.operation.state, OperationState::Failed, "{failed:?}");
    let summary = api.get(id).await.expect("the instance");
    assert_eq!(summary.readiness.state, InstanceReadinessState::NotReady);
    assert_eq!(summary.readiness.reason, "routes_mismatch");

    // Revision 3, on a not_ready instance: only the good backend moves.
    let view = cfg.view(id).await;
    let applied = apply_until_end(
        &api,
        id,
        2,
        settings_with(
            &view,
            json!({ "routes": [
                route("good", "Host(`good.localhost`)", charlie),
                route("broken", "Host(", bravo),
            ] }),
        ),
    )
    .await;
    // Characterized as it stands today (spec 0026 AC-10), not promised.
    assert_eq!(
        applied.operation.state,
        OperationState::Succeeded,
        "{applied:?}"
    );
    eventually("good answers from charlie", RELOAD_PATIENCE, || {
        get(web, "good.localhost").1 == "charlie"
    })
    .await;
    // The safe part, which must hold whatever the operation state.
    tokio::time::sleep(Duration::from_secs(3)).await;
    let stale = cfg.view(id).await;
    assert_eq!(stale.revision.number.get(), 3);
    assert_eq!(stale.revision.ready_at, None, "{stale:#?}");
    assert_eq!(stale.last_known_good.map(|n| n.get()), Some(1));
    let summary = api.get(id).await.expect("the instance");
    assert_eq!(summary.readiness.state, InstanceReadinessState::NotReady);
    assert_eq!(summary.readiness.reason, "routes_mismatch");
    assert_eq!(pid_of(&summary), pid);
    assert_eq!(status_code(&get(web, "app.localhost").0), 404);

    let rolled = apply_until_end(&api, id, 3, ConfigChangeRequest::Rollback).await;
    assert_eq!(
        rolled.operation.state,
        OperationState::Succeeded,
        "{rolled:?}"
    );
    api.wait_instance(id, "ready after the roll back", |summary| {
        summary.readiness.state == InstanceReadinessState::Ready
    })
    .await;
    let rolled_view = cfg.view(id).await;
    assert_eq!(rolled_view.revision.number.get(), 4);
    assert_eq!(rolled_view.revision.source_number.map(|n| n.get()), Some(1));
    last_known_good(&api, id, 4).await;
    eventually("the first route answers again", RELOAD_PATIENCE, || {
        get(web, "app.localhost").1 == "alpha"
    })
    .await;
    assert_eq!(pid_of(&api.get(id).await.expect("the instance")), pid);

    stop_instance(&api, id).await;
    assert!(!alive(pid));
    daemon.shutdown().await;
    ev.pass_with(
        "the apply on the not_ready instance ended succeeded; revision 3 got no ready_at, the last known good stayed 1, and the roll back made revision 4 ready",
    );
}

// covers: AC-2 (spec 0041)
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed Traefik 3.7.15 artifact in the runtime cache"]
async fn two_instances_with_the_same_route_stay_apart_after_each_one_restarts() {
    let ev = evidence!(
        covers = ["REQ-039", "AC-039"],
        kind = Kind::Native,
        expected = "two ready Traefik 3.7.15 instances route api with Host(`app.localhost`) to their own backend; a restart of A gives A a new pid answering from its own backend with the same revision and routes.yaml while B keeps its pid, revision, routes.yaml bytes, and answer, the same for a restart of B, and a reload of A afterwards still leaves B alone",
    );
    let Some(site) = Site::open(&ev, Serve::InProcess) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let cfg = Cfg { api: &api };
    let alpha = backend("alpha");
    let bravo = backend("bravo");
    let charlie = backend("charlie");

    let a = create_with(
        &site,
        &api,
        "edge-a",
        json!({ "routes": [route("api", APP_RULE, alpha)] }),
    )
    .await;
    let b = create_with(
        &site,
        &api,
        "edge-b",
        json!({ "routes": [route("api", APP_RULE, bravo)] }),
    )
    .await;
    let a_running = api.start_ready(a).await;
    api.start_ready(b).await;
    site.subject(&ev, &server_version(port_of(&a_running, "ping")));
    last_known_good(&api, a, 1).await;
    last_known_good(&api, b, 1).await;
    let a_before = snapshot(&site, &api, a, "app.localhost").await;
    let b_before = snapshot(&site, &api, b, "app.localhost").await;
    assert_eq!(a_before.answer, (200, "alpha".to_owned()));
    assert_eq!(b_before.answer, (200, "bravo".to_owned()));

    // A restarts: a new process with the same config, B untouched.
    restart_ready(&api, a).await;
    let a_restarted = snapshot(&site, &api, a, "app.localhost").await;
    assert_ne!(a_restarted.pid, a_before.pid, "a restart is a new process");
    assert!(!alive(a_before.pid));
    assert_eq!(a_restarted.revision, a_before.revision);
    assert_eq!(a_restarted.routes_file, a_before.routes_file);
    assert_eq!(a_restarted.answer, (200, "alpha".to_owned()));
    assert_eq!(snapshot(&site, &api, b, "app.localhost").await, b_before);

    // B restarts: A, as it is after its own restart, is untouched.
    restart_ready(&api, b).await;
    let b_restarted = snapshot(&site, &api, b, "app.localhost").await;
    assert_ne!(b_restarted.pid, b_before.pid);
    assert_eq!(b_restarted.revision, b_before.revision);
    assert_eq!(b_restarted.routes_file, b_before.routes_file);
    assert_eq!(b_restarted.answer, (200, "bravo".to_owned()));
    assert_eq!(snapshot(&site, &api, a, "app.localhost").await, a_restarted);

    // After both restarts, a reload of A still leaves B alone.
    let a_web = port_of(&api.get(a).await.expect("instance A"), "web");
    let view = cfg.view(a).await;
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
    eventually("A answers from charlie", RELOAD_PATIENCE, || {
        get(a_web, "app.localhost").1 == "charlie"
    })
    .await;
    assert_eq!(
        pid_of(&api.get(a).await.expect("instance A")),
        a_restarted.pid
    );
    assert_eq!(snapshot(&site, &api, b, "app.localhost").await, b_restarted);

    stop_instance(&api, a).await;
    stop_instance(&api, b).await;
    assert!(!alive(a_restarted.pid) && !alive(b_restarted.pid));
    daemon.shutdown().await;
    ev.pass_with(&format!(
        "A restarted as {} and B as {}; each kept its own backend and the other's pid, revision, routes.yaml, and answer",
        a_restarted.pid, b_restarted.pid
    ));
}
