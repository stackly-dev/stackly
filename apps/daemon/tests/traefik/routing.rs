//! Routing on two real Traefik 3.7.15 instances (spec 0041, step 4): the
//! namespace of a route is its instance, a value the schema refuses never
//! reaches the routes file, and a rule Traefik rejects fails the apply,
//! keeps the last known good, and is undone only by an explicit roll back.
//! Every test is native, `#[ignore = "native: ..."]`, and ends as `BLOCKED`
//! when the artifact is not in the runtime cache.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde_json::json;
use stackly_protocol::instance::InstanceReadinessState;
use stackly_protocol::instance::config::{ConfigChangeRequest, ConfigRevisionOrigin};
use stackly_protocol::operation::OperationState;
use stackly_testkit::evidence::{Kind, evidence};
use uuid::Uuid;

use crate::config_calls::{Cfg, pid_of, settings_with};
use crate::harness::{
    RELOAD_PATIENCE, Serve, Site, alive, api_json, apply_until_end, backend, create_instance,
    create_with, eventually, get, port_of, route, server_version, status_code, stop_instance,
};
use crate::support::Api;

/// The rule both instances share (spec 0041 AC-2).
const APP_RULE: &str = "Host(`app.localhost`)";

/// What a test compares to prove an instance was left alone.
#[derive(Debug, PartialEq, Eq)]
struct Untouched {
    pid: u32,
    revision: u64,
    routes_file: Vec<u8>,
    answer: (u16, String),
}

/// The routes file of the instance `id`.
fn routes_file(site: &Site, id: Uuid) -> PathBuf {
    site.instance_paths(id)
        .config
        .join("dynamic")
        .join("routes.yaml")
}

/// The PID, revision, routes file bytes, and answer for `host` of `id`.
async fn snapshot(site: &Site, api: &Api, id: Uuid, host: &str) -> Untouched {
    let summary = api.get(id).await.expect("the instance can be read");
    let view = Cfg { api }.view(id).await;
    let (status, body) = get(port_of(&summary, "web"), host);
    Untouched {
        pid: pid_of(&summary),
        revision: view.revision.number.get(),
        routes_file: std::fs::read(routes_file(site, id)).expect("the routes file reads"),
        answer: (status_code(&status), body),
    }
}

/// Waits until the config of `id` names `n` as its last known good.
async fn last_known_good(api: &Api, id: Uuid, n: u64) {
    Cfg { api }
        .wait_view(id, &format!("revision {n} as last known good"), |view| {
            view.last_known_good.map(|good| good.get()) == Some(n)
        })
        .await;
}

// covers: AC-2 (spec 0041)
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed Traefik 3.7.15 artifact in the runtime cache"]
async fn two_instances_with_the_same_route_name_and_rule_answer_from_their_own_backend() {
    let ev = evidence!(
        covers = ["REQ-039", "AC-039"],
        kind = Kind::Native,
        expected = "two ready Traefik 3.7.15 instances both route api with Host(`app.localhost`) to their own backend and each answers only from its own; a new backend on A answers within reloadSeconds with A's pid unchanged, while B keeps its pid, revision, routes.yaml bytes, and answer",
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
    let b_running = api.start_ready(b).await;
    site.subject(&ev, &server_version(port_of(&a_running, "ping")));
    let (a_web, b_web) = (port_of(&a_running, "web"), port_of(&b_running, "web"));
    assert_ne!(a_web, b_web);
    assert_ne!(pid_of(&a_running), pid_of(&b_running));
    assert_eq!(get(a_web, "app.localhost").1, "alpha");
    assert_eq!(get(b_web, "app.localhost").1, "bravo");
    last_known_good(&api, a, 1).await;
    last_known_good(&api, b, 1).await;
    let b_before = snapshot(&site, &api, b, "app.localhost").await;
    assert_eq!(b_before.answer, (200, "bravo".to_owned()));

    // A gets a new backend for the same route: a reload within reloadSeconds.
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
        Duration::from_secs(30).saturating_sub(changed_at.elapsed()),
        || get(a_web, "app.localhost").1 == "charlie",
    )
    .await;
    let reloaded_in = changed_at.elapsed();
    assert!(reloaded_in <= Duration::from_secs(30), "{reloaded_in:?}");
    let a_after = api.get(a).await.expect("instance A");
    assert_eq!(
        pid_of(&a_after),
        pid_of(&a_running),
        "a reload, not a restart"
    );
    last_known_good(&api, a, 2).await;

    let b_after = snapshot(&site, &api, b, "app.localhost").await;
    assert_eq!(b_after, b_before, "instance B was left alone");

    stop_instance(&api, a).await;
    stop_instance(&api, b).await;
    assert!(!alive(b_after.pid) && !alive(pid_of(&a_after)));
    daemon.shutdown().await;
    ev.pass_with(&format!(
        "A answered charlie {reloaded_in:?} after the apply with traefik {} unchanged; B kept {} and revision {}",
        pid_of(&a_after),
        b_after.pid,
        b_after.revision
    ));
}

