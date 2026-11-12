//! Until both MySQL launchd tests pass and `official/mysql` enters
//! `HANDOVER_PROVEN` (spec 0042 AC-17, spec 0030 AC-8), a planned daemon
//! restart stops a running MySQL instance and starts it again: the restart
//! preview of a manual `stacklyd` names it `stop_restore` with
//! `handover_unproven`. Needs no launchd job, only the cached artifact.

use stackly_protocol::restart::{PlanDecision, PlanReason};
use stackly_protocol::{Call, Reply};
use stackly_testkit::evidence::{Evidence, Kind, evidence};

use crate::endpoints::stop;
use crate::harness::{MY84, MY97, My, Site, create_instance};

async fn restart_preview_stops_and_restores_while_the_handover_is_unproven(my: My, ev: Evidence) {
    let Some(site) = Site::open(&ev, my) else {
        return;
    };
    let manual = site.binary(&[]);
    let api = site.binary_api(&manual).await;
    let made = create_instance(&site, &api, "unproven").await;
    api.start_ready(made.id).await;
    let version = site
        .mysql(made.id, made.port)
        .scalar(&made.password, "SELECT VERSION()");
    site.subject(&ev, &version);

    let preview = match api.client.call(Call::DaemonRestartPreview).await {
        Ok(Reply::DaemonRestartPreview(preview)) => preview,
        other => panic!("expected a preview, got {other:?}"),
    };
    let entry = preview
        .instances
        .iter()
        .find(|entry| entry.instance_id == made.id)
        .cloned()
        .expect("the running instance is in the plan");
    stop(&api, made.id).await;
    drop(api);
    manual.stop();

    assert_eq!(entry.provider, "mysql", "{entry:#?}");
    assert_eq!(
        (entry.decision, entry.reason),
        (PlanDecision::StopRestore, PlanReason::HandoverUnproven),
        "{entry:#?}"
    );
    ev.pass_with(&format!(
        "MySQL {version} is stop_restore with handover_unproven in the restart preview"
    ));
}

// covers: AC-17 (spec 0042)
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed MySQL 8.4 artifact in the runtime cache"]
async fn mysql_8_4_restart_preview_stops_and_restores_while_the_handover_is_unproven() {
    let ev = evidence!(
        covers = ["REQ-055"],
        kind = Kind::Native,
        expected = "daemon.restart_preview of a manual stacklyd lists a running MySQL 8.4 instance as stop_restore with handover_unproven",
    );
    restart_preview_stops_and_restores_while_the_handover_is_unproven(MY84, ev).await;
}

// covers: AC-17 (spec 0042)
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed MySQL 9.7 artifact in the runtime cache"]
async fn mysql_9_7_restart_preview_stops_and_restores_while_the_handover_is_unproven() {
    let ev = evidence!(
        covers = ["REQ-055"],
        kind = Kind::Native,
        expected = "daemon.restart_preview of a manual stacklyd lists a running MySQL 9.7 instance as stop_restore with handover_unproven",
    );
    restart_preview_stops_and_restores_while_the_handover_is_unproven(MY97, ev).await;
}
