//! The create form model and the equivalent command (spec 0020 AC-1, *Nama*,
//! *Evaluator create*).

use stackly_core::catalog::{CatalogEntry, CatalogState};
use stackly_core::instance::create::{
    FormError, PortRequestDraft, build_form, names_of, select_provider, suggested_name,
};
use stackly_domain::{InstanceName, RuntimeState};
use stackly_evidence::{Kind, evidence};
use stackly_testkit::RuntimeBuilder;

use crate::common::{Site, fixed, source};

fn name(text: &str) -> InstanceName {
    text.parse().expect("a valid name")
}

#[test]
fn the_suggested_name_is_software_and_line_with_a_suffix_when_taken() {
    let ev = evidence!(
        covers = ["REQ-008"],
        kind = Kind::Fixture,
        expected = "suggestedName is <software>-<line> with hyphens for dots, -2 to -99 when taken, none when all are taken, and the software part is cut so the name fits 63 characters",
    );
    assert_eq!(
        suggested_name("postgresql", "18", &[]),
        Some(name("postgresql-18"))
    );
    assert_eq!(suggested_name("mysql", "8.4", &[]), Some(name("mysql-8-4")));
    assert_eq!(
        suggested_name("postgresql", "18", &[name("postgresql-18")]),
        Some(name("postgresql-18-2"))
    );
    let mut taken = vec![name("postgresql-18")];
    taken.extend((2..=99).map(|n| name(&format!("postgresql-18-{n}"))));
    assert_eq!(suggested_name("postgresql", "18", &taken), None);
    let long = "a".repeat(80);
    let suggested = suggested_name(&long, "18", &[]).expect("cut to fit");
    assert_eq!(suggested.as_str().len(), 63);
    assert!(suggested.as_str().ends_with("-18"));
    let second = suggested_name(&long, "18", std::slice::from_ref(&suggested)).expect("cut to fit");
    assert_eq!(second.as_str().len(), 63);
    assert!(second.as_str().ends_with("-18-2"));
    ev.pass();
}

#[test]
fn the_form_lists_endpoints_by_purpose_and_fields_in_declaration_order() {
    let ev = evidence!(
        covers = ["VAL-012"],
        kind = Kind::Fixture,
        expected = "build_form lists the endpoints of nats as client then monitoring, the settings fields in the order the schema declares them, the provider and its source, the runtime, the data path text, and the capabilities",
    );
    let site = Site::with_provider("nats");
    let CatalogState::Loaded(catalog) = &site.catalog else {
        panic!("a loaded catalog");
    };
    let (entry, meta) = select_provider(&site.catalog, &site.runtime, None).expect("one entry");
    let taken = names_of(&[]);

    let form = build_form(entry, &site.runtime, &taken, meta);

    assert_eq!(form.provider.name.to_string(), "official/nats");
    assert_eq!(
        form.provider.source,
        stackly_core::instance::create::PackageSource::DevelopmentCatalog
    );
    assert_eq!(form.data_path, "instances/<new instance id>/data");
    let endpoints: Vec<_> = form.endpoints.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(endpoints, ["client", "monitoring"]);
    assert_eq!(form.endpoints[0].default_port, Some(4222));
    assert_eq!(form.endpoints[1].default_port, Some(8222));
    let fields: Vec<_> = form.fields.iter().map(|f| f.name().to_string()).collect();
    assert_eq!(
        fields,
        entry
            .package
            .settings_schema()
            .fields()
            .map(|f| f.name().to_string())
            .collect::<Vec<_>>()
    );
    assert_eq!(fields[0], "serverName", "declaration order, not name order");
    assert_eq!(
        form.suggested_name,
        Some(name(&format!(
            "nats-{}",
            site.runtime.line().as_str().replace('.', "-")
        )))
    );
    let _ = (catalog, &form.capabilities);
    ev.pass();
}

