//! `build_duplicate` over the real packages (spec 0024 AC-2 to AC-5): the
//! settings that are copied, the name, the ports, and the secrets.

use stackly_core::instance::create::{PackageSource, PortRequestDraft, SecretSourceKind};
use stackly_core::instance::duplicate::{DuplicateModel, build_duplicate};
use stackly_domain::{Instance, InstanceLifecycle};
use stackly_evidence::{Kind, evidence};

use crate::common::Site;
use crate::support::{add_source, facts, pin_package, set_config, use_secret};

/// A site for the provider `name` with a stopped source called `source`.
fn site_for(name: &str, source: &str) -> (Site, Instance) {
    let site = Site::with_provider(name);
    pin_package(&site, name, |_| {});
    let instance = add_source(&site, source);
    (site, instance)
}

fn duplicate(site: &Site, source: &Instance) -> DuplicateModel {
    build_duplicate(&facts(site, source))
}

#[test]
fn the_fake_settings_keep_every_plain_field_and_drop_the_secret_and_foreign_keys() {
    let ev = evidence!(
        covers = ["REQ-025", "AC-025"],
        kind = Kind::Fixture,
        expected = "draft.settings is the active values restricted to the schema fields that are not secret and not managed, as they are: defaults and immutableAfterInit fields stay, authToken and keys outside the schema go",
    );
    let (site, source) = site_for("fake", "fake-a");
    set_config(
        &site,
        &source,
        &serde_json::json!({
            "greeting": "hello from fake-a",
            "crashAfterMs": 0,
            "label": "fake",
            "failInit": false,
            "authToken": "local:instance/x/authtoken",
            "unknownKey": 1,
        }),
    );

    let model = duplicate(&site, &source);

    assert_eq!(
        model.draft.settings,
        serde_json::json!({
            "greeting": "hello from fake-a",
            "crashAfterMs": 0,
            "label": "fake",
            "failInit": false,
        })
    );
    ev.pass();
}

#[test]
fn the_postgresql_settings_drop_the_superuser_password_reference() {
    let (site, source) = site_for("postgresql", "pg17-dev");
    set_config(
        &site,
        &source,
        &serde_json::json!({
            "superuserPassword": "local:instance/x/superuserpassword",
            "maxConnections": 200,
            "logStatement": "ddl",
            "sharedBuffersMb": 128,
        }),
    );

    let model = duplicate(&site, &source);

    assert_eq!(
        model.draft.settings,
        serde_json::json!({ "maxConnections": 200, "logStatement": "ddl", "sharedBuffersMb": 128 })
    );
}

#[test]
fn the_nats_settings_drop_the_managed_server_name() {
    let (site, source) = site_for("nats", "queue-a");
    set_config(
        &site,
        &source,
        &serde_json::json!({ "serverName": "queue-a", "jetstream": false }),
    );

    let model = duplicate(&site, &source);

    assert_eq!(
        model.draft.settings,
        serde_json::json!({ "jetstream": false })
    );
}

#[test]
fn a_source_without_a_config_revision_gives_empty_settings() {
    let (site, source) = site_for("fake", "fake-a");

    assert_eq!(
        duplicate(&site, &source).draft.settings,
        serde_json::json!({})
    );
}

#[test]
fn every_ip_endpoint_asks_for_auto_whatever_the_source_asked() {
    let ev = evidence!(
        covers = ["BR-005", "REQ-025"],
        kind = Kind::Fixture,
        expected = "draft.endpoints holds every tcp and udp endpoint of the manifest in form order as auto, never a fixed port, and no unix endpoint",
    );
    // The default source of the builder asks for the fixed port 5432.
    let (site, source) = site_for("postgresql", "pg17-dev");
    let model = duplicate(&site, &source);
    assert_eq!(
        model.draft.endpoints,
        [("client".to_owned(), PortRequestDraft::Auto)]
    );

    let (site, source) = site_for("nats", "queue-a");
    let model = duplicate(&site, &source);
    assert_eq!(
        model.draft.endpoints,
        [
            ("client".to_owned(), PortRequestDraft::Auto),
            ("monitoring".to_owned(), PortRequestDraft::Auto)
        ]
    );
    assert!(
        model
            .draft
            .endpoints
            .iter()
            .all(|(_, request)| *request == PortRequestDraft::Auto)
    );
    ev.pass();
}

