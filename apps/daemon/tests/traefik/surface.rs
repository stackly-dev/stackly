//! What a real Traefik 3.7.15 instance serves (spec 0041, step 5): `/ping`
//! and the read only API only on the loopback monitoring port, the
//! dashboard nowhere, only your routes on `web`, two loopback listeners at
//! the registry ports and no other, and no TLS configuration in a rendered
//! file (the AC-7 rule, read on the parsed key paths), also with a route
//! named `tls`. The
//! test is native, `#[ignore = "native: ..."]`, and ends as `BLOCKED` when
//! the artifact is not in the runtime cache.

use std::process::Command;

use serde_json::json;
use stackly_protocol::operation::OperationState;
use stackly_testkit::evidence::{Kind, evidence};
use uuid::Uuid;

use crate::config_calls::{Cfg, pid_of, settings_with};
use crate::harness::{
    Serve, Site, alive, api_json, apply_until_end, backend, create_instance, eventually, get_path,
    port_of, route, server_version, status_code, stop_instance,
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

/// The key path of every mapping key in the block YAML `text`, such as
/// `["http", "routers", "api", "rule"]`. The rendered files are block YAML
/// with no flow mappings, so indentation alone gives the tree; a list item
/// (`- `) opens a level of its own, and quotes around a key are dropped.
/// `stacklyd` has no YAML parser among its dependencies, and this is the
/// same parse for the two rendered files only.
fn key_paths(text: &str) -> Vec<Vec<String>> {
    let mut stack: Vec<(usize, String)> = Vec::new();
    let mut paths = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let mut indent = line.len() - trimmed.len();
        let mut rest = trimmed;
        while let Some(item) = rest.strip_prefix("- ") {
            indent += 2;
            rest = item;
        }
        let Some((key, _)) = split_key(rest) else {
            continue;
        };
        while stack.last().is_some_and(|(level, _)| *level >= indent) {
            stack.pop();
        }
        stack.push((indent, key));
        paths.push(stack.iter().map(|(_, key)| key.clone()).collect());
    }
    paths
}

/// The key of a `key: value` or `key:` line, quotes dropped, or `None` for
/// a line that holds no key.
fn split_key(line: &str) -> Option<(String, &str)> {
    if let Some(quoted) = line.strip_prefix('"') {
        let (key, rest) = quoted.split_once('"')?;
        let rest = rest.strip_prefix(':')?;
        return Some((key.to_owned(), rest));
    }
    let (key, rest) = line.split_once(':')?;
    (!key.is_empty() && !key.contains(' ')).then(|| (key.to_owned(), rest))
}

/// The static file rule of AC-7: `traefik.yaml` configures TLS when a key
/// named `tls` or `certificatesResolvers` sits at any depth.
fn static_holds_tls(text: &str) -> bool {
    key_paths(text).iter().any(|path| {
        path.iter()
            .any(|key| key == "tls" || key == "certificatesResolvers")
    })
}

/// The routes file rule of AC-7: `dynamic/routes.yaml` configures TLS when
/// it has a top level `tls` key, or a `tls` key inside the body of one
/// router or one service. A router or service named `tls` is a name.
fn routes_hold_tls(text: &str) -> bool {
    key_paths(text).iter().any(|path| match path.as_slice() {
        [top, ..] if top == "tls" => true,
        [http, section, _name, body @ ..]
            if http == "http" && (section == "routers" || section == "services") =>
        {
            body.iter().any(|key| key == "tls")
        }
        _ => false,
    })
}

/// Reads the two rendered files of `id` and checks they hold no TLS
/// configuration and the dashboard is off.
fn rendered_files_hold_no_tls(site: &Site, id: Uuid) {
    let config = site.instance_paths(id).config;
    let static_config =
        std::fs::read_to_string(config.join("traefik.yaml")).expect("traefik.yaml reads");
    assert!(!static_holds_tls(&static_config), "{static_config}");
    assert!(
        static_config.contains("api:\n  dashboard: false\n"),
        "the dashboard is off: {static_config}"
    );
    let routes =
        std::fs::read_to_string(config.join("dynamic/routes.yaml")).expect("routes.yaml reads");
    assert!(!routes_hold_tls(&routes), "{routes}");
}

