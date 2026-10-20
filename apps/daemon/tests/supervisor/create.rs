//! `instance.create` of the first daemon build (spec 0010, create
//! sementara, K1, K12, K14): the refusals the method gives, the replay of a
//! repeated key, and the operation that ends `needs_recovery` when its init
//! fails.

use serde_json::json;
use stackly_protocol::codes::{instance, operation, request};
use stackly_protocol::instance::{InstanceLifecycle, PortRequest};
use stackly_protocol::operation::{AbandonOperationParams, OperationState};
use stackly_protocol::{Call, Reply};
use stackly_testkit::evidence::{Kind, evidence};

use crate::support::{Api, Site, key};

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

    let mut no_revision = api.params("norevision", json!({}));
    no_revision.provider.revision = None;
    let error = api.create(no_revision).await.unwrap_err();
    assert!(error.is(&request::INVALID_PARAMS), "{error:?}");
    assert_eq!(error.fields()[0].path(), "provider.revision");

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

    let mut no_token = api.params("notoken", json!({}));
    no_token.secrets.clear();
    let error = api.create(no_token).await.unwrap_err();
    assert!(error.is(&instance::SETTINGS_INVALID), "{error:?}");

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
async fn an_init_that_fails_leaves_needs_recovery_with_the_instance_failed_until_it_is_abandoned() {
    let ev = evidence!(
        covers = ["AC-013", "AC-029"],
        kind = Kind::Fixture,
        expected = "a create whose init fails ends needs_recovery after the point of no return with the instance failed and not initialized, start is refused with supervisor.not_initialized, and abandon closes the operation",
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
        .wait_state(accepted.operation.id, OperationState::NeedsRecovery)
        .await;
    let recovery = detail
        .operation
        .recovery
        .as_ref()
        .expect("a needs_recovery operation says what to do");
    assert!(
        recovery
            .error
            .as_ref()
            .is_some_and(|error| error.code() == "supervisor.init_failed"),
        "{recovery:?}"
    );
    let failed = api.get(id).await.expect("the instance stays registered");
    assert_eq!(failed.lifecycle, InstanceLifecycle::Failed);
    assert_eq!(failed.initialized, Some(false));

    let refused = api.start(id).await.unwrap_err();
    assert_eq!(refused.code(), "supervisor.not_initialized", "{refused:?}");

    // The name stays held until the operation is abandoned.
    let taken = api
        .create(api.params("broken", json!({})))
        .await
        .unwrap_err();
    assert!(
        taken.is(&instance::NAME_TAKEN) || taken.code() == "operation.recovery_required",
        "{taken:?}"
    );

    let reply = api
        .client
        .call(Call::OperationAbandon(AbandonOperationParams {
            operation_id: accepted.operation.id,
            expected_revision: detail.operation.revision,
        }))
        .await
        .expect("a needs_recovery operation can be abandoned");
    let Reply::OperationAbandon(abandoned) = reply else {
        panic!("not an abandon reply");
    };
    assert_eq!(abandoned.operation.state, OperationState::Failed);
    drop(api);
    daemon
        .shutdown(
            stackly_protocol::events::ShutdownReason::Requested,
            std::future::pending(),
        )
        .await;
    ev.pass();
}
