//! Validation rules of the provider engine (spec 0009): each test changes
//! one thing in a copy of `providers/fake` or the `fake-command` fixture
//! held in memory, then checks the issue code and where it points.
#![cfg(unix)]

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use stackly_core::provider::{
    EnvValue, InitStep, IssueCode, Location, NameCheck, PackageError, PackageFiles, PlanContext,
    PlanError, ResolvedEndpoint, RuntimeContext, SettingClass, Stdin, ValidatedPackage,
    read_package,
};
use stackly_core::secret::ResolvedSecrets;
use stackly_domain::{EndpointName, InstanceId, Port};
use stackly_evidence::{Kind, evidence};
use stackly_platform::{DirectoryLayout, RootSet, SecretValue};
use stackly_testkit::{FakePlatform, TestRoot, make_fifo};

/// The secret reference of the plan contexts here, in the daemon's form for
/// their instance (spec 0011).
const SECRET_REF: &str = "local:instance/0192f000-0000-7000-8000-00000000000b/auth-token";

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

/// A package being changed for one test.
struct Edit {
    folder: String,
    files: BTreeMap<String, Vec<u8>>,
}

impl Edit {
    fn fake() -> Self {
        Self {
            folder: "fake".into(),
            files: files_of(&fake_dir()),
        }
    }

    fn fake_command() -> Self {
        Self {
            folder: "fake-command".into(),
            files: files_of(&fake_command_dir()),
        }
    }

    fn text(&self, path: &str) -> String {
        String::from_utf8(self.files[path].clone()).expect("UTF-8 file")
    }

    /// Replaces `from` with `to` in a file, insisting that `from` exists.
    fn replace(mut self, path: &str, from: &str, to: &str) -> Self {
        let text = self.text(path);
        assert!(text.contains(from), "{path} does not contain {from:?}");
        self.files
            .insert(path.to_owned(), text.replacen(from, to, 1).into_bytes());
        self
    }

    fn set(mut self, path: &str, bytes: impl Into<Vec<u8>>) -> Self {
        self.files.insert(path.to_owned(), bytes.into());
        self
    }

    fn remove(mut self, path: &str) -> Self {
        self.files.remove(path);
        self
    }

    fn validate_as(self, check: NameCheck) -> Result<ValidatedPackage, PackageError> {
        ValidatedPackage::from_files(PackageFiles::new(self.folder, self.files), check)
    }

    fn validate(self) -> Result<ValidatedPackage, PackageError> {
        self.validate_as(NameCheck::FolderName)
    }

    /// Validates and returns the issues, failing when the package passes.
    #[track_caller]
    fn issues(self) -> Vec<(String, String, IssueCode)> {
        match self.validate() {
            Ok(_) => panic!("the package should be rejected"),
            Err(err) => err
                .issues
                .into_iter()
                .map(|issue| (issue.file, issue.location.to_string(), issue.code))
                .collect(),
        }
    }
}

/// Asserts that the issues hold exactly one with `code` at `location` in
/// `provider.yaml`.
#[track_caller]
fn assert_issue(
    issues: &[(String, String, IssueCode)],
    file: &str,
    location: &str,
    code: IssueCode,
) {
    assert!(
        issues
            .iter()
            .any(|(f, l, c)| f == file && l == location && *c == code),
        "expected {file} {location} {code}, got {issues:#?}"
    );
}

#[track_caller]
fn assert_manifest_issue(edit: Edit, location: &str, code: IssueCode) {
    assert_issue(&edit.issues(), "provider.yaml", location, code);
}

#[test]
fn both_packages_pass_unchanged() {
    assert!(Edit::fake().validate().is_ok());
    assert!(Edit::fake_command().validate().is_ok());
}

#[test]
fn closed_sets_reject_unknown_primitives_probes_formats_signals_and_strategies() {
    let ev = evidence!(
        covers = ["REQ-029"],
        kind = Kind::Fixture,
        expected = "a manifest with an unknown primitive, probe (script or native), template format, signal, or stop strategy is rejected at validation with its code and location, never at start",
    );
    let yaml = "provider.yaml";
    assert_manifest_issue(
        Edit::fake().replace(
            yaml,
            "primitive: createManagedDirectories",
            "primitive: runShell",
        ),
        "lifecycle.initialize[0].primitive",
        IssueCode::UnknownPrimitive,
    );
    assert_manifest_issue(
        Edit::fake().replace(
            yaml,
            "{ type: http, endpoint: http, path: /ready }",
            "{ type: script, endpoint: http }",
        ),
        "readiness.probe.type",
        IssueCode::UnknownProbe,
    );
    assert_manifest_issue(
        Edit::fake().replace(
            yaml,
            "{ type: http, endpoint: http, path: /ready }",
            "{ type: native, endpoint: http }",
        ),
        "readiness.probe.type",
        IssueCode::UnknownProbe,
    );
    assert_manifest_issue(
        Edit::fake().replace(yaml, "format: json", "format: ini"),
        "configFiles.fakeConfig.format",
        IssueCode::UnknownTemplateFormat,
    );
    assert_manifest_issue(
        Edit::fake().replace(yaml, "signal: term", "signal: kill"),
        "lifecycle.stop.signal",
        IssueCode::InvalidValue,
    );
    assert_manifest_issue(
        Edit::fake().replace(yaml, "strategy: gracefulProcess", "strategy: kill"),
        "lifecycle.stop.strategy",
        IssueCode::InvalidValue,
    );
    ev.pass();
}

#[test]
fn unknown_security_fields_are_rejected_with_their_path() {
    let ev = evidence!(
        covers = ["VAL-018"],
        kind = Kind::Fixture,
        expected = "unknown security fields such as permissions.network, permissions.sandbox, and lifecycle.start.shell are rejected with unknown_field and their path",
    );
    let yaml = "provider.yaml";
    assert_manifest_issue(
        Edit::fake().replace(
            yaml,
            "  executeRuntimeBinary: true",
            "  executeRuntimeBinary: true\n  network: true",
        ),
        "permissions.network",
        IssueCode::UnknownField,
    );
    assert_manifest_issue(
        Edit::fake().replace(
            yaml,
            "  executeRuntimeBinary: true",
            "  executeRuntimeBinary: true\n  sandbox: false",
        ),
        "permissions.sandbox",
        IssueCode::UnknownField,
    );
    assert_manifest_issue(
        Edit::fake().replace(
            yaml,
            "    cwd: \"${instance.data}\"",
            "    cwd: \"${instance.data}\"\n    shell: true",
        ),
        "lifecycle.start.shell",
        IssueCode::UnknownField,
    );
    ev.pass();
}

#[test]
fn standard_yaml_tags_give_the_untagged_value_and_still_meet_the_typed_decode() {
    let ev = evidence!(
        covers = ["VAL-018"],
        kind = Kind::Fixture,
        expected = "standard !! and verbatim yaml tags give the same value as the untagged scalar and still pass the typed decode, so !!str on a port is rejected, while a local tag stays rejected",
    );
    let yaml = "provider.yaml";
    let plain = Edit::fake().validate().expect("fake validates");
    let port = |package: &ValidatedPackage| {
        package
            .endpoints()
            .next()
            .and_then(|endpoint| endpoint.default_port())
            .map(Port::get)
    };

    for tagged in [
        "displayName: !!str Fake Provider",
        "displayName: !<tag:yaml.org,2002:str> Fake Provider",
    ] {
        let package = Edit::fake()
            .replace(yaml, "displayName: Fake Provider", tagged)
            .validate()
            .unwrap_or_else(|err| panic!("{tagged}: {:#?}", err.issues));
        assert_eq!(package.manifest().display_name(), "Fake Provider");
        assert_eq!(
            package.manifest().display_name(),
            plain.manifest().display_name()
        );
    }
    let int = Edit::fake()
        .replace(yaml, "defaultPort: 18080", "defaultPort: !!int \"18080\"")
        .validate()
        .unwrap_or_else(|err| panic!("{:#?}", err.issues));
    assert_eq!(port(&int), Some(18080));
    assert_eq!(port(&int), port(&plain));

    assert_manifest_issue(
        Edit::fake().replace(yaml, "defaultPort: 18080", "defaultPort: !!str 18080"),
        "resources.endpoints.http.defaultPort",
        IssueCode::InvalidValue,
    );
    assert_manifest_issue(
        Edit::fake().replace(
            yaml,
            "displayName: Fake Provider",
            "displayName: !custom Fake Provider",
        ),
        "metadata.displayName",
        IssueCode::InvalidValue,
    );
    ev.pass();
}

