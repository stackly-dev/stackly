//! The durable log of a real PostgreSQL instance (spec 0027 AC-6, AC-8,
//! *Critical test scenarios*, Native): `database system is ready` on stderr
//! and the `FATAL` line of a wrong login, read through the real `stackly`
//! binary with `instance logs --json`.

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;
use stackly_testkit::cargo_bin;
use stackly_testkit::evidence::{Evidence, Kind, evidence};

use crate::harness::{PG17, Pg, SUPERUSER, Site, Via, create_instance, stop_and_shutdown};
use crate::support::PATIENCE;

const READY: &str = "database system is ready to accept connections";

/// One line of `instance logs --json`: stream, level, and text.
#[derive(Debug)]
struct Line {
    stream: String,
    level: String,
    text: String,
}

/// The line items `stackly instance logs <name> --tail 500 --json` prints
/// for `root`, off the runtime's thread so the daemon in process keeps
/// serving.
async fn cli_lines(root: PathBuf, name: String) -> Vec<Line> {
    tokio::task::spawn_blocking(move || {
        let output = Command::new(cargo_bin("stackly-cli", "stackly"))
            .args(["instance", "logs", &name, "--tail", "500", "--json"])
            .env("STACKLY_ROOT", root)
            .env("TZ", "UTC")
            .stdin(Stdio::null())
            .output()
            .expect("stackly runs");
        assert_eq!(
            output.status.code(),
            Some(0),
            "instance logs: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .filter(|line| line["type"] == "item")
            .filter_map(|line| {
                let entry = &line["item"]["content"];
                let kind = &entry["kind"];
                (kind["type"] == "line").then(|| Line {
                    stream: entry["stream"].as_str().unwrap_or_default().to_owned(),
                    level: kind["content"]["level"]
                        .as_str()
                        .unwrap_or_default()
                        .to_owned(),
                    text: kind["content"]["text"]
                        .as_str()
                        .unwrap_or_default()
                        .to_owned(),
                })
            })
            .collect()
    })
    .await
    .expect("the run's thread ends")
}

async fn writes_ready_and_a_failed_login_to_its_log(pg: Pg, ev: Evidence) {
    let Some(site) = Site::open(&ev, pg) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let name = format!("logs{}", pg.major);
    let made = create_instance(&site, &api, &name).await;
    api.start_ready(made.id).await;
    let version = site
        .psql(made.id, made.port)
        .scalar(&made.password, "SHOW server_version");
    assert_eq!(version, pg.version);
    site.subject(&ev, &version);

    let refused = site.psql(made.id, made.port).run(
        Via::Tcp,
        SUPERUSER,
        Some("not-the-superuser-password"),
        "postgres",
        "SELECT 1",
    );
    assert!(!refused.ok, "a wrong password is refused: {refused:?}");

    let root = site.root.path().to_path_buf();
    let deadline = Instant::now() + PATIENCE;
    let (ready, fatal) = loop {
        let lines = cli_lines(root.clone(), name.clone()).await;
        let ready = lines
            .iter()
            .find(|line| line.text.contains(READY))
            .map(|line| (line.stream.clone(), line.level.clone()));
        let fatal = lines
            .iter()
            .find(|line| line.text.contains("FATAL:") && line.text.contains("password"))
            .map(|line| (line.stream.clone(), line.level.clone(), line.text.clone()));
        if let (Some(ready), Some(fatal)) = (&ready, &fatal) {
            break (ready.clone(), fatal.clone());
        }
        assert!(
            Instant::now() < deadline,
            "no ready or FATAL line yet: {lines:#?}"
        );
        tokio::time::sleep(Duration::from_millis(500)).await;
    };

    assert_eq!(ready.0, "stderr", "the ready line is on stderr");
    assert_eq!(ready.1, "normal", "the ready line is a normal line");
    assert_eq!(fatal.1, "error", "the FATAL line is an error: {}", fatal.2);
    assert!(
        !fatal.2.contains("not-the-superuser-password"),
        "the wrong password never reaches the log"
    );
    stop_and_shutdown(&api, daemon, made.id).await;
    ev.pass_with(&format!(
        "PostgreSQL {}: `{READY}` on {} as {}, the failed login `{}` on {} as {}",
        pg.version, ready.0, ready.1, fatal.2, fatal.0, fatal.1
    ));
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed PostgreSQL 17 artifact in the runtime cache"]
async fn postgresql_17_writes_ready_and_a_failed_login_to_its_durable_log() {
    let ev = evidence!(
        covers = ["REQ-017", "AC-017"],
        kind = Kind::Native,
        expected = "spec 0027 AC-6, AC-8: a PostgreSQL 17 instance started through the daemon has `database system is ready to accept connections` on stderr as a normal line in stackly instance logs --json, and the FATAL line of a login with a wrong password as an error line",
    );
    writes_ready_and_a_failed_login_to_its_log(PG17, ev).await;
}
