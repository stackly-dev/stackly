//! Changing a Traefik instance through the daemon (spec 0029, milestone 4):
//! the config methods against two real `traefik` processes of the managed
//! runtime. The `routes` setting is a list keyed by `name`; applying it is a
//! reload, so instance A keeps its process and serves the new route from its
//! watched dynamic file, and instance B is left alone. The test is native,
//! `#[ignore = "native: ..."]`, and ends as `BLOCKED` when the artifact is not
//! in the runtime cache.

use std::time::Duration;

use serde_json::json;
use stackly_protocol::instance::InstanceReadinessState;
use stackly_protocol::instance::config::{ConfigImpact, ConfigRevisionOrigin};
use stackly_testkit::evidence::{Kind, evidence};

use crate::config_calls::{Cfg, pid_of, settings_with};
use crate::harness::{
    Serve, Site, alive, backend, create_instance, eventually, get, port_of, server_version,
    stop_instance,
};

// covers: AC-4, AC-16
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed Traefik 3.7.15 artifact in the runtime cache"]
async fn traefik_applies_routes_with_a_reload_on_one_instance_and_leaves_the_other_alone() {
    let ev = evidence!(
        covers = ["REQ-015", "AC-015"],
        kind = Kind::Native,
        expected = "two Traefik 3.7.15 instances run side by side; a routes change on instance A with a repeated route name is invalid, a valid one is a reload impact that keeps the traefik pid, the new route answers on A with the body of its backend, instance B keeps its pid, revision, and 404 for that host, and revision 2 of A becomes the last known good after readiness",
    );
    let Some(site) = Site::open(&ev, Serve::InProcess) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let cfg = Cfg { api: &api };
    let alpha = backend("alpha");
    let bravo = backend("bravo");

    let a = create_instance(&site, &api, "edge-a").await;
    let b = create_instance(&site, &api, "edge-b").await;
    let a_running = api.start_ready(a).await;
    let b_running = api.start_ready(b).await;
    let (a_pid, b_pid) = (pid_of(&a_running), pid_of(&b_running));
    let (a_web, b_web) = (port_of(&a_running, "web"), port_of(&b_running, "web"));
    assert_ne!(a_web, b_web);
    site.subject(&ev, &server_version(port_of(&a_running, "ping")));

    let view = cfg
        .wait_view(a, "revision 1 as last known good", |view| {
            view.last_known_good.map(|n| n.get()) == Some(1)
        })
        .await;
    assert!(view.reload_supported);
    let b_before = cfg
        .wait_view(b, "revision 1 as last known good", |view| {
            view.last_known_good.map(|n| n.get()) == Some(1)
        })
        .await;
    let b_routes = site.instance_paths(b).config.join("dynamic/routes.yaml");
    let b_file = std::fs::read_to_string(&b_routes).expect("B has its routes file");

    // A list keyed by name: the same name twice is a field error, and no
    // revision is written.
    let route = |name: &str, host: &str, port: u16| json!({ "name": name, "rule": format!("Host(`{host}`)"), "backendUrl": format!("http://127.0.0.1:{port}") });
    let twice = settings_with(
        &view,
        json!({ "routes": [route("api", "a.localhost", alpha), route("api", "b.localhost", bravo)] }),
    );
    let verdict = cfg.validate(a, 1, twice).await;
    assert!(!verdict.valid, "{verdict:#?}");
    assert!(
        verdict
            .field_errors
            .iter()
            .any(|error| error.path().starts_with("settings.routes")),
        "{verdict:#?}"
    );

    // One valid route: a reload that keeps the process.
    let change = settings_with(
        &view,
        json!({ "routes": [route("api", "a.localhost", alpha)] }),
    );
    assert_eq!(cfg.impact(a, 1, change.clone()).await, ConfigImpact::Reload);
    cfg.apply_done(a, 1, change).await;
    eventually(
        "instance A serves the new route",
        Duration::from_secs(30),
        || {
            let (status, body) = get(a_web, "a.localhost");
            status.contains("200") && body == "alpha"
        },
    )
    .await;
    let a_after = cfg
        .wait_view(a, "revision 2 as last known good", |view| {
            view.last_known_good.map(|n| n.get()) == Some(2)
        })
        .await;
    assert_eq!(a_after.revision.origin, ConfigRevisionOrigin::Applied);
    let a_summary = api.get(a).await.expect("instance A");
    assert_eq!(pid_of(&a_summary), a_pid, "a reload keeps the process");
    assert_eq!(a_summary.readiness.state, InstanceReadinessState::Ready);

    // B was not touched: process, revision, file, and the route is not there.
    let b_summary = api.get(b).await.expect("instance B");
    assert_eq!(pid_of(&b_summary), b_pid);
    let b_after = cfg.view(b).await;
    assert_eq!(b_after.revision.number, b_before.revision.number);
    assert_eq!(b_after.revision.values, b_before.revision.values);
    assert_eq!(
        std::fs::read_to_string(&b_routes).expect("B routes file"),
        b_file
    );
    let (status, _) = get(b_web, "a.localhost");
    assert!(status.contains("404"), "B answered {status}");

    // B gets its own route; A keeps its own.
    let b_view = cfg.view(b).await;
    cfg.apply_done(
        b,
        1,
        settings_with(
            &b_view,
            json!({ "routes": [route("api", "b.localhost", bravo)] }),
        ),
    )
    .await;
    eventually(
        "instance B serves its route",
        Duration::from_secs(30),
        || {
            let (status, body) = get(b_web, "b.localhost");
            status.contains("200") && body == "bravo"
        },
    )
    .await;
    assert_eq!(get(a_web, "a.localhost").1, "alpha");
    assert!(get(a_web, "b.localhost").0.contains("404"));
    assert_eq!(pid_of(&api.get(a).await.expect("instance A")), a_pid);
    assert_eq!(pid_of(&api.get(b).await.expect("instance B")), b_pid);

    stop_instance(&api, a).await;
    stop_instance(&api, b).await;
    assert!(!alive(a_pid) && !alive(b_pid));
    daemon.shutdown().await;
    ev.pass_with(&format!(
        "route api on A answered alpha with traefik {a_pid} unchanged; B kept {b_pid} until its own route"
    ));
}