#[test]
fn another_api_version_or_kind_stops_before_the_rest_is_read() {
    let ev = evidence!(
        covers = ["VAL-018"],
        kind = Kind::Fixture,
        expected = "an apiVersion other than stackly/v1alpha1 or a kind other than Provider stops validation before the rest of provider.yaml is decoded",
    );
    let version = Edit::fake()
        .replace(
            "provider.yaml",
            "apiVersion: stackly/v1alpha1",
            "apiVersion: stackly/v1beta1",
        )
        .replace(
            "provider.yaml",
            "  executeRuntimeBinary: true",
            "  executeRuntimeBinary: true\n  network: true",
        )
        .issues();
    let kind = Edit::fake()
        .replace("provider.yaml", "kind: Provider", "kind: Plugin")
        .issues();

    assert_eq!(
        version,
        [(
            "provider.yaml".into(),
            "apiVersion".into(),
            IssueCode::UnsupportedApiVersion
        )]
    );
    assert_eq!(
        kind,
        [(
            "provider.yaml".into(),
            "kind".into(),
            IssueCode::UnknownKind
        )]
    );
    ev.pass();
}

#[test]
fn capabilities_that_cannot_be_fulfilled_are_rejected() {
    let yaml = "provider.yaml";
    let reload_block = "  reload:\n    render: [fakeConfig]\n    action: signal\n    signal: hup\n    verify: readiness\n";
    assert_manifest_issue(
        Edit::fake().replace(yaml, reload_block, ""),
        "capabilities.reload",
        IssueCode::CapabilityUnsatisfiable,
    );
    assert_manifest_issue(
        Edit::fake().replace(yaml, "  reload: true\n", ""),
        "lifecycle.reload",
        IssueCode::ReloadWithoutCapability,
    );
    assert_manifest_issue(
        Edit::fake().replace(yaml, "  reload: true\n", ""),
        "timeouts.reloadSeconds",
        IssueCode::ReloadWithoutCapability,
    );
    assert_manifest_issue(
        Edit::fake().replace(
            yaml,
            "  reload: true\n",
            "  reload: true\n  metrics: true\n",
        ),
        "capabilities.metrics",
        IssueCode::CapabilityUnsatisfiable,
    );
    assert_manifest_issue(
        Edit::fake().replace(yaml, "  reload: true\n", "  reload: true\n  tls: true\n"),
        "capabilities.tls",
        IssueCode::CapabilityUnsatisfiable,
    );
    assert_manifest_issue(
        Edit::fake().replace(yaml, "backup: [cold]", "backup: [cold, online]"),
        "capabilities.backup[1]",
        IssueCode::CapabilityUnsatisfiable,
    );
    assert_manifest_issue(
        Edit::fake().replace(yaml, "  persistence: true\n", ""),
        "capabilities.backup[0]",
        IssueCode::CapabilityUnsatisfiable,
    );
    let reload_setting = Edit::fake()
        .replace(yaml, "  reload: true\n", "")
        .replace(yaml, reload_block, "")
        .replace(yaml, "  reloadSeconds: 10\n", "")
        .issues();
    assert_issue(
        &reload_setting,
        "settings.schema.json",
        "properties.greeting.x-stackly.apply",
        IssueCode::CapabilityUnsatisfiable,
    );
}

#[test]
fn interpolation_rejects_shells_secrets_flags_escapes_and_unpermitted_folders() {
    let ev = evidence!(
        covers = ["REQ-031", "VAL-019"],
        kind = Kind::Fixture,
        expected = "interpolation rejects literal and shell executables, secrets outside stdin, flags from free settings, path escapes, settings in paths, unpermitted folders, unknown tokens, and loader env keys",
    );
    let yaml = "provider.yaml";
    let start = "    executable: \"${runtime.executable}\"\n    args:\n      - serve";
    assert_manifest_issue(
        Edit::fake().replace(
            yaml,
            start,
            "    executable: /bin/sh\n    args:\n      - serve",
        ),
        "lifecycle.start.executable",
        IssueCode::LiteralExecutable,
    );
    assert_manifest_issue(
        Edit::fake().replace(
            yaml,
            start,
            "    executable: \"${runtime.executable} -c\"\n    args:\n      - serve",
        ),
        "lifecycle.start.executable",
        IssueCode::LiteralExecutable,
    );
    assert_manifest_issue(
        Edit::fake().replace(
            yaml,
            "      - serve\n",
            "      - serve\n      - \"--token=${secret.authToken}\"\n",
        ),
        "lifecycle.start.args[1]",
        IssueCode::SecretNotAllowedHere,
    );
    assert_manifest_issue(
        Edit::fake().replace(
            yaml,
            "    cwd: \"${instance.data}\"",
            "    cwd: \"${instance.data}\"\n    env:\n      TOKEN: \"--pass=${secret.authToken}\"",
        ),
        "lifecycle.start.env.TOKEN",
        IssueCode::SecretNotAllowedHere,
    );
    assert_manifest_issue(
        Edit::fake().replace(
            yaml,
            "      - serve\n",
            "      - serve\n      - \"${settings.label}\"\n",
        ),
        "lifecycle.start.args[1]",
        IssueCode::ArgumentInjectionRisk,
    );
    assert_manifest_issue(
        Edit::fake().replace(
            yaml,
            "    cwd: \"${instance.data}\"",
            "    cwd: \"${instance.data}/../config\"",
        ),
        "lifecycle.start.cwd",
        IssueCode::PathEscapesScope,
    );
    assert_manifest_issue(
        Edit::fake().replace(
            yaml,
            "    cwd: \"${instance.data}\"",
            "    cwd: \"${instance.data}/${settings.label}\"",
        ),
        "lifecycle.start.cwd",
        IssueCode::TokenNotAllowedHere,
    );
    assert_manifest_issue(
        Edit::fake()
            .replace(
                yaml,
                "destination: \"${instance.secrets}/fake.json\"",
                "destination: \"${instance.logs}/x\"",
            )
            .replace(
                yaml,
                "managedFilesystem: [data, config, logs, temp, run]",
                "managedFilesystem: [data, config, temp, run]",
            ),
        "configFiles.fakeConfig.destination",
        IssueCode::PathNotPermitted,
    );
    assert_manifest_issue(
        Edit::fake()
            .replace(
                yaml,
                "managedFilesystem: [data, config, logs, temp, run]",
                "managedFilesystem: [data, config, logs, run]",
            )
            .replace(
                yaml,
                "        - \"--data=${instance.data}\"\n",
                "        - \"--data=${instance.temp}\"\n",
            ),
        "lifecycle.initialize[2].args[1]",
        IssueCode::PathNotPermitted,
    );
    assert_manifest_issue(
        Edit::fake().replace(
            yaml,
            "      - serve\n",
            "      - serve\n      - \"${unknown.x}\"\n",
        ),
        "lifecycle.start.args[1]",
        IssueCode::UnknownToken,
    );
    assert_manifest_issue(
        Edit::fake().replace(
            yaml,
            "    cwd: \"${instance.data}\"",
            "    cwd: \"${instance.data}\"\n    env:\n      DYLD_INSERT_LIBRARIES: x",
        ),
        "lifecycle.start.env.DYLD_INSERT_LIBRARIES",
        IssueCode::InvalidName,
    );
    ev.pass();
}

#[test]
fn a_whole_secret_env_value_is_accepted_only_where_secrets_resolve() {
    let ev = evidence!(
        covers = ["REQ-031"],
        kind = Kind::Fixture,
        expected = "a secret as a whole env value is accepted in lifecycle.start and runExecutable and becomes EnvValue::Secret, and is rejected in an exec probe, a stop command, and a reload run",
    );
    let yaml = "provider.yaml";
    let package = Edit::fake()
        .replace(
            yaml,
            "    cwd: \"${instance.data}\"",
            "    cwd: \"${instance.data}\"\n    env:\n      FAKE_TOKEN: \"${secret.authToken}\"",
        )
        .replace(
            yaml,
            "      stdin: { secret: authToken }",
            "      stdin: { secret: authToken }\n      env:\n        FAKE_TOKEN: \"${secret.authToken}\"",
        )
        .validate()
        .unwrap_or_else(|err| panic!("{:#?}", err.issues));
    let context = context_for(
        &package,
        serde_json::json!({ "authToken": SECRET_REF }),
        "/Volumes/T",
    );
    let token = |env: &[(String, EnvValue)]| {
        env.iter()
            .find(|(key, _)| key == "FAKE_TOKEN")
            .map(|(_, value)| value.clone())
    };

    let run = package.run_plan(&context).expect("the run plan builds");
    let init = package.init_plan(&context).expect("the init plan builds");

    assert!(
        matches!(token(&run.spawn.env), Some(EnvValue::Secret(secret)) if secret.reference.as_str() == SECRET_REF)
    );
    let init_run = init
        .steps
        .iter()
        .find_map(|step| match step {
            InitStep::Run(run) => Some(run),
            _ => None,
        })
        .expect("fake runs init");
    assert!(matches!(
        token(&init_run.command.env),
        Some(EnvValue::Secret(_))
    ));

    let env = "\n      env:\n        FAKE_TOKEN: \"${secret.authToken}\"";
    assert_manifest_issue(
        Edit::fake_command().replace(
            yaml,
            "      args: [version]\n  failureThreshold",
            &format!("      args: [version]{env}\n  failureThreshold"),
        ),
        "health.probe.command.env.FAKE_TOKEN",
        IssueCode::SecretNotAllowedHere,
    );
    assert_manifest_issue(
        Edit::fake_command().replace(
            yaml,
            "      args: [shutdown, \"--port=${endpoint.http.port}\"]",
            &format!("      args: [shutdown, \"--port=${{endpoint.http.port}}\"]{env}"),
        ),
        "lifecycle.stop.command.env.FAKE_TOKEN",
        IssueCode::SecretNotAllowedHere,
    );
    assert_manifest_issue(
        Edit::fake_command().replace(
            yaml,
            "      args: [version]\n    verify: none",
            &format!("      args: [version]{env}\n    verify: none"),
        ),
        "lifecycle.reload.command.env.FAKE_TOKEN",
        IssueCode::SecretNotAllowedHere,
    );
    ev.pass();
}

