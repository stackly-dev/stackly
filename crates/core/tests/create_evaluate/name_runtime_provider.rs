//! The `provider`, `runtime`, and `name` rows (spec 0020 AC-2, AC-3, AC-15).

use std::sync::Arc;

use stackly_core::catalog::{CatalogState, UnavailableReason};
use stackly_core::instance::create::{CheckStatus, FieldCode, ProviderChoice};
use stackly_domain::{InstanceLifecycle, OperationId, RuntimeState};
use stackly_evidence::{Kind, evidence};
use stackly_platform::Clock;
use stackly_testkit::RuntimeBuilder;

use crate::common::{Site, commit_runtime, error_codes, has_row, paths, row};

#[test]
fn a_valid_draft_is_ready_and_resolves_to_what_the_operation_needs() {
    let ev = evidence!(
        covers = ["REQ-008"],
        kind = Kind::Fixture,
        expected = "a valid draft on the fake provider is ready, shows no provider or settings row, and resolves to the exact package, runtime, name, one endpoint, and one generated secret plan",
    );
    let site = Site::new();

    let evaluation = site.evaluate(&site.draft("orders"));

    assert!(evaluation.ready(), "{:#?}", evaluation.checks);
    assert!(evaluation.field_issues.is_empty());
    assert!(!has_row(&evaluation, "provider"));
    assert!(!has_row(&evaluation, "settings"));
    for id in [
        "runtime",
        "name",
        "endpoint.http",
        "data",
        "disk",
        "secret.authToken",
    ] {
        assert_eq!(row(&evaluation, id).status, CheckStatus::Ok, "{id}");
    }
    let resolved = evaluation.resolved.expect("a ready draft resolves");
    assert_eq!(resolved.name.as_str(), "orders");
    assert_eq!(resolved.package.key(), site.package.key());
    assert_eq!(resolved.runtime.id(), site.runtime.id());
    assert_eq!(resolved.endpoints.len(), 1);
    assert_eq!(resolved.secrets.len(), 1);
    assert_eq!(resolved.secrets[0].field.as_str(), "authToken");
    ev.pass();
}

#[test]
fn a_bad_name_or_display_name_is_name_invalid_with_the_field_path() {
    let ev = evidence!(
        covers = ["VAL-001"],
        kind = Kind::Fixture,
        expected = "names that break InstanceName (uppercase, underscore, a leading digit, 64 characters, a path, a shell string, Unicode) and a display name over 128 characters or with a control character make instance.name_invalid with the path name or displayName",
    );
    let site = Site::new();
    let long = "a".repeat(64);
    for bad in [
        "Bad_Name",
        "1abc",
        long.as_str(),
        "../x",
        "a b",
        "a;rm",
        "名前",
        "",
    ] {
        let evaluation = site.evaluate(&site.draft(bad));

        assert_eq!(
            error_codes(&evaluation)[0],
            "instance.name_invalid",
            "{bad:?}"
        );
        assert_eq!(paths(&evaluation), [("name", "invalid_format")], "{bad:?}");
        assert!(evaluation.resolved.is_none());
    }
    let mut draft = site.draft("orders");
    draft.display_name = Some("two\nlines".to_owned());
    let evaluation = site.evaluate(&draft);
    assert_eq!(error_codes(&evaluation), ["instance.name_invalid"]);
    assert_eq!(paths(&evaluation), [("displayName", "invalid_format")]);
    draft.display_name = Some("x".repeat(129));
    assert_eq!(
        paths(&site.evaluate(&draft)),
        [("displayName", "invalid_format")]
    );
    draft.display_name = Some("x".repeat(128));
    assert!(site.evaluate(&draft).ready());
    ev.pass();
}

