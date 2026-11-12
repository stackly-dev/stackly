//! Data of one MySQL line never runs on a runtime of the other (spec 0042
//! AC-10, BR-009): the start is refused before a process is spawned, in
//! both directions, without an upgrade in place, `rebind::check` gives the
//! same outcome, and a missing or changed `mysql_upgrade_history` is
//! refused too.

use stackly_core::instance::rebind;
use stackly_core::operation::OperationError;
use stackly_domain::{RuntimeId, RuntimeLine};
use stackly_protocol::codes::supervisor;
use stackly_testkit::evidence::{Evidence, Kind, evidence};

use crate::harness::{
    MY84, MY97, Made, My, Site, create_instance, instance_id, mysqld_serving, refused_start,
    stop_and_shutdown, tree_digest,
};

/// The bytes of `mysql_upgrade_history` and `mysql.ibd` in `data`.
fn markers(data: &std::path::Path) -> (Vec<u8>, Vec<u8>) {
    (
        std::fs::read(data.join("mysql_upgrade_history")).expect("the marker can be read"),
        std::fs::read(data.join("mysql.ibd")).expect("mysql.ibd can be read"),
    )
}

/// `rebind::check` refuses `made` on `target` with the outcome the start
/// gives. No daemon runs when this is called.
fn check_refuses(site: &Site, made: &Made, target: RuntimeId, data: &str, runtime: &str) {
    let registry = site.open_registry();
    let refusal = rebind::check(
        &registry,
        &site.platform,
        &site.prepared,
        instance_id(made.id),
        target,
    )
    .expect_err("data of one line never runs on a runtime of the other");
    assert_eq!(refusal.code().as_str(), supervisor::DATA_INCOMPATIBLE.code);
    let find = |name: &str| {
        refusal
            .details()
            .iter()
            .find(|(key, _)| key.as_str() == name)
            .map(|(_, value)| value.as_str().to_owned())
    };
    assert_eq!(find("outcome").as_deref(), Some("incompatible"));
    assert_eq!(find("data").as_deref(), Some(data));
    assert_eq!(find("runtime").as_deref(), Some(runtime));
}

