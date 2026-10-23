//! A start the data refuses carries the migration hint on the wire (spec
//! 0025 AC-5): `supervisor.data_incompatible` fails the operation at
//! `verify`, before any spawn, and the daemon adds the static hint of its
//! `outcome` to the stored error.

use std::fs;

use serde_json::json;
use stackly_core::instance::rebind;
use stackly_core::operation::OperationError;
use stackly_domain::{InstanceId, RuntimeId};
use stackly_protocol::ErrorBody;
use stackly_protocol::ErrorDetail;
use stackly_protocol::codes::supervisor;
use stackly_protocol::instance::InstanceLifecycle;
use stackly_protocol::operation::{OperationDetail, OperationState};
use stackly_testkit::evidence::{Kind, evidence};
use uuid::Uuid;

use crate::support::{Api, Site, serving};

const INCOMPATIBLE_HINT: &str = "This data belongs to another major version, so this runtime never runs it. Moving it needs a migration: create an instance on this runtime and move the data with a dump and restore.";
const MARKER_HINT: &str = "The version marker in the data folder is missing, unreadable, or not what Stackly recorded, so Stackly can’t tell which runtime may run this data and won’t run it until the marker matches.";

/// Starts `id` and returns the error its failed operation stored.
async fn refused_start(api: &Api, id: Uuid) -> ErrorBody {
    let accepted = api
        .start(id)
        .await
        .expect("the start is accepted")
        .accepted
        .expect("a start from stopped begins an operation");
    let detail: OperationDetail = api
        .wait_state(accepted.operation.id, OperationState::Failed)
        .await;
    let failure = detail
        .operation
        .failure
        .expect("the failed operation carries its error");
    failure.error
}

