//! The manifest `remedies` block (spec 0034 AC-10): each test adds a block
//! to a copy of `providers/postgresql` held in memory, without the block the
//! repo package has, then checks that it loads or which issue it gets, at
//! which location.
#![cfg(unix)]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use stackly_core::provider::{Issue, IssueCode, NameCheck, PackageFiles, ValidatedPackage};

fn postgresql_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../providers/postgresql")
}

/// Reads a package's files into memory, the way `read_package` would.
fn files_of(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut files = BTreeMap::new();
    for path in [
        "provider.yaml",
        "versions.json",
        "schemas/settings.schema.json",
    ] {
        files.insert(
            path.to_owned(),
            fs::read(dir.join(path)).expect("package file"),
        );
    }
    for entry in fs::read_dir(dir.join("templates")).expect("templates folder") {
        let entry = entry.expect("template entry");
        let name = entry.file_name().into_string().expect("UTF-8 name");
        files.insert(
            format!("templates/{name}"),
            fs::read(entry.path()).expect("template"),
        );
    }
    files
}

/// The start of the top-level `remedies` block, the last block of the
/// PostgreSQL manifest.
const REMEDIES_BLOCK: &str = "\nremedies:\n";

/// `providers/postgresql` in memory.
struct Package {
    files: BTreeMap<String, Vec<u8>>,
}

impl Package {
    /// `providers/postgresql` without its own `remedies` block, so each test
    /// adds the block it checks: the tcp endpoint `client`, the unix
    /// endpoint `socket`, the aliases `initdb` and `psql`, the secret field
    /// `superuserPassword`, and the managed folders data, config, temp, and
    /// run.
    fn postgresql() -> Self {
        let mut files = files_of(&postgresql_dir());
        let manifest = files
            .get_mut("provider.yaml")
            .expect("the package has a manifest");
        let text = String::from_utf8(manifest.clone()).expect("a UTF-8 manifest");
        if let Some(at) = text.find(REMEDIES_BLOCK) {
            manifest.truncate(at + 1);
        }
        Self { files }
    }

    /// Appends a top-level `remedies` block written in YAML.
    fn with_remedies(self, block: &str) -> Self {
        self.append(&format!("remedies:\n{block}"))
    }

    fn append(mut self, text: &str) -> Self {
        let manifest = self
            .files
            .get_mut("provider.yaml")
            .expect("the package has a manifest");
        if !manifest.ends_with(b"\n") {
            manifest.push(b'\n');
        }
        manifest.extend_from_slice(text.as_bytes());
        self
    }

    fn validate(self) -> Result<ValidatedPackage, Vec<Issue>> {
        ValidatedPackage::from_files(
            PackageFiles::new("postgresql", self.files),
            NameCheck::FolderName,
        )
        .map_err(|err| err.issues)
    }

    /// Validates and returns the issues, failing when the package passes.
    #[track_caller]
    fn issues(self) -> Vec<Issue> {
        match self.validate() {
            Ok(_) => panic!("the package should be rejected"),
            Err(issues) => issues,
        }
    }
}

/// A YAML double quoted scalar.
fn yaml(text: &str) -> String {
    serde_json::to_string(text).expect("a string serializes")
}

/// One entry named `name` with `when` (inline YAML) and the step lines.
fn entry(name: &str, when: &str, steps: &str) -> String {
    format!("  - name: {name}\n    when: {when}\n    title: Fix it\n    steps:\n{steps}")
}

/// A step with only `text`.
fn text_step(text: &str) -> String {
    format!("      - text: {}\n", yaml(text))
}

/// A step with `text` and `command`.
fn command_step(command: &str) -> String {
    format!(
        "      - text: Run this.\n        command: {}\n",
        yaml(command)
    )
}

/// A step with `text` and `path`.
fn path_step(path: &str) -> String {
    format!("      - text: Look here.\n        path: {}\n", yaml(path))
}

const WHEN_EXCERPT: &str = "{ excerptContains: \"postmaster.pid\" }";

/// One valid entry with `step`.
fn with_step(step: &str) -> Package {
    Package::postgresql().with_remedies(&entry("fix", WHEN_EXCERPT, step))
}

