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

/// A manifest from its services block, under a fixed header.
fn manifest(services: &str) -> String {
    format!(
        "apiVersion: stackly/v1alpha1\nkind: Environment\nmetadata:\n  name: shop\nservices:\n{services}"
    )
}

/// The codes and locations of every finding, in order.
fn findings(validation: &Validation) -> Vec<(String, String)> {
    validation
        .findings
        .list()
        .iter()
        .map(|f| (f.code().as_code().to_owned(), f.location().to_string()))
        .collect()
}

// covers: AC-1 (spec 0044, YAML yang diterima)
#[test]
fn a_manifest_with_aliases_validates_like_the_one_that_writes_them_out() {
    let catalog = catalog();
    let dir = fixtures().join("aliases");
    let mut names = Vec::new();
    for entry in fs::read_dir(&dir).expect("the aliases folder") {
        let path = entry.expect("an entry").path();
        let name = path.file_name().and_then(|n| n.to_str()).expect("a name");
        let Some(stem) = name.strip_suffix(".yaml") else {
            continue;
        };
        if stem.ends_with(".expanded") {
            continue;
        }
        let aliased = fs::read(&path).expect("the fixture");
        let expanded = fs::read(dir.join(format!("{stem}.expanded.yaml"))).expect("its pair");
        assert!(
            String::from_utf8_lossy(&aliased).contains(" *"),
            "{stem} uses an alias"
        );
        let aliased = validate(&aliased, &catalog);
        let expanded = validate(&expanded, &catalog);

        assert_eq!(aliased.is_valid(), expanded.is_valid(), "{stem}: valid");
        assert_eq!(
            format!("{:?}", aliased.services),
            format!("{:?}", expanded.services),
            "{stem}: services"
        );
        assert_eq!(findings(&aliased), findings(&expanded), "{stem}: findings");
        assert!(
            !codes(&aliased).contains(&"yaml_alias"),
            "{stem}: {:?}",
            findings(&aliased)
        );
        names.push(stem.to_owned());
    }
    names.sort();
    assert_eq!(
        names,
        [
            "alias-brings-a-finding",
            "block-scalar",
            "by1",
            "by2",
            "by3",
            "exotic3",
            "shared-settings"
        ]
    );
}

// covers: AC-1, AC-3 (spec 0044)
#[test]
fn an_alias_never_carries_a_value_past_a_rule() {
    let validation = validate(&fixture("aliases/alias-brings-a-finding.yaml"), &catalog());
    let found = findings(&validation);
    for location in [
        "services.primary.secretRefs.password",
        "services.replica.secretRefs.password",
    ] {
        assert!(
            found.contains(&("secret_literal".to_owned(), location.to_owned())),
            "{location} in {found:?}"
        );
    }
    assert!(
        codes(&validation).contains(&"endpoint_conflict"),
        "{found:?}"
    );
    for finding in validation.findings.list() {
        assert!(!finding.message().contains("SENTINEL"));
        assert!(!finding.location().to_string().contains("SENTINEL"));
    }
}

// covers: AC-1 (spec 0044, YAML yang diterima)
#[test]
fn tags_merge_keys_missing_anchors_and_extra_documents_are_refused() {
    let fake = "    provider: testing/fake\n    version: \"0.1\"\n";
    let cases = [
        (
            manifest(&format!(
                "  api:\n{fake}    settings:\n      note: !local x\n"
            )),
            "yaml_alias",
            "services.api.settings.note",
        ),
        (
            manifest(&format!(
                "  api:\n{fake}    settings:\n      !local note: x\n"
            )),
            "yaml_alias",
            "services.api.settings",
        ),
        (
            manifest(&format!("  api: &a\n{fake}  web:\n    \"<<\": *a\n")),
            "yaml_alias",
            "services.web.<<",
        ),
        (
            manifest(&format!("  api:\n{fake}  web: *missing\n")),
            "yaml_syntax",
            "",
        ),
        (
            format!("{}---\nother: 1\n", manifest(&format!("  api:\n{fake}"))),
            "yaml_syntax",
            ".",
        ),
    ];
    for (text, code, location) in cases {
        let validation = validate(text.as_bytes(), &catalog());
        let found = findings(&validation);
        assert_eq!(found.len(), 1, "{text}: {found:?}");
        assert_eq!(found[0].0, code, "{text}");
        if location.is_empty() {
            assert!(found[0].1.contains(':'), "{text}: a position, {found:?}");
        } else {
            assert_eq!(found[0].1, location, "{text}");
        }
    }
}

// covers: AC-1 (spec 0044, YAML yang diterima)
#[test]
fn the_merge_key_fixture_points_at_its_mapping() {
    let validation = validate(&fixture("invalid/yaml_alias.yaml"), &catalog());
    assert_eq!(
        findings(&validation),
        [("yaml_alias".to_owned(), "services.web.<<".to_owned())]
    );
}

