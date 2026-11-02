//! The manifest `openWith` block (spec 0070 AC-1): each test adds a block
//! to a copy of `providers/fake` held in memory, then checks that it loads
//! or which issue it gets, with its location and reason.
#![cfg(unix)]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use stackly_core::provider::{
    Issue, IssueCode, NameCheck, OpenWithKind, PackageFiles, ValidatedPackage,
};

fn fake_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../providers/fake")
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

/// The start of the top-level `openWith` block of `providers/fake`.
const OPEN_WITH_BLOCK: &str = "\nopenWith:\n";

/// `providers/fake` in memory.
struct Package {
    files: BTreeMap<String, Vec<u8>>,
}

impl Package {
    /// `providers/fake` without its own `openWith` block (the last block of
    /// its manifest), so each test adds the block it checks: one tcp
    /// endpoint `http`, the secret field `authToken`, the sensitive config
    /// file `fakeConfig`, and the plain config file `fakeNotes`.
    fn fake() -> Self {
        let mut files = files_of(&fake_dir());
        let manifest = files
            .get_mut("provider.yaml")
            .expect("the package has a manifest");
        let text = String::from_utf8(manifest.clone()).expect("a UTF-8 manifest");
        let at = text
            .find(OPEN_WITH_BLOCK)
            .expect("providers/fake ends with an openWith block");
        manifest.truncate(at + 1);
        Self { files }
    }

