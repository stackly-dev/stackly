//! The Traefik journey from the `stackly` CLI against the real `stacklyd`
//! binary and Traefik 3.7.15 (spec 0041 step 11, AC-9, AC-13): a routes
//! change through `--set-json` is a reload that keeps the process, a log
//! level change through `--set` is a restart with a new one, `instance
//! connection --all` shows both endpoints, and `doctor --instance` brings
//! the `routes_not_loaded` remedy for a rule Traefik rejects. The CLI is the
//! `stackly` binary through `CliRunner`. The test is native,
//! `#[ignore = "native: ..."]`, and ends as `BLOCKED` when the artifact is
//! not in the runtime cache.

use std::time::{Duration, Instant};

use serde_json::{Value, json};
use stackly_protocol::instance::InstanceReadinessState;
use stackly_testkit::evidence::{Kind, evidence};
use stackly_testkit::{CliOutput, CliRunner, cargo_bin};

use crate::config_calls::pid_of;
use crate::harness::{
    Serve, Site, alive, backend, create_with, eventually, get, port_of, route, server_version,
    stop_instance,
};

/// The rule of the route the test moves.
const APP_RULE: &str = "Host(`app.localhost`)";
/// The instance name the CLI commands take.
const NAME: &str = "edge-cli";

/// The `result` of a run that exited 0.
fn result_of(run: &CliOutput) -> Value {
    assert_eq!(run.code(), Some(0), "{run:?}");
    run.json().expect("the envelope is JSON")["result"].clone()
}

/// The `--set-json` argument that sets `routes` to `routes`.
fn routes_arg(routes: &Value) -> String {
    format!("routes={routes}")
}

/// The impact `instance config apply <NAME> <change> --dry-run` judges.
fn dry_run_impact(cli: &CliRunner, change: &[&str]) -> String {
    let mut args = vec!["instance", "config", "apply", NAME];
    args.extend_from_slice(change);
    args.push("--dry-run");
    let verdict = result_of(&cli.run(&args).expect("the dry run runs"));
    assert_eq!(verdict["valid"], true, "{verdict}");
    verdict["impact"]
        .as_str()
        .unwrap_or_else(|| panic!("a dry run judges an impact: {verdict}"))
        .to_owned()
}

/// Runs `instance config apply <NAME> <change> --wait`.
fn apply(cli: &CliRunner, change: &[&str]) -> CliOutput {
    let mut args = vec!["instance", "config", "apply", NAME];
    args.extend_from_slice(change);
    args.push("--wait");
    cli.run(&args).expect("the apply runs")
}