// covers: AC-1 (spec 0044, YAML yang diterima, Security model)
#[test]
fn alias_bombs_under_256_kib_are_refused_within_two_seconds() {
    const LIMIT: usize = 256 * 1024;
    // One large anchor behind a chain of nine way aliases.
    let mut anchor = format!("a0: &a0 [{}]\n", vec!["x"; 1000].join(", "));
    for level in 1..=6 {
        let alias = format!("*a{}", level - 1);
        anchor.push_str(&format!(
            "a{level}: &a{level} [{}]\n",
            [alias.as_str(); 9].join(", ")
        ));
    }
    // A 100 KiB string behind as many aliases as fit.
    let mut string = format!("a: &a {}\nl:\n", "x".repeat(100 * 1024));
    while string.len() < LIMIT - 8 {
        string.push_str("- *a\n");
    }
    // The classic nested billion laughs.
    let mut nested = String::from("a0: &a0 [lol, lol, lol, lol, lol, lol, lol, lol, lol]\n");
    for n in 1..10 {
        let alias = format!("*a{}", n - 1);
        nested.push_str(&format!(
            "a{n}: &a{n} [{}]\n",
            [alias.as_str(); 9].join(", ")
        ));
    }
    let cases = [
        ("large anchor", anchor, &["yaml_alias"][..]),
        ("large string", string, &["yaml_alias"][..]),
        ("nested", nested, &["yaml_alias", "yaml_syntax"][..]),
    ];
    for (name, text, allowed) in cases {
        assert!(text.len() < LIMIT, "{name}: {}", text.len());
        let start = std::time::Instant::now();

        let validation = validate(text.as_bytes(), &catalog());

        assert!(
            start.elapsed() < std::time::Duration::from_secs(2),
            "{name}: {:?}",
            start.elapsed()
        );
        let found = findings(&validation);
        assert_eq!(found.len(), 1, "{name}: {found:?}");
        assert!(allowed.contains(&found[0].0.as_str()), "{name}: {found:?}");
        if found[0].0 == "yaml_alias" {
            assert_eq!(found[0].1, ".", "{name}");
        }
    }
}

// covers: AC-1 (spec 0044, K2)
#[test]
fn quoted_multi_line_values_quotes_in_plain_text_and_comments_stay_valid() {
    let text = "\
apiVersion: stackly/v1alpha1
kind: Environment
metadata:
  name: shop
services:
  # &not an anchor, *nor an alias, \"not a quote
  proxy:
    provider: official/traefik
    version: \"3.7\"
    endpoints:
      web: {port: 8000}
    settings:
      routes:
        - name: web
          rule: Host(`shop.localhost`) && PathPrefix(`/it's`)
          backendUrl: \"http://127.0.0.1:3000\"
        - name: api
          rule: Host(`shop.localhost`) &&
            PathPrefix(`/api`)
          backendUrl: 'http://127.0.0.1:3001'
        - name: docs
          rule: \"Host(`shop.localhost`) &&
            PathPrefix(`/docs`)\"
          backendUrl: \"http://127.0.0.1:3002\"  # &comment
";
    let validation = validate(text.as_bytes(), &catalog());
    assert!(
        validation.findings.list().is_empty(),
        "{:#?}",
        validation.findings.list()
    );
}

// covers: AC-1 (spec 0044, YAML yang diterima)
#[test]
fn quotes_block_scalars_unused_anchors_and_resolved_tags_stay_valid() {
    let route = |rule: &str| {
        manifest(&format!(
            "  proxy:\n    provider: official/traefik\n    version: \"3.7\"\n    settings:\n      routes:\n        - name: web\n{rule}\n          backendUrl: \"http://127.0.0.1:3000\"\n"
        ))
    };
    // Settings text holds no control character, so each block keeps one
    // line and strips its final line break.
    let note = |block: &str| {
        manifest(&format!(
            "  db:\n    provider: testing/fake\n    version: \"0.1\"\n    settings:\n      greeting: {block}\n"
        ))
    };
    let cases = [
        (
            "a quote that starts on its own line",
            route(
                "          rule:\n            \"Host(`a.localhost`) &&\n            PathPrefix(`/`)\"",
            ),
        ),
        ("a literal block", note("|-\n        &one *two \"three")),
        ("a folded block", note(">-\n        &one *two 'three")),
        (
            "an indentation indicator",
            note("|2-\n          &one *two \"three"),
        ),
        (
            "an unused anchor",
            manifest("  db: &unused\n    provider: testing/fake\n    version: \"0.1\"\n"),
        ),
        (
            "a version with a resolved tag",
            manifest("  db:\n    provider: testing/fake\n    version: !!str 0.1\n"),
        ),
    ];
    for (name, text) in cases {
        let validation = validate(text.as_bytes(), &catalog());
        assert!(
            validation.findings.list().is_empty(),
            "{name}: {:#?}",
            validation.findings.list()
        );
    }
    let erp = validate(&fixture("erp-development.yaml"), &catalog());
    assert!(erp.is_valid());
}

// covers: AC-1 (spec 0044)
#[test]
fn syntax_findings_point_at_a_line_and_column() {
    let validation = validate(&fixture("invalid/yaml_syntax.yaml"), &catalog());
    let found = findings(&validation);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].0, "yaml_syntax");
    let (line, column) = found[0].1.split_once(':').expect("line:column");
    assert!(line.parse::<usize>().is_ok() && column.parse::<usize>().is_ok());
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
    assert_eq!(
        declared[0].selector().map(ToString::to_string).as_deref(),
        Some("0.1")
    );
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
