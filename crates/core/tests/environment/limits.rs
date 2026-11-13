//! `environment::validate` at its limits: 32 services, 31 dependencies,
//! the number ranges, a key twice and an unknown field at every level,
//! more than 100 findings, and an alias bomb inside `settings` (spec 0044
//! AC-1, AC-2, AC-4).

use stackly_core::environment::{MAX_FINDINGS, Validation, validate};

use crate::common::catalog;

fn header(services: &str) -> String {
    format!(
        "apiVersion: stackly/v1alpha1\nkind: Environment\nmetadata:\n  name: shop\nservices:\n{services}"
    )
}

/// `count` fake services `s0` to `s<count - 1>` with `auto` ports.
fn services(count: usize) -> String {
    (0..count)
        .map(|n| format!("  s{n}:\n    provider: testing/fake\n    version: \"0.1\"\n"))
        .collect()
}

fn findings(validation: &Validation) -> Vec<(String, String)> {
    validation
        .findings
        .list()
        .iter()
        .map(|f| (f.code().as_code().to_owned(), f.location().to_string()))
        .collect()
}

// covers: AC-1 (spec 0044, Schema): 32 services is the most a manifest
// holds, 33 is one too many
#[test]
fn thirty_two_services_validate_and_thirty_three_do_not() {
    let catalog = catalog();

    let at = validate(header(&services(32)).as_bytes(), &catalog);
    let over = validate(header(&services(33)).as_bytes(), &catalog);

    assert_eq!(findings(&at), [], "32 services");
    assert_eq!(at.services.len(), 32);
    let found = findings(&over);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].0, "too_many_services");
    assert!(over.manifest.is_none());
}

/// 32 services where `s0` depends on `s1` to `s<deps>`.
fn dependencies(deps: usize) -> String {
    let mut text =
        String::from("  s0:\n    provider: testing/fake\n    version: \"0.1\"\n    dependsOn:\n");
    for n in 1..=deps {
        text.push_str(&format!("      - {{service: s{n}, condition: ready}}\n"));
    }
    for n in 1..32 {
        text.push_str(&format!(
            "  s{n}:\n    provider: testing/fake\n    version: \"0.1\"\n"
        ));
    }
    text
}

// covers: AC-2 (spec 0044, Schema): 31 dependencies is the most one
// service holds
#[test]
fn thirty_one_dependencies_validate_and_thirty_two_do_not() {
    let catalog = catalog();

    let at = validate(header(&dependencies(31)).as_bytes(), &catalog);

    assert_eq!(findings(&at), [], "31 dependencies");
    let mut over = dependencies(31);
    // A 32nd edge: a duplicate is the only alias left, so the count rule
    // and the duplicate rule both speak.
    over = over.replacen(
        "      - {service: s31, condition: ready}\n",
        "      - {service: s31, condition: ready}\n      - {service: s1, condition: ready}\n",
        1,
    );
    let found = findings(&validate(header(&over).as_bytes(), &catalog));
    assert!(
        found
            .iter()
            .any(|(code, _)| code == "too_many_dependencies"),
        "{found:?}"
    );
}

// covers: AC-1 (spec 0044, Schema): the bounds of `port` and
// `timeoutSeconds` are inclusive
#[test]
fn ports_and_timeouts_accept_their_bounds_and_refuse_one_past() {
    let catalog = catalog();
    let port = |value: &str| {
        header(&format!(
            "  api:\n    provider: testing/fake\n    version: \"0.1\"\n    endpoints:\n      http: {{port: {value}}}\n"
        ))
    };
    let timeout = |value: &str| {
        header(&format!(
            "  api:\n    provider: testing/fake\n    version: \"0.1\"\n    dependsOn:\n      - {{service: db, condition: ready, timeoutSeconds: {value}}}\n  db:\n    provider: testing/fake\n    version: \"0.1\"\n"
        ))
    };

    for ok in ["1", "65535", "auto"] {
        assert_eq!(
            findings(&validate(port(ok).as_bytes(), &catalog)),
            [],
            "port {ok}"
        );
    }
    for ok in ["1", "3600"] {
        assert_eq!(
            findings(&validate(timeout(ok).as_bytes(), &catalog)),
            [],
            "timeout {ok}"
        );
    }
    for bad in ["0", "65536", "-1", "\"5432\"", "fixed"] {
        let found = findings(&validate(port(bad).as_bytes(), &catalog));
        assert_eq!(
            found,
            [(
                "invalid_value".to_owned(),
                "services.api.endpoints.http.port".to_owned()
            )],
            "port {bad}"
        );
    }
    for bad in ["0", "3601"] {
        let found = findings(&validate(timeout(bad).as_bytes(), &catalog));
        assert_eq!(found.len(), 1, "timeout {bad}: {found:?}");
        assert_eq!(found[0].0, "invalid_value", "timeout {bad}");
        assert!(
            found[0].1.contains("timeoutSeconds"),
            "timeout {bad}: {found:?}"
        );
    }
}

