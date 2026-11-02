//! Ten real PostgreSQL clusters across a SIGKILL of the daemon (spec 0030,
//! AC-17, NFR-007): the new daemon classifies every one of them within
//! ten seconds of its first answered handshake. The clusters of the managed
//! runtimes from the cache of `cargo xtask`, five of 17 and five of 18; an
//! artifact that is not there ends the test as `BLOCKED`, never as a
//! download.

use std::time::Duration;

use stackly_protocol::events::{EventCursor, EventKind, SubscribeParams};
use stackly_protocol::instance::{
    InstanceLifecycle, InstanceReadinessState, ListInstancesParams, ReconcilePhase,
};
use stackly_protocol::json::DecodedFrame;
use stackly_protocol::operation::OperationState;
use stackly_protocol::{Call, Reply, Sequence};
use stackly_testkit::evidence::{Kind, evidence};

use crate::common;
use crate::harness::{PG17, PG18, Site, ten_running_instances};

/// NFR-007: how long the new daemon gets from its first answered handshake.
const INVENTORY_BOUND: Duration = Duration::from_secs(10);

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed PostgreSQL 17 and 18 artifacts in the runtime cache"]
async fn ten_postgresql_instances_are_inventoried_within_ten_seconds() {
    // covers: spec 0030 AC-17
    let ev = evidence!(
        covers = ["NFR-007"],
        kind = Kind::Native,
        expected = "after SIGKILL of the daemon with ten PostgreSQL instances running, five of 17 and five of 18, the new daemon publishes daemon.reconciling finished and instance.list classifies all ten within ten seconds of the first handshake that succeeds, each adopted with its old pid and start time",
    );
    let Some(site) = Site::open(&ev, PG17) else {
        return;
    };
    let Some(other) = site.install_other(&ev, PG18) else {
        return;
    };
    let first = site.binary(&[]);
    let api = site.binary_api(&first).await;
    let running = ten_running_instances(&site, &api, (other.0.as_uuid(), PG18.major)).await;
    site.subject(&ev, PG17.version);

    drop(api);
    first.kill();

    // The clock runs from the first handshake that succeeds, which is
    // recorded by the harness the moment the socket answers.
    let second = site.binary(&[]);
    let started = second.first_handshake;
    let (client, accepted) = common::client(&second.socket).await;
    let (_, mut events) = client
        .subscribe(SubscribeParams {
            since: Some(EventCursor {
                server_instance_id: accepted.server_instance_id,
                sequence: Sequence::new(0).expect("0 is a sequence"),
            }),
            kinds: vec!["daemon.reconciling".to_owned()],
            resource: None,
        })
        .await
        .expect("the subscription is accepted");
    let finished = loop {
        let frame = tokio::time::timeout(Duration::from_secs(60), events.next())
            .await
            .expect("daemon.reconciling finished arrives")
            .expect("the stream reads")
            .expect("the stream stays open");
        if let DecodedFrame::Event(event) = frame
            && let EventKind::DaemonReconciling(reconciling) = event.kind
            && reconciling.phase == ReconcilePhase::Finished
        {
            break (started.elapsed(), reconciling);
        }
    };
    let reconciled_after = finished.0;
    let classified = loop {
        match client
            .call(Call::InstanceList(ListInstancesParams::default()))
            .await
        {
            Ok(Reply::InstanceList(page)) => {
                if page.items.len() == 10
                    && page
                        .items
                        .iter()
                        .all(|item| item.lifecycle != InstanceLifecycle::Unknown)
                {
                    break (started.elapsed(), page.items);
                }
            }
            Ok(other) => panic!("not a list reply: {other:?}"),
            Err(error) if error.code() == "daemon.starting" => {}
            Err(error) => panic!("instance.list: {error:?}"),
        }
        assert!(started.elapsed() < Duration::from_secs(60), "never listed");
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    let inventoried_after = classified.0.max(reconciled_after);
    let (adopted, unknown, restarting) = (
        finished.1.adopted.get(),
        finished.1.unknown.get(),
        finished.1.restarting.get(),
    );
    assert!(
        inventoried_after <= INVENTORY_BOUND,
        "ten instances took {inventoried_after:?} after the first handshake (reconciled after {reconciled_after:?}, listed after {:?}); NFR-007 allows {INVENTORY_BOUND:?}",
        classified.0
    );
    drop(events);
    drop(client);

    // Nothing was lost on the way: the same processes, ready, answering.
    let api = site.binary_api(&second).await;
    for (made, before) in &running {
        let now = api
            .wait_instance(made.id, "adopted and ready", |now| {
                now.process.as_ref().is_some_and(|process| process.adopted)
                    && now.readiness.state == InstanceReadinessState::Ready
            })
            .await;
        let (then, now_process) = (
            before.process.as_ref().expect("a process before"),
            now.process.as_ref().expect("a process after"),
        );
        assert_eq!(now_process.pid, then.pid);
        assert_eq!(now_process.started_at, then.started_at);
        assert_eq!(now.lifecycle, InstanceLifecycle::Running);
        assert_eq!(
            site.psql(made.id, made.port)
                .scalar(&made.password, "SELECT 1"),
            "1"
        );
    }
    for (made, _) in &running {
        let accepted = api.stop_when_free(made.id).await.accepted;
        let accepted = accepted.expect("a stop");
        api.wait_state(accepted.operation.id, OperationState::Succeeded)
            .await;
    }
    second.stop();
    ev.pass_with(&format!(
        "ten PostgreSQL instances (five {}, five {}): daemon.reconciling finished {} ms and instance.list classified all ten {} ms after the first handshake of the new daemon (adopted {adopted}, unknown {unknown}, restarting {restarting}), bound {} ms",
        PG17.version,
        PG18.version,
        reconciled_after.as_millis(),
        inventoried_after.as_millis(),
        INVENTORY_BOUND.as_millis(),
    ));
}
