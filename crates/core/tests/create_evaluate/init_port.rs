//! The dry run of `init_plan` without a port for `Auto` endpoints (spec 0020
//! AC-2 and AC-9, update 2026-10-22; spec 0009 AC-21).

use std::path::Path;

use stackly_core::instance::create::{CheckStatus, CreateError};
use stackly_evidence::{Kind, evidence};

use crate::common::{Site, auto, error_codes, fixed, paths, row};

const NEEDS_HTTP: &str =
    "This provider needs a fixed port for http when the instance is created. Choose a port for it.";

/// The fake package with `--port=${endpoint.http.port}` added to the args of
/// its `runExecutable` initialize step, which the real package never does.
fn fake_init_uses_the_port(dir: &Path) {
    let manifest = dir.join("provider.yaml");
    let text = std::fs::read_to_string(&manifest).expect("the manifest reads");
    let edited = text.replacen(
        "- --token-stdin",
        "- --token-stdin\n        - \"--port=${endpoint.http.port}\"",
        1,
    );
    assert_ne!(edited, text, "the manifest changed");
    std::fs::write(&manifest, edited).expect("the manifest writes");
}

/// The NATS package in its `1.0.0` shape, which renders `nats.conf` at
/// initialize, with the addresses of both endpoints.
fn nats_renders_its_config_at_initialize(dir: &Path) {
    let manifest = dir.join("provider.yaml");
    let text = std::fs::read_to_string(&manifest).expect("the manifest reads");
    let edited = text.replacen(
        "- { primitive: createManagedDirectories }",
        "- { primitive: createManagedDirectories }\n    - { primitive: renderConfig, file: natsConf }",
        1,
    );
    assert_ne!(edited, text, "the manifest changed");
    std::fs::write(&manifest, edited).expect("the manifest writes");
}

#[test]
fn an_auto_endpoint_that_initialize_needs_is_endpoint_invalid_with_the_fixed_port_detail() {
    let ev = evidence!(
        covers = ["VAL-006"],
        kind = Kind::Fixture,
        expected = "a draft without a port request for an endpoint the init plan uses gets the settings row error instance.endpoint_invalid naming that endpoint, with the fixed port detail and no field issue; an explicit auto adds endpoints[0].port invalid_value",
    );
    let site = Site::with_provider_edit("fake", fake_init_uses_the_port);

    let silent = site.evaluate(&site.draft("orders"));
    assert!(!silent.ready());
    assert_eq!(error_codes(&silent), ["instance.endpoint_invalid"]);
    let settings = row(&silent, "settings");
    assert_eq!(settings.status, CheckStatus::Error);
    assert_eq!(settings.detail, NEEDS_HTTP);
    assert!(paths(&silent).is_empty(), "{:?}", paths(&silent));
    let Some(CreateError::EndpointInvalid { endpoint }) = silent.first_error().cloned() else {
        panic!("an endpoint error: {:?}", silent.first_error());
    };
    assert_eq!(endpoint, "http");

    let mut explicit = site.draft("orders");
    explicit.endpoints = vec![auto("http")];
    let evaluation = site.evaluate(&explicit);
    assert_eq!(error_codes(&evaluation), ["instance.endpoint_invalid"]);
    assert_eq!(row(&evaluation, "settings").detail, NEEDS_HTTP);
    assert_eq!(paths(&evaluation), [("endpoints[0].port", "invalid_value")]);
    assert_eq!(evaluation.first_error(), silent.first_error());
    ev.pass();
}

#[test]
fn a_fixed_port_for_the_same_endpoint_is_ready() {
    let site = Site::with_provider_edit("fake", fake_init_uses_the_port);
    let mut draft = site.draft("orders");
    draft.endpoints = vec![fixed("http", 20000)];

    let evaluation = site.evaluate(&draft);

    assert!(evaluation.ready(), "{:?}", evaluation.checks);
    assert!(paths(&evaluation).is_empty());
}

#[test]
fn a_fixed_port_outside_the_range_gets_no_fixed_port_detail() {
    let site = Site::with_provider_edit("fake", fake_init_uses_the_port);
    let mut draft = site.draft("orders");
    draft.endpoints = vec![fixed("http", 0)];

    let evaluation = site.evaluate(&draft);

    assert_eq!(error_codes(&evaluation), ["instance.endpoint_invalid"]);
    assert_eq!(paths(&evaluation), [("endpoints[0].port", "out_of_range")]);
    let http = row(&evaluation, "endpoint.http");
    assert_eq!(http.status, CheckStatus::Error);
    assert!(
        !evaluation
            .checks
            .iter()
            .any(|check| check.id == "settings" && check.detail.contains("fixed port")),
        "{:?}",
        evaluation.checks
    );
}

#[test]
fn packages_whose_initialize_needs_no_port_stay_ready_with_every_endpoint_auto() {
    for provider in ["fake", "nats", "postgresql"] {
        let site = Site::with_provider(provider);

        let evaluation = site.evaluate(&site.draft("orders"));

        assert!(evaluation.ready(), "{provider}: {:?}", evaluation.checks);
    }
}

#[test]
fn the_nats_1_0_0_shape_needs_a_fixed_port_for_each_endpoint_its_config_names() {
    let site = Site::with_provider_edit("nats", nats_renders_its_config_at_initialize);

    let silent = site.evaluate(&site.draft("orders"));
    assert_eq!(error_codes(&silent), ["instance.endpoint_invalid"]);
    assert!(
        row(&silent, "settings")
            .detail
            .starts_with("This provider needs a fixed port for "),
        "{}",
        row(&silent, "settings").detail
    );

    // One fixed endpoint is not enough: the refusal moves to the other one.
    let mut first = site.draft("orders");
    first.endpoints = vec![fixed("client", 24222)];
    let evaluation = site.evaluate(&first);
    assert_eq!(error_codes(&evaluation), ["instance.endpoint_invalid"]);
    assert_eq!(
        row(&evaluation, "settings").detail,
        "This provider needs a fixed port for monitoring when the instance is created. Choose a port for it."
    );

    let mut both = site.draft("orders");
    both.endpoints = vec![fixed("client", 24222), fixed("monitoring", 28222)];
    assert!(site.evaluate(&both).ready());
}
