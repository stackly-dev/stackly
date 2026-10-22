//! `stackly instance create` for PostgreSQL through the real `stackly`
//! binary against the daemon on this Mac (spec 0020 AC-25, the CLI half):
//! the dry run shows the real port conflict, and the create with
//! `--start --wait` leaves an instance that is `ready` with a generated
//! password on the port that was asked for.

use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use serde_json::Value;
use stackly_protocol::instance::{InstanceLifecycle, InstanceReadinessState};
use stackly_testkit::cargo_bin;
use stackly_testkit::evidence::{Evidence, Kind, evidence};

use crate::create::generated_password;
use crate::harness::{PG17, PG18, PROVIDER, Pg, Site, free_port, stop_and_shutdown};

/// What one run of `stackly` left.
#[derive(Debug)]
struct Run {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

impl Run {
    /// The JSON envelope of `--json`.
    fn envelope(&self) -> Value {
        serde_json::from_str(self.stdout.trim())
            .unwrap_or_else(|error| panic!("{error}: {self:?}; stderr: {}", self.stderr))
    }
}

/// Runs `stackly <args>` for `root` to its end, off the runtime's thread so
/// the daemon in process keeps serving.
async fn stackly(root: PathBuf, args: Vec<String>) -> Run {
    tokio::task::spawn_blocking(move || {
        let output = Command::new(cargo_bin("stackly-cli", "stackly"))
            .args(args)
            .env("STACKLY_ROOT", root)
            .env("TZ", "UTC")
            .env_remove("NO_COLOR")
            .env_remove("CLICOLOR")
            .env_remove("CLICOLOR_FORCE")
            .env_remove("STACKLY_NO_INPUT")
            .env_remove("CI")
            .env("TERM", "xterm-256color")
            .stdin(Stdio::null())
            .output()
            .expect("stackly runs");
        Run {
            code: output.status.code(),
            stdout: String::from_utf8(output.stdout).expect("stdout is utf-8"),
            stderr: String::from_utf8(output.stderr).expect("stderr is utf-8"),
        }
    })
    .await
    .expect("the run's thread ends")
}

fn args(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).to_owned()).collect()
}

async fn is_created_from_the_cli_and_started_to_ready(pg: Pg, ev: Evidence) {
    let Some(site) = Site::open(&ev, pg) else {
        return;
    };
    let daemon = site.in_process_offline(|_| {}).await;
    let api = site.api().await;
    let root = site.root.path().to_path_buf();
    let runtime = format!("postgresql@{}", pg.major);
    let name = format!("cli{}", pg.major);
    let port = free_port();
    let port_arg = format!("client={port}");

    // The dry run sees a real listener on the exact address.
    let taker = TcpListener::bind(("127.0.0.1", port)).expect("the port binds");
    let refused = stackly(
        root.clone(),
        args(&[
            "instance",
            "create",
            &name,
            "--runtime",
            &runtime,
            "--provider",
            PROVIDER,
            "--port",
            &port_arg,
            "--dry-run",
            "--json",
        ]),
    )
    .await;
    let envelope = refused.envelope();
    let preflight = &envelope["result"]["preflight"];
    assert_eq!(preflight["ready"], false, "{refused:?}");
    let row = preflight["checks"]
        .as_array()
        .and_then(|checks| checks.iter().find(|check| check["id"] == "endpoint.client"))
        .unwrap_or_else(|| panic!("no endpoint row: {refused:?}"));
    assert_eq!(row["status"], "error", "{row}");
    assert_eq!(row["error"]["code"], "supervisor.endpoint_in_use", "{row}");
    drop(taker);

    // The same draft with the port free is ready.
    let dry = stackly(
        root.clone(),
        args(&[
            "instance",
            "create",
            &name,
            "--runtime",
            &runtime,
            "--provider",
            PROVIDER,
            "--port",
            &port_arg,
            "--dry-run",
            "--json",
        ]),
    )
    .await;
    assert_eq!(dry.code, Some(0), "{dry:?}");
    assert_eq!(
        dry.envelope()["result"]["preflight"]["ready"],
        true,
        "{dry:?}"
    );

    let created = stackly(
        root,
        args(&[
            "instance",
            "create",
            &name,
            "--runtime",
            &runtime,
            "--provider",
            PROVIDER,
            "--port",
            &port_arg,
            "--start",
            "--wait",
            "--timeout",
            "180s",
            "--json",
        ]),
    )
    .await;
    assert_eq!(created.code, Some(0), "{created:?}");
    let envelope = created.envelope();
    let result = &envelope["result"];
    let id: uuid::Uuid = result["instance"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("no instance: {created:?}"))
        .parse()
        .expect("an instance id");
    assert!(result["start"].is_object(), "{created:?}");

    let running = api.get(id).await.expect("the instance can be read");
    assert_eq!(running.lifecycle, InstanceLifecycle::Running, "{running:?}");
    assert_eq!(running.readiness.state, InstanceReadinessState::Ready);
    let client = running
        .endpoints
        .iter()
        .find(|endpoint| endpoint.name == "client")
        .expect("a client endpoint");
    assert_eq!((client.port, client.bound), (Some(port), true));
    let password = generated_password(&site, &api, id).await;
    let version = site.psql(id, port).scalar(&password, "SHOW server_version");
    assert_eq!(version, pg.version);
    site.subject(&ev, &version);

    stop_and_shutdown(&api, daemon, id).await;
    ev.pass_with(&format!(
        "stackly instance create made PostgreSQL {version} on 127.0.0.1:{port} with a generated password and started it to ready; the dry run showed supervisor.endpoint_in_use while a listener held the port"
    ));
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed PostgreSQL 18 artifact in the runtime cache"]
async fn postgresql_18_is_created_from_the_cli_and_started_to_ready() {
    let pg = PG18;
    let ev = evidence!(
        covers = ["AC-027", "REQ-008"],
        kind = Kind::Native,
        expected = "stackly instance create --dry-run reports supervisor.endpoint_in_use while a listener holds 127.0.0.1:p and ready after it stops, and the same command with --start --wait leaves PostgreSQL 18 running and ready on that port with a generated password",
    );
    is_created_from_the_cli_and_started_to_ready(pg, ev).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed PostgreSQL 17 artifact in the runtime cache"]
async fn postgresql_17_is_created_from_the_cli_and_started_to_ready() {
    let pg = PG17;
    let ev = evidence!(
        covers = ["AC-027", "REQ-008"],
        kind = Kind::Native,
        expected = "stackly instance create --dry-run reports supervisor.endpoint_in_use while a listener holds 127.0.0.1:p and ready after it stops, and the same command with --start --wait leaves PostgreSQL 17 running and ready on that port with a generated password",
    );
    is_created_from_the_cli_and_started_to_ready(pg, ev).await;
}
