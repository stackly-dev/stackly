//! Readiness that means a query was answered (spec 0019 AC-9).

use std::time::Duration;

use stackly_protocol::doctor::{DoctorCheckId, DoctorFindingCode, DoctorParams};
use stackly_protocol::instance::InstanceReadinessState;
use stackly_protocol::{Call, Reply};
use stackly_testkit::evidence::{Evidence, Kind, evidence};

use crate::harness::{PG17, PG18, Pg, Site, Via, create_instance, stop_and_shutdown};

async fn is_not_ready_when_the_port_accepts_but_the_query_fails(pg: Pg, ev: Evidence) {
    let Some(site) = Site::open(&ev, pg) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let made = create_instance(&site, &api, "honest").await;
    api.start_ready(made.id).await;
    let psql = site.psql(made.id, made.port);
    let version = psql.scalar(&made.password, "SHOW server_version");
    site.subject(&ev, &version);

    assert_eq!(
        psql.scalar(
            &made.password,
            "SELECT rolsuper::text || '|' || rolcreatedb::text || '|' || rolcreaterole::text || '|' || rolreplication::text || '|' || rolbypassrls::text || '|' || rolconnlimit::text FROM pg_roles WHERE rolname = 'stackly_readiness'"
        ),
        "false|false|false|false|false|3"
    );
    let settings = psql.scalar(
        &made.password,
        "SELECT array_to_string(setconfig, ',') FROM pg_db_role_setting WHERE setrole = 'stackly_readiness'::regrole",
    );
    assert!(
        settings.contains("default_transaction_read_only=on"),
        "{settings}"
    );
    assert!(settings.contains("statement_timeout=2s"), "{settings}");
    let create = psql.run(
        Via::Socket,
        "stackly_readiness",
        None,
        "postgres",
        "CREATE TABLE readiness_must_not_write (id int)",
    );
    assert!(!create.ok, "the readiness role created a table");

    let ran = psql.admin(&made.password, "ALTER ROLE stackly_readiness NOLOGIN");
    assert!(ran.ok, "{ran:?}");
    let mut not_ready = None;
    for _ in 0..600 {
        let summary = api.get(made.id).await.expect("the instance can be read");
        if summary.readiness.state == InstanceReadinessState::NotReady {
            not_ready = Some(summary);
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let not_ready = not_ready.expect("the instance became not_ready");
    let detail = not_ready.readiness.detail.clone().unwrap_or_default();
    assert!(detail.contains("not permitted to log in"), "{detail}");
    assert!(
        std::net::TcpStream::connect(("127.0.0.1", made.port)).is_ok(),
        "the TCP port still accepts connections"
    );

    // The doctor shows the provider remedy for the role (spec 0034 AC-10),
    // and its command, pasted into a shell as is, brings readiness back.
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
        .expect("the readiness role remedy matches");
    assert_eq!(remedy.name, "readiness_role");
    let reset = remedy
        .steps
        .iter()
        .filter_map(|step| step.command.as_deref())
        .find(|command| command.contains("ALTER ROLE"))
        .expect("a command sets the role back");
    assert!(
        !reset.contains(&made.password),
        "the password never enters a command"
    );
    let ran = std::process::Command::new("/bin/sh")
        .arg("-c")
        .arg(reset)
        .env("PGPASSWORD", &made.password)
        .output()
        .expect("the shell runs");
    assert!(ran.status.success(), "{ran:?}");
    api.wait_instance(made.id, "ready again", |summary| {
        summary.readiness.state == InstanceReadinessState::Ready
    })
    .await;

    stop_and_shutdown(&api, daemon, made.id).await;
    ev.pass_with(&format!("not_ready with `{detail}`, then ready again"));
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed PostgreSQL 17 artifact in the runtime cache"]
async fn postgresql_17_is_not_ready_when_the_port_accepts_but_the_query_fails() {
    let pg = PG17;
    let ev = evidence!(
        covers = ["REQ-035", "AC-035"],
        kind = Kind::Native,
        expected = "the readiness role is unprivileged and read only, ALTER ROLE NOLOGIN makes the instance not_ready with the PostgreSQL message while TCP still accepts, and the ALTER ROLE command of the readiness_role remedy that doctor.run shows, run in a shell, makes it ready again",
    );
    is_not_ready_when_the_port_accepts_but_the_query_fails(pg, ev).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed PostgreSQL 18 artifact in the runtime cache"]
async fn postgresql_18_is_not_ready_when_the_port_accepts_but_the_query_fails() {
    let pg = PG18;
    let ev = evidence!(
        covers = ["REQ-035", "AC-035"],
        kind = Kind::Native,
        expected = "the readiness role is unprivileged and read only, ALTER ROLE NOLOGIN makes the instance not_ready with the PostgreSQL message while TCP still accepts, and the ALTER ROLE command of the readiness_role remedy that doctor.run shows, run in a shell, makes it ready again",
    );
    is_not_ready_when_the_port_accepts_but_the_query_fails(pg, ev).await;
}
