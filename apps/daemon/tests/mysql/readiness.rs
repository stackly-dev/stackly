//! Readiness that means a query was answered (spec 0042 AC-7): a locked
//! readiness account makes the instance not ready while the TCP port still
//! accepts, with the MySQL error as the detail, and unlocking it makes the
//! instance ready again.

use std::time::Duration;

use stackly_protocol::instance::InstanceReadinessState;
use stackly_testkit::evidence::{Evidence, Kind, evidence};

use crate::harness::{MY84, My, Site, create_instance, stop_and_shutdown};

async fn is_not_ready_while_its_port_accepts_but_the_readiness_account_is_locked(
    my: My,
    ev: Evidence,
) {
    let Some(site) = Site::open(&ev, my) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let made = create_instance(&site, &api, "honest").await;
    api.start_ready(made.id).await;
    let mysql = site.mysql(made.id, made.port);
    let version = mysql.scalar(&made.password, "SELECT VERSION()");
    site.subject(&ev, &version);

    let ran = mysql.admin(
        &made.password,
        "ALTER USER 'stackly_readiness'@'localhost' ACCOUNT LOCK",
    );
    assert!(ran.ok, "{ran:?}");
    let mut not_ready = None;
    for _ in 0..1200 {
        let summary = api.get(made.id).await.expect("the instance can be read");
        if summary.readiness.state == InstanceReadinessState::NotReady {
            not_ready = Some(summary);
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let not_ready = not_ready.expect("the instance became not_ready");
    let detail = not_ready.readiness.detail.clone().unwrap_or_default();
    assert!(
        detail.contains("Access denied for user 'stackly_readiness'@'localhost'"),
        "{detail}"
    );
    assert!(
        std::net::TcpStream::connect(("127.0.0.1", made.port)).is_ok(),
        "the TCP port still accepts connections"
    );

    let ran = mysql.admin(
        &made.password,
        "ALTER USER 'stackly_readiness'@'localhost' ACCOUNT UNLOCK",
    );
    assert!(ran.ok, "{ran:?}");
    api.wait_instance(made.id, "ready again", |summary| {
        summary.readiness.state == InstanceReadinessState::Ready
    })
    .await;

    stop_and_shutdown(&api, daemon, made.id).await;
    ev.pass_with(&format!(
        "not_ready with \"{detail}\" while the port accepted, ready again after ACCOUNT UNLOCK"
    ));
}

// covers: AC-7 (spec 0042)
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed MySQL 8.4 artifact in the runtime cache"]
async fn mysql_8_4_is_not_ready_while_its_port_accepts_but_the_readiness_account_is_locked() {
    let ev = evidence!(
        covers = ["REQ-055"],
        kind = Kind::Native,
        expected = "after ALTER USER stackly_readiness ACCOUNT LOCK the instance becomes not_ready with a detail that holds Access denied for user 'stackly_readiness'@'localhost' while its TCP port still accepts connections, and after ACCOUNT UNLOCK it is ready again",
    );
    is_not_ready_while_its_port_accepts_but_the_readiness_account_is_locked(MY84, ev).await;
}
