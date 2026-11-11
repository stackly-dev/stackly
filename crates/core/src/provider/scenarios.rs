//! `tests/scenarios.yaml` (spec 0036, *File skenario*): the optional list of
//! settings a package's conformance run tries.
//!
//! The file is read only by `check_package`, the harness, and the spike;
//! [`read_package`](super::read_package) never reads it, so it never changes
//! a package's `manifest_sha256`. It holds only settings that are not
//! secret, validated against the package's own settings schema, so it can
//! never name an executable, a path, or a command.

use std::collections::BTreeSet;
use std::path::Path;

use serde::Deserialize;
use stackly_domain::InstanceName;
use stackly_platform::{FileKind, Filesystem, PlatformError};

use super::issue::{FileIssues, Issue, IssueCode, Issues, Location};
use super::raw::{check_yaml_value, decode_yaml, parse_yaml};
use super::settings::SettingsSchema;

/// The scenarios file, relative to the package folder. Every issue about
/// it has this `file`.
pub const SCENARIOS_FILE: &str = "tests/scenarios.yaml";
/// The folder that holds [`SCENARIOS_FILE`].
const TESTS_DIR: &str = "tests";
/// The one `schema` value the file may have.
pub const SCENARIOS_SCHEMA: &str = "stackly.provider-scenarios/v1";
/// Largest scenarios file, in bytes.
pub const SCENARIOS_MAX_LEN: u64 = 64 * 1024;
/// Most scenarios in one file.
pub const SCENARIOS_MAX: usize = 16;
/// Longest scenario name, in characters, so the harness instance name
/// `c-<scenario>-<n>` stays within [`InstanceName`].
pub const SCENARIO_NAME_MAX: usize = 24;
/// The name of the scenario a package without the file has.
pub const DEFAULT_SCENARIO: &str = "default";
/// The secret reference that stands in for each secret field while a
/// scenario's settings are validated: the harness has the daemon generate
/// the real value (`SecretSource::Generate`), so the file never holds one.
const PLACEHOLDER_SECRET_REF: &str = "local:instance/0192f000-0000-7000-8000-000000000001/scenario";

/// How a scenario must end in the harness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Expect {
    /// The instance is created, starts, and becomes ready.
    #[default]
    Ready,
    /// The instance is created but its start fails.
    StartFails,
    /// `instance.preflight` or `instance.create` refuses it, or the create
    /// operation ends `failed`.
    CreateFails,
}

impl Expect {
    /// Returns the value as the file spells it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::StartFails => "start_fails",
            Self::CreateFails => "create_fails",
        }
    }
}

/// One scenario of a conformance run.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Scenario {
    /// The name, unique in the file: an [`InstanceName`] of at most
    /// [`SCENARIO_NAME_MAX`] characters.
    pub name: InstanceName,
    /// The settings over the schema defaults, always a JSON object without
    /// a secret field.
    pub settings: serde_json::Value,
    /// How the scenario must end.
    pub expect: Expect,
}

impl Scenario {
    /// Creates a scenario.
    #[must_use]
    pub const fn new(name: InstanceName, settings: serde_json::Value, expect: Expect) -> Self {
        Self {
            name,
            settings,
            expect,
        }
    }

    /// The scenario a package without [`SCENARIOS_FILE`] has: `default`,
    /// with empty settings, expecting `ready`.
    #[must_use]
    pub fn implied_default() -> Self {
        Self::new(
            DEFAULT_SCENARIO
                .parse()
                .expect("`default` is a valid instance name"),
            serde_json::Value::Object(serde_json::Map::new()),
            Expect::Ready,
        )
    }
}