/// The one `provider.yaml` issue at `location`, failing when there is none.
#[track_caller]
fn issue_at<'a>(issues: &'a [Issue], location: &str) -> &'a Issue {
    issues
        .iter()
        .find(|issue| issue.file == "provider.yaml" && issue.location.to_string() == location)
        .unwrap_or_else(|| panic!("expected an issue at {location}, got {issues:#?}"))
}

#[track_caller]
fn assert_rejected(package: Package, location: &str, code: IssueCode) {
    let issues = package.issues();
    let issue = issue_at(&issues, location);
    assert_eq!(issue.code, code, "{issue:#?}");
}

#[test]
fn a_package_with_a_remedies_block_loads_with_entries_in_manifest_order() {
    let block = format!(
        "{}  - name: pid\n    when: {{ failureCodes: [crash_loop, process_exited], excerptContains: \"postmaster.pid\" }}\n    evidence: {{ pidFile: \"${{instance.data}}/postmaster.pid\" }}\n    title: Old PID file\n    steps:\n{}{}",
        entry(
            "role",
            "{ readinessReasons: [probe_failed], readinessDetailContains: \"role\" }",
            &command_step(
                "${runtime.executables.psql} --host=${endpoint.socket.socketDir} --port=${endpoint.client.port} --dbname=${instance.name}"
            ),
        ),
        text_step("Stackly never removes this file."),
        path_step("${instance.data}/postmaster.pid"),
    );
    let package = Package::postgresql()
        .with_remedies(&block)
        .validate()
        .unwrap_or_else(|issues| panic!("{issues:#?}"));

    let entries: Vec<(&str, &str, bool)> = package
        .manifest()
        .remedies()
        .iter()
        .map(|remedy| (remedy.name(), remedy.title(), remedy.has_pid_file()))
        .collect();
    assert_eq!(
        entries,
        [("role", "Fix it", false), ("pid", "Old PID file", true)]
    );
}

#[test]
fn a_package_without_a_remedies_block_has_no_entries() {
    let package = Package::postgresql()
        .validate()
        .unwrap_or_else(|issues| panic!("{issues:#?}"));
    assert!(package.manifest().remedies().is_empty());
}

#[test]
fn the_block_is_part_of_the_manifest_digest() {
    let without = Package::postgresql()
        .validate()
        .expect("valid without the block");
    let with = with_step(&text_step("Do this."))
        .validate()
        .expect("valid with the block");
    assert_ne!(
        without.key().manifest_sha256(),
        with.key().manifest_sha256()
    );
}

#[test]
fn an_empty_or_oversized_block_is_invalid_value() {
    assert_rejected(
        Package::postgresql().append("remedies: []\n"),
        "remedies",
        IssueCode::InvalidValue,
    );
    let mut block = String::new();
    for index in 0..9 {
        block.push_str(&entry(
            &format!("fix{index}"),
            WHEN_EXCERPT,
            &text_step("Do this."),
        ));
    }
    assert_rejected(
        Package::postgresql().with_remedies(&block),
        "remedies",
        IssueCode::InvalidValue,
    );
}

#[test]
fn a_name_outside_the_pattern_or_used_twice_is_invalid_name() {
    for name in ["Fix", "2fix", "fix-it", &"a".repeat(33)] {
        assert_rejected(
            Package::postgresql().with_remedies(&entry(name, WHEN_EXCERPT, &text_step("Do."))),
            "remedies[0].name",
            IssueCode::InvalidName,
        );
    }
    let twice = format!(
        "{}{}",
        entry("fix", WHEN_EXCERPT, &text_step("Do.")),
        entry("fix", WHEN_EXCERPT, &text_step("Do."))
    );
    assert_rejected(
        Package::postgresql().with_remedies(&twice),
        "remedies[1].name",
        IssueCode::InvalidName,
    );
}

#[test]
fn an_empty_when_is_invalid_value() {
    assert_rejected(
        Package::postgresql().with_remedies(&entry("fix", "{}", &text_step("Do."))),
        "remedies[0].when",
        IssueCode::InvalidValue,
    );
}

