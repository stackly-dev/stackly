//! The `secret.*` methods in process (spec 0011 build step 5): the five
//! methods over the real socket against a `FakeSecretStore` behind the
//! real `SecretWorker`, the Keychain interaction of each kind of client,
//! `secret.changed`, the store in `daemon.get_info`, the probe at start,
//! and that no value reaches an error, an event, or the log.

use std::sync::Arc;

use serde_json::{Value, json};
use stackly_core::secret::{SecretWorker, WorkerConfig};
use stackly_domain::{AuditQuery, Registry, SecretRef};
use stackly_ipc::Client;
use stackly_platform::{Interaction, SecretStore};
use stackly_protocol::events::ShutdownReason;
use stackly_protocol::handshake::ClientKind;
use stackly_protocol::secret::{
    DeleteSecretParams, ListSecretsParams, RevealPurpose, RevealSecretParams, SecretInput,
    SecretMode, SecretOwnerKind, SecretStatus, SecretStatusParams, SecretStoreBackend,
    SecretStoreState, SecretText, SecretValueOrigin, SecretWarning, SetSecretParams,
};
use stackly_protocol::{Call, ErrorBody, Reply, Revision};
use stackly_testkit::evidence::FixtureCredential;
use stackly_testkit::{FakeSecretStore, SecretOp};
use stacklyd::server::DaemonHandle;

use crate::common::{Raw, Site, client, client_as, error_of, request};

const REFERENCE: &str = "local:erp/postgres/admin";

/// A site whose secret store is a fake behind the real worker.
fn site_with_store() -> (Site, FakeSecretStore) {
    let store = FakeSecretStore::new();
    let worker = SecretWorker::spawn(Arc::new(store.clone()), WorkerConfig::default())
        .expect("the worker thread starts");
    let site = Site::new().with_secret_store(worker as Arc<dyn SecretStore>);
    (site, store)
}

fn reference() -> SecretRef {
    SecretRef::parse(REFERENCE).expect("the test reference is valid")
}

fn set(reference: &str, input: SecretInput, mode: SecretMode) -> Call {
    Call::SecretSet(SetSecretParams {
        reference: reference.to_owned(),
        input,
        mode,
        expected_revision: None,
        confirm_stored_only: false,
    })
}

fn value(text: &str) -> SecretInput {
    SecretInput::Value(SecretText::new(text.to_owned()))
}

async fn ok(client: &Client, request: Call) -> Reply {
    match client.call(request).await {
        Ok(reply) => reply,
        Err(error) => panic!("the call failed: {error:?}"),
    }
}

async fn err(client: &Client, request: Call) -> ErrorBody {
    match client.call(request).await {
        Ok(reply) => panic!("the call should fail, got {reply:?}"),
        Err(error) => *error,
    }
}

async fn stop(daemon: DaemonHandle) {
    daemon
        .shutdown(ShutdownReason::Requested, std::future::pending())
        .await;
}