// covers: AC-9, AC-13 (spec 0041)
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed Traefik 3.7.15 artifact in the runtime cache"]
async fn cli_applies_routes_with_a_reload_and_the_log_level_with_a_restart() {
    let ev = evidence!(
        covers = ["REQ-039", "AC-039"],
        kind = Kind::Native,
        expected = "on Traefik 3.7.15 behind the stacklyd binary, stackly instance config apply --set-json routes=<json> is a reload impact, succeeds, keeps the pid, and the route answers from the new backend; --set logLevel=DEBUG is a restart impact with a new pid and the route still answers; instance connection --all shows the Web URL and the Health URL on the registry ports; an apply with a rule Traefik rejects fails with supervisor.reload_failed and doctor --instance brings the routes_not_loaded remedy with three steps, the third the dynamic folder of the instance",
    );
    let Some(site) = Site::open(&ev, Serve::Binary) else {
        return;
    };
    let alpha = backend("alpha");
    let bravo = backend("bravo");
    let good = backend("good");
    let daemon = site.spawn();
    let api = site.binary_api(&daemon).await;
    let id = create_with(
        &site,
        &api,
        NAME,
        json!({ "routes": [route("api", APP_RULE, alpha)] }),
    )
    .await;
    let running = api.start_ready(id).await;
    let (web, ping) = (port_of(&running, "web"), port_of(&running, "ping"));
    site.subject(&ev, &server_version(ping));
    let first_pid = pid_of(&running);
    assert_eq!(get(web, "app.localhost").1, "alpha");
    let cli = CliRunner::new(&cargo_bin("stackly-cli", "stackly"), &site.data_root);

    // routes: a reload
    let moved = routes_arg(&json!([route("api", APP_RULE, bravo)]));
    let change = ["--set-json", moved.as_str()];
    assert_eq!(dry_run_impact(&cli, &change), "reload");
    let changed_at = Instant::now();
    let applied = result_of(&apply(&cli, &change));
    assert_eq!(applied["operation"]["state"], "succeeded", "{applied}");
    eventually(
        "the route answers from its new backend",
        Duration::from_secs(30).saturating_sub(changed_at.elapsed()),
        || get(web, "app.localhost").1 == "bravo",
    )
    .await;
    let reloaded = api.get(id).await.expect("the instance can be read");
    assert_eq!(pid_of(&reloaded), first_pid, "a reload, not a restart");

    // logLevel: a restart
    let change = ["--set", "logLevel=DEBUG"];
    assert_eq!(dry_run_impact(&cli, &change), "restart");
    let applied = result_of(&apply(&cli, &change));
    assert_eq!(applied["operation"]["state"], "succeeded", "{applied}");
    let restarted = api
        .wait_instance(id, "ready after the restart", |summary| {
            summary.readiness.state == InstanceReadinessState::Ready
                && summary
                    .process
                    .as_ref()
                    .is_some_and(|process| process.pid != first_pid)
        })
        .await;
    let second_pid = pid_of(&restarted);
    assert!(!alive(first_pid), "the old process ended");
    assert_eq!(port_of(&restarted, "web"), web);
    eventually(
        "the route answers after the restart",
        Duration::from_secs(30),
        || get(web, "app.localhost").1 == "bravo",
    )
    .await;

    // connection --all
    let shown = result_of(
        &cli.run(&["instance", "connection", NAME, "--all"])
            .expect("the connection runs"),
    );
    let connections = shown["connections"]
        .as_array()
        .expect("the connections are a list");
    let masked: Vec<(&str, &str, &str)> = connections
        .iter()
        .map(|entry| {
            (
                entry["name"].as_str().unwrap_or_default(),
                entry["state"].as_str().unwrap_or_default(),
                entry["masked"].as_str().unwrap_or_default(),
            )
        })
        .collect();
    let web_url = format!("http://127.0.0.1:{web}");
    let ping_url = format!("http://127.0.0.1:{ping}/ping");
    assert_eq!(
        masked,
        [
            ("web", "available", web_url.as_str()),
            ("ping", "available", ping_url.as_str()),
        ],
        "{shown}"
    );

    // a rule Traefik rejects, then the doctor
    let broken = routes_arg(&json!([
        route("good", "Host(`good.localhost`)", good),
        route("broken", "Host(", bravo),
    ]));
    let failed = apply(&cli, &["--set-json", broken.as_str()]);
    assert_ne!(failed.code(), Some(0), "{failed:?}");
    let envelope = failed.json().expect("the envelope is JSON");
    let operation = &envelope["result"]["operation"];
    assert_eq!(operation["state"], "failed", "{envelope}");
    assert_eq!(
        operation["failure"]["error"]["code"], "supervisor.reload_failed",
        "{envelope}"
    );
    let report = result_of(
        &cli.run(&["doctor", "--instance", NAME])
            .expect("the doctor runs"),
    );
    let remedies: Vec<&Value> = report["report"]["checks"]
        .as_array()
        .expect("the report has checks")
        .iter()
        .flat_map(|check| check["findings"].as_array().into_iter().flatten())
        .filter_map(|finding| finding.get("remedy").filter(|remedy| !remedy.is_null()))
        .collect();
    let remedy = remedies
        .iter()
        .find(|remedy| remedy["name"] == "routes_not_loaded")
        .unwrap_or_else(|| panic!("the doctor brings routes_not_loaded: {report}"));
    assert_eq!(remedy["provider"], "official/traefik", "{remedy}");
    let steps = remedy["steps"].as_array().expect("a remedy has steps");
    assert_eq!(steps.len(), 3, "{remedy}");
    let dynamic = site.instance_paths(id).config.join("dynamic");
    assert_eq!(
        steps[2]["path"].as_str(),
        Some(dynamic.display().to_string().as_str()),
        "{remedy}"
    );

    stop_instance(&api, id).await;
    assert!(!alive(second_pid));
    drop(api);
    daemon.stop().expect("the daemon stops");
    ev.pass_with(&format!(
        "routes reloaded with traefik {first_pid} kept, logLevel restarted it as {second_pid}, connection --all showed {web_url} and {ping_url}, and doctor brought routes_not_loaded with {}",
        dynamic.display()
    ));
}