#[test]
fn when_codes_must_be_one_to_eight_domain_codes() {
    assert_rejected(
        Package::postgresql().with_remedies(&entry(
            "fix",
            "{ failureCodes: [] }",
            &text_step("Do."),
        )),
        "remedies[0].when.failureCodes",
        IssueCode::InvalidValue,
    );
    assert_rejected(
        Package::postgresql().with_remedies(&entry(
            "fix",
            "{ failureCodes: [a, b, c, d, e, f, g, h, i] }",
            &text_step("Do."),
        )),
        "remedies[0].when.failureCodes",
        IssueCode::InvalidValue,
    );
    assert_rejected(
        Package::postgresql().with_remedies(&entry(
            "fix",
            "{ failureCodes: [crash_loop, Crash] }",
            &text_step("Do."),
        )),
        "remedies[0].when.failureCodes[1]",
        IssueCode::InvalidValue,
    );
    assert_rejected(
        Package::postgresql().with_remedies(&entry(
            "fix",
            "{ readinessReasons: [\"probe failed\"] }",
            &text_step("Do."),
        )),
        "remedies[0].when.readinessReasons[0]",
        IssueCode::InvalidValue,
    );
}

#[test]
fn when_texts_are_one_to_two_hundred_characters_without_control_characters() {
    for needle in [String::new(), "x".repeat(201), "a\tb".to_owned()] {
        assert_rejected(
            Package::postgresql().with_remedies(&entry(
                "fix",
                &format!("{{ excerptContains: {} }}", yaml(&needle)),
                &text_step("Do."),
            )),
            "remedies[0].when.excerptContains",
            IssueCode::InvalidValue,
        );
    }
    assert_rejected(
        Package::postgresql().with_remedies(&entry(
            "fix",
            &format!("{{ readinessDetailContains: {} }}", yaml(&"x".repeat(201))),
            &text_step("Do."),
        )),
        "remedies[0].when.readinessDetailContains",
        IssueCode::InvalidValue,
    );
}

#[test]
fn a_title_or_step_text_outside_its_limit_is_invalid_value() {
    assert_rejected(
        Package::postgresql().with_remedies(
            &entry("fix", WHEN_EXCERPT, &text_step("Do."))
                .replace("title: Fix it", &format!("title: {}", "t".repeat(81))),
        ),
        "remedies[0].title",
        IssueCode::InvalidValue,
    );
    for text in [String::new(), "x".repeat(301), "line\nbreak".to_owned()] {
        assert_rejected(
            with_step(&text_step(&text)),
            "remedies[0].steps[0].text",
            IssueCode::InvalidValue,
        );
    }
    assert_rejected(
        with_step(&command_step(&"x".repeat(501))),
        "remedies[0].steps[0].command",
        IssueCode::InvalidValue,
    );
    assert_rejected(
        with_step(&path_step(&"x".repeat(301))),
        "remedies[0].steps[0].path",
        IssueCode::InvalidValue,
    );
}

#[test]
fn one_to_six_steps_are_allowed() {
    assert_rejected(
        Package::postgresql().append(&format!(
            "remedies:\n  - name: fix\n    when: {WHEN_EXCERPT}\n    title: Fix it\n    steps: []\n"
        )),
        "remedies[0].steps",
        IssueCode::InvalidValue,
    );
    assert_rejected(
        with_step(&text_step("Do.").repeat(7)),
        "remedies[0].steps",
        IssueCode::InvalidValue,
    );
    with_step(&text_step("Do.").repeat(6))
        .validate()
        .unwrap_or_else(|issues| panic!("{issues:#?}"));
}

#[test]
fn a_secret_token_is_refused_wherever_a_remedy_takes_tokens() {
    for token in ["${secret.superuserPassword}", "${secret.password}"] {
        assert_rejected(
            with_step(&command_step(&format!("psql --password={token}"))),
            "remedies[0].steps[0].command",
            IssueCode::SecretNotAllowedHere,
        );
        assert_rejected(
            with_step(&path_step(&format!("${{instance.data}}/{token}"))),
            "remedies[0].steps[0].path",
            IssueCode::SecretNotAllowedHere,
        );
    }
}

#[test]
fn a_setting_or_a_token_outside_the_remedy_set_is_token_not_allowed_here() {
    for token in [
        "${settings.maxConnections}",
        "${endpoint.client.host}",
        "${endpoint.client.address}",
        "${endpoint.socket.socket}",
        "${instance.temp}",
        "${instance.id}",
        "${instance.secrets}",
        "${runtime.root}",
        "${runtime.version}",
        "${provider.id}",
    ] {
        assert_rejected(
            with_step(&command_step(&format!("echo {token}"))),
            "remedies[0].steps[0].command",
            IssueCode::TokenNotAllowedHere,
        );
    }
}