// covers: AC-3 (spec 0041)
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed Traefik 3.7.15 artifact in the runtime cache"]
async fn invalid_route_values_are_refused_and_nothing_is_written() {
    let ev = evidence!(
        covers = ["REQ-039", "AC-039", "AC-015"],
        kind = Kind::Native,
        expected = "validate and apply refuse a repeated route name at the later item, a name outside the pattern, an empty rule, a backend that is not http on the loopback, and 65 routes, each at its settings.routes path; no revision is written, routes.yaml keeps its bytes, and the running route keeps answering",
    );
    let Some(site) = Site::open(&ev, Serve::InProcess) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let cfg = Cfg { api: &api };
    let alpha = backend("alpha");

    let id = create_with(
        &site,
        &api,
        "edge-valid",
        json!({ "routes": [route("api", APP_RULE, alpha)] }),
    )
    .await;
    let running = api.start_ready(id).await;
    site.subject(&ev, &server_version(port_of(&running, "ping")));
    last_known_good(&api, id, 1).await;
    let view = cfg.view(id).await;
    let before = snapshot(&site, &api, id, "app.localhost").await;
    assert_eq!(before.answer, (200, "alpha".to_owned()));
    let history = view.history.len();

    let backend_url = |url: &str| json!({ "name": "api", "rule": APP_RULE, "backendUrl": url });
    let many: Vec<_> = (0..65)
        .map(|n| route(&format!("r{n}"), &format!("Host(`r{n}.localhost`)"), alpha))
        .collect();
    let cases = [
        (
            "a repeated name",
            json!([
                route("api", APP_RULE, alpha),
                route("api", "Host(`b.localhost`)", alpha)
            ]),
            "settings.routes[1].name",
        ),
        (
            "a name outside the pattern",
            json!([route("Api_1", APP_RULE, alpha)]),
            "settings.routes[0].name",
        ),
        (
            "an empty rule",
            json!([route("api", "", alpha)]),
            "settings.routes[0].rule",
        ),
        (
            "a backend on another host",
            json!([backend_url("http://example.com")]),
            "settings.routes[0].backendUrl",
        ),
        (
            "an https backend",
            json!([backend_url(&format!("https://127.0.0.1:{alpha}"))]),
            "settings.routes[0].backendUrl",
        ),
        ("65 routes", json!(many), "settings.routes"),
    ];
    let mut seen = Vec::new();
    for (what, routes, path) in cases {
        let change = settings_with(&view, json!({ "routes": routes }));
        let verdict = cfg.validate(id, 1, change.clone()).await;
        assert!(!verdict.valid, "{what}: {verdict:#?}");
        let paths: Vec<&str> = verdict.field_errors.iter().map(|e| e.path()).collect();
        assert!(paths.contains(&path), "{what}: {paths:?}");
        let refused = cfg
            .apply(id, 1, change)
            .await
            .expect_err("the apply is refused");
        assert_eq!(refused.code(), "instance.settings_invalid", "{what}");
        let fields: Vec<&str> = refused.fields().iter().map(|e| e.path()).collect();
        assert!(fields.contains(&path), "{what}: {fields:?}");
        seen.push(format!("{what} at {path}"));
    }

    let after_view = cfg.view(id).await;
    assert_eq!(after_view.revision.number.get(), 1, "no revision written");
    assert_eq!(after_view.history.len(), history);
    let after = snapshot(&site, &api, id, "app.localhost").await;
    assert_eq!(
        after, before,
        "the routes file and the route did not change"
    );

    stop_instance(&api, id).await;
    assert!(!alive(pid_of(&running)));
    daemon.shutdown().await;
    ev.pass_with(&format!("refused: {}", seen.join("; ")));
}

