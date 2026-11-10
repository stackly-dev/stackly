//! The doctor on a real PostgreSQL that refuses an old `postmaster.pid`
//! (spec 0034 AC-2, AC-10): the file names a live `sleep` of this test, so
//! `postgres` refuses to start, and the doctor reports `start_failed` with
//! the `stale_postmaster_pid` remedy of the package and what the PID is,
//! while the file, the data, and the `sleep` stay exactly as they were.

use std::process::{Child, Command};

use stackly_protocol::doctor::{DoctorCheckId, DoctorFindingCode, DoctorParams, PidFileState};
use stackly_protocol::instance::InstanceLifecycle;
use stackly_protocol::{Call, Reply};
use stackly_testkit::evidence::{Evidence, Kind, evidence};

use crate::harness::{PG17, PG18, Pg, Site, create_instance};

/// A `sleep` of this test, killed and reaped on drop.
struct Sleeper(Child);

impl Drop for Sleeper {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

async fn an_old_postmaster_pid_gets_the_provider_remedy_and_stays(pg: Pg, ev: Evidence) {
    let Some(site) = Site::open(&ev, pg) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let made = create_instance(&site, &api, "lockfile").await;
    let sleeper = Sleeper(
        Command::new("/bin/sleep")
            .arg("600")
            .spawn()
            .expect("sleep starts"),
    );
    let pid = sleeper.0.id();
    let data = site.instance_paths(made.id).data;
    let pid_file = data.join("postmaster.pid");
    let text = format!("{pid}\n{}\n", data.display());
    std::fs::write(&pid_file, &text).expect("the old postmaster.pid is written");

    let operation = api
        .start(made.id)
        .await
        .expect("the start is accepted")
        .accepted
        .expect("a start of a stopped instance begins an operation")
        .operation
        .id;
    api.wait_final(operation).await;
    let failed = api.get(made.id).await.expect("the instance reads");
    assert_eq!(failed.lifecycle, InstanceLifecycle::Failed, "{failed:?}");

    let reply = api
        .client
        .call(Call::DoctorRun(DoctorParams {
            instance_id: Some(made.id),
            checks: vec![DoctorCheckId::InstanceFailures],
        }))
        .await
        .expect("doctor.run answers");
    let Reply::DoctorRun(report) = reply else {
        panic!("not a doctor reply: {reply:?}");
    };
    let finding = report.checks[0]
        .findings
        .iter()
        .find(|finding| finding.code == DoctorFindingCode::StartFailed)
        .expect("start_failed is reported");
    let remedy = finding
        .remedy
        .as_ref()
        .expect("the stale postmaster.pid remedy matches");
    assert_eq!(remedy.provider, "official/postgresql");
    assert_eq!(remedy.name, "stale_postmaster_pid");
    let evidence = remedy.pid_file.as_ref().expect("the PID file is read");
    assert_eq!(evidence.pid, Some(pid));
    assert_eq!(evidence.state, PidFileState::OtherProcess);
    assert_eq!(
        evidence
            .process
            .as_ref()
            .and_then(|process| process.executable.as_deref()),
        Some("/bin/sleep")
    );
    let sentence = format!("PID {pid} now belongs to /bin/sleep, not PostgreSQL.");
    assert!(
        finding.explanation.ends_with(&sentence),
        "{}",
        finding.explanation
    );
    assert!(
        finding
            .excerpt
            .iter()
            .any(|line| line.contains("lock file \"postmaster.pid\" already exists")),
        "{:?}",
        finding.excerpt
    );

    assert_eq!(
        std::fs::read_to_string(&pid_file).expect("postmaster.pid stays"),
        text,
        "the doctor never changes or removes postmaster.pid"
    );
    assert!(
        Command::new("/bin/kill")
            .args(["-0", &pid.to_string()])
            .status()
            .expect("kill -0 runs")
            .success(),
        "the sleep still runs"
    );
    drop(sleeper);
    daemon.shutdown().await;
    ev.pass_with(&format!("start_failed with {} and {sentence}", remedy.name));
}

// covers: AC-2, AC-10, AC-22 (spec 0034)
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed PostgreSQL 18 artifact in the runtime cache"]
async fn postgresql_18_with_an_old_postmaster_pid_gets_the_stale_postmaster_pid_remedy() {
    let ev = evidence!(
        covers = ["REQ-019", "AC-019"],
        kind = Kind::Native,
        expected = "a postmaster.pid naming a live sleep makes PostgreSQL 18 refuse to start; doctor.run reports start_failed with the stale_postmaster_pid remedy, the PID as other_process /bin/sleep, and the lock file excerpt, and postmaster.pid and the sleep stay",
    );
    an_old_postmaster_pid_gets_the_provider_remedy_and_stays(PG18, ev).await;
}

// covers: AC-2, AC-10, AC-22 (spec 0034)
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed PostgreSQL 17 artifact in the runtime cache"]
async fn postgresql_17_with_an_old_postmaster_pid_gets_the_stale_postmaster_pid_remedy() {
    let ev = evidence!(
        covers = ["REQ-019", "AC-019"],
        kind = Kind::Native,
        expected = "a postmaster.pid naming a live sleep makes PostgreSQL 17 refuse to start; doctor.run reports start_failed with the stale_postmaster_pid remedy, the PID as other_process /bin/sleep, and the lock file excerpt, and postmaster.pid and the sleep stay",
    );
    an_old_postmaster_pid_gets_the_provider_remedy_and_stays(PG17, ev).await;
}