    /// Appends a top-level `openWith` block written in YAML.
    fn with_open_with(self, block: &str) -> Self {
        self.append(&format!("openWith:\n{block}"))
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

    /// Replaces text in the manifest.
    fn replace(mut self, from: &str, to: &str) -> Self {
        let manifest = self
            .files
            .get_mut("provider.yaml")
            .expect("the package has a manifest");
        let text = String::from_utf8(manifest.clone()).expect("a UTF-8 manifest");
        assert!(text.contains(from), "the manifest holds {from}");
        *manifest = text.replacen(from, to, 1).into_bytes();
        self
    }

    fn validate(self) -> Result<ValidatedPackage, Vec<Issue>> {
        ValidatedPackage::from_files(PackageFiles::new("fake", self.files), NameCheck::FolderName)
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

/// One `app` entry named `client` with `url`.
fn app(url: &str) -> String {
    format!(
        "  - name: client\n    title: Client\n    kind: app\n    app:\n      bundleIds: [\"dev.stackly.fake-client\"]\n      url: {}\n",
        yaml(url)
    )
}

/// One `browser` entry named `page` with `url`.
fn browser(url: &str) -> String {
    format!(
        "  - name: page\n    title: Page\n    kind: browser\n    browser:\n      url: {}\n",
        yaml(url)
    )
}

/// One `terminal` entry named `cli` with the executable and the extra
/// lines of its table (args, env).
fn terminal(executable: &str, extra: &str) -> String {
    format!(
        "  - name: cli\n    title: CLI\n    kind: terminal\n    terminal:\n      executable: {}\n{extra}",
        yaml(executable)
    )
}

/// One `reveal` entry named `file` showing `config_file`.
fn reveal(config_file: &str) -> String {
    format!(
        "  - name: file\n    title: File\n    kind: reveal\n    reveal:\n      configFile: {config_file}\n"
    )
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

const URL: &str = "http://stackly@${endpoint.http.address}/";

#[test]
fn a_package_with_an_open_with_block_loads_with_entries_in_manifest_order() {
    let block = format!(
        "{}{}{}{}",
        app(URL),
        terminal("${runtime.executable}", "      args: [\"version\"]\n"),
        browser("http://${endpoint.http.address}/health"),
        reveal("fakeNotes")
    );
    let package = Package::fake()
        .with_open_with(&block)
        .validate()
        .unwrap_or_else(|issues| panic!("{issues:#?}"));

    let entries: Vec<(&str, &str, OpenWithKind)> = package
        .manifest()
        .open_with()
        .iter()
        .map(|entry| (entry.name(), entry.title(), entry.kind()))
        .collect();
    assert_eq!(
        entries,
        [
            ("client", "Client", OpenWithKind::App),
            ("cli", "CLI", OpenWithKind::Terminal),
            ("page", "Page", OpenWithKind::Browser),
            ("file", "File", OpenWithKind::Reveal),
        ]
    );
}

#[test]
fn a_package_without_an_open_with_block_has_no_entries() {
    let package = Package::fake()
        .validate()
        .unwrap_or_else(|issues| panic!("{issues:#?}"));
    assert!(package.manifest().open_with().is_empty());
}

#[test]
fn the_block_is_part_of_the_manifest_digest() {
    let without = Package::fake().validate().expect("valid without the block");
    let with = Package::fake()
        .with_open_with(&app(URL))
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
        Package::fake().append("openWith: []\n"),
        "openWith",
        IssueCode::InvalidValue,
        None,
    );
    let mut block = String::new();
    for index in 0..9 {
        block.push_str(&app(URL).replace("name: client", &format!("name: client{index}")));
    }
    assert_rejected(
        Package::fake().with_open_with(&block),
        "openWith",
        IssueCode::InvalidValue,
        None,
    );
}

#[test]
fn a_name_outside_the_pattern_is_open_with_invalid_with_reason_name() {
    for name in ["Client", "2client", "my-client", &"a".repeat(33)] {
        assert_rejected(
            Package::fake().with_open_with(
                &app(URL).replace("name: client", &format!("name: {}", yaml(name))),
            ),
            "openWith[0].name",
            IssueCode::OpenWithInvalid,
            Some("name"),
        );
    }
}

#[test]
fn a_duplicate_name_is_open_with_name_duplicate_without_a_reason() {
    assert_rejected(
        Package::fake().with_open_with(&format!("{}{}", app(URL), app(URL))),
        "openWith[1].name",
        IssueCode::OpenWithNameDuplicate,
        None,
    );
}

#[test]
fn an_empty_long_or_control_character_title_is_open_with_invalid_with_reason_title() {
    for title in ["", &"t".repeat(49), "Tab\there"] {
        assert_rejected(
            Package::fake().with_open_with(
                &app(URL).replace("title: Client", &format!("title: {}", yaml(title))),
            ),
            "openWith[0].title",
            IssueCode::OpenWithInvalid,
            Some("title"),
        );
    }
    Package::fake()
        .with_open_with(&app(URL).replace("title: Client", &format!("title: {}", "t".repeat(48))))
        .validate()
        .expect("a title of 48 characters is accepted");
}

#[test]
fn an_unknown_kind_or_a_table_that_does_not_match_the_kind_is_open_with_invalid() {
    assert_rejected(
        Package::fake().with_open_with(&app(URL).replace("kind: app", "kind: shell")),
        "openWith[0].kind",
        IssueCode::OpenWithInvalid,
        Some("kind"),
    );
    assert_rejected(
        Package::fake().with_open_with(&app(URL).replace("kind: app", "kind: browser")),
        "openWith[0]",
        IssueCode::OpenWithInvalid,
        Some("kind_table"),
    );
    assert_rejected(
        Package::fake().with_open_with(&format!(
            "{}    browser:\n      url: {}\n",
            app(URL),
            yaml(URL)
        )),
        "openWith[0]",
        IssueCode::OpenWithInvalid,
        Some("kind_table"),
    );
}

#[test]
fn secret_settings_and_endpoint_host_tokens_are_token_not_allowed_here_in_every_position() {
    for token in [
        "${secret.authToken}",
        "${settings.label}",
        "${endpoint.http.host}",
        "${runtime.executable}",
        "${instance.data}",
    ] {
        assert_rejected(
            Package::fake()
                .with_open_with(&app(&format!("http://{token}@${{endpoint.http.address}}/"))),
            "openWith[0].app.url",
            IssueCode::TokenNotAllowedHere,
            None,
        );
        assert_rejected(
            Package::fake().with_open_with(&terminal(
                "${runtime.executable}",
                &format!("      args: [{}]\n", yaml(&format!("--x={token}"))),
            )),
            "openWith[0].terminal.args[0]",
            IssueCode::TokenNotAllowedHere,
            None,
        );
        assert_rejected(
            Package::fake().with_open_with(&terminal(
                "${runtime.executable}",
                &format!("      env: {{ FAKE_X: {} }}\n", yaml(token)),
            )),
            "openWith[0].terminal.env.FAKE_X",
            IssueCode::TokenNotAllowedHere,
            None,
        );
    }
}

#[test]
fn a_missing_endpoint_is_unknown_endpoint() {
    assert_rejected(
        Package::fake().with_open_with(&app("http://${endpoint.nope.address}/")),
        "openWith[0].app.url",
        IssueCode::UnknownEndpoint,
        None,
    );
}

#[test]
fn copy_secret_names_a_secret_field_and_only_app_and_terminal_take_it() {
    for field in ["nope", "label"] {
        assert_rejected(
            Package::fake().with_open_with(&format!("{}    copySecret: {field}\n", app(URL))),
            "openWith[0].copySecret",
            IssueCode::UnknownSecret,
            None,
        );
    }
    assert_rejected(
        Package::fake().with_open_with(&format!(
            "{}    copySecret: authToken\n",
            browser("http://${endpoint.http.address}/")
        )),
        "openWith[0].copySecret",
        IssueCode::OpenWithInvalid,
        Some("copy_secret_not_allowed"),
    );
    assert_rejected(
        Package::fake().with_open_with(&format!(
            "{}    copySecret: authToken\n",
            reveal("fakeNotes")
        )),
        "openWith[0].copySecret",
        IssueCode::OpenWithInvalid,
        Some("copy_secret_not_allowed"),
    );
    Package::fake()
        .with_open_with(&format!("{}    copySecret: authToken\n", app(URL)))
        .validate()
        .expect("copySecret on an app entry is accepted");
}

#[test]
fn bundle_ids_hold_one_to_four_bundle_ids() {
    for list in [
        "[]",
        "[\"a.b\", \"c.d\", \"e.f\", \"g.h\", \"i.j\"]",
        "[\"TablePlus\"]",
        "[\"com.tiny app\"]",
    ] {
        assert_rejected(
            Package::fake()
                .with_open_with(&app(URL).replace("[\"dev.stackly.fake-client\"]", list)),
            "openWith[0].app.bundleIds",
            IssueCode::OpenWithInvalid,
            Some("bundle_id_invalid"),
        );
    }
}

#[test]
fn an_app_url_that_is_not_an_absolute_uri_is_not_a_uri() {
    for url in ["${endpoint.http.address}", "http://h/a b", "http://h/#x"] {
        assert_rejected(
            Package::fake().with_open_with(&app(url)),
            "openWith[0].app.url",
            IssueCode::OpenWithInvalid,
            Some("not_a_uri"),
        );
    }
    assert_rejected(
        Package::fake().with_open_with(&app(&"x".repeat(513))),
        "openWith[0].app.url",
        IssueCode::OpenWithInvalid,
        Some("url_length"),
    );
}

#[test]
fn a_browser_url_must_be_http_or_https() {
    for url in ["file:///etc/hosts", "nats://${endpoint.http.address}"] {
        assert_rejected(
            Package::fake().with_open_with(&browser(url)),
            "openWith[0].browser.url",
            IssueCode::OpenWithInvalid,
            Some("not_http"),
        );
    }
    Package::fake()
        .with_open_with(&browser("https://${endpoint.http.address}/"))
        .validate()
        .expect("https is accepted");
}

#[test]
fn a_terminal_executable_is_exactly_one_runtime_executable_token() {
    for executable in [
        "/usr/bin/psql",
        "${instance.name}",
        "${runtime.executable} x",
    ] {
        assert_rejected(
            Package::fake().with_open_with(&terminal(executable, "")),
            "openWith[0].terminal.executable",
            IssueCode::OpenWithInvalid,
            Some("executable_not_runtime"),
        );
    }
    assert_rejected(
        Package::fake().with_open_with(&terminal("${runtime.executables.nope}", "")),
        "openWith[0].terminal.executable",
        IssueCode::UnknownExecutable,
        None,
    );
}

#[test]
fn too_many_or_too_long_args_and_env_are_too_long() {
    let args: Vec<String> = (0..33).map(|index| yaml(&format!("a{index}"))).collect();
    assert_rejected(
        Package::fake().with_open_with(&terminal(
            "${runtime.executable}",
            &format!("      args: [{}]\n", args.join(", ")),
        )),
        "openWith[0].terminal.args",
        IssueCode::OpenWithInvalid,
        Some("too_long"),
    );
    assert_rejected(
        Package::fake().with_open_with(&terminal(
            "${runtime.executable}",
            &format!("      args: [{}]\n", yaml(&"a".repeat(1025))),
        )),
        "openWith[0].terminal.args[0]",
        IssueCode::OpenWithInvalid,
        Some("too_long"),
    );
    let env: Vec<String> = (0..17).map(|index| format!("FAKE_{index}: x")).collect();
    assert_rejected(
        Package::fake().with_open_with(&terminal(
            "${runtime.executable}",
            &format!("      env: {{ {} }}\n", env.join(", ")),
        )),
        "openWith[0].terminal.env",
        IssueCode::OpenWithInvalid,
        Some("too_long"),
    );
    assert_rejected(
        Package::fake().with_open_with(&terminal(
            "${runtime.executable}",
            &format!("      env: {{ FAKE_X: {} }}\n", yaml(&"v".repeat(1025))),
        )),
        "openWith[0].terminal.env.FAKE_X",
        IssueCode::OpenWithInvalid,
        Some("too_long"),
    );
}

#[test]
fn env_names_follow_the_env_rule_of_spec_0009() {
    for name in ["PATH", "HOME", "TMPDIR", "lower", "DYLD_X"] {
        assert_rejected(
            Package::fake().with_open_with(&terminal(
                "${runtime.executable}",
                &format!("      env: {{ {name}: x }}\n"),
            )),
            &format!("openWith[0].terminal.env.{name}"),
            IssueCode::OpenWithInvalid,
            Some("env_name_invalid"),
        );
    }
}

#[test]
fn a_terminal_entry_needs_the_temp_folder_in_managed_filesystem() {
    let package = Package::fake()
        .replace(
            "managedFilesystem: [data, config, logs, temp, run]",
            "managedFilesystem: [data, config, logs, run]",
        )
        .with_open_with(&terminal("${runtime.executable}", ""));
    assert_rejected(
        package,
        "openWith[0].terminal",
        IssueCode::PathNotPermitted,
        None,
    );
}

#[test]
fn a_reveal_entry_names_a_config_file_that_is_not_sensitive() {
    assert_rejected(
        Package::fake().with_open_with(&reveal("nope")),
        "openWith[0].reveal.configFile",
        IssueCode::OpenWithInvalid,
        Some("unknown_config_file"),
    );
    assert_rejected(
        Package::fake().with_open_with(&reveal("fakeConfig")),
        "openWith[0].reveal.configFile",
        IssueCode::OpenWithInvalid,
        Some("config_file_sensitive"),
    );
}