// covers: AC-1 (spec 0044): an unknown field is refused at every level of
// the document, at its field path
#[test]
fn an_unknown_field_is_refused_at_every_level_with_its_path() {
    let catalog = catalog();
    let service = "  api:\n    provider: testing/fake\n    version: \"0.1\"\n";
    let cases = [
        ("top", format!("{}extra: 1\n", header(service)), "extra"),
        (
            "metadata",
            header(service).replace("  name: shop\n", "  name: shop\n  labels: {a: b}\n"),
            "metadata.labels",
        ),
        (
            "service",
            header(&format!("{service}    autostart: true\n")),
            "services.api.autostart",
        ),
        (
            "endpoint",
            header(&format!(
                "{service}    endpoints:\n      http: {{port: 18080, tls: true}}\n"
            )),
            "services.api.endpoints.http.tls",
        ),
        (
            "storage",
            header(&format!(
                "{service}    storage: {{mode: managed, persistent: true, path: /tmp/x}}\n"
            )),
            "services.api.storage.path",
        ),
        (
            "dependency",
            header(&format!(
                "{service}    dependsOn:\n      - {{service: db, condition: ready, retries: 3}}\n  db:\n    provider: testing/fake\n    version: \"0.1\"\n"
            )),
            "services.api.dependsOn[0].retries",
        ),
    ];
    for (level, text, path) in cases {
        let found = findings(&validate(text.as_bytes(), &catalog));
        assert!(
            found
                .iter()
                .any(|(code, location)| code == "unknown_field" && location == path),
            "{level}: {found:?}"
        );
        assert!(
            found.iter().all(|(code, _)| code == "unknown_field"),
            "{level}: nothing else: {found:?}"
        );
    }
}

// covers: AC-1 (spec 0044, YAML yang diterima): a key twice in one mapping
// is `duplicate_key` at every level, and nothing else is reported
#[test]
fn a_key_twice_is_refused_at_every_level() {
    let catalog = catalog();
    let service = "  api:\n    provider: testing/fake\n    version: \"0.1\"\n";
    let cases = [
        ("top", format!("{}kind: Environment\n", header(service))),
        (
            "metadata",
            header(service).replace("  name: shop\n", "  name: shop\n  name: shop\n"),
        ),
        ("services", header(&format!("{service}{service}"))),
        (
            "service",
            header(&format!("{service}    version: \"0.1\"\n")),
        ),
        (
            "endpoints",
            header(&format!(
                "{service}    endpoints:\n      http: {{port: 18080}}\n      http: {{port: 18081}}\n"
            )),
        ),
        (
            "endpoint",
            header(&format!(
                "{service}    endpoints:\n      http: {{port: 18080, port: 18081}}\n"
            )),
        ),
        (
            "settings",
            header(&format!(
                "{service}    settings:\n      mode: a\n      mode: b\n"
            )),
        ),
        (
            "secretRefs",
            header(&format!(
                "{service}    secretRefs:\n      authToken: \"local:a/b\"\n      authToken: \"local:a/c\"\n"
            )),
        ),
        (
            "dependency",
            header(&format!(
                "{service}    dependsOn:\n      - {{service: db, service: db}}\n  db:\n    provider: testing/fake\n    version: \"0.1\"\n"
            )),
        ),
    ];
    for (level, text) in cases {
        let found = findings(&validate(text.as_bytes(), &catalog));
        assert_eq!(found.len(), 1, "{level}: {found:?}");
        assert_eq!(found[0].0, "duplicate_key", "{level}: {found:?}");
        assert!(
            !found[0].1.is_empty() && found[0].1 != ".",
            "{level}: a position or a path: {found:?}"
        );
    }
}