#[test]
fn the_tls_rules_of_ac_7_read_names_as_names_and_bodies_as_configuration() {
    let named = "http:\n  routers:\n    \"tls\":\n      rule: \"Host(`tls.localhost`)\"\n      service: \"tls\"\n  services:\n    \"tls\":\n      loadBalancer:\n        servers:\n          - url: \"http://127.0.0.1:9\"\n";
    assert!(!routes_hold_tls(named));
    assert!(routes_hold_tls(
        "http:\n  routers:\n    \"api\":\n      tls: {}\n"
    ));
    assert!(routes_hold_tls(
        "http:\n  services:\n    \"api\":\n      loadBalancer:\n        servers:\n          - url: x\n            tls: true\n"
    ));
    assert!(routes_hold_tls("tls:\n  options: {}\n"));
    assert!(static_holds_tls(
        "entryPoints:\n  web:\n    http:\n      tls:\n        certResolver: x\n"
    ));
    assert!(static_holds_tls("certificatesResolvers:\n  x: {}\n"));
    assert!(!static_holds_tls(
        "api:\n  dashboard: false\n  insecure: true\n"
    ));
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
    site.subject(&ev, &server_version(port_of(&running, "ping")));
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

// covers: AC-7 (spec 0041)
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed Traefik 3.7.15 artifact in the runtime cache"]
async fn a_route_named_tls_loads_enabled_and_the_rendered_files_hold_no_tls_configuration() {
    let ev = evidence!(
        covers = ["REQ-039", "AC-039"],
        kind = Kind::Native,
        expected = "a route named tls passes validate and apply, Traefik 3.7.15 loads the router and service tls@file enabled and answers from its backend on web, and the rendered files on disk hold a router and a service named tls but no TLS configuration",
    );
    let Some(site) = Site::open(&ev, Serve::InProcess) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let cfg = Cfg { api: &api };
    let alpha = backend("alpha");

    let id = create_instance(&site, &api, "edge-tls-name").await;
    let running = api.start_ready(id).await;
    let (web, ping) = (port_of(&running, "web"), port_of(&running, "ping"));
    site.subject(&ev, &server_version(ping));
    let pid = pid_of(&running);

    let view = cfg.view(id).await;
    let change = settings_with(
        &view,
        json!({ "routes": [route("tls", "Host(`tls.localhost`)", alpha)] }),
    );
    let verdict = cfg
        .validate(id, view.revision.number.get(), change.clone())
        .await;
    assert!(verdict.valid, "{verdict:#?}");
    let applied = apply_until_end(&api, id, view.revision.number.get(), change).await;
    assert_eq!(
        applied.operation.state,
        OperationState::Succeeded,
        "{applied:?}"
    );
    let loaded = |path: &str| {
        api_json(ping, path)
            .as_array()
            .expect("the API answers a list")
            .iter()
            .find(|entry| entry["name"] == "tls@file")
            .map(|entry| entry["status"].clone())
    };
    eventually(
        "tls@file is loaded",
        std::time::Duration::from_secs(30),
        || loaded("/api/http/routers").is_some(),
    )
    .await;
    assert_eq!(loaded("/api/http/routers"), Some(json!("enabled")));
    assert_eq!(loaded("/api/http/services"), Some(json!("enabled")));
    assert_eq!(get_path(web, "tls.localhost", "/").1, "alpha");
    let routes =
        std::fs::read_to_string(site.instance_paths(id).config.join("dynamic/routes.yaml"))
            .expect("routes.yaml reads");
    let paths = key_paths(&routes);
    for section in ["routers", "services"] {
        assert!(
            paths
                .iter()
                .any(|path| path.as_slice() == ["http", section, "tls"]),
            "{routes}"
        );
    }
    rendered_files_hold_no_tls(&site, id);
    assert_eq!(pid_of(&api.get(id).await.expect("the instance")), pid);

    stop_instance(&api, id).await;
    assert!(!alive(pid));
    daemon.shutdown().await;
    ev.pass_with("tls@file loaded enabled and answered alpha; the rendered files hold the name tls and no TLS configuration");
}