#[tokio::test]
async fn a_value_is_set_listed_probed_revealed_and_deleted_over_the_socket() {
    let (site, store) = site_with_store();
    let daemon = site.start().await;
    let (client, _) = client(&site).await;
    let credential = FixtureCredential::new();

    let Reply::SecretSet(stored) = ok(
        &client,
        set(REFERENCE, value(credential.value()), SecretMode::Create),
    )
    .await
    else {
        panic!("not a set reply");
    };
    let Reply::SecretList(listed) = ok(
        &client,
        Call::SecretList(ListSecretsParams {
            owner_kind: Some(SecretOwnerKind::User),
            status: Some(SecretStatus::Present),
        }),
    )
    .await
    else {
        panic!("not a list reply");
    };
    let Reply::SecretStatus(status) = ok(
        &client,
        Call::SecretStatus(SecretStatusParams {
            reference: REFERENCE.to_owned(),
        }),
    )
    .await
    else {
        panic!("not a status reply");
    };
    let Reply::SecretReveal(revealed) = ok(
        &client,
        Call::SecretReveal(RevealSecretParams {
            reference: REFERENCE.to_owned(),
            purpose: RevealPurpose::Copy,
        }),
    )
    .await
    else {
        panic!("not a reveal reply");
    };
    let Reply::SecretDelete(deleted) = ok(
        &client,
        Call::SecretDelete(DeleteSecretParams {
            reference: REFERENCE.to_owned(),
            expected_revision: Some(Revision::saturating(1)),
        }),
    )
    .await
    else {
        panic!("not a delete reply");
    };

    assert_eq!(stored.summary.reference, REFERENCE);
    assert_eq!(stored.summary.owner_kind, SecretOwnerKind::User);
    assert_eq!(stored.summary.owner_id, None);
    assert_eq!(stored.summary.purpose, "user");
    assert_eq!(stored.summary.status, SecretStatus::Present);
    assert_eq!(
        stored.summary.value_origin,
        Some(SecretValueOrigin::Entered)
    );
    assert_eq!(stored.summary.revision, Revision::saturating(1));
    assert!(stored.warnings.is_empty() && stored.effects.is_empty());
    assert_eq!(listed.items.len(), 1);
    assert_eq!(listed.items[0].reference, REFERENCE);
    assert_eq!(status.summary.status, SecretStatus::Present);
    assert_eq!(
        status.summary.revision,
        Revision::saturating(1),
        "a probe never raises the revision"
    );
    assert_eq!(status.store.backend, SecretStoreBackend::Fake);
    assert_eq!(revealed.value.expose(), credential.value());
    assert_eq!(deleted.reference, REFERENCE);
    assert!(!store.contains(&reference()));
    stop(daemon).await;
}

#[tokio::test]
async fn a_short_value_comes_back_with_the_warning_and_a_generated_one_is_not_returned() {
    let (site, store) = site_with_store();
    let daemon = site.start().await;
    let (client, _) = client(&site).await;

    let Reply::SecretSet(short) =
        ok(&client, set(REFERENCE, value("abc"), SecretMode::Create)).await
    else {
        panic!("not a set reply");
    };
    let Reply::SecretSet(generated) = ok(
        &client,
        set(
            "local:erp/postgres/root",
            SecretInput::Generate,
            SecretMode::Create,
        ),
    )
    .await
    else {
        panic!("not a set reply");
    };

    assert_eq!(short.warnings, [SecretWarning::ShortValueNotScrubbed]);
    assert_eq!(
        generated.summary.value_origin,
        Some(SecretValueOrigin::Generated)
    );
    let json = serde_json::to_string(&*generated).expect("the reply serializes");
    let stored = store
        .stored_value(&SecretRef::parse("local:erp/postgres/root").unwrap())
        .expect("the generated value is stored");
    assert_eq!(stored.len(), 32);
    assert!(!json.contains(&stored), "{json}");
    stop(daemon).await;
}