// covers: AC-4 (spec 0041)
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed Traefik 3.7.15 artifact in the runtime cache"]
async fn a_rule_traefik_rejects_fails_the_apply_keeps_last_known_good_and_rolls_back() {
    let ev = evidence!(
        covers = ["REQ-039", "AC-039"],
        kind = Kind::Native,
        expected = "an apply with the rule Host( fails with supervisor.reload_failed, readiness routes_mismatch names the router, the pid stays, the last known good stays the previous revision and the new one has no ready_at, the accepted route of that revision answers; a roll back writes a new revision from the last known good, readiness is ready with the same pid, and the old route answers; the second instance never changes",
    );
    let Some(site) = Site::open(&ev, Serve::InProcess) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let cfg = Cfg { api: &api };
    let alpha = backend("alpha");
    let bravo = backend("bravo");
    let other = backend("other");

    let a = create_with(
        &site,
        &api,
        "edge-a",
        json!({ "routes": [route("api", APP_RULE, alpha)] }),
    )
    .await;
    let b = create_instance(&site, &api, "edge-b").await;
    let a_running = api.start_ready(a).await;
    api.start_ready(b).await;
    site.subject(&ev, &server_version(port_of(&a_running, "ping")));
    let a_pid = pid_of(&a_running);
    let a_web = port_of(&a_running, "web");
    last_known_good(&api, a, 1).await;
    last_known_good(&api, b, 1).await;
    let b_before = snapshot(&site, &api, b, "app.localhost").await;
    assert_eq!(b_before.answer.0, 404, "B has no route");

    // Revision 2: the old route goes, a good route and a broken one come.
    let view = cfg.view(a).await;
    let failed = apply_until_end(
        &api,
        a,
        1,
        settings_with(
            &view,
            json!({ "routes": [
                route("good", "Host(`good.localhost`)", bravo),
                route("broken", "Host(", other),
            ] }),
        ),
    )
    .await;
    assert_eq!(failed.operation.state, OperationState::Failed, "{failed:?}");
    let failure = format!("{:?}", failed.operation.failure);
    assert!(failure.contains("supervisor.reload_failed"), "{failure}");
    let summary = api.get(a).await.expect("instance A");
    assert_eq!(pid_of(&summary), a_pid, "a failed reload keeps the process");
    assert_eq!(summary.readiness.state, InstanceReadinessState::NotReady);
    assert_eq!(summary.readiness.reason, "routes_mismatch");
    let readiness = summary.readiness.detail.clone().unwrap_or_default();
    assert!(readiness.contains("broken"), "{readiness}");
    let after_fail = cfg.view(a).await;
    assert_eq!(after_fail.revision.number.get(), 2);
    assert_eq!(after_fail.revision.ready_at, None, "{after_fail:#?}");
    assert_eq!(after_fail.last_known_good.map(|n| n.get()), Some(1));
    // Traefik loaded the new file with the broken router disabled: the good
    // route answers, the broken one and the old one do not.
    let (status, body) = get(a_web, "good.localhost");
    assert_eq!((status_code(&status), body.as_str()), (200, "bravo"));
    assert_eq!(status_code(&get(a_web, "app.localhost").0), 404);
    // No roll back happens on its own.
    tokio::time::sleep(Duration::from_secs(3)).await;
    let still = cfg.view(a).await;
    assert_eq!(still.revision.number.get(), 2, "Stackly never rolls back");
    assert_eq!(snapshot(&site, &api, b, "app.localhost").await, b_before);

    // The explicit roll back writes revision 3 from revision 1.
    let rolled = apply_until_end(&api, a, 2, ConfigChangeRequest::Rollback).await;
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
    assert_eq!(rolled_view.revision.number.get(), 3);
    assert_eq!(rolled_view.revision.origin, ConfigRevisionOrigin::Rollback);
    assert_eq!(rolled_view.revision.source_number.map(|n| n.get()), Some(1));
    eventually("the old route answers again", RELOAD_PATIENCE, || {
        get(a_web, "app.localhost").1 == "alpha"
    })
    .await;
    last_known_good(&api, a, 3).await;
    assert_eq!(snapshot(&site, &api, b, "app.localhost").await, b_before);

    stop_instance(&api, a).await;
    stop_instance(&api, b).await;
    assert!(!alive(a_pid) && !alive(b_before.pid));
    daemon.shutdown().await;
    ev.pass_with(&format!(
        "Host( failed the apply with traefik {a_pid} unchanged, readiness {readiness}; the roll back made revision 3 ready"
    ));
}

