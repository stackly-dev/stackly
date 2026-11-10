//! The audit notes the works of an instance set (spec 0035, *Catatan audit
//! pekerjaan*, build step 3): start, restart, reload, apply, a port change,
//! drift, and rename each end with an `operation.finished` row that carries
//! their config revision number and details, over the fake provider.

use std::time::Duration;

use serde_json::json;
use stackly_core::instance::config::ConfigChange;
use stackly_domain::{
    AuditAction, AuditEventRecord, AuditQuery, AuditResult, ConfigRevisionNumber, InstanceId,
    OperationId, OperationKind, OperationState, Port, PortRequest, Registry,
};
use stackly_platform::{ChildRole, ExitStatus};
use stackly_testkit::FakeSpawn;

use crate::common::{Rig, Spec};

async fn wait_for(what: &str, mut ready: impl FnMut() -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    while !ready() {
        assert!(tokio::time::Instant::now() < deadline, "timed out: {what}");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

fn main_spawns(rig: &Rig) -> usize {
    rig.processes()
        .spawns()
        .into_iter()
        .filter(|spawn| spawn.role == ChildRole::Main)
        .count()
}

/// Starts the instance and waits until it is ready; returns the spawn, the
/// port its server answers on, and the start operation.
async fn running(rig: &Rig, id: InstanceId) -> (FakeSpawn, u16, OperationId) {
    let op = rig.start(id).await;
    let spawn = rig.main_spawn(main_spawns(rig) + 1).await;
    let port = rig.serve(&spawn);
    rig.wait_state(op, OperationState::Succeeded).await;
    (spawn, port, op)
}

/// The one `operation.finished` row of `operation`.
fn finished(rig: &Rig, operation: OperationId) -> AuditEventRecord {
    let query = AuditQuery {
        operation: Some(operation),
        ..AuditQuery::page(50, None)
    };
    let rows: Vec<AuditEventRecord> = rig
        .registry
        .audit_events(query)
        .expect("the audit rows read")
        .value
        .items
        .into_iter()
        .filter(|row| row.action == AuditAction::OPERATION_FINISHED)
        .collect();
    assert_eq!(rows.len(), 1, "one finished row: {rows:?}");
    rows.into_iter().next().expect("checked above")
}

fn number(row: &AuditEventRecord) -> Option<u64> {
    row.config_revision.map(ConfigRevisionNumber::get)
}

// covers: AC-2 (spec 0035, start, restart, reload, rename, drift)
#[tokio::test]
async fn start_restart_reload_rename_and_drift_name_the_revision_they_ran() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());
    let (first, port, start) = running(&rig, id).await;

    let row = finished(&rig, start);
    assert_eq!(row.result, AuditResult::Succeeded);
    assert_eq!(row.instance, Some(id));
    assert_eq!(number(&row), Some(rig.active(id).number.get()));
    assert_eq!(
        row.safe_details.get("ports"),
        Some(format!("http {port}").as_str()),
        "the port the start chose"
    );

    let reload = rig.reload(id).await;
    rig.wait_state(reload, OperationState::Succeeded).await;
    let row = finished(&rig, reload);
    assert_eq!(number(&row), Some(1));
    assert_eq!(row.safe_details.get("ports"), None);

    let rename = rig.rename(id, "renamed-audit").await;
    rig.wait_state(rename, OperationState::Succeeded).await;
    let row = finished(&rig, rename);
    assert_eq!(row.safe_details.get("newName"), Some("renamed-audit"));
    assert!(row.safe_details.get("oldName").is_some());
    assert_eq!(number(&row), None, "a rename writes no revision");

    let restart = rig.restart(id).await;
    wait_for("the term arrives", || {
        !rig.processes().signals(first.pid).is_empty()
    })
    .await;
    rig.kill_servers();
    rig.processes().exit(first.pid, Some(ExitStatus::Exited(0)));
    let second = rig.main_spawn(2).await;
    rig.serve(&second);
    rig.wait_state(restart, OperationState::Succeeded).await;
    let row = finished(&rig, restart);
    assert_eq!(number(&row), Some(1));
    assert_eq!(
        row.safe_details.get("ports"),
        None,
        "the start half of a restart keeps the restart's note"
    );

    let keep = rig.keep_revision(id).await;
    rig.wait_state(keep, OperationState::Succeeded).await;
    let row = finished(&rig, keep);
    assert_eq!(number(&row), Some(1), "drift rewrites the active revision");
}

fn fixed(port: u16) -> PortRequest {
    PortRequest::Fixed(Port::new(port).expect("a test port is not 0"))
}

// covers: AC-2 (spec 0035, apply and a port change)
#[tokio::test]
async fn an_apply_and_a_port_change_name_the_new_revision_the_old_one_and_what_changed() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());

    let apply = rig.apply(id, json!({"label": "audited"})).await;
    rig.wait_state(apply, OperationState::Succeeded).await;
    let row = finished(&rig, apply);
    assert_eq!(number(&row), Some(2));
    assert_eq!(row.safe_details.get("oldRevision"), Some("1"));
    assert_eq!(row.safe_details.get("settings"), Some("label"));
    assert_eq!(row.safe_details.get("impact"), Some("next_start"));
    assert!(
        !format!("{row:?}").contains("audited"),
        "a setting value never reaches the row"
    );

    let old_port = Rig::free_port();
    let first = rig
        .change(
            OperationKind::ChangeEndpoints,
            id,
            2,
            ConfigChange::Endpoints(vec![(
                "http".parse().expect("an endpoint name"),
                fixed(old_port),
            )]),
        )
        .await;
    rig.wait_state(first, OperationState::Succeeded).await;
    let row = finished(&rig, first);
    assert_eq!(number(&row), Some(3));
    assert_eq!(
        row.safe_details.get("ports"),
        Some(format!("http auto \u{2192} {old_port}").as_str())
    );

    let new_port = Rig::free_port();
    let second = rig
        .change(
            OperationKind::ChangeEndpoints,
            id,
            3,
            ConfigChange::Endpoints(vec![(
                "http".parse().expect("an endpoint name"),
                fixed(new_port),
            )]),
        )
        .await;
    rig.wait_state(second, OperationState::Succeeded).await;
    let row = finished(&rig, second);
    assert_eq!(number(&row), Some(4));
    assert_eq!(row.safe_details.get("oldRevision"), Some("3"));
    assert_eq!(
        row.safe_details.get("ports"),
        Some(format!("http {old_port} \u{2192} {new_port}").as_str())
    );
}

// covers: AC-2 (spec 0035, a change that fails before its commit)
#[tokio::test]
async fn a_change_refused_before_its_commit_names_only_the_revision_it_follows() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());
    let first = rig.apply(id, json!({"label": "one"})).await;
    let second = rig.apply(id, json!({"label": "two"})).await;
    rig.wait_state(first, OperationState::Succeeded).await;
    // The second was read at revision 1, which is stale once the first ran.
    let detail = rig.wait_final(second).await;
    assert_eq!(detail.view.operation.state(), OperationState::Failed);
    let row = finished(&rig, second);
    assert_eq!(row.result, AuditResult::Failed);
    assert_eq!(number(&row), None, "no new revision was written");
    assert!(row.safe_details.get("errorCode").is_some());
}