#[test]
fn a_socket_endpoint_follows_the_ip_endpoints_with_its_template() {
    let site = Site::with_provider("postgresql");
    let (entry, meta) = select_provider(&site.catalog, &site.runtime, None).expect("one entry");

    let form = build_form(entry, &site.runtime, &[], meta);

    let endpoints: Vec<_> = form.endpoints.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(endpoints, ["client", "socket"]);
    assert_eq!(
        form.endpoints[1].socket_name.as_deref(),
        Some(".s.PGSQL.${endpoint.client.port}")
    );
    assert_eq!(form.endpoints[1].default_port, None);
}

#[test]
fn the_provider_is_picked_from_the_software_of_the_runtime_with_the_errors_of_ac_1() {
    let ev = evidence!(
        covers = ["REQ-029"],
        kind = Kind::Fixture,
        expected = "select_provider reports an unloaded catalog, no entry for the software, a named provider of other software, a planned or missing name, and a runtime that is not installed, each as its own FormError",
    );
    let site = Site::new();
    assert!(select_provider(&site.catalog, &site.runtime, None).is_ok());
    assert!(select_provider(&site.catalog, &site.runtime, Some("testing/fake")).is_ok());
    assert_eq!(
        select_provider(&site.catalog, &site.runtime, Some("testing/other")).unwrap_err(),
        FormError::ProviderNotFound
    );
    assert_eq!(
        select_provider(&site.catalog, &site.runtime, Some("not a name")).unwrap_err(),
        FormError::ProviderNotFound
    );
    assert_eq!(
        select_provider(&CatalogState::NotConfigured, &site.runtime, None).unwrap_err(),
        FormError::CatalogUnavailable {
            reason: "not_configured"
        }
    );
    let other = RuntimeBuilder::new()
        .software("postgresql")
        .state(RuntimeState::Installed)
        .build();
    assert_eq!(
        select_provider(&site.catalog, &other, None).unwrap_err(),
        FormError::ProviderNotFound,
        "no entry runs that software"
    );
    assert_eq!(
        select_provider(&site.catalog, &other, Some("testing/fake")).unwrap_err(),
        FormError::RuntimeMismatch
    );
    let broken = RuntimeBuilder::new().software("fake").build();
    assert_eq!(
        select_provider(&site.catalog, &broken, None).unwrap_err(),
        FormError::RuntimeUnavailable { state: "resolving" }
    );
    let CatalogState::Loaded(catalog) = &site.catalog else {
        panic!("loaded");
    };
    assert!(matches!(catalog.entries()[0], CatalogEntry::Provider(_)));
    ev.pass();
}

#[test]
fn the_command_spec_keeps_only_settings_that_differ_from_the_default() {
    let site = Site::new();
    let mut draft = site.draft("orders");
    draft.display_name = Some("Orders".to_owned());
    draft.endpoints = vec![fixed("http", 20000)];
    draft.settings = serde_json::json!({ "greeting": "hello from stackly", "label": "custom", "readyDelayMs": 0 });
    draft.secrets = vec![source(
        "authToken",
        stackly_core::instance::create::SecretSourceKind::Generate,
    )];
    draft.start = true;

    let command = site.evaluate(&draft).command;

    assert_eq!(command.name, "orders");
    assert_eq!(command.display_name.as_deref(), Some("Orders"));
    assert_eq!(
        command.ports,
        [("http".to_owned(), PortRequestDraft::Fixed(20000))]
    );
    assert_eq!(
        command.settings,
        [("label".to_owned(), serde_json::json!("custom"))],
        "the defaults of greeting and readyDelayMs are left out"
    );
    assert_eq!(command.secrets.len(), 1);
    assert!(command.start);
    assert_eq!(command.provider.as_deref(), Some("testing/fake"));
    assert_eq!(command.revision, None);
}

#[test]
fn the_runtime_label_is_software_at_version_when_one_installed_runtime_matches() {
    let site = Site::new();

    let command = site.evaluate(&site.draft("orders")).command;

    assert_eq!(
        command.runtime_label,
        format!("fake@{}", site.runtime.version())
    );

    let mut draft = site.draft("orders");
    draft.runtime_id = crate::common::unused_runtime();
    let command = site.evaluate(&draft).command;
    assert_eq!(command.runtime_label, draft.runtime_id.to_string());
}
