//! The manifest `connection` block (spec 0021 AC-1): each test adds a
//! block to a copy of `providers/fake` or the `fake-command` fixture held
//! in memory, then checks that it loads or which issue it gets, with its
//! location and reason.
#![cfg(unix)]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use stackly_core::provider::{
    CONNECTION_ENTRIES_MAX, Issue, IssueCode, NameCheck, PackageFiles, ValidatedPackage,
};

fn fake_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../providers/fake")
}

fn fake_command_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/packages/fake-command")
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

/// The start of the top-level `connection` block of `providers/fake`.
const CONNECTION_BLOCK: &[u8] = b"\nconnection:\n";

/// A package with extra manifest text appended at the top level.
struct Package {
    folder: &'static str,
    files: BTreeMap<String, Vec<u8>>,
}

impl Package {
    /// `providers/fake` without its own `connection` block (the last
    /// block of its manifest), so each test adds the block it checks: one
    /// tcp endpoint `http` and the secret field `authToken`.
    fn fake() -> Self {
        let mut files = files_of(&fake_dir());
        let manifest = files
            .get_mut("provider.yaml")
            .expect("the package has a manifest");
        let at = manifest
            .windows(CONNECTION_BLOCK.len())
            .position(|window| window == CONNECTION_BLOCK)
            .expect("providers/fake ends with a connection block");
        manifest.truncate(at + 1);
        Self {
            folder: "fake",
            files,
        }
    }

    /// The `fake-command` fixture: a tcp endpoint `http`, a unix endpoint
    /// `control` named after the http port, and the secret `authToken`.
    fn fake_command() -> Self {
        Self {
            folder: "fake-command",
            files: files_of(&fake_command_dir()),
        }
    }

    /// Appends a top-level `connection` block made of the given entries,
    /// each `(name, title, template)`.
    fn with_connection(self, entries: &[(&str, &str, &str)]) -> Self {
        let mut block = String::from("connection:\n");
        for (name, title, template) in entries {
            block.push_str(&format!(
                "  - name: {}\n    title: {}\n    template: {}\n",
                yaml(name),
                yaml(title),
                yaml(template)
            ));
        }
        self.append(&block)
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
            PackageFiles::new(self.folder, self.files),
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

/// Returns the one `provider.yaml` issue at `location`, failing when there
/// is none.
#[track_caller]
fn issue_at<'a>(issues: &'a [Issue], location: &str) -> &'a Issue {
    issues
        .iter()
        .find(|issue| issue.file == "provider.yaml" && issue.location.to_string() == location)
        .unwrap_or_else(|| panic!("expected an issue at {location}, got {issues:#?}"))
}

#[track_caller]
fn assert_rejected(
    package: Package,
    location: &str,
    code: IssueCode,
    reason: Option<&'static str>,
) {
    let issues = package.issues();
    let issue = issue_at(&issues, location);
    assert_eq!((issue.code, issue.reason), (code, reason), "{issue:#?}");
}

const FAKE_URI: &str = "http://stackly:${secret.authToken}@${endpoint.http.address}/";

#[test]
fn a_package_with_a_connection_block_loads_with_entries_in_manifest_order() {
    let package = Package::fake()
        .with_connection(&[
            ("uri", "Connection string", FAKE_URI),
            (
                "health",
                "Health URL",
                "http://${endpoint.http.address}/health?instance=${instance.name}&id=${instance.id}&provider=${provider.id}&revision=${provider.revision}&runtime=${runtime.version}",
            ),
        ])
        .validate()
        .expect("the connection block is valid");

    let entries: Vec<(&str, &str)> = package
        .manifest()
        .connections()
        .iter()
        .map(|entry| (entry.name(), entry.title()))
        .collect();
    assert_eq!(
        entries,
        [("uri", "Connection string"), ("health", "Health URL")]
    );
}

#[test]
fn a_package_without_a_connection_block_has_no_entries() {
    let package = Package::fake().validate().expect("the fake package loads");

    assert!(package.manifest().connections().is_empty());
}

#[test]
fn socket_path_and_socket_folder_tokens_are_encoded_into_a_valid_uri() {
    let package = Package::fake_command()
        .with_connection(&[(
            "socket",
            "Unix socket",
            "fake://user:${secret.authToken}@/db?socket=${endpoint.control.socket}&dir=${endpoint.control.socketDir}&port=${endpoint.http.port}",
        )])
        .validate();

    assert!(package.is_ok(), "{package:#?}");
}

#[test]
fn the_block_is_part_of_the_manifest_digest() {
    let plain = Package::fake().validate().expect("the fake package loads");
    let with_block = Package::fake()
        .with_connection(&[("uri", "Connection string", FAKE_URI)])
        .validate()
        .expect("the connection block is valid");

    assert_ne!(plain.key(), with_block.key());
}

#[test]
fn a_name_outside_the_pattern_is_connection_invalid_with_reason_name() {
    for name in ["Uri", "2uri", "my-uri", "", &"a".repeat(33)] {
        assert_rejected(
            Package::fake().with_connection(&[(name, "Connection string", FAKE_URI)]),
            "connection[0].name",
            IssueCode::ConnectionInvalid,
            Some("name"),
        );
    }
}