// covers: AC-15 (spec 0041)
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed Traefik 3.7.15 artifact in the runtime cache"]
async fn a_backend_url_with_a_path_a_query_a_fragment_or_a_bad_escape_is_refused_and_nothing_is_written()
 {
    let ev = evidence!(
        covers = ["REQ-039", "AC-039", "AC-015"],
        kind = Kind::Native,
        expected = "validate and apply refuse a backendUrl with a bad escape, a path, a query, or a fragment with instance.settings_invalid at settings.routes[0].backendUrl; no revision is written, routes.yaml keeps its bytes, and the running route still answers",
    );
    let Some(site) = Site::open(&ev, Serve::InProcess) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let cfg = Cfg { api: &api };
    let alpha = backend("alpha");

    let id = create_with(
        &site,
        &api,
        "edge-url",
        json!({ "routes": [route("api", APP_RULE, alpha)] }),
    )
    .await;
    let running = api.start_ready(id).await;
    site.subject(&ev, &server_version(port_of(&running, "ping")));
    last_known_good(&api, id, 1).await;
    let view = cfg.view(id).await;
    let before = snapshot(&site, &api, id, "app.localhost").await;
    assert_eq!(before.answer, (200, "alpha".to_owned()));
    let history = view.history.len();

    let path = "settings.routes[0].backendUrl";
    let mut seen = Vec::new();
    for url in [
        "http://127.0.0.1:9/%zz",
        "http://127.0.0.1:9/api",
        "http://127.0.0.1:9/?a=b",
        "http://127.0.0.1:9/#x",
    ] {
        let change = settings_with(
            &view,
            json!({ "routes": [{ "name": "api", "rule": APP_RULE, "backendUrl": url }] }),
        );
        let verdict = cfg.validate(id, 1, change.clone()).await;
        assert!(!verdict.valid, "{url}: {verdict:#?}");
        let paths: Vec<&str> = verdict.field_errors.iter().map(|e| e.path()).collect();
        assert_eq!(paths, [path], "{url}");
        let refused = cfg
            .apply(id, 1, change)
            .await
            .expect_err("the apply is refused");
        assert_eq!(refused.code(), "instance.settings_invalid", "{url}");
        let fields: Vec<&str> = refused.fields().iter().map(|e| e.path()).collect();
        assert_eq!(fields, [path], "{url}");
        seen.push(url);
    }

    let after_view = cfg.view(id).await;
    assert_eq!(after_view.revision.number.get(), 1, "no revision written");
    assert_eq!(after_view.history.len(), history);
    let after = snapshot(&site, &api, id, "app.localhost").await;
    assert_eq!(
        after, before,
        "the routes file and the route did not change"
    );

    stop_instance(&api, id).await;
    assert!(!alive(pid_of(&running)));
    daemon.shutdown().await;
    ev.pass_with(&format!("refused at {path}: {}", seen.join(", ")));
}

/// The `name@file` entries of a Traefik API list, by name.
fn by_name(list: &serde_json::Value) -> std::collections::BTreeMap<String, serde_json::Value> {
    list.as_array()
        .expect("the API answers a list")
        .iter()
        .filter_map(|entry| Some((entry["name"].as_str()?.to_owned(), entry.clone())))
        .collect()
}