#[test]
fn every_token_of_the_remedy_set_is_allowed() {
    with_step(&command_step(
        "${runtime.executable} ${runtime.executables.psql} ${runtime.executables.initdb} --host=${endpoint.socket.socketDir} --port=${endpoint.client.port} ${instance.name} ${instance.data} ${instance.config}",
    ))
    .validate()
    .unwrap_or_else(|issues| panic!("{issues:#?}"));
}

#[test]
fn an_endpoint_or_alias_the_package_does_not_have_keeps_its_code() {
    assert_rejected(
        with_step(&command_step("echo ${endpoint.nope.port}")),
        "remedies[0].steps[0].command",
        IssueCode::UnknownEndpoint,
    );
    assert_rejected(
        with_step(&command_step("${runtime.executables.pgctl}")),
        "remedies[0].steps[0].command",
        IssueCode::UnknownExecutable,
    );
    assert_rejected(
        with_step(&command_step("echo ${endpoint.socket.port}")),
        "remedies[0].steps[0].command",
        IssueCode::TokenNotAllowedHere,
    );
    assert_rejected(
        with_step(&command_step("echo ${nothing.here}")),
        "remedies[0].steps[0].command",
        IssueCode::UnknownToken,
    );
}

#[test]
fn a_title_or_text_takes_no_token() {
    assert_rejected(
        with_step(&text_step("Look in ${instance.data}.")),
        "remedies[0].steps[0].text",
        IssueCode::TokenNotAllowedHere,
    );
    assert_rejected(
        Package::postgresql().with_remedies(
            &entry("fix", WHEN_EXCERPT, &text_step("Do."))
                .replace("title: Fix it", "title: \"Fix ${instance.name}\""),
        ),
        "remedies[0].title",
        IssueCode::TokenNotAllowedHere,
    );
}

/// One entry with `pid_file` as its `evidence.pidFile`.
fn with_pid_file(pid_file: &str) -> Package {
    Package::postgresql().with_remedies(&format!(
        "  - name: fix\n    when: {WHEN_EXCERPT}\n    evidence: {{ pidFile: {} }}\n    title: Fix it\n    steps:\n{}",
        yaml(pid_file),
        text_step("Do.")
    ))
}

#[test]
fn a_pid_file_must_be_a_file_under_the_data_or_config_folder() {
    for pid_file in [
        "${instance.data}/postmaster.pid",
        "${instance.config}/run/server.pid",
    ] {
        with_pid_file(pid_file)
            .validate()
            .unwrap_or_else(|issues| panic!("{pid_file}: {issues:#?}"));
    }
    let at = "remedies[0].evidence.pidFile";
    assert_rejected(
        with_pid_file("${instance.temp}/x.pid"),
        at,
        IssueCode::TokenNotAllowedHere,
    );
    assert_rejected(
        with_pid_file("${instance.data}/../x.pid"),
        at,
        IssueCode::PathEscapesScope,
    );
    assert_rejected(with_pid_file("/tmp/x.pid"), at, IssueCode::PathEscapesScope);
    assert_rejected(
        with_pid_file("${instance.data}"),
        at,
        IssueCode::InvalidValue,
    );
    assert_rejected(
        with_pid_file("${instance.data}/${instance.name}.pid"),
        at,
        IssueCode::TokenNotAllowedHere,
    );
    assert_rejected(
        with_pid_file("${instance.data}/${secret.superuserPassword}"),
        at,
        IssueCode::SecretNotAllowedHere,
    );
}

#[test]
fn a_field_the_block_does_not_know_is_unknown_field() {
    let issues = Package::postgresql()
        .with_remedies(&format!(
            "  - name: fix\n    when: {WHEN_EXCERPT}\n    title: Fix it\n    run: rm -f x\n    steps:\n{}",
            text_step("Do.")
        ))
        .issues();
    assert!(
        issues
            .iter()
            .any(|issue| issue.code == IssueCode::UnknownField),
        "{issues:#?}"
    );
}
