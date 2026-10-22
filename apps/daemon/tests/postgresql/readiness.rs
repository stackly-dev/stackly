//! Readiness that means a query was answered (spec 0019 AC-9).

use std::time::Duration;

use stackly_protocol::instance::InstanceReadinessState;
use stackly_testkit::evidence::{Kind, evidence};

use crate::harness::{PG17, PG18, Pg, Site, Via, create_instance, stop_and_shutdown};

async fn is_not_ready_when_the_port_accepts_but_the_query_fails(pg: Pg) {
    let ev = evidence!(
        covers = ["REQ-035", "AC-035"],
        kind = Kind::Native,
        expected = "the readiness role is unprivileged and read only, ALTER ROLE NOLOGIN makes the instance not_ready with the PostgreSQL message while TCP still accepts, and LOGIN makes it ready again",
    );
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

    let ran = psql.admin(&made.password, "ALTER ROLE stackly_readiness LOGIN");
    assert!(ran.ok, "{ran:?}");
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
    is_not_ready_when_the_port_accepts_but_the_query_fails(PG17).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed PostgreSQL 18 artifact in the runtime cache"]
async fn postgresql_18_is_not_ready_when_the_port_accepts_but_the_query_fails() {
    is_not_ready_when_the_port_accepts_but_the_query_fails(PG18).await;
}
