//! The `traefik-routes` readiness probe against a real `traefik` process
//! (spec 0009 AC-22, milestone 6 step 25): a start with one route is ready
//! only once Traefik loaded it, a reload with a new route is verified ready,
//! and a reload with a rule the rule parser of Traefik rejects fails its
//! verify, while `/ping` keeps answering 200. The test is native,
//! `#[ignore = "native: ..."]`, and ends as `BLOCKED` when the artifact is
//! not in the runtime cache.

use std::time::Duration;

use serde_json::json;
use stackly_protocol::instance::{InstanceReadinessState, SettingsDocument};
use stackly_protocol::operation::{OperationDetail, OperationState};
use stackly_testkit::evidence::{Kind, evidence};
use uuid::Uuid;

use crate::config_calls::{Cfg, pid_of, settings_with};
use crate::harness::{Serve, Site, alive, backend, create_params, get, port_of, stop_instance};
use crate::support::Api;

/// Longer than `timeouts.reloadSeconds` of the package (30 s), so a reload
/// whose verify runs out its deadline still ends inside the wait.
const RELOAD_PATIENCE: Duration = Duration::from_secs(90);

/// Applies `change` and waits until the operation ended, whatever its
/// state.
async fn apply_until_end(
    cfg: &Cfg<'_>,
    api: &Api,
    id: Uuid,
    base: u64,
    change: stackly_protocol::instance::config::ConfigChangeRequest,
) -> OperationDetail {
    let accepted = cfg
        .apply(id, base, change)
        .await
        .expect("the apply is accepted")
        .accepted
        .expect("a change begins an operation");
    let deadline = tokio::time::Instant::now() + RELOAD_PATIENCE;
    loop {
        let detail = api.operation(accepted.operation.id).await;
        if matches!(
            detail.operation.state,
            OperationState::Succeeded
                | OperationState::Failed
                | OperationState::Cancelled
                | OperationState::NeedsRecovery
        ) {
            return detail;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the apply did not end: {detail:?}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// The status line of `GET /ping` on the loopback entry point `port`.
fn ping_status(port: u16) -> String {
    use std::io::{Read, Write};

    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).expect("the port accepts");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("a read timeout can be set");
    stream
        .write_all(b"GET /ping HTTP/1.0\r\nHost: 127.0.0.1\r\n\r\n")
        .expect("the request is written");
    let mut reply = Vec::new();
    let _ = stream.read_to_end(&mut reply);
    String::from_utf8_lossy(&reply)
        .lines()
        .next()
        .unwrap_or_default()
        .to_owned()
}

// covers: AC-22
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed Traefik 3.7.15 artifact in the runtime cache"]
async fn traefik_routes_readiness_follows_the_routers_traefik_loaded_through_start_and_reloads() {
    let ev = evidence!(
        covers = ["REQ-029", "REQ-015", "AC-015"],
        kind = Kind::Native,
        expected = "Traefik 3.7.15 created with one route starts ready and serves it; a reload adding a second route succeeds once Traefik loaded it; a reload with the rule Host( fails its verify with the same traefik pid while /ping still answers 200",
    );
    let Some(site) = Site::open(&ev, Serve::InProcess) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let cfg = Cfg { api: &api };
    let alpha = backend("alpha");
    let bravo = backend("bravo");
    let route = |name: &str, rule: &str, port: u16| json!({ "name": name, "rule": rule, "backendUrl": format!("http://127.0.0.1:{port}") });

    // Start with one route: ready means Traefik loaded it.
    let mut params = create_params(&site, "edge-probe");
    let mut settings = serde_json::Map::new();
    settings.insert(
        "routes".to_owned(),
        json!([route("api", "Host(`a.localhost`)", alpha)]),
    );
    params.settings = SettingsDocument::new(settings);
    let accepted = api
        .create(params)
        .await
        .expect("the create is accepted")
        .accepted
        .expect("a create starts an operation");
    let id = accepted
        .operation
        .resource
        .as_ref()
        .map(|resource| resource.id.parse::<Uuid>().expect("an instance id"))
        .expect("the operation names the instance");
    let created = api
        .wait_state(accepted.operation.id, OperationState::Succeeded)
        .await;
    assert_eq!(
        created.operation.state,
        OperationState::Succeeded,
        "{created:?}"
    );
    let running = api.start_ready(id).await;
    let pid = pid_of(&running);
    let (web, ping) = (port_of(&running, "web"), port_of(&running, "ping"));
    site.subject(&ev, "3.7.15");
    assert_eq!(running.readiness.state, InstanceReadinessState::Ready);
    let (status, body) = get(web, "a.localhost");
    assert!(
        status.contains("200") && body == "alpha",
        "ready before the route was served: {status} {body}"
    );
    let view = cfg
        .wait_view(id, "revision 1 as last known good", |view| {
            view.last_known_good.map(|n| n.get()) == Some(1)
        })
        .await;

    // A reload with a second route is verified once Traefik loaded it.
    let two = settings_with(
        &view,
        json!({ "routes": [
            route("api", "Host(`a.localhost`)", alpha),
            route("docs", "Host(`b.localhost`) && PathPrefix(`/`)", bravo),
        ] }),
    );
    let reloaded = apply_until_end(&cfg, &api, id, 1, two).await;
    assert_eq!(
        reloaded.operation.state,
        OperationState::Succeeded,
        "{reloaded:?}"
    );
    let (status, body) = get(web, "b.localhost");
    assert!(
        status.contains("200") && body == "bravo",
        "the verified reload does not serve the new route: {status} {body}"
    );
    let view = cfg
        .wait_view(id, "revision 2 as last known good", |view| {
            view.last_known_good.map(|n| n.get()) == Some(2)
        })
        .await;
    assert_eq!(pid_of(&api.get(id).await.expect("the instance")), pid);

    // A rule the parser rejects: Traefik keeps answering /ping, but the
    // router is not loaded as enabled, so the verify fails.
    let broken = settings_with(
        &view,
        json!({ "routes": [
            route("api", "Host(`a.localhost`)", alpha),
            route("docs", "Host(", bravo),
        ] }),
    );
    let failed = apply_until_end(&cfg, &api, id, 2, broken).await;
    assert_eq!(failed.operation.state, OperationState::Failed, "{failed:?}");
    let failure = format!("{:?}", failed.operation.failure);
    assert!(
        ping_status(ping).contains("200"),
        "Traefik stopped answering /ping"
    );
    let summary = api.get(id).await.expect("the instance");
    assert_eq!(pid_of(&summary), pid, "a failed reload keeps the process");
    let after = cfg.view(id).await;
    assert_eq!(
        after.last_known_good.map(|n| n.get()),
        Some(2),
        "{after:#?}"
    );
    assert!(failure.contains("supervisor.reload_failed"), "{failure}");
    assert_eq!(summary.readiness.state, InstanceReadinessState::NotReady);
    assert_eq!(summary.readiness.reason, "routes_mismatch");
    let readiness = summary.readiness.detail.clone().unwrap_or_default();
    assert!(readiness.contains("\"docs\""), "{readiness}");
    assert!(
        !readiness.contains("Host("),
        "a detail carries no rule text: {readiness}"
    );

    stop_instance(&api, id).await;
    assert!(!alive(pid));
    daemon.shutdown().await;
    ev.pass_with(&format!(
        "start and the two route reload were ready with traefik {pid}; the Host( reload failed verify, readiness routes_mismatch: {readiness}"
    ));
}