#[tokio::test]
async fn each_failure_has_its_catalog_code() {
    let (site, store) = site_with_store();
    let daemon = site.start().await;
    let (client, _) = client(&site).await;
    ok(
        &client,
        set(REFERENCE, value("fixture-value-1"), SecretMode::Create),
    )
    .await;

    let exists = err(
        &client,
        set(REFERENCE, value("fixture-value-2"), SecretMode::Create),
    )
    .await;
    let missing = err(
        &client,
        set(
            "local:erp/postgres/none",
            value("fixture-value-2"),
            SecretMode::Replace,
        ),
    )
    .await;
    let mismatch = err(
        &client,
        Call::SecretSet(SetSecretParams {
            reference: REFERENCE.to_owned(),
            input: value("fixture-value-3"),
            mode: SecretMode::Replace,
            expected_revision: Some(Revision::saturating(9)),
            confirm_stored_only: false,
        }),
    )
    .await;
    let unknown = err(
        &client,
        Call::SecretStatus(SecretStatusParams {
            reference: "local:erp/never/seen".to_owned(),
        }),
    )
    .await;
    let reserved = err(
        &client,
        set(
            "local:stackly/internal/key",
            value("fixture-value-4"),
            SecretMode::Create,
        ),
    )
    .await;
    let instance = err(
        &client,
        set(
            "local:instance/0192f3a5-0000-7000-8000-000000000001/admin",
            value("fixture-value-4"),
            SecretMode::Create,
        ),
    )
    .await;
    store.lock();
    let locked = err(
        &client,
        Call::SecretReveal(RevealSecretParams {
            reference: REFERENCE.to_owned(),
            purpose: RevealPurpose::Show,
        }),
    )
    .await;
    store.unlock();
    store.deny(&reference());
    let denied = err(
        &client,
        Call::SecretReveal(RevealSecretParams {
            reference: REFERENCE.to_owned(),
            purpose: RevealPurpose::Show,
        }),
    )
    .await;

    assert_eq!(exists.code(), "secret.conflict");
    assert_eq!(exists.detail("reason"), Some("already_exists"));
    assert_eq!(missing.code(), "secret.not_found");
    assert_eq!(missing.detail("reference"), Some("local:erp/postgres/none"));
    assert_eq!(mismatch.code(), "resource.revision_mismatch");
    assert_eq!(mismatch.detail("expected"), Some("9"));
    assert_eq!(mismatch.detail("actual"), Some("1"));
    assert_eq!(unknown.code(), "secret.not_found");
    assert_eq!(reserved.code(), "secret.invalid_reference");
    assert_eq!(instance.code(), "secret.not_found");
    assert_eq!(locked.code(), "secret.store_locked");
    assert_eq!(denied.code(), "secret.store_access_denied");
    assert_eq!(denied.detail("byUser"), Some("false"));
    stop(daemon).await;
}

#[tokio::test]
async fn params_that_break_the_value_or_reference_rules_never_reach_the_store() {
    let (site, store) = site_with_store();
    let daemon = site.start().await;
    let mut raw = Raw::ready(&site.socket()).await;
    let cases = [
        (
            "bind",
            json!({"reference": REFERENCE, "input": {"type": "bind", "content": "local:erp/other"}, "mode": "create"}),
            "input",
        ),
        (
            "empty",
            json!({"reference": REFERENCE, "input": {"type": "value", "content": ""}, "mode": "create"}),
            "input.content",
        ),
        (
            "nul",
            json!({"reference": REFERENCE, "input": {"type": "value", "content": "a\u{0}b"}, "mode": "create"}),
            "input.content",
        ),
        (
            "format",
            json!({"reference": "keychain:erp/postgres/admin", "input": {"type": "generate"}, "mode": "create"}),
            "reference",
        ),
        (
            "mode",
            json!({"reference": REFERENCE, "input": {"type": "generate"}, "mode": "overwrite"}),
            "mode",
        ),
    ];

    for (id, params, path) in cases {
        raw.send(&request(id, "secret.set", Some(params))).await;
        let frame = raw.answer(id).await;
        let error = error_of(&frame);
        assert_eq!(error.code(), "request.invalid_params", "{id}");
        assert_eq!(error.fields()[0].path(), path, "{id}");
    }
    assert!(store.calls().is_empty(), "{:?}", store.calls());
    stop(daemon).await;
}

#[tokio::test]
async fn only_an_interactive_client_may_see_a_keychain_dialog() {
    let (site, store) = site_with_store();
    let daemon = site.start().await;
    let (plain, _) = client_as(&site, ClientKind::Cli, None).await;
    let (interactive, _) = client_as(&site, ClientKind::Gui, Some(true)).await;
    ok(
        &plain,
        set(REFERENCE, value("fixture-value-1"), SecretMode::Create),
    )
    .await;

    ok(
        &plain,
        Call::SecretReveal(RevealSecretParams {
            reference: REFERENCE.to_owned(),
            purpose: RevealPurpose::Show,
        }),
    )
    .await;
    ok(
        &interactive,
        Call::SecretReveal(RevealSecretParams {
            reference: REFERENCE.to_owned(),
            purpose: RevealPurpose::Show,
        }),
    )
    .await;

    let reads: Vec<_> = store
        .calls()
        .into_iter()
        .filter(|call| call.op == SecretOp::Read)
        .map(|call| call.interaction)
        .collect();
    assert_eq!(
        reads,
        [Some(Interaction::Never), Some(Interaction::AllowPrompt)]
    );
    stop(daemon).await;
}

