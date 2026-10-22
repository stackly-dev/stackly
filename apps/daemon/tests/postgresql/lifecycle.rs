//! The happy path of a PostgreSQL instance through the daemon (spec 0019
//! AC-5, AC-13).

use std::sync::Arc;

use stackly_protocol::instance::{
    InstanceDesired, InstanceHealthState, InstanceLifecycle, InstanceReadinessState,
};
use stackly_protocol::operation::OperationState;
use stackly_testkit::evidence::{Evidence, Kind, evidence};

use crate::harness::{PG17, PG18, Pg, Site, Via, create_instance, stop_and_shutdown};
use crate::support::LogRecorder;

async fn is_created_started_served_and_stopped_through_the_daemon(pg: Pg, ev: Evidence) {
    let Some(site) = Site::open(&ev, pg) else {
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

    let made = create_instance(&site, &api, &format!("pg{}", pg.major)).await;
    let paths = site.instance_paths(made.id);
    let manifest: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(&paths.manifest).expect("the manifest is written"),
    )
    .expect("the manifest is JSON");
    assert_eq!(manifest["data"]["initializedLine"], pg.major);
    assert_eq!(manifest["runtime"]["line"], pg.major);
    assert!(
        !site
            .run_dir(made.id)
            .join("secrets/superuser-password")
            .exists(),
        "the password file outlived the init"
    );
    let created = api.get(made.id).await.expect("the instance can be read");
    assert_eq!(created.lifecycle, InstanceLifecycle::Stopped);
    assert_eq!(created.initialized, Some(true));

    let running = api.start_ready(made.id).await;
    assert_eq!(running.lifecycle, InstanceLifecycle::Running);
    assert_eq!(running.desired, InstanceDesired::Running);
    assert_eq!(running.readiness.state, InstanceReadinessState::Ready);
    assert!(
        running
            .process
            .as_ref()
            .is_some_and(|process| !process.adopted),
        "{running:?}"
    );
    api.wait_instance(made.id, "healthy", |summary| {
        summary.health.state == InstanceHealthState::Healthy
    })
    .await;

    let psql = site.psql(made.id, made.port);
    let version = psql.scalar(&made.password, "SHOW server_version");
    assert_eq!(version, pg.version);
    site.subject(&ev, &version);
    for via in [Via::Socket, Via::Tcp] {
        let ran = psql.run(
            via,
            "postgres",
            Some(&made.password),
            "postgres",
            "SELECT 1",
        );
        assert!(ran.ok, "{via:?}: {ran:?}");
    }
    let wrote = psql.admin(
        &made.password,
        "CREATE TABLE kept (note text); INSERT INTO kept VALUES ('before the stop')",
    );
    assert!(wrote.ok, "{wrote:?}");

    let first_run = recorder.text_of(made.id);
    assert!(
        first_run
            .iter()
            .any(|line| line.contains("database system is ready to accept connections")),
        "{first_run:#?}"
    );
    assert!(
        first_run.iter().any(|line| line.contains(" UTC [")),
        "no UTC stamped line: {first_run:#?}"
    );

    let accepted = api
        .stop_when_free(made.id)
        .await
        .accepted
        .expect("a stop starts an operation");
    api.wait_state(accepted.operation.id, OperationState::Succeeded)
        .await;
    let stopped = api.get(made.id).await.expect("the instance can be read");
    assert_eq!(stopped.lifecycle, InstanceLifecycle::Stopped);
    assert!(stopped.process.is_none());

    let running = api.start_ready(made.id).await;
    assert_eq!(running.readiness.state, InstanceReadinessState::Ready);
    assert_eq!(
        psql.scalar(&made.password, "SELECT note FROM kept"),
        "before the stop"
    );
    let both = recorder.text_of(made.id);
    assert!(both.len() > first_run.len());
    assert!(
        both.starts_with(&first_run),
        "the lines of the first run are still read after the second start"
    );

    stop_and_shutdown(&api, daemon, made.id).await;
    ev.pass_with(&format!(
        "PostgreSQL {version} created, ran twice, kept its row, and stopped"
    ));
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed PostgreSQL 17 artifact in the runtime cache"]
async fn postgresql_17_is_created_started_served_and_stopped_through_the_daemon() {
    let pg = PG17;
    let ev = evidence!(
        covers = ["REQ-035", "AC-035"],
        kind = Kind::Native,
        expected = &format!(
            "a PostgreSQL {} instance is created with its superuser password, initialized with data.initializedLine {}, starts to ready, shows lifecycle, desired, readiness, health, and process apart, answers queries over the socket and TCP, sends UTC stamped log lines to the sink, keeps a row over a stop and a start, and stops",
            pg.major, pg.major
        ),
    );
    is_created_started_served_and_stopped_through_the_daemon(pg, ev).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed PostgreSQL 18 artifact in the runtime cache"]
async fn postgresql_18_is_created_started_served_and_stopped_through_the_daemon() {
    let pg = PG18;
    let ev = evidence!(
        covers = ["REQ-035", "AC-035"],
        kind = Kind::Native,
        expected = &format!(
            "a PostgreSQL {} instance is created with its superuser password, initialized with data.initializedLine {}, starts to ready, shows lifecycle, desired, readiness, health, and process apart, answers queries over the socket and TCP, sends UTC stamped log lines to the sink, keeps a row over a stop and a start, and stops",
            pg.major, pg.major
        ),
    );
    is_created_started_served_and_stopped_through_the_daemon(pg, ev).await;
}
