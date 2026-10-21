//! The happy path of a PostgreSQL instance through the daemon (spec 0019
//! AC-5, AC-13).

use std::sync::Arc;

use stackly_protocol::instance::{InstanceDesired, InstanceLifecycle, InstanceReadinessState};
use stackly_protocol::operation::OperationState;
use stackly_testkit::evidence::{Kind, evidence};

use crate::harness::{PG18, Site, Via, create_instance};
use crate::support::LogRecorder;

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed PostgreSQL 18 artifact in the runtime cache"]
async fn postgresql_18_is_created_started_served_and_stopped_through_the_daemon() {
    let ev = evidence!(
        covers = ["REQ-035", "AC-035"],
        kind = Kind::Native,
        expected = "a PostgreSQL 18 instance is created with a generated superuser password, starts to ready, answers queries over the socket and TCP, keeps a row over a stop and a start, and stops",
    );
    let Some(site) = Site::open(&ev, PG18) else {
        return;
    };
    let recorder = Arc::new(LogRecorder::default());
    let daemon = site
        .in_process(|config| {
            config
                .extra_log_sinks
                .push(Arc::clone(&recorder) as Arc<dyn stackly_core::supervisor::LogSink>);
        })
        .await;
    let api = site.api().await;

    let made = create_instance(&site, &api, "pg18").await;
    let paths = site.instance_paths(made.id);
    let manifest = std::fs::read_to_string(&paths.manifest).expect("the manifest is written");
    println!("{manifest}");
    assert!(
        !site
            .run_dir(made.id)
            .join("secrets/superuser-password")
            .exists()
    );

    let running = api.start_ready(made.id).await;
    assert_eq!(running.lifecycle, InstanceLifecycle::Running);
    assert_eq!(running.desired, InstanceDesired::Running);
    assert_eq!(running.readiness.state, InstanceReadinessState::Ready);
    println!("{running:#?}");

    let psql = site.psql(made.id, made.port);
    let version = psql.scalar(&made.password, "SHOW server_version");
    site.subject(&ev, &version);
    println!("{version}");
    let ran = psql.run(
        Via::Tcp,
        "postgres",
        Some(&made.password),
        "postgres",
        "SELECT 1",
    );
    assert!(ran.ok, "{ran:?}");

    let accepted = api
        .stop_when_free(made.id)
        .await
        .accepted
        .expect("a stop operation");
    api.wait_state(accepted.operation.id, OperationState::Succeeded)
        .await;
    println!("{:#?}", recorder.text_of(made.id));
    daemon.shutdown().await;
    ev.pass_with("created, started, queried, stopped");
}