fn details(error: &ErrorBody) -> Vec<(String, String)> {
    ["outcome", "data", "runtime"]
        .into_iter()
        .filter_map(|key| {
            error
                .detail(key)
                .map(|value| (key.to_owned(), value.to_owned()))
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn data_incompatible_failure_carries_the_hint() {
    // covers: AC-5 (the hint on the stored failure of the operation)
    let ev = evidence!(
        covers = ["BR-009"],
        kind = Kind::Fixture,
        expected = "a start on data whose marker names another line fails with supervisor.data_incompatible, outcome marker_mismatch, and the marker hint, and a start on an instance bound to a runtime of another line fails with outcome incompatible, data 0.1, runtime 0.2, and the migration hint; both stop at verify, the instance stays stopped, and no process serves its data",
    );
    let site = Site::new();
    let daemon = site.start().await;
    let api = site.api().await;
    let mismatch = api.create_ready("fake-mismatch", json!({})).await;
    let bound = api.create_ready("fake-bound", json!({})).await;

    // The marker names another line than the instance records.
    let marker = site.instance_paths(mismatch).data.join("FAKE_VERSION");
    assert_eq!(
        fs::read_to_string(&marker).expect("init wrote the marker"),
        "0.1\n"
    );
    fs::write(&marker, "0.2\n").expect("the marker is changed");
    let error = refused_start(&api, mismatch).await;
    assert_eq!(error.code(), supervisor::DATA_INCOMPATIBLE.code);
    assert_eq!(
        details(&error),
        [
            ("outcome".to_owned(), "marker_mismatch".to_owned()),
            ("runtime".to_owned(), "0.1".to_owned()),
        ]
    );
    assert_eq!(error.hint(), Some(MARKER_HINT));

    // The instance is bound to a runtime of another line, with the daemon
    // stopped (the domain refuses such a rebind, which is the point).
    drop(api);
    daemon
        .shutdown(
            stackly_protocol::events::ShutdownReason::Requested,
            std::future::pending(),
        )
        .await;
    let second = site.install_fake_runtime_line("0.2.0", "0.2");
    site.bind_runtime(bound, second);
    let daemon = site.start().await;
    let api = site.api().await;
    let error = refused_start(&api, bound).await;
    assert_eq!(error.code(), supervisor::DATA_INCOMPATIBLE.code);
    assert_eq!(
        details(&error),
        [
            ("outcome".to_owned(), "incompatible".to_owned()),
            ("data".to_owned(), "0.1".to_owned()),
            ("runtime".to_owned(), "0.2".to_owned()),
        ]
    );
    assert_eq!(error.hint(), Some(INCOMPATIBLE_HINT));

    for id in [mismatch, bound] {
        let summary = api.get(id).await.expect("the instance can be read");
        assert_eq!(summary.lifecycle, InstanceLifecycle::Stopped);
        assert!(summary.process.is_none(), "{summary:?}");
        assert!(serving(&site.instance_paths(id).data).is_empty());
    }

    drop(api);
    daemon
        .shutdown(
            stackly_protocol::events::ShutdownReason::Requested,
            std::future::pending(),
        )
        .await;

    // The rebind check refuses the same data with the same code and details,
    // so the hint of the wire fits its error too (spec 0025 AC-6). The marker
    // of the first instance goes back to what the registry records.
    fs::write(
        site.instance_paths(mismatch).data.join("FAKE_VERSION"),
        "0.1\n",
    )
    .expect("the marker is restored");
    let registry = site.open_registry();
    let refused = rebind::check(
        &registry,
        &site.platform,
        &site.prepared,
        InstanceId::from_uuid(mismatch).expect("an instance id"),
        RuntimeId::from_uuid(second).expect("a runtime id"),
    )
    .expect_err("data of line 0.1 never runs on line 0.2");
    let details: Vec<_> = refused
        .details()
        .iter()
        .map(|(key, value)| ErrorDetail::new(key.as_str(), value.as_str()))
        .collect();
    assert_eq!(refused.code().as_str(), supervisor::DATA_INCOMPATIBLE.code);
    assert_eq!(
        supervisor::hint(refused.code().as_str(), &details),
        Some(INCOMPATIBLE_HINT)
    );
    ev.pass();
}

/// The static hint the daemon would pick for an error of the rebind check.
fn rebind_hint(error: &rebind::RebindCheckError) -> Option<&'static str> {
    let details: Vec<_> = error
        .details()
        .iter()
        .map(|(key, value)| ErrorDetail::new(key.as_str(), value.as_str()))
        .collect();
    supervisor::hint(error.code().as_str(), &details)
}

#[tokio::test(flavor = "multi_thread")]
async fn a_rebind_check_refusal_carries_the_hint_of_its_outcome() {
    // covers: AC-6 (the hint of `codes::supervisor::hint` on the error of
    // `rebind::check`), AC-5 (the same texts)
    let ev = evidence!(
        covers = ["REQ-043", "BR-009"],
        kind = Kind::Fixture,
        expected = "the read only rebind check refuses data of line 0.1 on a runtime of line 0.2 with outcome incompatible and the migration hint, refuses a changed marker with outcome marker_mismatch and the marker hint, and gives no hint to a refusal that is no data outcome (runtime_not_found); fixture only, the user actions of AC-043 belong to feature 58",
    );
    let site = Site::new();
    let daemon = site.start().await;
    let api = site.api().await;
    let changed = api.create_ready("fake-changed", json!({})).await;
    let intact = api.create_ready("fake-intact", json!({})).await;
    drop(api);
    daemon
        .shutdown(
            stackly_protocol::events::ShutdownReason::Requested,
            std::future::pending(),
        )
        .await;
    fs::write(
        site.instance_paths(changed).data.join("FAKE_VERSION"),
        "0.2\n",
    )
    .expect("the marker is changed");
    let second = site.install_fake_runtime_line("0.2.0", "0.2");
    let registry = site.open_registry();
    let check = |id: Uuid, target: Uuid| {
        rebind::check(
            &registry,
            &site.platform,
            &site.prepared,
            InstanceId::from_uuid(id).expect("an instance id"),
            RuntimeId::from_uuid(target).expect("a runtime id"),
        )
        .expect_err("the check refuses")
    };

    let refused = check(intact, second);
    assert_eq!(refused.code().as_str(), supervisor::DATA_INCOMPATIBLE.code);
    assert_eq!(rebind_hint(&refused), Some(INCOMPATIBLE_HINT));

    let refused = check(changed, second);
    assert_eq!(refused.code().as_str(), supervisor::DATA_INCOMPATIBLE.code);
    assert_eq!(rebind_hint(&refused), Some(MARKER_HINT));

    let refused = check(intact, Uuid::now_v7());
    assert_eq!(refused.code().as_str(), "instance.runtime_not_found");
    assert_eq!(rebind_hint(&refused), None);
    ev.pass();
}