/// The literal stdin line of the `fake-command` init step, as written in
/// its `provider.yaml`.
const STDIN_TEXT_LINE: &str =
    r#"text: "first line ü 空\n\tsecond line with $(touch pwned) and 'quotes'\n""#;

#[test]
fn stdin_text_is_literal_one_key_bounded_and_free_of_tokens_and_most_control_characters() {
    let yaml = "provider.yaml";
    let at = "lifecycle.initialize[4].stdin";
    let with_text =
        |text: &str| Edit::fake_command().replace(yaml, STDIN_TEXT_LINE, &format!("text: {text}"));
    assert!(with_text(r#""a\tb\nc""#).validate().is_ok());
    assert_manifest_issue(
        with_text(r#""home is ${HOME}""#),
        &format!("{at}.text"),
        IssueCode::TokenNotAllowedHere,
    );
    assert_manifest_issue(
        with_text(r#""""#),
        &format!("{at}.text"),
        IssueCode::InvalidValue,
    );
    assert_manifest_issue(
        with_text(&format!("\"{}\"", "x".repeat(16 * 1024 + 1))),
        &format!("{at}.text"),
        IssueCode::InvalidValue,
    );
    assert!(
        with_text(&format!("\"{}\"", "x".repeat(16 * 1024)))
            .validate()
            .is_ok()
    );
    for control in [r"\r", r"\u0000", r"\u007f", r"\u0085"] {
        assert_manifest_issue(
            with_text(&format!("\"a{control}b\"")),
            &format!("{at}.text"),
            IssueCode::InvalidValue,
        );
    }
    assert_manifest_issue(
        Edit::fake_command().replace(
            yaml,
            STDIN_TEXT_LINE,
            &format!("{STDIN_TEXT_LINE}\n        secret: authToken"),
        ),
        at,
        IssueCode::InvalidValue,
    );
    assert_manifest_issue(
        Edit::fake_command().replace(
            yaml,
            &format!("stdin:\n        {STDIN_TEXT_LINE}"),
            "stdin: {}",
        ),
        at,
        IssueCode::InvalidValue,
    );
    assert_manifest_issue(
        Edit::fake_command().replace(
            yaml,
            "      args: [version]\n  failureThreshold",
            "      args: [version]\n      stdin: { text: x }\n  failureThreshold",
        ),
        "health.probe.command.stdin",
        IssueCode::UnknownField,
    );
}

#[test]
fn stdin_text_reaches_the_plan_exactly_and_changes_the_digest() {
    let package = Edit::fake_command()
        .validate()
        .expect("the fixture is valid");
    let init = package
        .init_plan(&context_for(
            &package,
            serde_json::json!({ "authToken": SECRET_REF }),
            "/Volumes/T",
        ))
        .expect("the init plan builds");
    let run = init
        .steps
        .iter()
        .find_map(|step| match step {
            InitStep::Run(run) => Some(run),
            _ => None,
        })
        .expect("the init step runs");

    assert_eq!(
        run.stdin,
        Stdin::Text("first line ü 空\n\tsecond line with $(touch pwned) and 'quotes'\n".into())
    );
    assert!(format!("{:?}", run.stdin).contains("second line"));

    let changed = Edit::fake_command()
        .replace("provider.yaml", "second line with", "second line wiTh")
        .validate()
        .expect("still valid");
    assert_ne!(
        package.key().manifest_sha256(),
        changed.key().manifest_sha256()
    );
}

#[test]
fn materialize_secret_takes_one_file_name_for_the_secrets_folder() {
    let yaml = "provider.yaml";
    let destination = "destination: token }";
    for (text, code) in [
        (
            "destination: \"${instance.temp}/token\" }",
            IssueCode::TokenNotAllowedHere,
        ),
        (
            "destination: \"${instance.run}/token\" }",
            IssueCode::TokenNotAllowedHere,
        ),
        ("destination: secrets/token }", IssueCode::InvalidName),
        ("destination: \"..\" }", IssueCode::PathEscapesScope),
    ] {
        assert_manifest_issue(
            Edit::fake_command().replace(yaml, destination, text),
            "lifecycle.initialize[1].destination",
            code,
        );
    }
    assert_manifest_issue(
        Edit::fake_command().replace(
            yaml,
            "managedFilesystem: [data, config, logs, temp, run]",
            "managedFilesystem: [data, config, logs, temp]",
        ),
        "lifecycle.initialize[1].destination",
        IssueCode::PathNotPermitted,
    );
}

#[test]
fn the_secrets_folder_token_is_accepted_only_in_its_positions() {
    let yaml = "provider.yaml";
    let schema = "schemas/settings.schema.json";
    let template = "templates/fake.json.tmpl";
    let name_line = "  \"instanceName\": {{ instance.name }},";
    let secrets_line =
        "  \"instanceName\": {{ instance.name }},\n  \"secrets\": {{ instance.secrets }},";
    Edit::fake()
        .replace(template, name_line, secrets_line)
        .validate()
        .unwrap_or_else(|err| panic!("a template may name the secrets folder: {:#?}", err.issues));
    // A sensitive config of `fake` cannot stay in the model without `run`
    // (its destination is in the secrets folder), so the template position
    // is checked on a plain copy of it in the config folder.
    let without_run = Edit::fake()
        .replace(
            yaml,
            "destination: \"${instance.secrets}/fake.json\"",
            "destination: \"${instance.config}/fake.json\"",
        )
        .replace(yaml, "    sensitive: true\n", "")
        .replace(
            yaml,
            "\"--config=${instance.secrets}/fake.json\"",
            "\"--config=${instance.config}/fake.json\"",
        )
        .replace(
            template,
            "  \"authToken\": {{ secret.authToken }}",
            "  \"secrets\": {{ instance.secrets }}",
        )
        .replace(
            yaml,
            "managedFilesystem: [data, config, logs, temp, run]",
            "managedFilesystem: [data, config, logs, temp]",
        )
        .issues();
    assert!(
        without_run
            .iter()
            .any(|(file, _, code)| file == template && *code == IssueCode::PathNotPermitted),
        "{without_run:#?}"
    );
    assert_manifest_issue(
        Edit::fake().replace(
            yaml,
            "    cwd: \"${instance.data}\"",
            "    cwd: \"${instance.secrets}\"",
        ),
        "lifecycle.start.cwd",
        IssueCode::TokenNotAllowedHere,
    );
    assert_manifest_issue(
        Edit::fake_command().replace(
            yaml,
            "files: [\"${instance.logs}/fake.log\"]",
            "files: [\"${instance.secrets}/fake.log\"]",
        ),
        "logs.files[0]",
        IssueCode::TokenNotAllowedHere,
    );
    assert_manifest_issue(
        Edit::fake_command().replace(
            yaml,
            "destination: \"${instance.config}/peers.yaml\"",
            "destination: \"${instance.secrets}/peers.yaml\"",
        ),
        "configFiles.peers.destination",
        IssueCode::TokenNotAllowedHere,
    );
    assert_issue(
        &Edit::fake_command()
            .replace(
                schema,
                "\"managed\": \"${instance.name}\"",
                "\"managed\": \"${instance.secrets}\"",
            )
            .issues(),
        "settings.schema.json",
        "properties.serverName.x-stackly.managed",
        IssueCode::TokenNotAllowedHere,
    );
    assert_manifest_issue(
        Edit::fake()
            .replace(
                yaml,
                "managedFilesystem: [data, config, logs, temp, run]",
                "managedFilesystem: [data, config, logs, temp]",
            )
            .replace(
                yaml,
                "      - serve\n",
                "      - serve\n      - \"--token-file=${instance.secrets}/token\"\n",
            ),
        "lifecycle.start.args[1]",
        IssueCode::PathNotPermitted,
    );

    for folder in ["config", "data"] {
        let issues = Edit::fake()
            .replace(
                yaml,
                "destination: \"${instance.secrets}/fake.json\"",
                &format!("destination: \"${{instance.{folder}}}/fake.json\""),
            )
            .issues();

        let at_destination: Vec<_> = issues
            .iter()
            .filter(|(file, location, _)| {
                file == yaml && location == "configFiles.fakeConfig.destination"
            })
            .collect();
        assert_eq!(
            at_destination.len(),
            1,
            "one issue for a sensitive config in {folder}: {issues:#?}"
        );
        assert_eq!(at_destination[0].2, IssueCode::TokenNotAllowedHere);
        for location in ["lifecycle.initialize[1].file", "lifecycle.reload.render[0]"] {
            assert_issue(&issues, yaml, location, IssueCode::UnknownConfigFile);
        }
    }

    let package = Edit::fake()
        .validate()
        .unwrap_or_else(|err| panic!("{:#?}", err.issues));
    let context = context_for(
        &package,
        serde_json::json!({ "authToken": SECRET_REF }),
        "/Volumes/T",
    );
    let secrets = context.paths.run.join("secrets");
    let file = secrets.join("fake.json");

    let init = package.init_plan(&context).expect("the init plan builds");
    let run = package.run_plan(&context).expect("the run plan builds");

    let write_at = init
        .steps
        .iter()
        .position(|step| matches!(step, InitStep::WriteFile(write) if write.path == file))
        .expect("init writes the sensitive config into the secrets folder");
    assert!(
        matches!(&init.steps[write_at - 1], InitStep::EnsureDirectories(dirs) if dirs == std::slice::from_ref(&secrets)),
        "{:#?}",
        init.steps
    );
    assert!(
        matches!(run.render.as_slice(), [InitStep::EnsureDirectories(dirs), InitStep::WriteFile(write)] if dirs == std::slice::from_ref(&secrets) && write.path == file),
        "{:#?}",
        run.render
    );
    assert_eq!(
        run.spawn.args[1],
        format!("--config={}", file.to_str().expect("UTF-8 path"))
    );
}

#[test]
fn a_doubled_dollar_is_a_literal_dollar_brace() {
    let package = Edit::fake()
        .replace(
            "provider.yaml",
            "      - serve\n",
            "      - serve\n      - \"--home=$${HOME}\"\n",
        )
        .validate()
        .unwrap_or_else(|err| panic!("{:#?}", err.issues));
    let context = context_for(
        &package,
        serde_json::json!({ "authToken": SECRET_REF }),
        "/Volumes/T",
    );

    let run = package.run_plan(&context).expect("the plan builds");

    assert_eq!(run.spawn.args[1], "--home=${HOME}");
}

#[test]
fn managed_expressions_cannot_read_settings() {
    let ev = evidence!(
        covers = ["VAL-019"],
        kind = Kind::Fixture,
        expected = "a managed settings expression that reads another setting is rejected",
    );
    let issues = Edit::fake_command()
        .replace(
            "schemas/settings.schema.json",
            "\"managed\": \"${instance.name}\"",
            "\"managed\": \"${settings.greeting}\"",
        )
        .issues();

    assert_issue(
        &issues,
        "settings.schema.json",
        "properties.serverName.x-stackly.managed",
        IssueCode::TokenNotAllowedHere,
    );
    ev.pass();
}

#[test]
fn the_settings_dialect_rejects_keywords_and_rules_outside_it() {
    let ev = evidence!(
        covers = ["VAL-012", "VAL-019"],
        kind = Kind::Fixture,
        expected = "the settings schema dialect rejects keywords such as $ref, oneOf, and format, and every rule outside the restricted dialect",
    );
    let schema = "schemas/settings.schema.json";
    let file = "settings.schema.json";
    let label = "\"title\": \"Label\",";
    assert_issue(
        &Edit::fake()
            .replace(schema, label, "\"title\": \"Label\", \"$ref\": \"#/x\",")
            .issues(),
        file,
        "properties.label.$ref",
        IssueCode::SettingsSchemaKeyword,
    );
    assert_issue(
        &Edit::fake()
            .replace(
                schema,
                label,
                "\"title\": \"Label\", \"oneOf\": [{ \"type\": \"string\" }],",
            )
            .issues(),
        file,
        "properties.label.oneOf",
        IssueCode::SettingsSchemaKeyword,
    );
    assert_issue(
        &Edit::fake()
            .replace(
                schema,
                label,
                "\"title\": \"Label\", \"format\": \"email\",",
            )
            .issues(),
        file,
        "properties.label.format",
        IssueCode::SettingsSchemaKeyword,
    );
    assert_issue(
        &Edit::fake()
            .replace(
                schema,
                "\"maxLength\": 256,\n      \"default\": \"fake\"",
                "\"default\": \"fake\"",
            )
            .issues(),
        file,
        "properties.label.maxLength",
        IssueCode::SettingsSchemaRule,
    );
    assert_issue(
        &Edit::fake_command()
            .replace(
                schema,
                "\"enum\": [\"quiet\", \"loud\"]",
                "\"enum\": [\"quiet\", \"-x\"]",
            )
            .issues(),
        file,
        "properties.mode.enum[1]",
        IssueCode::SettingsSchemaRule,
    );
    assert_issue(
        &Edit::fake()
            .replace(
                schema,
                "\"minLength\": 1,",
                "\"minLength\": 1, \"default\": \"x\",",
            )
            .issues(),
        file,
        "properties.authToken.default",
        IssueCode::SettingsSchemaRule,
    );
    assert_issue(
        &Edit::fake()
            .replace(
                schema,
                "\"default\": \"fake\"",
                &format!("\"default\": \"{}\"", "x".repeat(257)),
            )
            .issues(),
        file,
        "properties.label.default",
        IssueCode::SettingsSchemaRule,
    );
    assert_issue(
        &Edit::fake_command()
            .replace(
                schema,
                "\"required\": [\"authToken\"]\n}",
                "\"required\": [\"authToken\", \"serverName\"]\n}",
            )
            .issues(),
        file,
        "properties.serverName",
        IssueCode::SettingsSchemaRule,
    );
    let many: String = (0..65).map(|n| format!("\"f{n}\": {{ \"type\": \"boolean\", \"title\": \"F\", \"default\": true, \"x-stackly\": {{ \"apply\": \"restart\" }} }},")).collect();
    assert_issue(
        &Edit::fake()
            .replace(
                schema,
                "\"properties\": {",
                &format!("\"properties\": {{{many}"),
            )
            .issues(),
        file,
        "properties",
        IssueCode::SettingsSchemaRule,
    );
    assert_issue(
        &Edit::fake_command().replace(schema, "\"default\": 4222 }", "\"default\": 4222 },\n          \"inner\": { \"type\": \"object\", \"title\": \"I\" }").issues(),
        file,
        "properties.peers.items.properties.inner.type",
        IssueCode::SettingsSchemaRule,
    );
    assert_issue(
        &Edit::fake()
            .replace(
                schema,
                "    \"authToken\": {",
                "    \"authTOken\": { \"type\": \"string\", \"title\": \"Other token\", \"maxLength\": 256, \"x-stackly\": { \"apply\": \"restart\", \"secret\": true } },\n    \"authToken\": {",
            )
            .replace(
                schema,
                "\"required\": [\"authToken\"]",
                "\"required\": [\"authToken\", \"authTOken\"]",
            )
            .issues(),
        file,
        "properties.authToken",
        IssueCode::SecretPurposeCollision,
    );
    ev.pass();
}

#[test]
fn a_reload_field_used_in_args_is_rejected() {
    let issues = Edit::fake()
        .replace(
            "provider.yaml",
            "      - serve\n",
            "      - serve\n      - \"--greeting=${settings.greeting}\"\n",
        )
        .issues();

    assert_issue(
        &issues,
        "settings.schema.json",
        "properties.greeting.x-stackly.apply",
        IssueCode::SettingsSchemaRule,
    );
}

#[test]
fn settings_values_are_checked_with_their_field_path() {
    let ev = evidence!(
        covers = ["VAL-019"],
        kind = Kind::Fixture,
        expected = "settings values with an unknown key, a filled managed field, a duplicate item key, a non integral integer, a control character, or an out of range value are rejected with the field path",
    );
    let package = Edit::fake_command()
        .validate()
        .expect("fake-command validates");
    let schema = package.settings_schema();
    let codes = |value: serde_json::Value| -> Vec<(String, IssueCode)> {
        schema
            .validate(&value)
            .unwrap_err()
            .issues
            .into_iter()
            .map(|issue| (issue.location.to_string(), issue.code))
            .collect()
    };
    let token = SECRET_REF;

    assert_eq!(
        codes(serde_json::json!({ "authToken": token, "extra": 1 })),
        [("extra".into(), IssueCode::UnknownSetting)]
    );
    assert_eq!(
        codes(serde_json::json!({ "authToken": token, "serverName": "x" })),
        [("serverName".into(), IssueCode::SettingsValue)]
    );
    assert_eq!(
        codes(
            serde_json::json!({ "authToken": token, "peers": [{ "name": "a" }, { "name": "a" }] })
        ),
        [("peers[1].name".into(), IssueCode::SettingsValue)]
    );
    assert_eq!(
        codes(serde_json::json!({ "authToken": token, "peers": [{ "name": "a", "port": 5.0 }] })),
        [("peers[0].port".into(), IssueCode::SettingsValue)]
    );
    assert_eq!(
        codes(serde_json::json!({ "authToken": token, "greeting": "two\nlines" })),
        [("greeting".into(), IssueCode::SettingsValue)]
    );
    assert_eq!(
        codes(serde_json::json!({ "authToken": token, "peers": [{ "name": "a", "port": 70000 }] })),
        [("peers[0].port".into(), IssueCode::SettingsValue)]
    );
    assert_eq!(
        codes(serde_json::json!({})),
        [("authToken".into(), IssueCode::SettingsValue)]
    );
    let values = schema
        .validate(&serde_json::json!({ "authToken": token }))
        .expect("defaults fill in");
    assert_eq!(
        values.to_json(),
        serde_json::json!({ "authToken": token, "greeting": "hello from fake-command", "mode": "quiet", "peers": [] })
    );
    ev.pass();
}

#[test]
fn patterns_always_match_the_whole_value() {
    let package = Edit::fake_command()
        .replace(
            "schemas/settings.schema.json",
            "\"pattern\": \"[a-z][a-z0-9-]*\"",
            "\"pattern\": \"a|b\"",
        )
        .validate()
        .expect("the changed package validates");
    let check = |name: &str| {
        package
            .settings_schema()
            .validate(&serde_json::json!({ "authToken": SECRET_REF, "peers": [{ "name": name }] }))
            .is_ok()
    };

    assert!(check("a"));
    assert!(check("b"));
    assert!(!check("ab"));
    assert!(!check("xa"));
}

#[test]
fn a_pattern_that_compiles_only_inside_the_whole_value_wrapper_is_rejected() {
    // Wrapped as `\A(?:a)|(b)\z`, this would accept any text that starts
    // with a or ends with b.
    assert_issue(
        &Edit::fake()
            .replace(
                "schemas/settings.schema.json",
                "\"maxLength\": 256,\n      \"default\": \"fake\",",
                "\"maxLength\": 256,\n      \"pattern\": \"a)|(b\",\n      \"default\": \"abc\",",
            )
            .issues(),
        "settings.schema.json",
        "properties.label.pattern",
        IssueCode::SettingsSchemaRule,
    );
    // Wrapped, it compiles with exactly the one capture group a data
    // marker needs.
    assert_manifest_issue(
        Edit::fake().replace(
            "provider.yaml",
            "pattern: \"([0-9]+\\\\.[0-9]+)\"",
            "pattern: \"0)|([0-9]+\\\\.[0-9]+\"",
        ),
        "compatibility.dataMarker.pattern",
        IssueCode::InvalidValue,
    );
}

#[test]
fn settings_fields_carry_their_val_012_classes() {
    let ev = evidence!(
        covers = ["VAL-012"],
        kind = Kind::Fixture,
        expected = "the typed settings model carries the six VAL-012 classes: managed, advanced, secret, immutable after init, reloadable, and restart required",
    );
    let package = Edit::fake_command()
        .validate()
        .expect("fake-command validates");
    let classes = |name: &str| {
        package
            .settings_schema()
            .field(name)
            .expect("field exists")
            .classes()
    };

    assert_eq!(classes("serverName"), [SettingClass::Managed]);
    assert_eq!(
        classes("peers"),
        [SettingClass::Advanced, SettingClass::RestartRequired]
    );
    assert_eq!(
        classes("authToken"),
        [SettingClass::Secret, SettingClass::ImmutableAfterInit]
    );
    assert_eq!(classes("greeting"), [SettingClass::Reloadable]);
    ev.pass();
}

#[test]
fn templates_reject_bad_types_names_secrets_and_structure_with_a_position() {
    let peers = "templates/peers.yaml.tmpl";
    let server = "templates/server.json.tmpl";
    assert_issue(
        &Edit::fake_command()
            .set(peers, "x: {{ settings.peers }}\n")
            .issues(),
        peers,
        "1:4",
        IssueCode::TemplateType,
    );
    assert_issue(
        &Edit::fake_command()
            .set(peers, "{{#each settings.greeting as g}}\n{{/each}}\n")
            .issues(),
        peers,
        "1:1",
        IssueCode::TemplateType,
    );
    assert_issue(
        &Edit::fake_command()
            .set(peers, "{{#each settings.peers as settings}}\n{{/each}}\n")
            .issues(),
        peers,
        "1:1",
        IssueCode::InvalidName,
    );
    assert_issue(
        &Edit::fake_command()
            .set(peers, "t: {{ secret.authToken }}\n")
            .issues(),
        peers,
        "1:4",
        IssueCode::SecretNotAllowedHere,
    );
    assert_issue(
        &Edit::fake_command()
            .set(peers, "{{#if settings.peers}}\n")
            .issues(),
        peers,
        "1:1",
        IssueCode::TemplateSyntax,
    );
    let deep = "{{#if settings.peers}}".repeat(5) + &"{{/if}}".repeat(5);
    assert_issue(
        &Edit::fake_command().set(peers, deep).issues(),
        peers,
        "1:89",
        IssueCode::TemplateSyntax,
    );
    assert_issue(
        &Edit::fake_command()
            .set(peers, "a: \"prefix-{{ endpoint.http.address }}\"\n")
            .issues(),
        peers,
        ".",
        IssueCode::RenderedOutputInvalid,
    );
    assert_issue(
        &Edit::fake_command()
            .set(server, "{\"a\": {{ settings.nope }}}\n")
            .issues(),
        server,
        "1:7",
        IssueCode::UnknownSetting,
    );
}

#[test]
fn endpoint_rules_reject_lan_hosts_bad_names_long_sockets_and_missing_targets() {
    let yaml = "provider.yaml";
    assert_manifest_issue(
        Edit::fake().replace(
            yaml,
            "defaultHost: \"127.0.0.1\"",
            "defaultHost: \"0.0.0.0\"",
        ),
        "resources.endpoints.http.defaultHost",
        IssueCode::EndpointNotLoopback,
    );
    assert_manifest_issue(
        Edit::fake_command().replace(
            yaml,
            "socketName: \"fake.${endpoint.http.port}\"",
            "socketName: \".s.PGSQL.${endpoint.http.port}xy\"",
        ),
        "resources.endpoints.control.socketName",
        IssueCode::SocketNameTooLong,
    );
    assert_manifest_issue(
        Edit::fake_command().replace(
            yaml,
            "socketName: \"fake.${endpoint.http.port}\"",
            "socketName: \"../x\"",
        ),
        "resources.endpoints.control.socketName",
        IssueCode::InvalidName,
    );
    assert_manifest_issue(
        Edit::fake().replace(
            yaml,
            "    http: { transport: tcp",
            "    client-tls: { transport: tcp",
        ),
        "resources.endpoints.client-tls",
        IssueCode::InvalidName,
    );
    assert_manifest_issue(
        Edit::fake().replace(
            yaml,
            "{ type: http, endpoint: http, path: /ready }",
            "{ type: http, endpoint: web, path: /ready }",
        ),
        "readiness.probe.endpoint",
        IssueCode::UnknownEndpoint,
    );
    assert_manifest_issue(
        Edit::fake()
            .replace(yaml, "    http: { transport: tcp, purpose: client, defaultHost: \"127.0.0.1\", defaultPort: 18080 }", "    http: { transport: tcp, purpose: client, defaultHost: \"127.0.0.1\", defaultPort: 18080 }\n    dns: { transport: udp, purpose: admin, defaultHost: \"127.0.0.1\", defaultPort: 5353 }")
            .replace(yaml, "  probe: { type: http, endpoint: http, path: /health }", "  probe: { type: tcp, endpoint: dns }"),
        "health.probe.endpoint",
        IssueCode::UnknownEndpoint,
    );
}

#[test]
fn provider_ids_and_revisions_follow_val_002() {
    let ev = evidence!(
        covers = ["VAL-002"],
        kind = Kind::Fixture,
        expected = "provider IDs need a namespace and valid names and revisions cannot be empty or hold .., following VAL-002",
    );
    for id in ["official/../x", "Official/Nats", "nats"] {
        assert_manifest_issue(
            Edit::fake().replace(
                "provider.yaml",
                "id: testing/fake",
                &format!("id: \"{id}\""),
            ),
            "metadata.id",
            IssueCode::InvalidName,
        );
    }
    for revision in ["\"\"", "\"1..2\""] {
        assert_manifest_issue(
            Edit::fake().replace(
                "provider.yaml",
                "revision: \"0.3.0\"",
                &format!("revision: {revision}"),
            ),
            "metadata.revision",
            IssueCode::InvalidName,
        );
    }
    assert_manifest_issue(
        Edit::fake().replace("provider.yaml", "category: testing", "category: database"),
        "metadata.category",
        IssueCode::InvalidValue,
    );
    ev.pass();
}

#[test]
fn the_folder_name_must_match_unless_the_check_is_skipped() {
    let ev = evidence!(
        covers = ["VAL-002"],
        kind = Kind::Fixture,
        expected = "the name part of metadata.id must equal the package folder unless the pinned copy check is skipped",
    );
    let mut renamed = Edit::fake();
    renamed.folder = "fake2".into();
    let issues = renamed.issues();
    let mut pinned = Edit::fake();
    pinned.folder = "0123abcd".into();

    assert_issue(
        &issues,
        "provider.yaml",
        "metadata.id",
        IssueCode::DirectoryNameMismatch,
    );
    assert!(pinned.validate_as(NameCheck::Skip).is_ok());
    ev.pass();
}

#[test]
fn timeouts_follow_their_bounds_and_deadlines() {
    let yaml = "provider.yaml";
    assert_manifest_issue(
        Edit::fake().replace(yaml, "stopSeconds: 10", "stopSeconds: 0"),
        "timeouts.stopSeconds",
        IssueCode::TimeoutOutOfRange,
    );
    assert_manifest_issue(
        Edit::fake().replace(yaml, "readinessSeconds: 30", "readinessSeconds: 601"),
        "timeouts.readinessSeconds",
        IssueCode::TimeoutOutOfRange,
    );
    assert_manifest_issue(
        Edit::fake().replace(
            yaml,
            "      stdin: { secret: authToken }",
            "      stdin: { secret: authToken }\n      timeoutSeconds: 31",
        ),
        "lifecycle.initialize[2].timeoutSeconds",
        IssueCode::TimeoutOutOfRange,
    );
    assert_manifest_issue(
        Edit::fake().replace(
            yaml,
            "attemptTimeoutMilliseconds: 1000",
            "attemptTimeoutMilliseconds: 11000",
        ),
        "readiness.attemptTimeoutMilliseconds",
        IssueCode::TimeoutOutOfRange,
    );
}

#[test]
fn the_digest_follows_behavior_files_only() {
    let base = Edit::fake()
        .validate()
        .expect("fake validates")
        .key()
        .clone();
    let template = Edit::fake()
        .replace("templates/fake.json.tmpl", "{\n", "{ \n")
        .validate()
        .expect("still valid")
        .key()
        .clone();
    let versions = Edit::fake()
        .replace(
            "versions.json",
            "\"license\": \"MIT OR Apache-2.0\",",
            "\"license\": \"MIT\",",
        )
        .validate()
        .expect("still valid")
        .key()
        .clone();

    assert_ne!(base.manifest_sha256(), template.manifest_sha256());
    assert_eq!(base.manifest_sha256(), versions.manifest_sha256());
}

#[test]
fn versions_json_rules_and_cross_checks_hold() {
    let versions = "versions.json";
    let check = |edit: Edit, location: &str, code: IssueCode| {
        assert_issue(&edit.issues(), versions, location, code)
    };
    let official = Edit::fake()
        .replace("provider.yaml", "id: testing/fake", "id: official/fake")
        .replace("provider.yaml", "category: testing", "category: database");
    check(
        official,
        "artifacts[0].status",
        IssueCode::FixtureOutsideTesting,
    );
    check(
        Edit::fake().replace(
            versions,
            "\"status\": \"fixture\",",
            "\"status\": \"available\",",
        ),
        "artifacts[0].sha256",
        IssueCode::VersionsRule,
    );
    check(
        Edit::fake().replace(versions, "\"version\": \"0.1.0\"", "\"version\": \"0.2.0\""),
        "artifacts[0].version",
        IssueCode::VersionsRule,
    );
    check(
        Edit::fake().replace(versions, "\"os\": \"macos\"", "\"os\": \"linux\""),
        "artifacts[0]",
        IssueCode::VersionsRule,
    );
    check(
        Edit::fake().replace(
            versions,
            "\"executables\": [\"stackly-fake-server\"]",
            "\"executables\": [\"other\"]",
        ),
        "artifacts[0].executables",
        IssueCode::ExecutableNotInArtifact,
    );
    let doubled = Edit::fake().text(versions).replace(
        "  ]\n}",
        ",\n    { \"id\": \"fake-0.1.0-macos-aarch64\", \"line\": \"0.1\", \"version\": \"0.1.1\", \"os\": \"macos\", \"arch\": \"aarch64\", \"status\": \"fixture\", \"executables\": [\"stackly-fake-server\"] }\n  ]\n}",
    );
    check(
        Edit::fake().set(versions, doubled.replacen("}\n,", "},", 1)),
        "artifacts[1].id",
        IssueCode::VersionsRule,
    );
}

#[test]
fn package_files_outside_the_layout_are_rejected() {
    let unreferenced = Edit::fake().set("templates/extra.tmpl", "x\n").issues();
    let bom = Edit::fake()
        .set(
            "templates/fake.json.tmpl",
            [b"\xef\xbb\xbf".as_slice(), b"{}\n"].concat(),
        )
        .issues();
    let large = Edit::fake()
        .set("provider.yaml", vec![b'#'; 256 * 1024 + 1])
        .issues();
    let missing = Edit::fake().remove("versions.json").issues();

    assert_issue(
        &unreferenced,
        "templates/extra.tmpl",
        ".",
        IssueCode::UnreferencedTemplate,
    );
    assert_issue(&bom, "templates/fake.json.tmpl", ".", IssueCode::NotUtf8);
    assert_issue(&large, "provider.yaml", ".", IssueCode::FileTooLarge);
    assert_issue(&missing, "versions.json", ".", IssueCode::MissingFile);
}

/// Copies the fake package into a test root, lets `change` alter it on
/// disk, and reads it back.
fn read_changed(change: impl FnOnce(&Path)) -> Result<PackageFiles, PackageError> {
    let root = TestRoot::new().expect("a test root can be created");
    let dir = root.path().join("fake");
    copy(&fake_dir(), &dir);
    change(&dir);
    let platform = FakePlatform::new(&root);
    read_package(platform.fs(), &dir)
}

fn copy(from: &Path, to: &Path) {
    fs::create_dir_all(to).expect("copy target");
    for entry in fs::read_dir(from).expect("listing") {
        let entry = entry.expect("entry");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("type").is_dir() {
            copy(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target).expect("copy");
        }
    }
}

fn read_codes(change: impl FnOnce(&Path)) -> Vec<(String, IssueCode)> {
    read_changed(change)
        .expect_err("the package should be rejected")
        .issues
        .into_iter()
        .map(|issue| (issue.file, issue.code))
        .collect()
}

#[test]
fn read_package_rejects_symlinks_fifos_and_stray_entries_and_skips_ds_store() {
    let ev = evidence!(
        covers = ["REQ-029"],
        kind = Kind::Fixture,
        expected = "reading a package rejects symlinks, FIFOs, and stray entries, skips .DS_Store, and never reads tests/",
    );
    let symlink = read_codes(|dir| {
        std::os::unix::fs::symlink(dir.join("provider.yaml"), dir.join("templates/link.tmpl"))
            .expect("symlink")
    });
    let fifo = read_codes(|dir| {
        fs::remove_file(dir.join("provider.yaml")).expect("remove");
        make_fifo(&dir.join("provider.yaml")).expect("fifo");
    });
    let notes = read_codes(|dir| fs::write(dir.join("notes.txt"), "x").expect("write"));
    let skipped = read_changed(|dir| {
        fs::write(dir.join(".DS_Store"), "x").expect("write");
        fs::create_dir(dir.join("tests")).expect("tests folder");
        fs::write(dir.join("tests/anything.bin"), [0xff]).expect("write");
    });

    assert_eq!(
        symlink,
        [(
            "templates/link.tmpl".to_owned(),
            IssueCode::SymlinkNotAllowed
        )]
    );
    assert_eq!(
        fifo,
        [("provider.yaml".to_owned(), IssueCode::NotARegularFile)]
    );
    assert_eq!(
        notes,
        [("notes.txt".to_owned(), IssueCode::UnexpectedEntry)]
    );
    let files = skipped.expect("skipped entries are fine");
    assert!(files.get(".DS_Store").is_none());
    assert!(files.iter().all(|(path, _)| !path.starts_with("tests")));
    ev.pass();
}

fn context_for(package: &ValidatedPackage, settings: serde_json::Value, base: &str) -> PlanContext {
    let base = Path::new(base);
    let roots = RootSet::new(
        base.join("data"),
        base.join("logs"),
        base.join("cache"),
        PathBuf::from("/private/tmp/skt-rules"),
    )
    .expect("valid roots");
    let id: InstanceId = "0192f000-0000-7000-8000-00000000000b"
        .parse()
        .expect("valid ID");
    let endpoints = package
        .endpoints()
        .map(|endpoint| {
            let resolved = match endpoint.default_host() {
                Some(host) => ResolvedEndpoint::Ip {
                    host,
                    port: endpoint.default_port(),
                },
                None => ResolvedEndpoint::Unix,
            };
            (endpoint.name().clone(), resolved)
        })
        .collect();
    PlanContext {
        instance_id: id,
        instance_name: "rules".parse().expect("valid name"),
        paths: DirectoryLayout::new(roots).instance_paths(id),
        runtime: RuntimeContext {
            root: base.join("runtime"),
            version: "0.1.0".parse().expect("valid version"),
            line: "0.1".parse().expect("valid line"),
        },
        endpoints,
        settings: package
            .settings_schema()
            .validate(&settings)
            .unwrap_or_else(|err| panic!("{:#?}", err.issues)),
    }
}

#[test]
fn plans_report_unresolved_and_lan_endpoints_and_long_arguments() {
    let package = Edit::fake().validate().expect("fake validates");
    let settings = serde_json::json!({ "authToken": SECRET_REF });
    let http: EndpointName = "http".parse().expect("valid name");

    let mut auto = context_for(&package, settings.clone(), "/Volumes/T");
    auto.endpoints.insert(
        http.clone(),
        ResolvedEndpoint::Ip {
            host: [127, 0, 0, 1].into(),
            port: None,
        },
    );
    let mut lan = context_for(&package, settings.clone(), "/Volumes/T");
    lan.endpoints.insert(
        http.clone(),
        ResolvedEndpoint::Ip {
            host: [0, 0, 0, 0].into(),
            port: Port::new(1).ok(),
        },
    );

    assert_eq!(
        package.run_plan(&auto).unwrap_err(),
        PlanError::EndpointUnresolved {
            endpoint: http.clone()
        }
    );
    assert_eq!(
        package.run_plan(&lan).unwrap_err(),
        PlanError::EndpointNotLoopback { endpoint: http }
    );

    let long = Edit::fake()
        .replace(
            "schemas/settings.schema.json",
            "\"maxLength\": 256,\n      \"default\": \"fake\"",
            "\"maxLength\": 4096,\n      \"default\": \"fake\"",
        )
        .validate()
        .expect("still valid");
    let label = "x".repeat(4089);
    let context = context_for(
        &long,
        serde_json::json!({ "authToken": SECRET_REF, "label": label }),
        "/Volumes/T",
    );
    assert_eq!(
        long.run_plan(&context).unwrap_err(),
        PlanError::ArgumentTooLong {
            position: "lifecycle.start.args[5]".into()
        }
    );
}

/// The first config file written by an init plan.
fn first_write(
    package: &ValidatedPackage,
    settings: serde_json::Value,
) -> stackly_core::provider::FileWrite {
    let init = package
        .init_plan(&context_for(package, settings, "/Volumes/T"))
        .expect("the init plan builds");
    init.steps
        .into_iter()
        .find_map(|step| match step {
            InitStep::WriteFile(write) => Some(write),
            _ => None,
        })
        .expect("the config is written")
}

fn token(text: &str) -> ResolvedSecrets {
    ResolvedSecrets::for_tests([(
        "authToken".parse().expect("valid field"),
        SecretValue::new(text.to_owned()).expect("a valid secret value"),
    )])
}

#[test]
fn postgresql_conf_renders_quoted_text_decimal_integers_and_on_off_booleans() {
    let package = Edit::fake()
        .replace("provider.yaml", "format: json", "format: postgresqlConf")
        .set(
            "templates/fake.json.tmpl",
            "greeting = {{ settings.greeting }}\nfail_init = {{ settings.failInit }}\nignore_term = {{ settings.ignoreTerm }}\nready_delay = {{ settings.readyDelayMs }}\ntoken = {{ secret.authToken }}\n",
        )
        .validate()
        .unwrap_or_else(|err| panic!("{:#?}", err.issues));
    let write = first_write(
        &package,
        serde_json::json!({
            "authToken": SECRET_REF,
            "greeting": "it's a\\b ü",
            "failInit": true,
            "readyDelayMs": 250,
        }),
    );

    assert_eq!(
        write.content.materialize(&token("x'y")).expect("the config materializes"),
        "greeting = 'it''s a\\\\b ü'\nfail_init = on\nignore_term = off\nready_delay = 250\ntoken = 'x''y'\n".as_bytes()
    );
    let err = write.content.materialize(&token("a\nb")).unwrap_err();
    assert_eq!(
        err,
        PlanError::UnrepresentableValue {
            field: "secret.authToken".into()
        }
    );
    assert!(!err.to_string().contains("a\nb"), "{err}");
}

#[test]
fn plain_templates_keep_text_and_comments_and_reject_output_and_block_tags() {
    let template = "templates/fake.json.tmpl";
    let package = Edit::fake()
        .replace("provider.yaml", "format: json", "format: plain")
        .set(
            template,
            "{{! a comment line disappears }}\nlocal   all   all   scram-sha-256\n\\{{ stays literal }}\nmap /^.*$ x\n",
        )
        .validate()
        .unwrap_or_else(|err| panic!("{:#?}", err.issues));
    let write = first_write(&package, serde_json::json!({ "authToken": SECRET_REF }));
    assert_eq!(
        write
            .content
            .materialize(&token("unused"))
            .expect("the file materializes"),
        b"local   all   all   scram-sha-256\n{{ stays literal }}\nmap /^.*$ x\n"
    );

    let plain = || Edit::fake().replace("provider.yaml", "format: json", "format: plain");
    assert_issue(
        &plain()
            .set(template, "a\n{{ settings.greeting }}\n")
            .issues(),
        template,
        "2:1",
        IssueCode::TemplateType,
    );
    assert_issue(
        &plain()
            .set(template, "{{#if settings.failInit}}x{{/if}}\n")
            .issues(),
        template,
        "1:1",
        IssueCode::TemplateType,
    );
}

#[test]
fn nats_conf_values_the_format_cannot_hold_fail_without_their_value() {
    let package = Edit::fake()
        .replace("provider.yaml", "format: json", "format: natsConf")
        .set(
            "templates/fake.json.tmpl",
            "data: {{ instance.data }}\ntoken: {{ secret.authToken }}\n",
        )
        .validate()
        .unwrap_or_else(|err| panic!("{:#?}", err.issues));
    let settings = serde_json::json!({ "authToken": SECRET_REF });
    let quoted = context_for(&package, settings.clone(), "/Volumes/a\"b");

    let err = package.init_plan(&quoted).unwrap_err();

    assert_eq!(
        err,
        PlanError::UnrepresentableValue {
            field: "instance.data".into()
        }
    );
    assert!(!err.to_string().contains("a\"b"), "{err}");

    let init = package
        .init_plan(&context_for(&package, settings, "/Volumes/T"))
        .expect("plain paths render");
    let write = init
        .steps
        .iter()
        .find_map(|step| match step {
            InitStep::WriteFile(write) => Some(write),
            _ => None,
        })
        .expect("the config is written");
    let token = |text: &str| {
        ResolvedSecrets::for_tests([(
            "authToken".parse().expect("valid field"),
            SecretValue::new(text.to_owned()).expect("a valid secret value"),
        )])
    };
    let err = write.content.materialize(&token("a$b")).unwrap_err();
    assert_eq!(
        err,
        PlanError::UnrepresentableValue {
            field: "secret.authToken".into()
        }
    );
    assert!(!err.to_string().contains("a$b"));
    assert_eq!(
        write.content.materialize(&token("ok")).expect("plain secrets materialize"),
        b"data: \"/Volumes/T/data/instances/0192f000-0000-7000-8000-00000000000b/data\"\ntoken: \"ok\"\n"
    );
    assert_eq!(
        write
            .content
            .materialize(&ResolvedSecrets::for_tests([]))
            .unwrap_err(),
        PlanError::SecretUnavailable {
            field: "authToken".parse().expect("valid field")
        }
    );
}

#[test]
fn a_working_folder_that_is_not_utf8_fails_the_run_plan() {
    let package = Edit::fake()
        .replace(
            "provider.yaml",
            "      - \"--data=${instance.data}\"\n      - \"--port",
            "      - \"--data=${instance.temp}\"\n      - \"--port",
        )
        .validate()
        .unwrap_or_else(|err| panic!("{:#?}", err.issues));
    let mut context = context_for(
        &package,
        serde_json::json!({ "authToken": SECRET_REF }),
        "/Volumes/T",
    );
    context.paths.data = PathBuf::from(OsStr::from_bytes(b"/Volumes/T/\xff/data"));

    assert_eq!(
        package.run_plan(&context).unwrap_err(),
        PlanError::PathNotUtf8 {
            position: "instance.data".into()
        }
    );
}

#[test]
fn every_plan_fails_for_a_folder_it_uses_that_is_not_utf8() {
    let package = Edit::fake_command()
        .validate()
        .unwrap_or_else(|err| panic!("{:#?}", err.issues));
    let settings = serde_json::json!({ "authToken": SECRET_REF });
    // fake-command's initialize and start use all five folders (the run
    // folder through its socket and `${instance.secrets}`); its reload
    // renders the sensitive config into the secrets folder of the run
    // folder and runs a command in the data folder.
    let reload_uses = ["data", "temp", "run"];

    for folder in ["data", "config", "logs", "temp", "run"] {
        let mut context = context_for(&package, settings.clone(), "/Volumes/T");
        let path = PathBuf::from(OsStr::from_bytes(
            &[b"/Volumes/T/\xff/".as_slice(), folder.as_bytes()].concat(),
        ));
        let paths = &mut context.paths;
        match folder {
            "data" => paths.data = path,
            "config" => paths.config = path,
            "logs" => paths.logs = path,
            "temp" => paths.temp = path,
            _ => paths.run = path,
        }
        let expected = PlanError::PathNotUtf8 {
            position: format!("instance.{folder}"),
        };

        assert_eq!(package.init_plan(&context).unwrap_err(), expected);
        assert_eq!(package.run_plan(&context).unwrap_err(), expected);
        if reload_uses.contains(&folder) {
            assert_eq!(package.reload_plan(&context).unwrap_err(), expected);
        } else {
            assert!(
                package.reload_plan(&context).is_ok(),
                "the reload plan holds no path from {folder}"
            );
        }
    }
}

#[test]
fn a_package_without_config_files_renders_nothing_before_the_spawn() {
    let yaml = "provider.yaml";
    let package = Edit::fake()
        .replace(
            yaml,
            "configFiles:\n  fakeConfig:\n    template: templates/fake.json.tmpl\n    format: json\n    destination: \"${instance.secrets}/fake.json\"\n    sensitive: true\n",
            "",
        )
        .replace(yaml, "    - { primitive: renderConfig, file: fakeConfig }\n", "")
        .replace(yaml, "render: [fakeConfig]", "render: []")
        .remove("templates/fake.json.tmpl")
        .validate()
        .unwrap_or_else(|err| panic!("{:#?}", err.issues));
    let context = context_for(
        &package,
        serde_json::json!({ "authToken": SECRET_REF }),
        "/Volumes/T",
    );

    let run = package.run_plan(&context).expect("the run plan builds");
    let reload = package
        .reload_plan(&context)
        .expect("the reload plan builds");

    assert!(run.render.is_empty(), "{:#?}", run.render);
    assert!(reload.steps.is_empty(), "{:#?}", reload.steps);
}

#[test]
fn issues_name_their_location_kind() {
    let issues = Edit::fake()
        .replace(
            "templates/fake.json.tmpl",
            "{{ settings.greeting }}",
            "{{ settings.greeting",
        )
        .validate()
        .unwrap_err()
        .issues;

    assert!(
        matches!(
            issues[0].location,
            Location::Position {
                line: 2,
                column: 15
            }
        ),
        "{issues:#?}"
    );
}

#[test]
fn a_256_kib_manifest_of_nested_aliases_is_rejected_within_two_seconds() {
    let ev = evidence!(
        covers = ["VAL-018"],
        kind = Kind::Fixture,
        expected = "a 256 KiB provider.yaml of nested YAML aliases (billion laughs) is rejected with yaml_syntax within 2 seconds",
    );
    let mut text = String::from("a0: &a0 [lol, lol, lol, lol, lol, lol, lol, lol, lol]\n");
    for n in 1..10 {
        let previous = format!("*a{}", n - 1);
        let list = [previous.as_str(); 9].join(", ");
        text.push_str(&format!("a{n}: &a{n} [{list}]\n"));
    }
    while text.len() < 256 * 1024 - 80 {
        text.push_str("# padding up to the provider.yaml size limit\n");
    }
    assert!(text.len() <= 256 * 1024);
    let start = std::time::Instant::now();

    let issues = Edit::fake().set("provider.yaml", text).issues();

    let elapsed = start.elapsed();
    assert!(elapsed < std::time::Duration::from_secs(2), "{elapsed:?}");
    assert!(
        issues
            .iter()
            .any(|(file, _, code)| file == "provider.yaml" && *code == IssueCode::YamlSyntax),
        "{issues:#?}"
    );
    ev.pass_with(&format!("rejected with yaml_syntax in {elapsed:?}"));
}

/// One large anchor behind six levels of nine way aliases: a 2 KiB text that
/// expands to 531 million nodes after only about 600 thousand alias jumps,
/// far below the repetition limit of `serde_yaml_ng`.
fn large_anchor_alias_bomb() -> String {
    let mut text = format!("x0: &x0 [{}]\n", vec!["a"; 1000].join(", "));
    for level in 1..=6 {
        let alias = format!("*x{}", level - 1);
        let list = [alias.as_str(); 9].join(", ");
        text.push_str(&format!("x{level}: &x{level} [{list}]\n"));
    }
    text
}

#[test]
fn a_large_anchor_alias_bomb_is_rejected_in_the_manifest_and_in_template_output() {
    let ev = evidence!(
        covers = ["VAL-018"],
        kind = Kind::Fixture,
        expected = "a provider.yaml or a yaml template whose large anchor expands through a short alias chain is rejected within 2 seconds, without building the expanded document",
    );
    let bomb = large_anchor_alias_bomb();
    let start = std::time::Instant::now();

    let manifest = Edit::fake();
    let manifest_text = format!("{}{bomb}", manifest.text("provider.yaml"));
    let manifest_issues = manifest.set("provider.yaml", manifest_text).issues();
    let template_issues = Edit::fake()
        .replace("provider.yaml", "format: json", "format: yaml")
        .set(
            "templates/fake.json.tmpl",
            format!("greeting: {{{{ settings.greeting }}}}\n{bomb}"),
        )
        .issues();

    let elapsed = start.elapsed();
    assert!(elapsed < std::time::Duration::from_secs(2), "{elapsed:?}");
    assert_issue(
        &manifest_issues,
        "provider.yaml",
        ".",
        IssueCode::YamlSyntax,
    );
    assert!(
        template_issues
            .iter()
            .any(|(_, _, code)| *code == IssueCode::RenderedOutputInvalid),
        "{template_issues:#?}"
    );
    ev.pass_with(&format!("both rejected in {elapsed:?}"));
}

#[test]
fn readme_and_tests_never_change_the_digest_of_a_read_package() {
    let read = |change: &dyn Fn(&Path)| {
        let files = read_changed(change).expect("the package reads");
        ValidatedPackage::from_files(files, NameCheck::FolderName)
            .expect("the package validates")
            .key()
            .clone()
    };

    let base = read(&|_| {});
    let documented = read(&|dir| {
        fs::write(dir.join("README.md"), "# changed\n").expect("README.md can be written");
        fs::write(dir.join("LICENSE-NOTICES.md"), "notices\n").expect("notices can be written");
        fs::create_dir_all(dir.join("tests")).expect("tests/ can be created");
        fs::write(dir.join("tests/case.yaml"), [0xff, 0xfe]).expect("a test file can be written");
    });
    let changed = read(&|dir| {
        let template = dir.join("templates/fake.json.tmpl");
        let text = fs::read_to_string(&template).expect("the template reads");
        fs::write(&template, text.replacen('{', "{ ", 1)).expect("the template can be written");
    });

    assert_eq!(base, documented);
    assert_ne!(base.manifest_sha256(), changed.manifest_sha256());
}
