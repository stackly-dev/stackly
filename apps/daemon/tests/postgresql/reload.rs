//! A reload of the running postmaster (spec 0019 AC-13, spec 0010 AC-14).
//!
//! No contract method reloads yet (feature 29), so the test stops its
//! in process daemon, which leaves the postmaster running, builds a
//! supervisor of its own on the same registry that adopts that postmaster,
//! writes the new config revision through the registry, and calls
//! `execute_reload`, the way the spec says. // provisional: F29

use stackly_testkit::evidence::{Evidence, Kind, evidence};

use crate::harness::{PG17, PG18, Pg, Site, create_instance, stop_and_shutdown};
use crate::phases::{readopt, reload_log_statement};

async fn applies_a_reloadable_setting_with_sighup_and_keeps_its_pid(pg: Pg, ev: Evidence) {
    let Some(site) = Site::open(&ev, pg) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let made = create_instance(&site, &api, "reloaded").await;
    let running = api.start_ready(made.id).await;
    let pid = running.process.as_ref().expect("a running process").pid;
    let psql = site.psql(made.id, made.port);
    let version = psql.scalar(&made.password, "SHOW server_version");
    site.subject(&ev, &version);
    assert_eq!(psql.scalar(&made.password, "SHOW log_statement"), "none");
    let started = psql.scalar(&made.password, "SELECT pg_postmaster_start_time()");
    drop(api);
    daemon.shutdown().await;

    // The postmaster outlives the daemon; a supervisor of the test adopts it.
    reload_log_statement(&site, &made, pid, &started).await;

    // A daemon adopts the postmaster again and stops it.
    let (daemon, api) = readopt(&site, &made, pid).await;
    stop_and_shutdown(&api, daemon, made.id).await;
    ev.pass_with(&format!("log_statement became all on postmaster {pid}"));
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed PostgreSQL 17 artifact in the runtime cache"]
async fn postgresql_17_applies_a_reloadable_setting_with_sighup_and_keeps_its_pid() {
    let pg = PG17;
    let ev = evidence!(
        covers = ["REQ-035", "AC-035"],
        kind = Kind::Native,
        expected = "after logStatement is changed to all in a new config revision, execute_reload makes SHOW log_statement say all with the same postmaster pid and start time",
    );
    applies_a_reloadable_setting_with_sighup_and_keeps_its_pid(pg, ev).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed PostgreSQL 18 artifact in the runtime cache"]
async fn postgresql_18_applies_a_reloadable_setting_with_sighup_and_keeps_its_pid() {
    let pg = PG18;
    let ev = evidence!(
        covers = ["REQ-035", "AC-035"],
        kind = Kind::Native,
        expected = "after logStatement is changed to all in a new config revision, execute_reload makes SHOW log_statement say all with the same postmaster pid and start time",
    );
    applies_a_reloadable_setting_with_sighup_and_keeps_its_pid(pg, ev).await;
}
