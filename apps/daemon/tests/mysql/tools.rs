//! Only the programs of the bound runtime run, and they read no option file
//! but their own (spec 0042 AC-12).

use std::os::unix::fs::PermissionsExt;
use std::time::Duration;

use stackly_protocol::operation::OperationState;
use stackly_testkit::evidence::{Evidence, Kind, evidence};

use crate::harness::{MY84, MY97, My, Site, create_instance};

async fn never_runs_a_decoy_and_ignores_a_broken_my_cnf_in_its_home(my: My, ev: Evidence) {
    let Some(site) = Site::open(&ev, my) else {
        return;
    };
    let decoys = site.root.path().join("decoys");
    let marker = site.root.path().join("decoy-ran");
    std::fs::create_dir_all(&decoys).expect("the decoy folder can be made");
    for name in ["mysql", "mysqld", "mysqladmin"] {
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
    // The temp folder is HOME of every child, so an option file there would
    // be read by a program that does not start with --no-defaults or
    // --defaults-file.
    let temp = site.instance_paths(made.id).temp;
    std::fs::write(
        temp.join(".my.cnf"),
        "[mysqld]\nstackly_unknown_option = 1\n[client]\nstackly_unknown_option = 1\n[mysql]\nstackly_unknown_option = 1\n",
    )
    .expect("the broken option file can be written");
    api.start_ready(made.id).await;
    let mysql = site.mysql(made.id, made.port);
    let version = mysql.scalar(&made.password, "SELECT VERSION()");
    site.subject(&ev, &version);
    // Let several probe rounds run.
    tokio::time::sleep(Duration::from_secs(2)).await;
    let accepted = api.stop_when_free(made.id).await.accepted.expect("a stop");
    api.wait_state(accepted.operation.id, OperationState::Succeeded)
        .await;
    daemon.stop();

    assert!(
        !marker.exists(),
        "a decoy ran: {}",
        std::fs::read_to_string(&marker).unwrap_or_default()
    );
    ev.pass_with("create, start, readiness, and stop worked with decoys on PATH and a broken ~/.my.cnf, and no decoy ran");
}

// covers: AC-12 (spec 0042)
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed MySQL 8.4 artifact in the runtime cache"]
async fn mysql_8_4_never_runs_a_decoy_and_ignores_a_broken_my_cnf_in_its_home() {
    let ev = evidence!(
        covers = ["REQ-055"],
        kind = Kind::Native,
        expected = "with decoy mysql, mysqld, and mysqladmin first on the daemon's PATH and a .my.cnf with an unknown option in the instance temp folder that is HOME of the children, create, start, readiness, and stop work and no decoy ever runs",
    );
    never_runs_a_decoy_and_ignores_a_broken_my_cnf_in_its_home(MY84, ev).await;
}

// covers: AC-12 (spec 0042)
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed MySQL 9.7 artifact in the runtime cache"]
async fn mysql_9_7_never_runs_a_decoy_and_ignores_a_broken_my_cnf_in_its_home() {
    let ev = evidence!(
        covers = ["REQ-055"],
        kind = Kind::Native,
        expected = "with decoy mysql, mysqld, and mysqladmin first on the daemon's PATH and a .my.cnf with an unknown option in the instance temp folder that is HOME of the children, create, start, readiness, and stop work and no decoy ever runs",
    );
    never_runs_a_decoy_and_ignores_a_broken_my_cnf_in_its_home(MY97, ev).await;
}