async fn refuses_data_of_the_other_line_and_a_marker_that_is_missing_or_changed(
    my: My,
    other: My,
    ev: Evidence,
) {
    let Some(site) = Site::open(&ev, my) else {
        return;
    };
    let Some(other_runtime) = site.install_other(&ev, other) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let made = create_instance(&site, &api, "mismatch").await;
    let data = site.instance_paths(made.id).data;
    // A start on its own runtime writes the data once, so the bytes below
    // are the ones a later start on the other runtime would upgrade.
    api.start_ready(made.id).await;
    let version = site
        .mysql(made.id, made.port)
        .scalar(&made.password, "SELECT VERSION()");
    assert_eq!(version, my.version);
    site.subject(&ev, &version);
    let accepted = api.stop_when_free(made.id).await.accepted.expect("a stop");
    api.wait_state(
        accepted.operation.id,
        stackly_protocol::operation::OperationState::Succeeded,
    )
    .await;
    drop(api);
    daemon.shutdown().await;
    let before = markers(&data);
    let tree = tree_digest(&data);

    check_refuses(&site, &made, other_runtime.0, my.line, other.line);

    // The data of one line on a runtime of the other.
    site.rebind_instance(made.id, &other_runtime);
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let error = refused_start(&api, made.id).await;
    assert_eq!(error.code(), "supervisor.data_incompatible", "{error:?}");
    assert_eq!(error.detail("outcome"), Some("incompatible"));
    assert_eq!(error.detail("data"), Some(my.line));
    assert_eq!(error.detail("runtime"), Some(other.line));
    assert!(mysqld_serving(&data).is_empty(), "a mysqld was spawned");
    assert_eq!(markers(&data), before, "the data was changed in place");
    assert_eq!(tree_digest(&data), tree, "a data file was changed");
    drop(api);
    daemon.shutdown().await;

    // Back on its own runtime, the marker decides.
    let own: (RuntimeId, RuntimeLine) = site.own_runtime();
    site.rebind_instance(made.id, &own);
    let marker = data.join("mysql_upgrade_history");
    let original = std::fs::read_to_string(&marker).expect("the marker can be read");
    let mode = std::fs::metadata(&marker)
        .expect("the marker has metadata")
        .permissions();
    std::fs::remove_file(&marker).expect("the marker can be removed");
    let mut without_marker = tree.clone();
    without_marker
        .remove("mysql_upgrade_history")
        .expect("the tree holds the marker");
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let error = refused_start(&api, made.id).await;
    assert_eq!(error.code(), "supervisor.data_unavailable", "{error:?}");
    assert_eq!(error.detail("reason"), Some("the data marker is missing"));
    assert!(mysqld_serving(&data).is_empty());
    assert!(!marker.exists(), "a refused start wrote the marker again");
    assert_eq!(
        tree_digest(&data),
        without_marker,
        "a data file was changed while the marker was missing"
    );

    let changed = original.replace(my.version, "8.0.40");
    assert_ne!(changed, original, "the marker names {}", my.version);
    std::fs::write(&marker, &changed).expect("the marker can be written");
    let error = refused_start(&api, made.id).await;
    assert_eq!(error.code(), "supervisor.data_incompatible", "{error:?}");
    assert_eq!(error.detail("outcome"), Some("marker_mismatch"));
    assert!(mysqld_serving(&data).is_empty());
    assert_eq!(
        std::fs::read_to_string(&marker).expect("the marker can be read"),
        changed,
        "the changed marker was rewritten"
    );
    let mut rest = tree_digest(&data);
    rest.remove("mysql_upgrade_history");
    assert_eq!(
        rest, without_marker,
        "a data file was changed while the marker was wrong"
    );

    std::fs::write(&marker, &original).expect("the marker can be restored");
    // Removing and writing the marker gave it the umask mode; put it back.
    std::fs::set_permissions(&marker, mode).expect("the marker mode can be restored");
    assert_eq!(tree_digest(&data), tree, "the data is back as it was");
    api.start_ready(made.id).await;
    stop_and_shutdown(&api, daemon, made.id).await;
    ev.pass_with(&format!(
        "data {} refused on runtime {} by the start and rebind::check, a missing marker as data_unavailable, 8.0.40 as marker_mismatch, every data file byte for byte unchanged after each refusal, and a start once the marker was back",
        my.line, other.line
    ));
}

// covers: AC-10 (spec 0042)
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs both managed MySQL artifacts in the runtime cache"]
async fn mysql_8_4_data_is_refused_by_a_runtime_9_7_and_a_missing_or_changed_marker_is_refused() {
    let ev = evidence!(
        covers = ["REQ-055", "BR-009", "AC-081"],
        kind = Kind::Native,
        expected = "data initialized by MySQL 8.4 is refused by a runtime 9.7 with supervisor.data_incompatible (incompatible, data 8.4, runtime 9.7) before any mysqld is spawned, every data file keeps its bytes, rebind::check gives the same outcome, a missing marker is supervisor.data_unavailable, a marker of 8.0.40 is marker_mismatch, neither refusal changes a data file, and the instance starts once the marker is back",
    );
    refuses_data_of_the_other_line_and_a_marker_that_is_missing_or_changed(MY84, MY97, ev).await;
}

// covers: AC-10 (spec 0042)
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs both managed MySQL artifacts in the runtime cache"]
async fn mysql_9_7_data_is_refused_by_a_runtime_8_4_and_a_missing_or_changed_marker_is_refused() {
    let ev = evidence!(
        covers = ["REQ-055", "BR-009", "AC-081"],
        kind = Kind::Native,
        expected = "data initialized by MySQL 9.7 is refused by a runtime 8.4 with supervisor.data_incompatible (incompatible, data 9.7, runtime 8.4) before any mysqld is spawned, every data file keeps its bytes, rebind::check gives the same outcome, a missing marker is supervisor.data_unavailable, a marker of 8.0.40 is marker_mismatch, neither refusal changes a data file, and the instance starts once the marker is back",
    );
    refuses_data_of_the_other_line_and_a_marker_that_is_missing_or_changed(MY97, MY84, ev).await;
}