// covers: AC-15 (spec 0041)
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed Traefik 3.7.15 artifact in the runtime cache"]
async fn a_backend_only_change_to_every_accepted_url_form_is_loaded_and_becomes_last_known_good() {
    let ev = evidence!(
        covers = ["REQ-039", "AC-039"],
        kind = Kind::Native,
        expected = "one apply that moves six routes to the six accepted backendUrl forms succeeds; within reloadSeconds the API on the ping port shows the six services with the new url and every service and router enabled, readiness stays ready with the same pid, the last known good is the new revision, and r1 answers from the second backend",
    );
    let Some(site) = Site::open(&ev, Serve::InProcess) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let cfg = Cfg { api: &api };
    let first = backend("first");
    let second = backend("second");
    let rule = |n: usize| format!("Host(`r{n}.localhost`)");

    let routes: Vec<_> = (1..=6)
        .map(|n| route(&format!("r{n}"), &rule(n), first))
        .collect();
    let id = create_with(&site, &api, "edge-forms", json!({ "routes": routes })).await;
    let running = api.start_ready(id).await;
    let (web, ping) = (port_of(&running, "web"), port_of(&running, "ping"));
    site.subject(&ev, &server_version(ping));
    let pid = pid_of(&running);
    last_known_good(&api, id, 1).await;
    assert_eq!(get(web, "r1.localhost").1, "first");

    let urls = [
        format!("http://127.0.0.1:{second}"),
        format!("http://127.0.0.1:{second}/"),
        format!("http://localhost:{second}"),
        format!("http://[::1]:{second}"),
        "http://127.0.0.1:0".to_owned(),
        "http://127.0.0.1:99999".to_owned(),
    ];
    let moved: Vec<_> = urls
        .iter()
        .enumerate()
        .map(|(i, url)| json!({ "name": format!("r{}", i + 1), "rule": rule(i + 1), "backendUrl": url }))
        .collect();
    let view = cfg.view(id).await;
    let changed_at = Instant::now();
    let applied = apply_until_end(
        &api,
        id,
        1,
        settings_with(&view, json!({ "routes": moved })),
    )
    .await;
    assert_eq!(
        applied.operation.state,
        OperationState::Succeeded,
        "{applied:?}"
    );
    let revision = cfg.view(id).await.revision.number.get();
    assert_eq!(revision, 2);

    let loaded = |services: &std::collections::BTreeMap<String, serde_json::Value>| {
        urls.iter().enumerate().all(|(i, url)| {
            services
                .get(&format!("r{}@file", i + 1))
                .is_some_and(|service| service["loadBalancer"]["servers"][0]["url"] == **url)
        })
    };
    eventually(
        "the six services carry the new urls",
        Duration::from_secs(30).saturating_sub(changed_at.elapsed()),
        || {
            loaded(&by_name(&api_json(
                ping,
                "/api/http/services?per_page=1000",
            )))
        },
    )
    .await;
    let loaded_in = changed_at.elapsed();
    let services = by_name(&api_json(ping, "/api/http/services?per_page=1000"));
    let routers = by_name(&api_json(ping, "/api/http/routers?per_page=1000"));
    for n in 1..=6 {
        let name = format!("r{n}@file");
        assert_eq!(services[&name]["status"], "enabled", "service {name}");
        assert_eq!(routers[&name]["status"], "enabled", "router {name}");
    }
    let after = api.get(id).await.expect("the instance can be read");
    assert_eq!(after.readiness.state, InstanceReadinessState::Ready);
    assert_eq!(pid_of(&after), pid, "a reload, not a restart");
    last_known_good(&api, id, revision).await;
    eventually(
        "r1 answers from the second backend",
        RELOAD_PATIENCE,
        || get(web, "r1.localhost").1 == "second",
    )
    .await;

    stop_instance(&api, id).await;
    assert!(!alive(pid));
    daemon.shutdown().await;
    ev.pass_with(&format!(
        "the six accepted forms loaded enabled {loaded_in:?} after the apply; revision {revision} is the last known good with traefik {pid} unchanged"
    ));
}