// covers: AC-4 (spec 0044): at most 100 findings come back, and the count
// and the omitted number still say how many there are
#[test]
fn more_than_a_hundred_findings_keep_the_first_hundred_and_count_the_rest() {
    // 32 services, each with four unknown endpoints: 128 findings.
    let text: String = (0..32)
        .map(|n| {
            format!(
                "  s{n}:\n    provider: testing/fake\n    version: \"0.1\"\n    endpoints:\n      a: {{port: auto}}\n      b: {{port: auto}}\n      c: {{port: auto}}\n      d: {{port: auto}}\n"
            )
        })
        .collect();

    let validation = validate(header(&text).as_bytes(), &catalog());

    assert_eq!(validation.findings.list().len(), MAX_FINDINGS);
    assert_eq!(validation.findings.count(), 128);
    assert_eq!(validation.findings.omitted(), 28);
    assert!(validation.findings.has_errors());
    assert!(validation.manifest.is_none());
}

// covers: AC-1 (spec 0044, YAML yang diterima, Security model): a bomb
// built inside `settings` of a real service is refused like one at the
// top, within two seconds and before its value is built
#[test]
fn an_alias_bomb_inside_settings_is_refused_within_two_seconds() {
    const LIMIT: usize = 256 * 1024;
    let mut text = header(&format!(
        "  api:\n    provider: testing/fake\n    version: \"0.1\"\n    settings:\n      big: &b {}\n      many:\n",
        "x".repeat(64 * 1024)
    ));
    while text.len() < LIMIT - 16 {
        text.push_str("        - *b\n");
    }
    assert!(text.len() < LIMIT);
    let start = std::time::Instant::now();

    let validation = validate(text.as_bytes(), &catalog());

    assert!(
        start.elapsed() < std::time::Duration::from_secs(2),
        "{:?}",
        start.elapsed()
    );
    assert_eq!(
        findings(&validation),
        [("yaml_alias".to_owned(), ".".to_owned())]
    );
    assert!(validation.services.is_empty());
}

// covers: AC-1 (spec 0044, YAML yang diterima): a nested bomb whose
// anchors sit inside `settings` stops at a limit too
#[test]
fn a_nested_alias_bomb_inside_settings_is_refused() {
    let mut nested = String::from("      a0: &a0 [lol, lol, lol, lol, lol, lol, lol, lol, lol]\n");
    for n in 1..10 {
        let alias = format!("*a{}", n - 1);
        nested.push_str(&format!(
            "      a{n}: &a{n} [{}]\n",
            [alias.as_str(); 9].join(", ")
        ));
    }
    let text = header(&format!(
        "  api:\n    provider: testing/fake\n    version: \"0.1\"\n    settings:\n{nested}"
    ));
    let start = std::time::Instant::now();

    let validation = validate(text.as_bytes(), &catalog());

    assert!(start.elapsed() < std::time::Duration::from_secs(2));
    let found = findings(&validation);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        ["yaml_alias", "yaml_syntax"].contains(&found[0].0.as_str()),
        "{found:?}"
    );
}

// covers: AC-1 (spec 0044, YAML yang diterima): a file near 256 KiB with
// no alias never reaches the expand budget
#[test]
fn a_file_just_under_256_kib_without_aliases_validates() {
    const LIMIT: usize = 256 * 1024;
    let mut text = header(&services(32));
    let comment = format!("# {}\n", "padding ".repeat(12));
    while text.len() + comment.len() <= LIMIT {
        text.push_str(&comment);
    }
    assert!(text.len() <= LIMIT);

    let validation = validate(text.as_bytes(), &catalog());

    assert_eq!(findings(&validation), []);
}