#[test]
fn a_name_an_instance_that_is_not_retained_holds_is_taken_but_a_retained_name_is_free() {
    let site = Site::new();
    let existing = site.add_instance(|builder| builder.name("orders"));

    let taken = site.evaluate(&site.draft("orders"));

    assert_eq!(error_codes(&taken), ["instance.name_taken"]);
    assert_eq!(paths(&taken), [("name", "invalid_value")]);
    let error = taken.first_error().expect("an error row");
    assert_eq!(
        *error,
        stackly_core::instance::create::CreateError::NameTaken {
            existing: existing.id()
        }
    );

    let retained = site.add_instance(|builder| {
        builder
            .name("archive")
            .lifecycle(InstanceLifecycle::Retained)
            .retained(OperationId::generate(), site.fake.clock().now())
    });
    assert!(retained.name().as_str() == "archive");
    assert!(site.evaluate(&site.draft("archive")).ready());
}

#[test]
fn a_bad_name_and_a_used_port_are_both_reported_with_the_name_row_first() {
    let site = Site::new();
    site.add_instance(|builder| builder.name("taken"));
    let mut draft = site.draft("Bad Name");
    draft.endpoints = vec![crate::common::fixed("http", 5432)];

    let evaluation = site.evaluate(&draft);

    assert_eq!(
        error_codes(&evaluation),
        ["instance.name_invalid", "supervisor.endpoint_conflict"]
    );
    assert_eq!(
        evaluation.first_error().map(|error| error.code()),
        Some("instance.name_invalid")
    );
    assert!(!evaluation.ready());
}

#[test]
fn provider_problems_are_one_error_row_and_hide_the_rows_that_depend_on_the_package() {
    let ev = evidence!(
        covers = ["REQ-008"],
        kind = Kind::Fixture,
        expected = "an unknown, ambiguous, planned, or unavailable provider, and a revision with no pinned package, give one provider error row with the code of the resolution table, and no endpoint, settings, or secret row",
    );
    let site = Site::new();
    let mut draft = site.draft("orders");
    draft.provider = ProviderChoice {
        name: "official/missing".to_owned(),
        revision: None,
    };

    let evaluation = site.evaluate(&draft);

    assert_eq!(error_codes(&evaluation), ["instance.provider_not_found"]);
    assert_eq!(paths(&evaluation), [("provider.name", "unknown_value")]);
    for id in ["endpoint.http", "settings", "secret.authToken"] {
        assert!(!has_row(&evaluation, id), "{id}");
    }
    assert_eq!(row(&evaluation, "runtime").status, CheckStatus::Ok);

    draft.provider = ProviderChoice {
        name: site.package.key().provider().to_string(),
        revision: Some("2099.1".to_owned()),
    };
    let evaluation = site.evaluate(&draft);
    assert_eq!(error_codes(&evaluation), ["instance.provider_not_found"]);
    assert_eq!(paths(&evaluation), [("provider.revision", "unknown_value")]);

    let mut facts = site.facts(&site.draft("orders"));
    facts.provider =
        Err(stackly_core::instance::create::ProviderProblem::Unavailable { state: "planned" });
    let evaluation = stackly_core::instance::create::evaluate(&site.draft("orders"), &facts);
    assert_eq!(error_codes(&evaluation), ["instance.provider_unavailable"]);
    assert_eq!(paths(&evaluation), [("provider.name", "invalid_value")]);

    facts.provider = Err(stackly_core::instance::create::ProviderProblem::Ambiguous);
    let evaluation = stackly_core::instance::create::evaluate(&site.draft("orders"), &facts);
    assert_eq!(error_codes(&evaluation), ["instance.provider_ambiguous"]);
    ev.pass();
}

#[test]
fn a_catalog_that_is_not_loaded_is_catalog_unavailable_without_a_field_issue() {
    let mut site = Site::new();
    site.catalog = CatalogState::NotConfigured;

    let evaluation = site.evaluate(&site.draft("orders"));

    assert_eq!(error_codes(&evaluation), ["catalog.unavailable"]);
    assert!(evaluation.field_issues.is_empty());

    site.catalog = CatalogState::Unavailable {
        reason: UnavailableReason::IndexInvalid,
        detail: "bad".to_owned(),
    };
    let evaluation = site.evaluate(&site.draft("orders"));
    assert_eq!(
        evaluation.first_error().map(|e| e.code()),
        Some("catalog.unavailable")
    );
}