#[test]
fn an_empty_long_or_control_character_title_is_connection_invalid_with_reason_title() {
    for title in ["", &"t".repeat(65), "line\nbreak", "tab\there"] {
        assert_rejected(
            Package::fake().with_connection(&[("uri", title, FAKE_URI)]),
            "connection[0].title",
            IssueCode::ConnectionInvalid,
            Some("title"),
        );
    }
}

#[test]
fn a_title_of_64_characters_is_accepted() {
    let title = "é".repeat(64);
    let package = Package::fake()
        .with_connection(&[("uri", &title, FAKE_URI)])
        .validate();

    assert!(package.is_ok(), "{package:#?}");
}

#[test]
fn an_empty_or_oversized_template_is_connection_invalid_with_reason_template_length() {
    let long = format!("http://h/{}", "a".repeat(504));
    assert_eq!(long.len(), 513);
    for template in ["", long.as_str()] {
        assert_rejected(
            Package::fake().with_connection(&[("uri", "Connection string", template)]),
            "connection[0].template",
            IssueCode::ConnectionInvalid,
            Some("template_length"),
        );
    }
}

#[test]
fn a_duplicate_name_is_connection_name_duplicate_without_a_reason() {
    assert_rejected(
        Package::fake().with_connection(&[
            ("uri", "Connection string", FAKE_URI),
            ("uri", "Again", FAKE_URI),
        ]),
        "connection[1].name",
        IssueCode::ConnectionNameDuplicate,
        None,
    );
}

#[test]
fn tokens_outside_the_connection_template_row_are_token_not_allowed_here() {
    for template in [
        "http://${endpoint.http.host}:8080/",
        "http://h/?label=${settings.label}",
        "http://h/?missing=${settings.noSuchSetting}",
        "file://${instance.data}",
        "file://h/?root=${runtime.root}",
        "file://h/?line=${runtime.line}",
        "file://h/?key=${instance.shortKey}",
    ] {
        assert_rejected(
            Package::fake().with_connection(&[("uri", "Connection string", template)]),
            "connection[0].template",
            IssueCode::TokenNotAllowedHere,
            None,
        );
    }
}

#[test]
fn a_missing_endpoint_or_secret_is_unknown_endpoint_or_unknown_secret() {
    assert_rejected(
        Package::fake().with_connection(&[(
            "uri",
            "Connection string",
            "http://${endpoint.admin.address}/",
        )]),
        "connection[0].template",
        IssueCode::UnknownEndpoint,
        None,
    );
    assert_rejected(
        Package::fake().with_connection(&[(
            "uri",
            "Connection string",
            "http://u:${secret.password}@${endpoint.http.address}/",
        )]),
        "connection[0].template",
        IssueCode::UnknownSecret,
        None,
    );
}

#[test]
fn a_template_that_is_not_an_absolute_uri_with_sentinel_values_is_not_a_uri() {
    for template in [
        "${endpoint.http.address}",
        "//${endpoint.http.address}/",
        "http://h/a b",
        "http://h/#fragment",
    ] {
        assert_rejected(
            Package::fake().with_connection(&[("uri", "Connection string", template)]),
            "connection[0].template",
            IssueCode::ConnectionInvalid,
            Some("not_a_uri"),
        );
    }
}

#[test]
fn the_sentinel_render_also_tries_the_ipv6_loopback_host() {
    // With 127.0.0.1 this renders `x:127.0.0.1:15432`, a valid rootless
    // URI; with ::1 it renders `x:[::1]:15432`, where `[` is not allowed.
    assert_rejected(
        Package::fake().with_connection(&[(
            "uri",
            "Connection string",
            "x:${endpoint.http.address}",
        )]),
        "connection[0].template",
        IssueCode::ConnectionInvalid,
        Some("not_a_uri"),
    );
}

#[test]
fn an_empty_block_or_one_over_the_limit_is_rejected() {
    assert_rejected(
        Package::fake().append("connection: []\n"),
        "connection",
        IssueCode::InvalidValue,
        None,
    );
    let names: Vec<String> = (0..=CONNECTION_ENTRIES_MAX)
        .map(|n| format!("uri{n}"))
        .collect();
    let entries: Vec<(&str, &str, &str)> = names
        .iter()
        .map(|name| (name.as_str(), "Connection string", FAKE_URI))
        .collect();
    assert_rejected(
        Package::fake().with_connection(&entries),
        "connection",
        IssueCode::InvalidValue,
        None,
    );
}

#[test]
fn an_unknown_field_in_an_entry_is_rejected() {
    let issues = Package::fake()
        .append("connection:\n  - name: uri\n    title: URI\n    template: \"http://h/\"\n    encoding: none\n")
        .issues();

    assert!(
        issues
            .iter()
            .any(|issue| issue.code == IssueCode::UnknownField),
        "{issues:#?}"
    );
}
