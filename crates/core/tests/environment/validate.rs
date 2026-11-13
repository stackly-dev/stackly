//! `environment::validate`: AC-1, AC-2, AC-3.

use std::fs;

use stackly_core::environment::{FindingCode, Validation, validate};
use stackly_domain::{Ownership, PortRequest};

use crate::common::{catalog, fixture, fixtures};

fn codes(validation: &Validation) -> Vec<&'static str> {
    validation
        .findings
        .list()
        .iter()
        .map(|finding| finding.code().as_code())
        .collect()
}

// covers: AC-1 (spec 0044)
#[test]
fn the_erp_example_validates_with_no_finding_against_the_catalog() {
    let validation = validate(&fixture("erp-development.yaml"), &catalog());

    assert_eq!(
        codes(&validation),
        Vec::<&str>::new(),
        "{:#?}",
        validation.findings
    );
    let manifest = validation.manifest.expect("a valid manifest");
    assert_eq!(manifest.name.as_str(), "erp-development");
    let aliases: Vec<&str> = manifest.services.iter().map(|s| s.alias.as_str()).collect();
    assert_eq!(
        aliases,
        [
            "postgres",
            "mysql",
            "nats",
            "clickhouse",
            "valkey",
            "traefik"
        ],
        "manifest order"
    );
    let postgres = &manifest.services[0];
    assert_eq!(postgres.ownership, Ownership::Dedicated);
    assert_eq!(
        postgres.instance_name.as_ref().map(|n| n.as_str()),
        Some("erp-development-postgres")
    );
    assert_eq!(postgres.selector_text.as_deref(), Some("18"));
    assert!(matches!(postgres.endpoints[0].port, PortRequest::Fixed(port) if port.get() == 5432));
    assert_eq!(
        postgres.secret_refs[0].1.to_string(),
        "local:erp/postgres/admin"
    );
    assert_eq!(validation.services.len(), 6);
    assert_eq!(
        validation.sha256,
        stackly_core::environment::manifest_digest(&fixture("erp-development.yaml"))
    );
}

// covers: AC-1, AC-2 (spec 0044)
#[test]
fn every_invalid_fixture_reports_its_own_code() {
    let catalog = catalog();
    let mut seen = Vec::new();
    for entry in fs::read_dir(fixtures().join("invalid")).expect("the invalid folder") {
        let path = entry.expect("an entry").path();
        let code = path
            .file_stem()
            .and_then(|s| s.to_str())
            .expect("a stem")
            .to_owned();
        let validation = validate(&fs::read(&path).expect("the fixture"), &catalog);
        assert!(
            codes(&validation).contains(&code.as_str()),
            "{code}: got {:#?}",
            validation.findings
        );
        assert!(validation.manifest.is_none(), "{code} is not valid");
        for finding in validation.findings.list() {
            assert!(
                finding.message().starts_with(finding.code().template()),
                "{code}: the message is the template"
            );
        }
        seen.push(code);
    }
    seen.sort();
    let mut expected: Vec<&str> = FindingCode::ALL.iter().map(|c| c.as_code()).collect();
    expected.sort_unstable();
    assert_eq!(seen, expected, "one fixture per code");
}

// covers: AC-2 (spec 0044)
#[test]
fn a_cycle_names_its_path_and_every_finding_comes_back_at_once() {
    let text = "\
apiVersion: stackly/v1alpha1
kind: Environment
metadata:
  name: shop
services:
  api:
    provider: testing/fake
    dependsOn: [{service: db}]
  db:
    provider: testing/fake
    dependsOn: [{service: cache}]
  cache:
    provider: testing/fake
    dependsOn: [{service: api}]
    endpoints:
      http: {port: 18080}
  web:
    provider: testing/fake
    endpoints:
      http: {port: 18080}
    dependsOn: [{service: web}, {service: nothing}]
";
    let validation = validate(text.as_bytes(), &catalog());

    let cycle = validation
        .findings
        .list()
        .iter()
        .find(|f| f.code() == FindingCode::DependencyCycle)
        .expect("a cycle");
    assert_eq!(
        cycle.message(),
        "dependency cycle: api -> db -> cache -> api"
    );
    let found = codes(&validation);
    for code in ["self_dependency", "unknown_dependency", "endpoint_conflict"] {
        assert!(found.contains(&code), "{code} in {found:?}");
    }
    let conflicts: Vec<String> = validation
        .findings
        .list()
        .iter()
        .filter(|f| f.code() == FindingCode::EndpointConflict)
        .map(|f| f.location().to_string())
        .collect();
    assert_eq!(
        conflicts,
        [
            "services.cache.endpoints.http.port",
            "services.web.endpoints.http.port"
        ],
        "both endpoints are named"
    );
}

// covers: AC-1 (spec 0044)
#[test]
fn syntax_findings_point_at_a_line_and_column() {
    let validation = validate(&fixture("invalid/yaml_alias.yaml"), &catalog());
    let locations: Vec<String> = validation
        .findings
        .list()
        .iter()
        .map(|f| f.location().to_string())
        .collect();
    assert_eq!(locations, ["7:8", "9:8"], "the anchor and the alias");
    let validation = validate(&fixture("invalid/yaml_syntax.yaml"), &catalog());
    assert!(
        validation.findings.list()[0]
            .location()
            .to_string()
            .contains(':')
    );
}