/// Parsing and validation of [`SCENARIOS_FILE`].
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct Scenarios;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawScenarios {
    #[expect(dead_code, reason = "checked on the YAML value before decoding")]
    schema: String,
    scenarios: Vec<RawScenario>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawScenario {
    name: String,
    #[serde(default)]
    settings: Option<serde_json::Map<String, serde_json::Value>>,
    #[serde(default)]
    expect: Expect,
}

impl Scenarios {
    /// Parses the text of [`SCENARIOS_FILE`] with the YAML rules of the
    /// manifest: unknown fields, repeated keys, tags, and merge keys are
    /// rejected. `schema` must be [`SCENARIOS_SCHEMA`]; otherwise nothing
    /// else is decoded. There must be 1 to [`SCENARIOS_MAX`] scenarios,
    /// each with a unique name that is an [`InstanceName`] of at most
    /// [`SCENARIO_NAME_MAX`] characters.
    ///
    /// # Errors
    ///
    /// Returns every issue found, each with file [`SCENARIOS_FILE`]: the
    /// codes are `yaml_syntax`, `duplicate_key`, `invalid_value`,
    /// `unknown_field`, `missing_field`, and `invalid_name`.
    pub fn parse(text: &str) -> Result<Vec<Scenario>, Vec<Issue>> {
        let mut issues = Issues::new();
        let scenarios = parse_into(text, &mut issues);
        match scenarios {
            Some(scenarios) if issues.is_empty() => Ok(scenarios),
            _ => Err(issues.into_vec()),
        }
    }

    /// Checks every scenario's settings against a package's settings
    /// schema. A key that names a secret field is rejected with
    /// `secret_not_allowed_here`; the rest of the settings, with a stand
    /// in reference for each secret field, must pass
    /// [`SettingsSchema::validate`], whose issues are kept with their code
    /// and message at `scenarios[<i>].settings.<key>`.
    ///
    /// # Errors
    ///
    /// Returns every issue found, each with file [`SCENARIOS_FILE`].
    pub fn validate(scenarios: &[Scenario], schema: &SettingsSchema) -> Result<(), Vec<Issue>> {
        let mut issues = Issues::new();
        for (index, scenario) in scenarios.iter().enumerate() {
            validate_one(index, scenario, schema, &mut issues);
        }
        if issues.is_empty() {
            Ok(())
        } else {
            Err(issues.into_vec())
        }
    }
}

fn parse_into(text: &str, issues: &mut Issues) -> Option<Vec<Scenario>> {
    let value = parse_yaml(SCENARIOS_FILE, text)
        .map_err(|issue| issues.push(issue))
        .ok()?;
    if let Err(issue) = check_yaml_value(SCENARIOS_FILE, &value) {
        issues.push(issue);
        return None;
    }
    let mut sink = FileIssues::new(SCENARIOS_FILE, issues);
    if !value.is_mapping() {
        sink.at(
            ".",
            IssueCode::InvalidValue,
            "tests/scenarios.yaml must be a mapping",
        );
        return None;
    }
    match value.get("schema") {
        None => {
            sink.at("schema", IssueCode::MissingField, "missing field `schema`");
            return None;
        }
        Some(found) if found.as_str() == Some(SCENARIOS_SCHEMA) => {}
        Some(_) => {
            sink.at(
                "schema",
                IssueCode::InvalidValue,
                format!("schema must be {SCENARIOS_SCHEMA}"),
            );
            return None;
        }
    }
    let raw: RawScenarios = decode_yaml(SCENARIOS_FILE, value, IssueCode::UnknownField)
        .map_err(|issue| issues.push(issue))
        .ok()?;
    let mut sink = FileIssues::new(SCENARIOS_FILE, issues);
    if raw.scenarios.is_empty() || raw.scenarios.len() > SCENARIOS_MAX {
        sink.at(
            "scenarios",
            IssueCode::InvalidValue,
            format!("scenarios must list 1 to {SCENARIOS_MAX} scenarios"),
        );
    }
    let mut seen = BTreeSet::new();
    let mut scenarios = Vec::with_capacity(raw.scenarios.len());
    for (index, raw) in raw.scenarios.into_iter().enumerate() {
        let location = format!("scenarios[{index}].name");
        let name = match raw.name.parse::<InstanceName>() {
            Ok(name) if raw.name.chars().count() <= SCENARIO_NAME_MAX => name,
            _ => {
                sink.at(
                    location,
                    IssueCode::InvalidName,
                    format!(
                        "a scenario name must be 1 to {SCENARIO_NAME_MAX} characters: a lowercase letter, then lowercase letters, digits, and single hyphens, not ending in a hyphen"
                    ),
                );
                continue;
            }
        };
        if !seen.insert(raw.name) {
            sink.at(
                location,
                IssueCode::DuplicateKey,
                format!("scenario {name} is listed twice"),
            );
            continue;
        }
        let settings = serde_json::Value::Object(raw.settings.unwrap_or_default());
        scenarios.push(Scenario::new(name, settings, raw.expect));
    }
    Some(scenarios)
}

fn validate_one(index: usize, scenario: &Scenario, schema: &SettingsSchema, issues: &mut Issues) {
    let prefix = format!("scenarios[{index}].settings");
    let mut document = match &scenario.settings {
        serde_json::Value::Object(object) => object.clone(),
        _ => {
            FileIssues::new(SCENARIOS_FILE, issues).at(
                prefix,
                IssueCode::InvalidValue,
                "settings must be a mapping",
            );
            return;
        }
    };
    let mut sink = FileIssues::new(SCENARIOS_FILE, issues);
    let mut secret_keys = Vec::new();
    for key in document.keys() {
        if schema.field(key).is_some_and(|field| field.is_secret()) {
            sink.at(
                format!("{prefix}.{key}"),
                IssueCode::SecretNotAllowedHere,
                format!(
                    "{key} is a secret field; the harness has the daemon generate it, so a scenario never sets it"
                ),
            );
            secret_keys.push(key.clone());
        }
    }
    for key in &secret_keys {
        document.remove(key);
    }
    for field in schema
        .fields()
        .filter(|field| field.is_secret() && field.managed().is_none())
    {
        document.insert(
            field.name().to_string(),
            serde_json::Value::String(PLACEHOLDER_SECRET_REF.to_owned()),
        );
    }
    if let Err(err) = schema.validate(&serde_json::Value::Object(document)) {
        for mut issue in err.issues {
            issue.location = match &issue.location {
                Location::Field(path) if path != "." => Location::Field(format!("{prefix}.{path}")),
                _ => Location::Field(prefix.clone()),
            };
            SCENARIOS_FILE.clone_into(&mut issue.file);
            issues.push(issue);
        }
    }
}

/// Reads [`SCENARIOS_FILE`] of the package in `dir` through the
/// `Filesystem` port: [`Filesystem::symlink_metadata`] and
/// [`Filesystem::read_file_limited`] only, so reading never writes or runs
/// anything. A package without `tests/` or without the file has none.
///
/// # Errors
///
/// Returns an issue with file [`SCENARIOS_FILE`] when `tests/` or the file
/// is a symlink (`symlink_not_allowed`), when either is not what it must be
/// (`not_a_regular_file`), when the file holds more than
/// [`SCENARIOS_MAX_LEN`] bytes (`file_too_large`), or when it is not UTF-8
/// or starts with a byte order mark (`not_utf8`).
pub fn read_scenarios(fs: &dyn Filesystem, dir: &Path) -> Result<Option<String>, Issue> {
    let tests = dir.join(TESTS_DIR);
    match fs.symlink_metadata(&tests) {
        Ok(None) => return Ok(None),
        Ok(Some(info)) => match info.kind {
            FileKind::Dir => {}
            FileKind::Symlink => {
                return Err(file_issue(
                    IssueCode::SymlinkNotAllowed,
                    "the tests folder is a symlink",
                ));
            }
            _ => {
                return Err(file_issue(
                    IssueCode::NotARegularFile,
                    "tests is not a folder",
                ));
            }
        },
        Err(err) => return Err(read_failed(&err)),
    }
    let path = dir.join(SCENARIOS_FILE);
    match fs.symlink_metadata(&path) {
        Ok(None) => return Ok(None),
        Ok(Some(info)) => match info.kind {
            FileKind::File => {}
            FileKind::Symlink => {
                return Err(file_issue(
                    IssueCode::SymlinkNotAllowed,
                    "symlinks are not allowed in a provider package",
                ));
            }
            _ => {
                return Err(file_issue(IssueCode::NotARegularFile, "not a regular file"));
            }
        },
        Err(err) => return Err(read_failed(&err)),
    }
    let bytes = match fs.read_file_limited(&path, SCENARIOS_MAX_LEN) {
        Ok(bytes) => bytes,
        Err(PlatformError::FileTooLarge { .. }) => {
            return Err(file_issue(
                IssueCode::FileTooLarge,
                format!("file is larger than {SCENARIOS_MAX_LEN} bytes"),
            ));
        }
        Err(PlatformError::NotARegularFile { .. }) => {
            return Err(file_issue(IssueCode::NotARegularFile, "not a regular file"));
        }
        Err(err) => return Err(read_failed(&err)),
    };
    if bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
        return Err(file_issue(
            IssueCode::NotUtf8,
            "file starts with a byte order mark",
        ));
    }
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|_| file_issue(IssueCode::NotUtf8, "file is not UTF-8"))
}

