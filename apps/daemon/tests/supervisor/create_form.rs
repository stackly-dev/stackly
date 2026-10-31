//! `instance.get_create_form` and `instance.preflight` through the real
//! daemon (spec 0020 AC-1, AC-2, AC-3, AC-4, AC-5): the form of the fake
//! package, the preflight that judges a draft without writing anything, and
//! the same answer from create for the same draft.

use std::net::TcpListener;

use serde_json::json;
use stackly_protocol::instance::{EndpointPurpose, FieldKind, PreflightStatus};
use stackly_testkit::evidence::{Kind, evidence};

use crate::support::{Api, Site, fast_with_catalog};

#[tokio::test(flavor = "multi_thread")]
async fn the_form_of_an_installed_runtime_lists_the_package_in_declaration_order() {
    let ev = evidence!(
        covers = ["REQ-029"],
        kind = Kind::Fixture,
        expected = "get_create_form builds the provider, the endpoint, the suggested name, and the settings fields of the fake package in the order its schema declares them, and refuses an unknown runtime and an unknown provider with their own codes",
    );
    let site = Site::new();
    let daemon = site.start_with(fast_with_catalog()).await;
    let api = site.api().await;

    let form = api
        .form(site.runtime_id, None)
        .await
        .expect("the form is built");

    assert_eq!(form.provider.name, "testing/fake");
    assert_eq!(form.provider.revision, "0.3.0");
    assert_eq!(form.data_path, "instances/<new instance id>/data");
    assert_eq!(form.runtime.id, site.runtime_id);
    assert!(
        form.suggested_name
            .as_deref()
            .is_some_and(|name| name.starts_with("fake-")),
        "{:?}",
        form.suggested_name
    );
    assert_eq!(form.endpoints.len(), 1);
    assert_eq!(form.endpoints[0].name, "http");
    assert_eq!(form.endpoints[0].purpose, EndpointPurpose::Client);
    assert_eq!(form.endpoints[0].default_port, Some(18080));
    let keys: Vec<&str> = form.fields.iter().map(|field| field.key.as_str()).collect();
    assert_eq!(
        &keys[..3],
        ["greeting", "label", "readyDelayMs"],
        "the order of `properties`: {keys:?}"
    );
    let secret = form
        .fields
        .iter()
        .find(|field| field.secret)
        .expect("the fake package has a secret field");
    assert_eq!(secret.key, "authToken");
    assert!(
        form.fields
            .iter()
            .any(|field| field.kind == FieldKind::Integer)
    );
    assert_eq!(form.capabilities.len(), 8);

    let error = api.form(uuid::Uuid::now_v7(), None).await.unwrap_err();
    assert_eq!(error.code(), "instance.runtime_not_found", "{error:?}");
    let error = api
        .form(site.runtime_id, Some("testing/nothing"))
        .await
        .unwrap_err();
    assert_eq!(error.code(), "instance.provider_not_found", "{error:?}");
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
async fn a_daemon_without_a_catalog_answers_the_form_with_catalog_unavailable() {
    let ev = evidence!(
        covers = ["REQ-029"],
        kind = Kind::Fixture,
        expected = "get_create_form on a daemon without a catalog answers catalog.unavailable with its reason, and preflight shows the provider row as an error",
    );
    let site = Site::new();
    let daemon = site.start().await;
    let api = site.api().await;

    let error = api.form(site.runtime_id, None).await.unwrap_err();
    assert_eq!(error.code(), "catalog.unavailable", "{error:?}");
    assert_eq!(error.detail("reason"), Some("not_configured"));

    // A draft that names a pinned revision does not need the catalog.
    let pinned = api
        .preflight(api.preflight_params("pinned", json!({})))
        .await
        .expect("a preflight is answered");
    assert!(pinned.ready, "{pinned:?}");
    let mut absent = api.preflight_params("absent", json!({}));
    absent.provider.revision = None;
    let preflight = api
        .preflight(absent)
        .await
        .expect("a preflight is answered");
    assert!(!preflight.ready);
    assert!(preflight.provider.is_none());
    let row = preflight
        .checks
        .iter()
        .find(|check| check.id == "provider")
        .expect("the provider row is there");
    assert_eq!(row.status, PreflightStatus::Error);
    assert_eq!(
        row.error.as_ref().map(|error| error.code()),
        Some("catalog.unavailable")
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
async fn a_preflight_judges_a_draft_ready_and_writes_nothing() {
    let ev = evidence!(
        covers = ["REQ-008", "AC-056"],
        kind = Kind::Fixture,
        expected = "a preflight of a good draft is ready with a row for the runtime, the name, the endpoint, the data folder, the disk, and the secret, carries the equivalent command, and leaves no instance row, no folder, and no secret behind",
    );
    let site = Site::new();
    let daemon = site.start_with(fast_with_catalog()).await;
    let api = site.api().await;
    let before = site.secrets.calls().len();

    let preflight = api
        .preflight(api.preflight_params("demo", json!({})))
        .await
        .expect("a preflight is answered");

    assert!(preflight.ready, "{preflight:?}");
    let ids: Vec<&str> = preflight.checks.iter().map(|c| c.id.as_str()).collect();
    for expected in [
        "runtime",
        "name",
        "endpoint.http",
        "data",
        "disk",
        "secret.authToken",
    ] {
        assert!(ids.contains(&expected), "{expected} is missing: {ids:?}");
    }
    assert!(
        preflight.checks.iter().all(|check| check.error.is_none()),
        "{preflight:?}"
    );
    assert!(
        preflight
            .command
            .starts_with("stackly instance create demo --runtime "),
        "{}",
        preflight.command
    );
    assert!(preflight.command.contains("--secret authToken=prompt"));
    assert!(!preflight.command.contains(crate::support::TOKEN));
    let provider = preflight.provider.expect("the package is known");
    assert_eq!(provider.name, "testing/fake");

    assert!(api.list(Default::default()).await.items.is_empty());
    let created = std::fs::read_dir(site.prepared.layout().instances_dir())
        .map(|entries| entries.count())
        .unwrap_or(0);
    assert_eq!(created, 0, "no instance folder");
    assert_eq!(
        site.secrets.calls().len(),
        before,
        "the keychain was not touched"
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
async fn a_preflight_with_start_ends_the_equivalent_command_with_start() {
    let ev = evidence!(
        covers = ["REQ-008"],
        kind = Kind::Fixture,
        expected = "a preflight that asks for start carries an equivalent command ending in --start, and the same draft without start does not",
    );
    let site = Site::new();
    let daemon = site.start_with(fast_with_catalog()).await;
    let api = site.api().await;

    let plain = api
        .preflight(api.preflight_params("demo", json!({})))
        .await
        .expect("a preflight is answered");
    let mut params = api.preflight_params("demo", json!({}));
    params.start = true;
    let started = api
        .preflight(params)
        .await
        .expect("a preflight with start is answered");

    assert!(!plain.command.contains("--start"), "{}", plain.command);
    assert!(started.command.ends_with(" --start"), "{}", started.command);
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
async fn preflight_and_create_give_a_bad_draft_the_same_code_and_the_same_fields() {
    let ev = evidence!(
        covers = ["AC-028"],
        kind = Kind::Fixture,
        expected = "for a bad name, a taken port, an unknown setting, an unknown endpoint, and a secret field the schema does not know, the first error row of the preflight has the code create answers with, and create carries the same field issues exactly when that code lists omitted and no items",
    );
    let site = Site::new();
    let daemon = site.start_with(fast_with_catalog()).await;
    let api = site.api().await;
    let held = TcpListener::bind(("127.0.0.1", 0)).expect("a free port binds");
    let port = held.local_addr().expect("an address").port();

    let mut cases: Vec<(&str, stackly_protocol::instance::PreflightCreateParams)> = Vec::new();
    cases.push(("Bad_Name", api.preflight_params("Bad_Name", json!({}))));
    let mut taken = api.preflight_params("held", json!({}));
    taken.endpoints = vec![Api::http_port(port)];
    cases.push(("port", taken));
    cases.push((
        "setting",
        api.preflight_params("setting", json!({ "nosuch": 1, "readyDelayMs": "abc" })),
    ));
    let mut endpoint = api.preflight_params("endpoint", json!({}));
    endpoint.endpoints = vec![stackly_protocol::instance::EndpointRequest {
        name: "nosuch".to_owned(),
        port: stackly_protocol::instance::PortRequest::Auto,
    }];
    cases.push(("endpoint", endpoint));
    let mut secret = api.preflight_params("secret", json!({}));
    secret.secrets[0].field = "nosuch".to_owned();
    cases.push(("secret", secret));

    for (label, preflight_params) in cases {
        let preflight = api
            .preflight(preflight_params.clone())
            .await
            .expect("a preflight is answered");
        assert!(!preflight.ready, "{label}: {preflight:?}");
        let first = preflight
            .checks
            .iter()
            .find(|check| check.status == PreflightStatus::Error)
            .and_then(|check| check.error.as_ref())
            .unwrap_or_else(|| panic!("{label}: no error row"));

        let create = stackly_protocol::instance::CreateInstanceParams {
            name: preflight_params.name.clone(),
            display_name: preflight_params.display_name.clone(),
            provider: preflight_params.provider.clone(),
            runtime_id: preflight_params.runtime_id,
            endpoints: preflight_params.endpoints.clone(),
            settings: preflight_params.settings.clone(),
            secrets: vec![stackly_protocol::instance::SecretFieldInput {
                field: preflight_params.secrets[0].field.clone(),
                input: stackly_protocol::secret::SecretInput::Generate,
            }],
        };
        let refused = api.create(create).await.unwrap_err();
        assert_eq!(refused.code(), first.code(), "{label}");
        let carries = !first.fields().is_empty();
        if carries {
            assert_eq!(refused.fields(), first.fields(), "{label}");
            assert_eq!(refused.fields(), preflight.fields.as_slice(), "{label}");
        } else {
            assert!(
                refused.fields().is_empty() || refused.fields() == preflight.fields.as_slice(),
                "{label}: {refused:?}"
            );
        }
    }
    assert!(api.list(Default::default()).await.items.is_empty());
    drop((api, held));
    daemon
        .shutdown(
            stackly_protocol::events::ShutdownReason::Requested,
            std::future::pending(),
        )
        .await;
    ev.pass();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_name_that_breaks_the_rules_is_refused_before_any_path_exists() {
    let ev = evidence!(
        covers = ["VAL-001", "NFR-009"],
        kind = Kind::Fixture,
        expected = "a name with a path, a space, a shell character, a capital, a leading digit, or Unicode is refused with instance.name_invalid at the field name, and no instance folder or row exists afterwards",
    );
    let site = Site::new();
    let daemon = site.start_with(fast_with_catalog()).await;
    let api = site.api().await;
    for name in [
        "../x",
        "a b",
        "a;rm",
        "Orders",
        "1abc",
        "caf\u{e9}",
        "a--b",
        &"x".repeat(64),
    ] {
        let error = api.create(api.params(name, json!({}))).await.unwrap_err();
        assert_eq!(error.code(), "instance.name_invalid", "{name:?}: {error:?}");
        assert_eq!(error.fields()[0].path(), "name", "{name:?}");
    }
    assert!(api.list(Default::default()).await.items.is_empty());
    let folders = std::fs::read_dir(site.prepared.layout().instances_dir())
        .map(|entries| entries.count())
        .unwrap_or(0);
    assert_eq!(folders, 0);
    drop(api);
    daemon
        .shutdown(
            stackly_protocol::events::ShutdownReason::Requested,
            std::future::pending(),
        )
        .await;
    ev.pass();
}