// covers: AC-1 (spec 0044)
#[test]
fn an_unquoted_version_asks_for_quotes() {
    let validation = validate(&fixture("invalid/invalid_value.yaml"), &catalog());
    let finding = &validation.findings.list()[0];
    assert_eq!(finding.location().to_string(), "services.api.version");
    assert_eq!(
        finding.message(),
        "invalid value: write the version in quotes"
    );
}

// covers: AC-3 (spec 0044)
#[test]
fn no_message_or_location_holds_a_value_from_the_file() {
    const SENTINEL: &str = "SENTINELx9q";
    let cases = [
        format!("    secretRefs:\n      authToken: {SENTINEL}\n"),
        format!("    secretRefs:\n      authToken: \"local:instance/{SENTINEL}\"\n"),
        format!("    settings:\n      authToken: {SENTINEL}\n"),
        format!("    settings:\n      readyDelayMs: {SENTINEL}\n"),
        format!("    settings:\n      greeting: {{nested: {SENTINEL}}}\n"),
        format!("    secretRefs:\n      authToken: {{nested: {SENTINEL}}}\n"),
        format!("    endpoints:\n      http: {{port: {SENTINEL}}}\n"),
        format!("    version: {{x: {SENTINEL}}}\n"),
    ];
    let catalog = catalog();
    for case in cases {
        let text = format!(
            "apiVersion: stackly/v1alpha1\nkind: Environment\nmetadata:\n  name: shop\nservices:\n  api:\n    provider: testing/fake\n{case}"
        );
        let validation = validate(text.as_bytes(), &catalog);
        assert!(validation.findings.count() > 0, "{case}");
        let shown = format!("{:?}", validation.findings);
        assert!(!shown.contains("SENTINEL"), "{case}: {shown}");
        let declared = format!("{:?}", validation.services);
        assert!(!declared.contains("SENTINEL"), "{case}: {declared}");
    }
}

// covers: AC-3 (spec 0044)
#[test]
fn each_secret_case_has_its_own_code() {
    let catalog = catalog();
    let case = |file: &str| codes(&validate(&fixture(file), &catalog));
    assert!(case("invalid/secret_literal.yaml").contains(&"secret_literal"));
    assert!(case("invalid/secret_reference_reserved.yaml").contains(&"secret_reference_reserved"));
    assert!(case("invalid/secret_in_settings.yaml").contains(&"secret_in_settings"));
    let stackly = "apiVersion: stackly/v1alpha1\nkind: Environment\nmetadata:\n  name: shop\nservices:\n  api:\n    provider: testing/fake\n    secretRefs:\n      authToken: \"local:stackly/internal\"\n";
    assert_eq!(
        codes(&validate(stackly.as_bytes(), &catalog)),
        ["secret_reference_reserved"]
    );
}

// covers: AC-1, AC-8 (spec 0044)
#[test]
fn the_graph_fixture_validates_and_keeps_its_snapshot() {
    let validation = validate(&fixture("graph.yaml"), &catalog());
    let manifest = validation.manifest.expect("valid");
    let declared = manifest.declared();
    assert_eq!(declared.len(), 4);
    assert_eq!(declared[0].depends_on()[0].timeout_seconds(), Some(30));
    assert_eq!(declared[1].depends_on().len(), 2);
    assert!(declared[0].selector().is_none(), "no version written");
}

// covers: AC-8 (spec 0044)
#[test]
fn a_shared_service_writes_only_its_provider_and_instance_ref() {
    let text = "\
apiVersion: stackly/v1alpha1
kind: Environment
metadata:
  name: shop
services:
  cache:
    provider: official/valkey
    instanceRef: shared-valkey
    instanceName: mine
    endpoints: {client: {port: 6379}}
    settings: {persistence: aof}
    secretRefs: {password: \"local:shop/valkey\"}
    storage: {mode: managed, persistent: true}
";
    let validation = validate(text.as_bytes(), &catalog());
    let fields: Vec<String> = validation
        .findings
        .list()
        .iter()
        .filter(|f| f.code() == FindingCode::InstanceRefConflict)
        .map(|f| f.location().to_string())
        .collect();
    assert_eq!(
        fields,
        [
            "services.cache.instanceName",
            "services.cache.endpoints",
            "services.cache.settings",
            "services.cache.secretRefs",
            "services.cache.storage"
        ]
    );
    let shared = "apiVersion: stackly/v1alpha1\nkind: Environment\nmetadata:\n  name: shop\nservices:\n  cache:\n    provider: official/valkey\n    instanceRef: shared-valkey\n";
    let manifest = validate(shared.as_bytes(), &catalog())
        .manifest
        .expect("valid");
    assert_eq!(manifest.services[0].ownership, Ownership::Shared);
    assert!(manifest.services[0].instance_name.is_none());
}

// covers: AC-1 (spec 0044)
#[test]
fn a_derived_instance_name_over_63_characters_asks_for_instance_name() {
    let name = "a".repeat(60);
    let text = format!(
        "apiVersion: stackly/v1alpha1\nkind: Environment\nmetadata:\n  name: {name}\nservices:\n  api:\n    provider: testing/fake\n"
    );
    let validation = validate(text.as_bytes(), &catalog());
    assert_eq!(codes(&validation), ["instance_name_invalid"]);
}
