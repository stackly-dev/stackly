//! Changing a NATS instance through the daemon (spec 0029, milestone 4): the
//! config methods against the real `nats-server` of the managed runtime.
//! NATS has no reload, so every setting is a restart: the apply ends with a
//! new server that reports the new limit on `/varz`, and the revision that
//! reached ready becomes the last known good. The test is native,
//! `#[ignore = "native: ..."]`, and ends as `BLOCKED` when the artifact is
//! not in the runtime cache.

use serde_json::json;
use stackly_protocol::instance::InstanceReadinessState;
use stackly_protocol::instance::config::{ConfigImpact, ConfigRevisionOrigin};
use stackly_testkit::evidence::{Kind, evidence};

use crate::config_calls::{Cfg, pid_of, settings_with};
use crate::harness::{Serve, Site, alive, create_instance, port_of, stop_instance, varz};

// covers: AC-5, AC-7, AC-16
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed NATS 2.15.1 artifact in the runtime cache"]
async fn nats_applies_a_setting_with_a_restart_and_the_new_revision_becomes_the_last_known_good() {
    let ev = evidence!(
        covers = ["REQ-015", "AC-015"],
        kind = Kind::Native,
        expected = "a NATS 2.15.1 instance does not support reload, changing maxPayloadBytes is a restart impact that ends with a new nats-server pid whose /varz reports the new max_payload while the old process is gone, and revision 2 becomes the last known good after the instance was ready on it",
    );
    let Some(site) = Site::open(&ev, Serve::InProcess) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let cfg = Cfg { api: &api };
    let id = create_instance(&site, &api, "cfg-nats", json!({})).await;
    let running = api.start_ready(id).await;
    let first = pid_of(&running);
    let monitoring = port_of(&running, "monitoring");
    let report = varz(monitoring);
    site.subject(&ev, report["version"].as_str().unwrap_or("unknown"));
    assert_eq!(report["max_payload"], json!(1_048_576));

    let view = cfg
        .wait_view(id, "revision 1 as last known good", |view| {
            view.last_known_good.map(|n| n.get()) == Some(1)
        })
        .await;
    assert!(!view.reload_supported, "NATS has no reload: {view:#?}");
    assert!(view.drift.is_none());

    let refused = api
        .reload(id)
        .await
        .expect_err("a provider without reload refuses a reload");
    assert!(!refused.code().is_empty(), "{refused:?}");
    assert_eq!(pid_of(&api.get(id).await.expect("instance")), first);

    let change = settings_with(&view, json!({ "maxPayloadBytes": 2_097_152 }));
    assert_eq!(
        cfg.impact(id, 1, change.clone()).await,
        ConfigImpact::Restart
    );
    cfg.apply_done(id, 1, change).await;
    let summary = api
        .wait_instance(id, "a new nats-server", |summary| {
            summary
                .process
                .as_ref()
                .is_some_and(|process| process.pid != first)
                && summary.readiness.state == InstanceReadinessState::Ready
        })
        .await;
    assert!(!alive(first), "the old nats-server {first} is still alive");
    let view = cfg
        .wait_view(id, "revision 2 as last known good", |view| {
            view.last_known_good.map(|n| n.get()) == Some(2)
        })
        .await;
    assert_eq!(view.revision.number.get(), 2);
    assert_eq!(view.revision.origin, ConfigRevisionOrigin::Applied);
    assert_eq!(view.history.len(), 2);
    assert_eq!(
        varz(port_of(&summary, "monitoring"))["max_payload"],
        json!(2_097_152)
    );

    // Putting the old value back is a new revision, not an edit of revision 1.
    let second = pid_of(&summary);
    let back = settings_with(&view, json!({ "maxPayloadBytes": 1_048_576 }));
    cfg.apply_done(id, 2, back).await;
    let summary = api
        .wait_instance(id, "a third nats-server", |summary| {
            summary
                .process
                .as_ref()
                .is_some_and(|process| process.pid != second)
                && summary.readiness.state == InstanceReadinessState::Ready
        })
        .await;
    let view = cfg
        .wait_view(id, "revision 3 as last known good", |view| {
            view.last_known_good.map(|n| n.get()) == Some(3)
        })
        .await;
    assert_eq!(view.history.len(), 3);
    assert_eq!(
        varz(port_of(&summary, "monitoring"))["max_payload"],
        json!(1_048_576)
    );

    stop_instance(&api, id).await;
    daemon.shutdown().await;
    ev.pass_with(&format!(
        "restart replaced nats-server {first}, /varz max_payload followed revision 2, revision 3 is the last known good"
    ));
}
