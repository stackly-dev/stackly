//! Only the programs of the bound runtime run (spec 0019 AC-12).

use std::os::unix::fs::PermissionsExt;

use stackly_testkit::evidence::{Kind, evidence};

use crate::harness::{PG17, PG18, Pg, Site, create_instance};

async fn never_runs_a_decoy_psql_initdb_or_postgres_from_the_daemon_path(pg: Pg) {
    let ev = evidence!(
        covers = ["REQ-035", "AC-035"],
        kind = Kind::Native,
        expected = "with decoy psql, initdb, and postgres first on the daemon's PATH, create, start, readiness, and stop work and no decoy ever runs",
    );
    let Some(site) = Site::open(&ev, pg) else {
        return;
    };
    let decoys = site.root.path().join("decoys");
    let marker = site.root.path().join("decoy-ran");
    std::fs::create_dir_all(&decoys).expect("the decoy folder can be made");
    for name in ["psql", "initdb", "postgres", "pg_ctl"] {
        let file = decoys.join(name);
        std::fs::write(
            &file,
            format!(
                "#!/bin/sh\necho \"$0 $*\" >> '{}'\nexit 1\n",
                marker.display()
            ),
        )
        .expect("a decoy can be written");
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755))
            .expect("a decoy can be made executable");
    }
    let path = format!("{}:/usr/bin:/bin", decoys.display());
    let daemon = site.binary(&[("PATH", &path)]);
    let api = site.binary_api(&daemon).await;

    let made = create_instance(&site, &api, "decoyed").await;
    api.start_ready(made.id).await;
    let psql = site.psql(made.id, made.port);
    let version = psql.scalar(&made.password, "SHOW server_version");
    site.subject(&ev, &version);
    // Let several probe rounds run.
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    let accepted = api.stop_when_free(made.id).await.accepted.expect("a stop");
    api.wait_state(
        accepted.operation.id,
        stackly_protocol::operation::OperationState::Succeeded,
    )
    .await;
    daemon.stop();

    assert!(
        !marker.exists(),
        "a decoy ran: {}",
        std::fs::read_to_string(&marker).unwrap_or_default()
    );
    ev.pass_with("create, start, readiness, and stop ran with decoys first on PATH and none ran");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed PostgreSQL 17 artifact in the runtime cache"]
async fn postgresql_17_never_runs_a_decoy_psql_initdb_or_postgres_from_the_daemon_path() {
    never_runs_a_decoy_psql_initdb_or_postgres_from_the_daemon_path(PG17).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed PostgreSQL 18 artifact in the runtime cache"]
async fn postgresql_18_never_runs_a_decoy_psql_initdb_or_postgres_from_the_daemon_path() {
    never_runs_a_decoy_psql_initdb_or_postgres_from_the_daemon_path(PG18).await;
}