#[test]
fn every_secret_field_is_generate_and_names_the_reference_the_source_uses() {
    let ev = evidence!(
        covers = ["REQ-025", "AC-025"],
        kind = Kind::Fixture,
        expected = "draft.secrets holds generate for each secret field in form order, never copyFrom or bind, and secret_sources lists the reference the source uses for the field, or none",
    );
    let (site, source) = site_for("fake", "fake-a");
    let reference = use_secret(&site, &source, "authToken");

    let model = duplicate(&site, &source);

    assert_eq!(
        model.draft.secrets,
        [("authToken".to_owned(), SecretSourceKind::Generate)]
    );
    assert_eq!(model.secret_sources.len(), 1);
    assert_eq!(model.secret_sources[0].0.as_str(), "authToken");
    assert_eq!(model.secret_sources[0].1.as_ref(), Some(&reference));

    // A source that uses no reference still offers the field, with none.
    let (site, source) = site_for("postgresql", "pg17-dev");
    let model = duplicate(&site, &source);
    assert_eq!(
        model.draft.secrets,
        [("superuserPassword".to_owned(), SecretSourceKind::Generate)]
    );
    assert_eq!(model.secret_sources[0].1, None);

    // A package without a secret field has none.
    let (site, source) = site_for("nats", "queue-a");
    let model = duplicate(&site, &source);
    assert!(model.draft.secrets.is_empty());
    assert!(model.secret_sources.is_empty());
    ev.pass();
}

#[test]
fn the_draft_keeps_the_runtime_and_provider_revision_of_the_source() {
    let ev = evidence!(
        covers = ["REQ-009", "REQ-025"],
        kind = Kind::Fixture,
        expected = "the draft and the form name the runtime ID and the provider revision of the source, the form is built from the pinned package, the display name is empty, and start is false",
    );
    let (site, source) = site_for("fake", "fake-a");

    let model = duplicate(&site, &source);

    let key = source.provider_revision();
    assert_eq!(model.source, (source.id(), source.name().clone()));
    assert_eq!(model.draft.runtime_id, source.runtime_id());
    assert_eq!(model.draft.provider.name, key.provider().to_string());
    assert_eq!(
        model.draft.provider.revision.as_deref(),
        Some(key.revision().to_string().as_str())
    );
    assert_eq!(model.draft.display_name, None);
    assert!(!model.draft.start);
    assert!(model.draft.secret_values.is_empty());
    assert_eq!(model.form.provider.source, PackageSource::Pinned);
    assert_eq!(&model.form.provider.name, key.provider());
    assert_eq!(&model.form.provider.revision, key.revision());
    assert_eq!(&model.form.provider.manifest_sha256, key.manifest_sha256());
    assert_eq!(model.form.runtime.id(), source.runtime_id());
    ev.pass();
}

#[test]
fn the_form_and_the_draft_share_the_duplicate_name() {
    let (site, source) = site_for("fake", "fake-a");
    let model = duplicate(&site, &source);
    assert_eq!(model.draft.name, "fake-a-copy");
    assert_eq!(
        model.form.suggested_name.as_ref().map(|name| name.as_str()),
        Some("fake-a-copy")
    );

    // The name is taken by an instance that is not retained.
    add_source(&site, "fake-a-copy");
    let model = duplicate(&site, &source);
    assert_eq!(model.draft.name, "fake-a-copy-2");
    assert_eq!(
        model.form.suggested_name.as_ref().map(|name| name.as_str()),
        Some("fake-a-copy-2")
    );
}

#[test]
fn a_retained_instance_does_not_hold_its_name() {
    let (site, source) = site_for("fake", "fake-a");
    site.add_instance(|builder| {
        builder.name("fake-a-copy").retained(
            stackly_domain::OperationId::generate(),
            stackly_platform::Clock::now(site.fake.clock()),
        )
    });

    assert_eq!(duplicate(&site, &source).draft.name, "fake-a-copy");
}

#[test]
fn no_name_is_left_when_all_ninety_nine_are_taken() {
    let (site, source) = site_for("fake", "fake-a");
    add_source(&site, "fake-a-copy");
    for n in 2..=99 {
        add_source(&site, &format!("fake-a-copy-{n}"));
    }

    let model = duplicate(&site, &source);

    assert_eq!(model.draft.name, "");
    assert_eq!(model.form.suggested_name, None);
}

#[test]
fn a_running_source_is_duplicated_like_a_stopped_one() {
    let site = Site::new();
    pin_package(&site, "fake", |_| {});
    let source =
        site.add_instance(|builder| builder.name("fake-a").lifecycle(InstanceLifecycle::Running));

    assert_eq!(duplicate(&site, &source).draft.name, "fake-a-copy");
}
