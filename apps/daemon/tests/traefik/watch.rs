//! The watched dynamic folder of a real Traefik 3.7.15 instance (spec 0041,
//! step 6): applies replace `routes.yaml` atomically, so a route that stays
//! never drops while another changes, and a router in a file you put in the
//! folder yourself makes the instance not ready, with the doctor remedy that
//! points at the folder, until you move the file out. Every test is native,
//! `#[ignore = "native: ..."]`, and ends as `BLOCKED` when the artifact is
//! not in the runtime cache.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde_json::json;
use stackly_protocol::doctor::{DoctorCheckId, DoctorFindingCode, DoctorParams};
use stackly_protocol::instance::InstanceReadinessState;
use stackly_protocol::operation::OperationState;
use stackly_protocol::{Call, Reply};
use stackly_testkit::evidence::{Kind, evidence};

use crate::config_calls::{Cfg, pid_of, settings_with};
use crate::harness::{
    Serve, Site, alive, apply_until_end, backend, create_with, eventually, get, port_of, route,
    server_version, status_code, stop_instance,
};

/// How many applies in a row the test runs (spec 0041 AC-8).
const APPLIES: usize = 10;

// covers: AC-8 (spec 0041)
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed Traefik 3.7.15 artifact in the runtime cache"]
async fn ten_applies_in_a_row_never_drop_the_route_that_stays() {
    let ev = evidence!(
        covers = ["REQ-039", "AC-039"],
        kind = Kind::Native,
        expected = "ten applies in a row that swap the backend of one route and keep a second route all succeed, and a poller on the second route sees 200 on every request, never 404 or 502",
    );
    let Some(site) = Site::open(&ev, Serve::InProcess) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let cfg = Cfg { api: &api };
    let stays = backend("stays");
    let swaps = [backend("one"), backend("two")];
    let routes = |n: usize| {
        json!({ "routes": [
            route("moving", "Host(`moving.localhost`)", swaps[n % 2]),
            route("steady", "Host(`steady.localhost`)", stays),
        ] })
    };

    let id = create_with(&site, &api, "edge-watch", routes(0)).await;
    let running = api.start_ready(id).await;
    site.subject(&ev, &server_version(port_of(&running, "ping")));
    let pid = pid_of(&running);
    let web = port_of(&running, "web");
    assert_eq!(get(web, "steady.localhost").1, "stays");

    // The poller sends one request after another until the applies end.
    let done = Arc::new(AtomicBool::new(false));
    let poller = {
        let done = Arc::clone(&done);
        std::thread::spawn(move || {
            let mut answers = 0_usize;
            let mut wrong = Vec::new();
            while !done.load(Ordering::SeqCst) {
                let (status, body) = get(web, "steady.localhost");
                answers += 1;
                if status_code(&status) != 200 || body != "stays" {
                    wrong.push(format!("{status} {body}"));
                }
            }
            (answers, wrong)
        })
    };

    for n in 1..=APPLIES {
        let view = cfg.view(id).await;
        let base = view.revision.number.get();
        let applied = apply_until_end(&api, id, base, settings_with(&view, routes(n))).await;
        assert_eq!(
            applied.operation.state,
            OperationState::Succeeded,
            "apply {n}: {applied:?}"
        );
        // The router names stay the same, so the verify can pass before
        // Traefik loaded the new backend: wait for it within reloadSeconds.
        let expected = if n % 2 == 0 { "one" } else { "two" };
        eventually(
            &format!("apply {n} moves the route to {expected}"),
            Duration::from_secs(30),
            || get(web, "moving.localhost").1 == expected,
        )
        .await;
    }
    done.store(true, Ordering::SeqCst);
    let (answers, wrong) = poller.join().expect("the poller ends");
    assert!(answers > APPLIES, "only {answers} requests");
    assert!(wrong.is_empty(), "the steady route dropped: {wrong:?}");
    assert_eq!(pid_of(&api.get(id).await.expect("the instance")), pid);

    stop_instance(&api, id).await;
    assert!(!alive(pid));
    daemon.shutdown().await;
    ev.pass_with(&format!(
        "{APPLIES} applies succeeded with traefik {pid}; {answers} requests to the steady route all answered 200"
    ));
}

