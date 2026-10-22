//! Data of one major never runs on a runtime of the other (spec 0019 AC-11,
//! BR-009): the start is refused in `verify`, before a process is spawned,
//! in both directions, and a missing or changed `PG_VERSION` is refused too.

use stackly_testkit::evidence::{Evidence, Kind, evidence};

use crate::harness::{PG17, PG18, Pg, Site, create_instance, stop_and_shutdown};
use crate::phases::{nothing_was_spawned, refuse_other_major, refused_start};

/// Data initialized by `pg` is refused by a runtime of `other`, and
/// `PG_VERSION` is checked on its own runtime.
async fn refuses_data_of_another_major_and_a_marker_that_is_missing_or_changed(
    pg: Pg,
    other: Pg,
    ev: Evidence,
) {
    let Some(site) = Site::open(&ev, pg) else {
        return;
    };
    let Some(other_runtime) = site.install_other(&ev, other) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let made = create_instance(&site, &api, "mismatch").await;
    let data = site.instance_paths(made.id).data;
    drop(api);
    daemon.shutdown().await;

    // The data of one major on a runtime of the other.
    refuse_other_major(&site, &made, pg, other, &other_runtime).await;

    // Back on its own runtime, the marker decides.
    site.rebind_instance(made.id, &site.own_runtime());
    let marker = data.join("PG_VERSION");
    let original = std::fs::read_to_string(&marker).expect("initdb wrote PG_VERSION");
    assert_eq!(original.trim_end(), pg.major);
    std::fs::remove_file(&marker).expect("PG_VERSION can be removed");
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let error = refused_start(&api, made.id).await;
    assert_eq!(error.code(), "supervisor.data_incompatible", "{error:?}");
    assert_eq!(error.detail("outcome"), Some("marker_missing"));
    nothing_was_spawned(&data);

    std::fs::write(&marker, "16\n").expect("PG_VERSION can be written");
    let error = refused_start(&api, made.id).await;
    assert_eq!(error.code(), "supervisor.data_incompatible", "{error:?}");
    assert_eq!(error.detail("outcome"), Some("marker_mismatch"));
    nothing_was_spawned(&data);

    std::fs::write(&marker, original).expect("PG_VERSION can be restored");
    let running = api.start_ready(made.id).await;
    let psql = site.psql(made.id, made.port);
    let version = psql.scalar(&made.password, "SHOW server_version");
    assert_eq!(version, pg.version);
    assert!(running.process.is_some(), "{running:?}");
    site.subject(&ev, &version);

    stop_and_shutdown(&api, daemon, made.id).await;
    ev.pass_with(&format!(
        "data {} refused on runtime {}, marker_missing, marker_mismatch, and a start once PG_VERSION was back",
        pg.major, other.major
    ));
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs both managed PostgreSQL artifacts in the runtime cache"]
async fn postgresql_17_data_is_refused_by_a_runtime_18_and_a_missing_or_changed_marker_is_refused()
{
    let pg = PG17;
    let other = PG18;
    let ev = evidence!(
        covers = ["REQ-035", "AC-035", "BR-009"],
        kind = Kind::Native,
        expected = &format!(
            "data initialized by PostgreSQL {} is refused by a runtime of {} with supervisor.data_incompatible before any process is spawned, a missing PG_VERSION is marker_missing, PG_VERSION 16 is marker_mismatch, and the instance starts again once the marker is back",
            pg.major, other.major
        ),
    );
    refuses_data_of_another_major_and_a_marker_that_is_missing_or_changed(pg, other, ev).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs both managed PostgreSQL artifacts in the runtime cache"]
async fn postgresql_18_data_is_refused_by_a_runtime_17_and_a_missing_or_changed_marker_is_refused()
{
    let pg = PG18;
    let other = PG17;
    let ev = evidence!(
        covers = ["REQ-035", "AC-035", "BR-009"],
        kind = Kind::Native,
        expected = &format!(
            "data initialized by PostgreSQL {} is refused by a runtime of {} with supervisor.data_incompatible before any process is spawned, a missing PG_VERSION is marker_missing, PG_VERSION 16 is marker_mismatch, and the instance starts again once the marker is back",
            pg.major, other.major
        ),
    );
    refuses_data_of_another_major_and_a_marker_that_is_missing_or_changed(pg, other, ev).await;
}