#[tokio::test]
async fn every_reveal_and_every_set_writes_an_audit_row_without_the_value() {
    let (site, _store) = site_with_store();
    let daemon = site.start().await;
    let (client, _) = client(&site).await;
    let credential = FixtureCredential::new();
    ok(
        &client,
        set(REFERENCE, value(credential.value()), SecretMode::Create),
    )
    .await;
    ok(
        &client,
        Call::SecretReveal(RevealSecretParams {
            reference: REFERENCE.to_owned(),
            purpose: RevealPurpose::Show,
        }),
    )
    .await;
    err(
        &client,
        Call::SecretReveal(RevealSecretParams {
            reference: "local:erp/never/seen".to_owned(),
            purpose: RevealPurpose::Copy,
        }),
    )
    .await;

    let registry = daemon.core().registry().expect("the registry is ready");
    let page = registry
        .audit_events(AuditQuery {
            limit: 20,
            before: None,
        })
        .expect("the audit rows read")
        .value;
    let actions: Vec<String> = page
        .items
        .iter()
        .map(|row| format!("{} {}", row.action, row.result.as_code()))
        .collect();

    assert_eq!(page.items.len(), 3, "{actions:?}");
    assert!(
        actions.contains(&"secret.set succeeded".to_owned()),
        "{actions:?}"
    );
    assert!(
        actions.contains(&"secret.reveal succeeded".to_owned()),
        "{actions:?}"
    );
    assert!(
        actions.contains(&"secret.reveal failed".to_owned()),
        "{actions:?}"
    );
    credential.assert_absent(&format!("{page:?}"), "the audit rows");
    stop(daemon).await;
}

#[tokio::test]
async fn a_change_is_published_as_secret_changed_and_a_delete_raises_the_revision() {
    let (site, _store) = site_with_store();
    let daemon = site.start().await;
    let (client, _) = client(&site).await;
    let mut raw = Raw::ready(&site.socket()).await;
    raw.send(&request(
        "sub",
        "events.subscribe",
        Some(json!({"kinds": ["secret."]})),
    ))
    .await;
    raw.answer("sub").await;

    ok(
        &client,
        set(REFERENCE, value("fixture-value-1"), SecretMode::Create),
    )
    .await;
    ok(
        &client,
        Call::SecretDelete(DeleteSecretParams {
            reference: REFERENCE.to_owned(),
            expected_revision: None,
        }),
    )
    .await;

    let stored = raw.recv().await.expect("the first event arrives");
    let deleted = raw.recv().await.expect("the second event arrives");
    for event in [&stored, &deleted] {
        assert_eq!(event["type"], "event", "{event}");
        assert_eq!(event["content"]["kind"]["type"], "secret.changed");
        assert_eq!(
            event["content"]["resource"],
            json!({"kind": "secret", "id": REFERENCE})
        );
    }
    assert_eq!(stored["content"]["kind"]["content"]["status"], "present");
    assert_eq!(stored["content"]["revision"], 1);
    assert_eq!(deleted["content"]["kind"]["content"]["status"], "deleted");
    assert_eq!(deleted["content"]["revision"], 2);
    stop(daemon).await;
}

#[tokio::test]
async fn get_info_reports_the_store_without_calling_it() {
    let (site, store) = site_with_store();
    let daemon = site.start().await;
    let (client, _) = client(&site).await;

    let Reply::DaemonGetInfo(info) = ok(&client, Call::DaemonGetInfo).await else {
        panic!("not an info reply");
    };

    let reported = info.secret_store.expect("the store is reported");
    assert_eq!(reported.backend, SecretStoreBackend::Fake);
    assert_eq!(reported.state, SecretStoreState::Unknown);
    assert_eq!(reported.target, None);
    assert!(
        store.calls().iter().all(|call| call.op == SecretOp::Probe),
        "only the probe at start may have called the store: {:?}",
        store.calls()
    );
    stop(daemon).await;
}