#[test]
fn a_package_without_multi_instance_allows_one_instance_only() {
    let ev = evidence!(
        covers = ["BR-016"],
        kind = Kind::Fixture,
        expected = "a provider without the multi_instance capability, with an instance of it that is not retained, makes instance.capability_unsupported with the existing instance",
    );
    let site = Site::with_provider_edit("fake", |dir| {
        let manifest = dir.join("provider.yaml");
        let text = std::fs::read_to_string(&manifest).expect("the manifest reads");
        std::fs::write(
            &manifest,
            text.replace("multiInstance: true", "multiInstance: false"),
        )
        .expect("the manifest writes");
    });
    let existing = site.add_instance(|builder| builder.name("first"));

    let evaluation = site.evaluate(&site.draft("second"));

    assert_eq!(
        error_codes(&evaluation)[0],
        "instance.capability_unsupported"
    );
    assert_eq!(
        evaluation.first_error().cloned(),
        Some(
            stackly_core::instance::create::CreateError::CapabilityUnsupported {
                provider: site.package.key().provider().to_string(),
                existing: existing.id(),
            }
        )
    );
    assert_eq!(paths(&evaluation), [("provider.name", "invalid_value")]);
    ev.pass();
}

#[test]
fn runtime_problems_are_the_runtime_row_with_the_runtime_id_path() {
    let ev = evidence!(
        covers = ["REQ-008"],
        kind = Kind::Fixture,
        expected = "a missing runtime, one of other software, one that is not installed, and one without the programs of the package each make the runtime row an error with its own code and the path runtimeId",
    );
    let site = Site::new();

    let mut draft = site.draft("orders");
    draft.runtime_id = crate::common::unused_runtime();
    let evaluation = site.evaluate(&draft);
    assert_eq!(error_codes(&evaluation), ["instance.runtime_not_found"]);
    assert_eq!(paths(&evaluation), [("runtimeId", "unknown_value")]);

    // Another software.
    let other = RuntimeBuilder::new()
        .software("postgresql")
        .version("17.6", "17")
        .state(RuntimeState::Installed)
        .build();
    let installed = commit_runtime(&site, other);
    let mut draft = site.draft("orders");
    draft.runtime_id = installed;
    let evaluation = site.evaluate(&draft);
    assert_eq!(error_codes(&evaluation), ["instance.runtime_mismatch"]);
    assert_eq!(paths(&evaluation), [("runtimeId", "invalid_value")]);

    // Not installed.
    let broken = RuntimeBuilder::new()
        .software("fake")
        .version("0.2.0", "0.2")
        .build();
    let id = commit_runtime(&site, broken);
    let mut draft = site.draft("orders");
    draft.runtime_id = id;
    let evaluation = site.evaluate(&draft);
    assert_eq!(error_codes(&evaluation), ["supervisor.runtime_unavailable"]);
    assert_eq!(paths(&evaluation), [("runtimeId", "invalid_value")]);
    ev.pass();
}

#[test]
fn a_runtime_without_a_program_of_the_package_is_incompatible() {
    let site = Site::new();
    let layout = site.prepared.layout();
    let stackly_domain::InstallMode::Managed { artifact_sha256 } = site.runtime.install() else {
        panic!("a managed runtime");
    };
    let dir = layout.runtime_dir(
        site.runtime.software(),
        site.runtime.version(),
        site.runtime.target(),
        artifact_sha256,
    );
    std::fs::remove_file(dir.join(stackly_core::supervisor::fixture::FAKE_EXECUTABLE))
        .expect("the program can be removed");

    let evaluation = site.evaluate(&site.draft("orders"));

    assert_eq!(error_codes(&evaluation), ["instance.runtime_incompatible"]);
    assert_eq!(paths(&evaluation), [("runtimeId", "invalid_value")]);
    let _ = Arc::strong_count(&site.package);
    let _ = FieldCode::InvalidValue;
}
