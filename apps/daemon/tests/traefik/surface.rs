//! What a real Traefik 3.7.15 instance serves (spec 0041, step 5): `/ping`
//! and the read only API only on the loopback monitoring port, the
//! dashboard nowhere, only your routes on `web`, two loopback listeners at
//! the registry ports and no other, and no `tls` key in a rendered file. The
//! test is native, `#[ignore = "native: ..."]`, and ends as `BLOCKED` when
//! the artifact is not in the runtime cache.

use std::process::Command;

use serde_json::json;
use stackly_protocol::operation::OperationState;
use stackly_testkit::evidence::{Kind, evidence};
use uuid::Uuid;

use crate::config_calls::{Cfg, pid_of, settings_with};
use crate::harness::{
    Serve, Site, alive, apply_until_end, backend, create_instance, get_path, port_of, route,
    status_code, stop_instance,
};

/// The status code of `GET <path>` on `port` with `host`.
fn code(port: u16, host: &str, path: &str) -> u16 {
    status_code(&get_path(port, host, path).0)
}

/// The TCP lines of `lsof` for `pid`.
fn tcp_of(pid: u32) -> String {
    let output = Command::new("/usr/sbin/lsof")
        .args(["-a", "-p", &pid.to_string(), "-i", "-n", "-P"])
        .output()
        .expect("lsof runs");
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Whether a line of the YAML `text` is a `tls` key, at any indent. The
/// rendered files hold no flow mappings with keys, so a key is always the
/// start of a line.
fn holds_tls_key(text: &str) -> bool {
    text.lines().any(|line| {
        let key = line.trim_start().trim_start_matches("- ");
        key.starts_with("tls:") || key.starts_with("\"tls\":") || key == "tls"
    })
}

/// Reads the two rendered files of `id` and checks they hold no `tls` key
/// and the dashboard is off.
fn rendered_files_hold_no_tls(site: &Site, id: Uuid) {
    let config = site.instance_paths(id).config;
    for file in ["traefik.yaml", "dynamic/routes.yaml"] {
        let text = std::fs::read_to_string(config.join(file)).expect("a rendered file reads");
        assert!(!holds_tls_key(&text), "{file}: {text}");
        if file == "traefik.yaml" {
            assert!(
                text.contains("api:\n  dashboard: false\n"),
                "the dashboard is off: {text}"
            );
        }
    }
}

// covers: AC-5, AC-6, AC-7 (spec 0041)
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed Traefik 3.7.15 artifact in the runtime cache"]
async fn ping_and_the_api_answer_only_on_the_monitoring_port_and_the_dashboard_is_off() {
    let ev = evidence!(
        covers = ["REQ-039", "AC-039"],
        kind = Kind::Native,
        expected = "on Traefik 3.7.15 /ping and /api/http/routers answer 200 on the ping port and /dashboard/ 404; on the web port all three answer 404 with no route and for another Host with one route; the process listens only on 127.0.0.1 at the two registry ports; the rendered files hold no tls key and dashboard false",
    );
    let Some(site) = Site::open(&ev, Serve::InProcess) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let cfg = Cfg { api: &api };
    let alpha = backend("alpha");

    let id = create_instance(&site, &api, "edge-surface").await;
    let running = api.start_ready(id).await;
    site.subject(&ev, "3.7.15");
    let pid = pid_of(&running);
    let (web, ping) = (port_of(&running, "web"), port_of(&running, "ping"));

    // No route yet: Traefik answers its own paths only on ping.
    assert_eq!(code(ping, "127.0.0.1", "/ping"), 200);
    assert_eq!(code(ping, "127.0.0.1", "/api/http/routers"), 200);
    assert_eq!(code(ping, "127.0.0.1", "/dashboard/"), 404);
    for path in ["/ping", "/api/http/routers", "/dashboard/", "/"] {
        assert_eq!(code(web, "127.0.0.1", path), 404, "web {path}");
    }
    rendered_files_hold_no_tls(&site, id);

    // Two listeners, both loopback, both the registry ports.
    let tcp = tcp_of(pid);
    let mut listening: Vec<String> = tcp
        .lines()
        .filter(|line| line.contains("(LISTEN)"))
        .filter_map(|line| line.split_whitespace().rev().nth(1).map(str::to_owned))
        .collect();
    listening.sort();
    let mut expected = vec![format!("127.0.0.1:{web}"), format!("127.0.0.1:{ping}")];
    expected.sort();
    assert_eq!(listening, expected, "{tcp}");

    // One route: only a request for its host reaches the backend.
    let view = cfg.view(id).await;
    let applied = apply_until_end(
        &api,
        id,
        view.revision.number.get(),
        settings_with(
            &view,
            json!({ "routes": [route("app", "Host(`app.localhost`)", alpha)] }),
        ),
    )
    .await;
    assert_eq!(
        applied.operation.state,
        OperationState::Succeeded,
        "{applied:?}"
    );
    assert_eq!(get_path(web, "app.localhost", "/").1, "alpha");
    for path in ["/ping", "/api/http/routers", "/dashboard/"] {
        assert_eq!(code(web, "other.localhost", path), 404, "web {path}");
    }
    assert_eq!(code(ping, "127.0.0.1", "/dashboard/"), 404);
    assert_eq!(code(ping, "127.0.0.1", "/ping"), 200);
    rendered_files_hold_no_tls(&site, id);
    let tcp = tcp_of(pid);
    let listeners = tcp.lines().filter(|line| line.contains("(LISTEN)")).count();
    assert_eq!(listeners, 2, "{tcp}");

    stop_instance(&api, id).await;
    assert!(!alive(pid));
    daemon.shutdown().await;
    ev.pass_with(&format!(
        "ping {ping} served /ping and the API, web {web} served only the route; listeners {listening:?}"
    ));
}