#[tokio::test]
async fn a_platform_without_a_secret_store_answers_unsupported() {
    let site = Site::new();
    let daemon = site.start().await;
    let (client, _) = client(&site).await;

    let Reply::DaemonGetInfo(info) = ok(&client, Call::DaemonGetInfo).await else {
        panic!("not an info reply");
    };
    let refused = err(
        &client,
        set(REFERENCE, value("fixture-value-1"), SecretMode::Create),
    )
    .await;

    let reported = info.secret_store.expect("the store is reported");
    assert_eq!(reported.backend, SecretStoreBackend::Unsupported);
    assert_eq!(reported.state, SecretStoreState::Unsupported);
    assert_eq!(refused.code(), "secret.store_unsupported");
    stop(daemon).await;
}

#[tokio::test]
async fn the_start_probes_every_reference_so_a_vanished_item_reads_missing() {
    let (site, store) = site_with_store();
    let first = site.start().await;
    let (client_one, _) = client(&site).await;
    ok(
        &client_one,
        set(REFERENCE, value("fixture-value-1"), SecretMode::Create),
    )
    .await;
    drop(client_one);
    stop(first).await;
    store.remove(&reference());

    let second = site.start().await;
    let (client_two, _) = client(&site).await;
    let Reply::SecretList(listed) =
        ok(&client_two, Call::SecretList(ListSecretsParams::default())).await
    else {
        panic!("not a list reply");
    };

    assert_eq!(listed.items.len(), 1);
    assert_eq!(listed.items[0].status, SecretStatus::Missing);
    assert_eq!(listed.items[0].revision, Revision::saturating(1));
    stop(second).await;
}

#[tokio::test]
async fn no_value_reaches_an_error_frame_even_one_a_handler_built_with_the_value_in_it() {
    use stackly_protocol::ErrorBody as Body;
    use stackly_protocol::codes::internal;
    use stacklyd::dispatch::{HandlerFuture, Handlers, RequestContext};
    use stacklyd::state::DaemonCore;

    struct Leaky(String);

    impl Handlers for Leaky {
        fn get_info<'a>(
            &'a self,
            core: &'a DaemonCore,
            _context: &'a RequestContext,
        ) -> HandlerFuture<'a, stackly_protocol::daemon::DaemonInfo> {
            let leaked = self.0.clone();
            Box::pin(async move {
                core.redactor().learn(
                    &stackly_platform::SecretValue::new(leaked.clone())
                        .expect("the test value is valid"),
                );
                Err(Box::new(
                    Body::new(
                        &internal::UNEXPECTED,
                        format!("failed with {leaked} inside"),
                    )
                    .with_hint(format!("not {leaked} again")),
                ))
            })
        }
    }

    let credential = FixtureCredential::new();
    let site = Site::new();
    let daemon = site.start_with(Leaky(credential.value().to_owned())).await;
    let mut raw = Raw::ready(&site.socket()).await;

    raw.send(&request("leak", "daemon.get_info", None)).await;
    let frame = raw.answer("leak").await;

    assert_eq!(frame["type"], "error", "{frame}");
    credential.assert_absent(&frame.to_string(), "the error frame");
    assert!(
        frame["content"]["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("****")),
        "{frame}"
    );
    stop(daemon).await;
}

#[tokio::test]
async fn the_secret_methods_are_refused_while_the_registry_is_in_recovery() {
    let (site, _store) = site_with_store();
    let layout = site.prepared.layout().clone();
    let db = layout.registry_db();
    std::fs::create_dir_all(db.parent().expect("the registry has a folder"))
        .expect("the registry folder is created");
    std::fs::write(
        &db,
        b"this is not a sqlite database at all, just text to break it",
    )
    .expect("a broken registry is written");
    let daemon = site.start().await;
    let (client, _) = client(&site).await;

    let refused: Vec<Value> = vec![
        json!(
            err(&client, Call::SecretList(ListSecretsParams::default()))
                .await
                .code()
        ),
        json!(
            err(
                &client,
                set(REFERENCE, value("fixture-value-1"), SecretMode::Create)
            )
            .await
            .code()
        ),
    ];

    assert_eq!(
        refused,
        [
            json!("registry.recovery_mode"),
            json!("registry.recovery_mode")
        ]
    );
    stop(daemon).await;
}