// covers: AC-8, AC-13 (spec 0041)
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed Traefik 3.7.15 artifact in the runtime cache"]
async fn a_router_in_a_file_the_user_added_makes_the_instance_not_ready_until_it_is_moved() {
    let ev = evidence!(
        covers = ["REQ-039", "AC-039"],
        kind = Kind::Native,
        expected = "a router in a file the user put in config/dynamic makes readiness not_ready with routes_mismatch and is loaded but not in the routes file; doctor.run shows the routes_not_loaded remedy with three steps, the third naming the dynamic folder; Stackly leaves the file as it is, and readiness is ready again once it is moved out",
    );
    let Some(site) = Site::open(&ev, Serve::InProcess) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let alpha = backend("alpha");
    let extra = backend("extra");

    let id = create_with(
        &site,
        &api,
        "edge-foreign",
        json!({ "routes": [route("api", "Host(`app.localhost`)", alpha)] }),
    )
    .await;
    let running = api.start_ready(id).await;
    site.subject(&ev, &server_version(port_of(&running, "ping")));
    let pid = pid_of(&running);
    let dynamic = site.instance_paths(id).config.join("dynamic");
    let foreign = dynamic.join("mine.yaml");
    let text = format!(
        "http:\n  routers:\n    mine:\n      rule: \"Host(`mine.localhost`)\"\n      service: mine\n  services:\n    mine:\n      loadBalancer:\n        servers:\n          - url: \"http://127.0.0.1:{extra}\"\n"
    );
    std::fs::write(&foreign, &text).expect("the user file is written");

    let not_ready = api
        .wait_instance(id, "not ready", |summary| {
            summary.readiness.state == InstanceReadinessState::NotReady
        })
        .await;
    assert_eq!(not_ready.readiness.reason, "routes_mismatch");
    let detail = not_ready.readiness.detail.clone().unwrap_or_default();
    assert!(
        detail.contains("is loaded but not in the routes file"),
        "{detail}"
    );
    assert_eq!(pid_of(&not_ready), pid);

    let reply = api
        .client
        .call(Call::DoctorRun(DoctorParams {
            instance_id: Some(id),
            checks: vec![DoctorCheckId::InstanceFailures],
        }))
        .await
        .expect("doctor.run answers");
    let Reply::DoctorRun(report) = reply else {
        panic!("not a doctor reply: {reply:?}");
    };
    let finding = report.checks[0]
        .findings
        .iter()
        .find(|finding| finding.code == DoctorFindingCode::NotReady)
        .expect("not_ready is reported");
    let remedy = finding.remedy.as_ref().expect("the remedy matches");
    assert_eq!(remedy.name, "routes_not_loaded");
    assert_eq!(remedy.provider, "official/traefik");
    assert_eq!(remedy.steps.len(), 3, "{remedy:#?}");
    assert_eq!(
        remedy.steps[2].path.as_deref(),
        Some(dynamic.to_str().expect("a UTF-8 path"))
    );
    assert_eq!(
        std::fs::read_to_string(&foreign).expect("the user file is still there"),
        text,
        "Stackly never changes the file"
    );

    let outside = site.root.path().join("mine.yaml");
    std::fs::rename(&foreign, &outside).expect("the user file moves out");
    api.wait_instance(id, "ready again", |summary| {
        summary.readiness.state == InstanceReadinessState::Ready
    })
    .await;
    assert_eq!(pid_of(&api.get(id).await.expect("the instance")), pid);

    stop_instance(&api, id).await;
    assert!(!alive(pid));
    daemon.shutdown().await;
    ev.pass_with(&format!(
        "mine.yaml made traefik {pid} not ready ({detail}); the remedy pointed at {}",
        dynamic.display()
    ));
}
