//! The doctor on a MySQL instance whose readiness account is gone (spec
//! 0042 AC-18, spec 0034 AC-10): `doctor.run` shows the `readiness_account`
//! remedy of the package with its command rendered for this instance, and
//! runs nothing.

use std::time::Duration;

use stackly_protocol::doctor::{DoctorCheckId, DoctorFindingCode, DoctorParams};
use stackly_protocol::instance::InstanceReadinessState;
use stackly_protocol::{Call, Reply};
use stackly_testkit::evidence::{Evidence, Kind, evidence};

use crate::harness::{MY84, MY97, My, Site, create_instance, stop_and_shutdown};

/// The accounts called `stackly_readiness`, as `root` reads them.
fn readiness_accounts(mysql: &crate::harness::Mysql, password: &str) -> String {
    mysql.scalar(
        password,
        "SELECT COUNT(*) FROM mysql.user WHERE user = 'stackly_readiness'",
    )
}

async fn a_dropped_readiness_account_gets_the_provider_remedy(my: My, ev: Evidence) {
    let Some(site) = Site::open(&ev, my) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let made = create_instance(&site, &api, "noaccount").await;
    api.start_ready(made.id).await;
    let mysql = site.mysql(made.id, made.port);
    let version = mysql.scalar(&made.password, "SELECT VERSION()");
    site.subject(&ev, &version);

    let ran = mysql.admin(&made.password, "DROP USER 'stackly_readiness'@'localhost'");
    assert!(ran.ok, "{ran:?}");
    let mut not_ready = false;
    for _ in 0..1200 {
        let summary = api.get(made.id).await.expect("the instance can be read");
        if summary.readiness.state == InstanceReadinessState::NotReady {
            not_ready = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(not_ready, "the instance became not_ready");

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
        .find(|finding| finding.code == DoctorFindingCode::NotReady)
        .expect("not_ready is reported");
    let remedy = finding
        .remedy
        .as_ref()
        .expect("the readiness account remedy matches");
    assert_eq!(remedy.provider, "official/mysql");
    assert_eq!(remedy.name, "readiness_account");
    let command = remedy
        .steps
        .iter()
        .find_map(|step| step.command.as_deref())
        .expect("a step has a command");
    assert!(
        command.starts_with(&format!(
            "{} --no-defaults --protocol=socket --socket={} --user=root --password --execute=",
            site.bin("mysql").display(),
            mysql.socket().display()
        )),
        "{command}"
    );
    assert!(command.contains("CREATE USER IF NOT EXISTS 'stackly_readiness'@'localhost'"));
    assert!(
        !command.contains(&made.password),
        "the password never enters a command"
    );
    assert_eq!(
        readiness_accounts(&mysql, &made.password),
        "0",
        "the doctor ran nothing"
    );

    // Put the account back the way the remedy says, and readiness returns.
    let ran = mysql.admin(
        &made.password,
        "CREATE USER IF NOT EXISTS 'stackly_readiness'@'localhost' IDENTIFIED BY '' WITH MAX_USER_CONNECTIONS 3",
    );
    assert!(ran.ok, "{ran:?}");
    api.wait_instance(made.id, "ready again", |summary| {
        summary.readiness.state == InstanceReadinessState::Ready
    })
    .await;

    stop_and_shutdown(&api, daemon, made.id).await;
    ev.pass_with("not_ready, doctor.run showed readiness_account with the rendered mysql command, and ran nothing");
}

// covers: AC-18 (spec 0042)
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed MySQL 8.4 artifact in the runtime cache"]
async fn mysql_8_4_with_a_dropped_readiness_account_gets_the_readiness_account_remedy() {
    let ev = evidence!(
        covers = ["REQ-055"],
        kind = Kind::Native,
        expected = "after DROP USER stackly_readiness the instance is not_ready, doctor.run reports not_ready with the readiness_account remedy of official/mysql whose command is rendered with the runtime mysql and the instance socket and holds no password, and the account is still missing afterwards",
    );
    a_dropped_readiness_account_gets_the_provider_remedy(MY84, ev).await;
}

// covers: AC-18 (spec 0042)
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed MySQL 9.7 artifact in the runtime cache"]
async fn mysql_9_7_with_a_dropped_readiness_account_gets_the_readiness_account_remedy() {
    let ev = evidence!(
        covers = ["REQ-055"],
        kind = Kind::Native,
        expected = "after DROP USER stackly_readiness the MySQL 9.7 instance is not_ready, doctor.run reports not_ready with the readiness_account remedy of official/mysql whose command is rendered with the runtime mysql and the instance socket and holds no password, and the account is still missing afterwards",
    );
    a_dropped_readiness_account_gets_the_provider_remedy(MY97, ev).await;
}
