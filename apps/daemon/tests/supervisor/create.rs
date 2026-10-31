//! `instance.create` (spec 0010 K1, K12, K14, and spec 0020): the refusals
//! the method gives, the replay of a repeated key, and the operation that
//! ends `failed` with everything it made undone when its init fails, and
//! the readiness a created instance reads before its first start.

use serde_json::json;
use stackly_protocol::codes::{instance, operation};
use stackly_protocol::instance::{InstanceHealthState, InstanceReadinessState, PortRequest};
use stackly_protocol::operation::OperationState;
use stackly_testkit::allocate_port;
use stackly_testkit::evidence::{Kind, evidence};

use crate::support::{Api, Dialog, Site, key};

#[tokio::test(flavor = "multi_thread")]
async fn a_create_that_the_registry_or_the_package_refuses_gets_its_own_code_and_no_operation() {
    let ev = evidence!(
        covers = ["AC-029"],
        kind = Kind::Fixture,
        expected = "instance.create refuses a taken name, an unknown provider, a runtime of another software, bad settings, and a bad endpoint, each with its code and no operation",
    );
    let site = Site::new();
    let daemon = site.start().await;
    let api = site.api().await;
    api.create_ready("taken", json!({})).await;

    let taken = api
        .create(api.params("taken", json!({})))
        .await
        .unwrap_err();
    assert!(taken.is(&instance::NAME_TAKEN), "{taken:?}");

    let mut unknown = api.params("unknown", json!({}));
    unknown.provider.name = "testing/nothing".to_owned();
    let error = api.create(unknown).await.unwrap_err();
    assert!(error.is(&instance::PROVIDER_NOT_FOUND), "{error:?}");

    let mut other_revision = api.params("revision", json!({}));
    other_revision.provider.revision = Some("9.9.9".to_owned());
    let error = api.create(other_revision).await.unwrap_err();
    assert!(error.is(&instance::PROVIDER_NOT_FOUND), "{error:?}");

    // An absent revision means the revision of the loaded catalog (spec
    // 0020); this daemon has no catalog.
    let mut no_revision = api.params("norevision", json!({}));
    no_revision.provider.revision = None;
    let error = api.create(no_revision).await.unwrap_err();
    assert_eq!(error.code(), "catalog.unavailable", "{error:?}");

    let mut lost = api.params("lost", json!({}));
    lost.runtime_id = uuid::Uuid::now_v7();
    let error = api.create(lost).await.unwrap_err();
    assert!(error.is(&instance::RUNTIME_NOT_FOUND), "{error:?}");

    let error = api
        .create(api.params("unknownsetting", json!({ "nosuch": 1 })))
        .await
        .unwrap_err();
    assert!(error.is(&instance::SETTINGS_INVALID), "{error:?}");
    assert!(
        error
            .fields()
            .iter()
            .any(|field| field.path() == "settings.nosuch"),
        "{error:?}"
    );

    let mut endpoint = api.params("badendpoint", json!({}));
    endpoint.endpoints = vec![stackly_protocol::instance::EndpointRequest {
        name: "nosuch".to_owned(),
        port: PortRequest::Auto,
    }];
    let error = api.create(endpoint).await.unwrap_err();
    assert!(error.is(&instance::ENDPOINT_INVALID), "{error:?}");

    let mut zero = api.params("zeroport", json!({}));
    zero.endpoints = vec![Api::http_port(0)];
    let error = api.create(zero).await.unwrap_err();
    assert!(error.is(&instance::ENDPOINT_INVALID), "{error:?}");

    let page = api.list(Default::default()).await;
    assert_eq!(
        page.items.len(),
        1,
        "only the first create made an instance"
    );
    drop(api);
    daemon
        .shutdown(
            stackly_protocol::events::ShutdownReason::Requested,
            std::future::pending(),
        )
        .await;
    ev.pass();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_repeated_create_with_the_same_key_gets_its_reply_back_not_name_taken() {
    let ev = evidence!(
        covers = ["AC-029"],
        kind = Kind::Fixture,
        expected = "instance.create sent again with its key after it succeeded answers with the same operation and replayed true, while a new key for the name is refused",
    );
    let site = Site::new();
    let daemon = site.start().await;
    let api = site.api().await;
    let key = key();

    let first = api
        .create_with_key(api.params("again", json!({})), &key)
        .await
        .expect("the create is accepted")
        .accepted
        .expect("a create starts an operation");
    api.wait_state(first.operation.id, OperationState::Succeeded)
        .await;
    let again = api
        .create_with_key(api.params("again", json!({})), &key)
        .await
        .expect("the same key replays")
        .accepted
        .expect("the replay names the operation");
    assert_eq!(again.operation.id, first.operation.id);
    assert!(again.replayed);

    let fresh = api
        .create_with_key(api.params("again", json!({})), &crate::support::key())
        .await
        .unwrap_err();
    assert!(fresh.is(&instance::NAME_TAKEN), "{fresh:?}");

    let reused = api
        .create_with_key(api.params("another", json!({})), &key)
        .await
        .unwrap_err();
    assert!(reused.is(&operation::IDEMPOTENCY_KEY_REUSED), "{reused:?}");
    drop(api);
    daemon
        .shutdown(
            stackly_protocol::events::ShutdownReason::Requested,
            std::future::pending(),
        )
        .await;
    ev.pass();
}

#[tokio::test(flavor = "multi_thread")]
async fn an_init_that_fails_is_compensated_and_leaves_neither_instance_nor_name() {
    let ev = evidence!(
        covers = ["AC-013", "AC-029", "BR-023"],
        kind = Kind::Fixture,
        expected = "a create whose init fails ends failed with supervisor.init_failed after the compensation removed the instance row, the instance folder, and its secret reference, so the instance is gone, the name is free for the next create, and no operation waits for recovery",
    );
    let site = Site::new();
    let daemon = site.start().await;
    let api = site.api().await;

    let accepted = api
        .create(api.params("broken", json!({ "failInit": true })))
        .await
        .expect("the create is accepted")
        .accepted
        .expect("a create starts an operation");
    let id: uuid::Uuid = accepted
        .operation
        .resource
        .as_ref()
        .expect("the operation names the instance")
        .id
        .parse()
        .expect("an instance id");
    let detail = api
        .wait_state(accepted.operation.id, OperationState::Failed)
        .await;
    assert!(
        detail
            .operation
            .failure
            .as_ref()
            .is_some_and(|failure| failure.error.code() == "supervisor.init_failed"),
        "{detail:?}"
    );
    assert!(detail.operation.recovery.is_none(), "{detail:?}");

    let gone = api.get(id).await.unwrap_err();
    assert_eq!(gone.code(), "resource.not_found", "{gone:?}");
    assert!(
        api.list(Default::default()).await.items.is_empty(),
        "no row of the failed create is left"
    );
    assert!(
        !site
            .prepared
            .layout()
            .instance_paths(stackly_domain::InstanceId::from_uuid(id).expect("a v7 id"))
            .root
            .exists(),
        "the instance folder is gone"
    );

    // The name is free at once.
    let again = api
        .create(api.params("broken", json!({})))
        .await
        .expect("the same name creates after the failure")
        .accepted
        .expect("a create starts an operation");
    api.wait_state(again.operation.id, OperationState::Succeeded)
        .await;
    drop(api);
    daemon
        .shutdown(
            stackly_protocol::events::ShutdownReason::Requested,
            std::future::pending(),
        )
        .await;
    ev.pass();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_created_instance_that_never_started_reads_unknown_with_reason_never_observed() {
    let site = Site::new();
    let daemon = site.start().await;
    let api = site.api().await;

    let id = api.create_ready("fresh", json!({})).await;

    // Spec 0004: a new instance reads `never_observed` until it first runs;
    // `daemon_restarted` is for instances loaded when the daemon started.
    let got = api.get(id).await.expect("the created instance can be read");
    let page = api.list(Default::default()).await;
    let listed = page
        .items
        .iter()
        .find(|item| item.id == id)
        .expect("the list holds the created instance");
    for summary in [&got, listed] {
        assert_eq!(summary.readiness.state, InstanceReadinessState::Unknown);
        assert_eq!(summary.readiness.reason, "never_observed", "{summary:?}");
        assert_eq!(summary.health.state, InstanceHealthState::Unknown);
        assert_eq!(summary.health.reason, "never_observed", "{summary:?}");
    }
    drop(api);
    daemon
        .shutdown(
            stackly_protocol::events::ShutdownReason::Requested,
            std::future::pending(),
        )
        .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_never_started_instance_reports_the_restart_policy_before_and_after_a_daemon_restart() {
    let site = Site::new();
    let daemon = site.start().await;
    let api = site.api().await;
    let id = api.create_ready("idle", json!({})).await;

    // Spec 0010, *Restart policy*: 5 attempts in 5 minutes, never 0 of 0.
    let created = api.get(id).await.expect("the created instance can be read");
    drop(api);
    daemon
        .shutdown(
            stackly_protocol::events::ShutdownReason::Requested,
            std::future::pending(),
        )
        .await;
    let daemon = site.start().await;
    let api = site.api().await;
    let loaded = api.get(id).await.expect("the loaded instance can be read");

    for summary in [&created, &loaded] {
        assert_eq!(summary.restart.attempts, 0, "{summary:?}");
        assert_eq!(summary.restart.budget, 5, "{summary:?}");
        assert_eq!(summary.restart.window_seconds, 300, "{summary:?}");
        assert_eq!(summary.restart.next_attempt_at, None, "{summary:?}");
        assert!(!summary.restart.exhausted, "{summary:?}");
    }
    drop(api);
    daemon
        .shutdown(
            stackly_protocol::events::ShutdownReason::Requested,
            std::future::pending(),
        )
        .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn five_creates_released_together_behind_a_keychain_dialog_all_succeed() {
    let ev = evidence!(
        covers = ["REQ-008"],
        kind = Kind::Fixture,
        expected = "in three rounds, five creates with different names and different fixed ports sent while a Keychain call of another action waits behind its dialog all end succeeded once the dialog is answered, and the registry lists all fifteen new instances",
    );
    const ROUNDS: usize = 3;
    const PER_ROUND: usize = 5;
    let site = Site::new();
    let daemon = site.start().await;
    let api = site.api().await;

    for round in 0..ROUNDS {
        let dialog = Dialog::open(&site, &format!("local:guarded/round-{round}/value")).await;
        let mut operations = Vec::new();
        for number in 0..PER_ROUND {
            let mut params = api.params(&format!("round-{round}-{number}"), json!({}));
            params.endpoints = vec![Api::http_port(allocate_port().expect("a free port"))];
            let accepted = api
                .create(params)
                .await
                .expect("the create is accepted")
                .accepted
                .expect("a create starts an operation");
            operations.push(accepted.operation.id);
        }
        dialog.close().await;
        for operation in operations {
            let ended = api.wait_final(operation).await;
            assert_eq!(
                ended.operation.state,
                OperationState::Succeeded,
                "round {round}: {ended:?}"
            );
        }
    }

    let page = api.list(Default::default()).await;
    assert_eq!(page.items.len(), ROUNDS * PER_ROUND);
    drop(api);
    daemon
        .shutdown(
            stackly_protocol::events::ShutdownReason::Requested,
            std::future::pending(),
        )
        .await;
    ev.pass();
}
