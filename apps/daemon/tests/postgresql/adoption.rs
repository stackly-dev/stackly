//! A postmaster that outlives its daemon (spec 0019 AC-13, spec 0010 AC-9).

use stackly_protocol::instance::{InstanceLifecycle, InstanceReadinessState};
use stackly_testkit::evidence::{Kind, evidence};

use crate::harness::{PG17, PG18, Pg, Site, create_instance, output_of};

async fn keeps_running_after_sigkill_of_the_daemon_and_the_next_daemon_adopts_it(pg: Pg) {
    let ev = evidence!(
        covers = ["REQ-035", "AC-035"],
        kind = Kind::Native,
        expected = "after stacklyd is killed with SIGKILL the postmaster keeps running, the next stacklyd adopts the same pid and start time and shows it ready, and the postmaster and its children hold instance.lock at fd 3",
    );
    let Some(site) = Site::open(&ev, pg) else {
        return;
    };
    let first = site.binary(&[]);
    let api = site.binary_api(&first).await;
    let made = create_instance(&site, &api, "adopted").await;
    let running = api.start_ready(made.id).await;
    let process = running.process.clone().expect("a running process");
    let psql = site.psql(made.id, made.port);
    let version = psql.scalar(&made.password, "SHOW server_version");
    site.subject(&ev, &version);

    let postmaster = process.pid.to_string();
    let children = output_of("/usr/bin/pgrep", &["-P", &postmaster]);
    let holders: Vec<String> = std::iter::once(postmaster.clone())
        .chain(children.split_whitespace().map(str::to_owned))
        .collect();
    assert!(holders.len() > 1, "the postmaster has children");
    for pid in &holders {
        let open = output_of("/usr/sbin/lsof", &["-a", "-p", pid, "-d", "3", "-n", "-P"]);
        assert!(open.contains("instance.lock"), "pid {pid} fd 3: {open}");
    }

    drop(api);
    first.kill();
    assert!(
        std::process::Command::new("/bin/kill")
            .args(["-0", &postmaster])
            .status()
            .expect("kill runs")
            .success(),
        "the postmaster outlives the daemon"
    );

    let second = site.binary(&[]);
    let api = site.binary_api(&second).await;
    let adopted = api
        .wait_instance(made.id, "adopted and ready", |summary| {
            summary
                .process
                .as_ref()
                .is_some_and(|process| process.adopted)
                && summary.readiness.state == InstanceReadinessState::Ready
        })
        .await;
    let adopted_process = adopted.process.clone().expect("the process is adopted");
    assert_eq!(adopted_process.pid, process.pid);
    assert_eq!(adopted_process.started_at, process.started_at);
    assert_eq!(adopted.lifecycle, InstanceLifecycle::Running);

    let accepted = api.stop_when_free(made.id).await.accepted.expect("a stop");
    api.wait_state(
        accepted.operation.id,
        stackly_protocol::operation::OperationState::Succeeded,
    )
    .await;
    second.stop();
    ev.pass_with("the same postmaster was adopted after SIGKILL and stopped by the new daemon");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed PostgreSQL 17 artifact in the runtime cache"]
async fn postgresql_17_keeps_running_after_sigkill_of_the_daemon_and_the_next_daemon_adopts_it() {
    keeps_running_after_sigkill_of_the_daemon_and_the_next_daemon_adopts_it(PG17).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed PostgreSQL 18 artifact in the runtime cache"]
async fn postgresql_18_keeps_running_after_sigkill_of_the_daemon_and_the_next_daemon_adopts_it() {
    keeps_running_after_sigkill_of_the_daemon_and_the_next_daemon_adopts_it(PG18).await;
}