fn file_issue(code: IssueCode, message: impl Into<String>) -> Issue {
    Issue::new(SCENARIOS_FILE, Location::File, code, message)
}

fn read_failed(err: &PlatformError) -> Issue {
    file_issue(IssueCode::NotARegularFile, format!("cannot read: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn codes(text: &str) -> Vec<(String, IssueCode)> {
        Scenarios::parse(text)
            .unwrap_err()
            .into_iter()
            .map(|issue| (issue.location.to_string(), issue.code))
            .collect()
    }

    #[test]
    fn a_full_file_parses_in_order_with_defaults() {
        let text = "schema: stackly.provider-scenarios/v1\nscenarios:\n  - name: default\n    settings: { greeting: \"hello\" }\n  - name: slow-ready\n    settings: { readyDelayMs: 1500 }\n  - name: crash-on-start\n    settings: { readyDelayMs: 60000, crashAfterMs: 300 }\n    expect: start_fails\n  - name: bare\n";

        let scenarios = Scenarios::parse(text).unwrap();

        let names: Vec<&str> = scenarios.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["default", "slow-ready", "crash-on-start", "bare"]);
        assert_eq!(scenarios[0].expect, Expect::Ready);
        assert_eq!(scenarios[2].expect, Expect::StartFails);
        assert_eq!(
            scenarios[1].settings,
            serde_json::json!({ "readyDelayMs": 1500 })
        );
        assert_eq!(scenarios[3].settings, serde_json::json!({}));
    }

    #[test]
    fn another_schema_stops_before_the_rest_is_decoded() {
        assert_eq!(
            codes("schema: stackly.provider-scenarios/v2\nscenarios: 7\nextra: 1\n"),
            [("schema".to_owned(), IssueCode::InvalidValue)]
        );
        assert_eq!(
            codes("scenarios: []\n"),
            [("schema".to_owned(), IssueCode::MissingField)]
        );
    }

    #[test]
    fn names_must_be_short_instance_names_and_unique() {
        let long = "a".repeat(25);
        let text = format!(
            "schema: stackly.provider-scenarios/v1\nscenarios:\n  - name: Bad\n  - name: {long}\n  - name: ok\n  - name: ok\n  - name: {}\n",
            "a".repeat(24)
        );
        assert_eq!(
            codes(&text),
            [
                ("scenarios[0].name".to_owned(), IssueCode::InvalidName),
                ("scenarios[1].name".to_owned(), IssueCode::InvalidName),
                ("scenarios[3].name".to_owned(), IssueCode::DuplicateKey),
            ]
        );
    }

    #[test]
    fn an_empty_list_or_more_than_sixteen_is_rejected() {
        assert_eq!(
            codes("schema: stackly.provider-scenarios/v1\nscenarios: []\n"),
            [("scenarios".to_owned(), IssueCode::InvalidValue)]
        );
        let mut text = String::from("schema: stackly.provider-scenarios/v1\nscenarios:\n");
        for n in 0..17 {
            text.push_str(&format!("  - name: s{n}\n"));
        }
        assert_eq!(
            codes(&text),
            [("scenarios".to_owned(), IssueCode::InvalidValue)]
        );
    }

    #[test]
    fn strict_yaml_rules_apply() {
        let head = "schema: stackly.provider-scenarios/v1\n";
        assert_eq!(
            codes(&format!("{head}scenarios:\n  - name: a\n    extra: 1\n"))[0].1,
            IssueCode::UnknownField
        );
        assert_eq!(
            codes(&format!("{head}scenarios:\n  - name: a\n    name: b\n"))[0].1,
            IssueCode::DuplicateKey
        );
        assert_eq!(
            codes(&format!("{head}scenarios:\n  - name: !x a\n"))[0].1,
            IssueCode::InvalidValue
        );
        assert_eq!(
            codes(&format!(
                "{head}base: &b {{ name: a }}\nscenarios:\n  - <<: *b\n"
            ))[0]
                .1,
            IssueCode::InvalidValue
        );
        assert_eq!(
            codes(&format!("{head}scenarios:\n  - settings: {{}}\n"))[0].1,
            IssueCode::MissingField
        );
        assert_eq!(
            codes(&format!(
                "{head}scenarios:\n  - name: a\n    expect: crash\n"
            ))[0]
                .1,
            IssueCode::InvalidValue
        );
        assert_eq!(
            codes(&format!(
                "{head}scenarios:\n  - name: a\n    settings: [1]\n"
            ))[0]
                .1,
            IssueCode::InvalidValue
        );
        assert_eq!(codes("scenarios: [\n")[0].1, IssueCode::YamlSyntax);
        assert!(
            Scenarios::parse("schema: x\n")
                .unwrap_err()
                .iter()
                .all(|issue| issue.file == SCENARIOS_FILE)
        );
    }

    #[test]
    fn the_implied_default_is_ready_with_empty_settings() {
        let scenario = Scenario::implied_default();
        assert_eq!(scenario.name.as_str(), "default");
        assert_eq!(scenario.settings, serde_json::json!({}));
        assert_eq!(scenario.expect, Expect::Ready);
        assert_eq!(Expect::CreateFails.as_str(), "create_fails");
    }
}
