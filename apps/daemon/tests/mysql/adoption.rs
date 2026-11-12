//! A `mysqld` that outlives its daemon (spec 0042 AC-13, spec 0010 AC-9).

use stackly_protocol::instance::{InstanceLifecycle, InstanceReadinessState};
use stackly_testkit::evidence::{Evidence, Kind, evidence};

use crate::harness::{MY84, My, Site, create_instance, output_of};

async fn keeps_running_after_sigkill_of_the_daemon_and_the_next_daemon_adopts_it(
    my: My,
    ev: Evidence,
) {
    let Some(site) = Site::open(&ev, my) else {
        return;
    };
    let first = site.binary(&[]);
    let api = site.binary_api(&first).await;
    let made = create_instance(&site, &api, "adopted").await;
    let running = api.start_ready(made.id).await;
    let process = running.process.clone().expect("a running process");
    let mysql = site.mysql(made.id, made.port);
    let version = mysql.scalar(&made.password, "SELECT VERSION()");
    site.subject(&ev, &version);

    let pid = process.pid.to_string();
    let open = output_of("/usr/sbin/lsof", &["-a", "-p", &pid, "-d", "3", "-n", "-P"]);
    assert!(open.contains("instance.lock"), "mysqld fd 3: {open}");

    drop(api);
    first.kill();
    assert!(
        std::process::Command::new("/bin/kill")
            .args(["-0", &pid])
            .status()
            .expect("kill runs")
            .success(),
        "mysqld outlives the daemon"
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
    ev.pass_with(
        "the same mysqld was adopted after SIGKILL of the daemon and stopped by the new one",
    );
}

// covers: AC-13 (spec 0042)
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed MySQL 8.4 artifact in the runtime cache"]
async fn mysql_8_4_keeps_running_after_sigkill_of_the_daemon_and_the_next_daemon_adopts_it() {
    let ev = evidence!(
        covers = ["REQ-055"],
        kind = Kind::Native,
        expected = "mysqld holds instance.lock at fd 3, keeps running after stacklyd is killed with SIGKILL, and the next stacklyd adopts the same pid and start time and shows it ready",
    );
    keeps_running_after_sigkill_of_the_daemon_and_the_next_daemon_adopts_it(MY84, ev).await;
}
