//! The package validator (spec 0036, *Validator*): one function that the
//! xtask, the daemon, the CLI, the harness, the spike, and import (feature
//! 37) all call, so no two validators can disagree.
//!
//! [`check_package`] reads only through the `Filesystem` port. It never
//! runs a process, writes a file, resolves a secret, or touches the
//! network, so it is safe on a package nobody approved yet.

use std::ffi::OsStr;
use std::path::Path;

use stackly_platform::Filesystem;

use super::issue::{Issue, IssueCode, Location};
use super::package::{MANIFEST_FILE, NameCheck, PackageFiles, read_package};
use super::raw::{API_VERSION, parse_yaml};
use super::scenarios::{Scenario, Scenarios, read_scenarios};
use super::validate::ValidatedPackage;

/// One `apiVersion` a build of Stackly loads, with its status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApiVersionSupport {
    /// The `apiVersion` value, such as `stackly/v1alpha1`.
    pub name: &'static str,
    /// Whether new packages should use it.
    pub status: ApiVersionStatus,
}

/// The status of a supported `apiVersion` (spec 0036, *Kebijakan
/// kompatibilitas*).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiVersionStatus {
    /// The version for new packages.
    Current,
    /// Still loads and runs; `check_package` adds one warning, and the repo
    /// check refuses it for packages in `providers/`.
    Deprecated,
}

impl ApiVersionStatus {
    /// Returns the status as a lowercase word, `current` or `deprecated`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Current => "current",
            Self::Deprecated => "deprecated",
        }
    }
}

/// Every `apiVersion` this build loads. A manifest whose `apiVersion` is
/// not named here gets one `unsupported_api_version` issue and nothing
/// else of `provider.yaml` is decoded.
pub const SUPPORTED_API_VERSIONS: &[ApiVersionSupport] = &[ApiVersionSupport {
    name: API_VERSION,
    status: ApiVersionStatus::Current,
}];

/// Returns whether `value` names a version of [`SUPPORTED_API_VERSIONS`].
pub(crate) fn is_supported_api_version(value: &str) -> bool {
    SUPPORTED_API_VERSIONS
        .iter()
        .any(|version| version.name == value)
}

/// Longest `apiVersion` text the `unsupported_api_version` message repeats,
/// in characters, so a hostile value cannot flood a report line.
const SHOWN_API_VERSION_MAX: usize = 128;

/// The message of `unsupported_api_version`: `apiVersion "<value>" is not
/// supported; this Stackly supports: <names>`, where `<value>` is the
/// string, or `<not a string>` for any other YAML value.
pub(crate) fn unsupported_api_version_message(value: Option<&str>) -> String {
    let shown = value.map_or_else(
        || "<not a string>".to_owned(),
        |text| {
            let mut shown = String::new();
            for ch in text.chars().take(SHOWN_API_VERSION_MAX) {
                if ch.is_control() || ch == '"' {
                    shown.extend(ch.escape_default());
                } else {
                    shown.push(ch);
                }
            }
            if text.chars().count() > SHOWN_API_VERSION_MAX {
                shown.push_str("...");
            }
            shown
        },
    );
    let names: Vec<&str> = SUPPORTED_API_VERSIONS
        .iter()
        .map(|version| version.name)
        .collect();
    format!(
        "apiVersion \"{shown}\" is not supported; this Stackly supports: {}",
        names.join(", ")
    )
}

/// How serious a [`Finding`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Severity {
    /// The package is not publishable.
    Error,
    /// The package is publishable, but something should change.
    Warning,
}

/// Which rule set a [`Finding`] comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FindingOrigin {
    /// [`read_package`] or [`ValidatedPackage::from_files`]: the package
    /// cannot load anywhere.
    Package,
    /// A tightening for package authors that `from_files` never applies, so
    /// pinned copies of older packages still load
    /// ([`ValidatedPackage::authoring_findings`]).
    Authoring,
    /// `tests/scenarios.yaml`.
    Scenarios,
}

/// One problem [`check_package`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// How serious it is.
    pub severity: Severity,
    /// Which rule set found it.
    pub origin: FindingOrigin,
    /// The issue, with the file, location, code, and message of spec 0009.
    pub issue: Issue,
}

impl Finding {
    /// Creates an error finding.
    #[must_use]
    pub const fn error(origin: FindingOrigin, issue: Issue) -> Self {
        Self {
            severity: Severity::Error,
            origin,
            issue,
        }
    }

    /// Creates a warning finding.
    #[must_use]
    pub const fn warning(origin: FindingOrigin, issue: Issue) -> Self {
        Self {
            severity: Severity::Warning,
            origin,
            issue,
        }
    }
}

/// What [`check_package`] found in one package folder.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct PackageCheck {
    /// The last component of the folder path.
    pub folder: String,
    /// The raw `apiVersion` string of `provider.yaml`, or `None` when the
    /// package could not be read or the field is absent or not a string.
    pub api_version: Option<String>,
    /// The validated package, or `None` when it has a `package` finding.
    pub package: Option<ValidatedPackage>,
    /// The scenarios of a conformance run: those of `tests/scenarios.yaml`,
    /// or the implied `default` without that file. Empty when the package
    /// does not validate or the file has a finding.
    pub scenarios: Vec<Scenario>,
    /// Every finding: `package`, then `authoring`, then `scenarios`, each
    /// in the order its source reports them.
    pub findings: Vec<Finding>,
}

impl PackageCheck {
    /// Returns whether the package loads: no finding has origin `package`.
    #[must_use]
    pub fn loadable(&self) -> bool {
        !self
            .findings
            .iter()
            .any(|finding| finding.origin == FindingOrigin::Package)
    }

    /// Returns whether the package may be published: no finding is an
    /// error.
    #[must_use]
    pub fn publishable(&self) -> bool {
        !self
            .findings
            .iter()
            .any(|finding| finding.severity == Severity::Error)
    }
}

impl ValidatedPackage {
    /// Returns the tightenings for package authors that
    /// [`from_files`](Self::from_files) never applies, each as an `error`
    /// with origin `authoring`. Today that is every issue of
    /// [`init_port_issues`](Self::init_port_issues), in its order.
    #[must_use]
    pub fn authoring_findings(&self) -> Vec<Finding> {
        self.init_port_issues()
            .into_iter()
            .map(|issue| Finding::error(FindingOrigin::Authoring, issue))
            .collect()
    }
}

/// Validates the provider package in `dir` without running anything.
///
/// The steps are: [`read_package`]; the raw `apiVersion`;
/// [`ValidatedPackage::from_files`] with `name_check`;
/// [`ValidatedPackage::authoring_findings`]; and [`read_scenarios`] with
/// [`Scenarios::parse`] and [`Scenarios::validate`] against the package's
/// settings schema. A failed read or validation ends the check with its
/// `package` findings. A package without `tests/scenarios.yaml` gets the
/// implied [`Scenario::implied_default`].
///
/// Only [`Filesystem::symlink_metadata`], [`Filesystem::read_dir`], and
/// [`Filesystem::read_file_limited`] are called.
#[must_use]
pub fn check_package(fs: &dyn Filesystem, dir: &Path, name_check: NameCheck) -> PackageCheck {
    let mut check = PackageCheck {
        folder: dir
            .file_name()
            .and_then(OsStr::to_str)
            .unwrap_or_default()
            .to_owned(),
        api_version: None,
        package: None,
        scenarios: Vec::new(),
        findings: Vec::new(),
    };
    let files = match read_package(fs, dir) {
        Ok(files) => files,
        Err(err) => {
            check.push_all(FindingOrigin::Package, err.issues);
            return check;
        }
    };
    check.api_version = raw_api_version(&files);
    let package = match ValidatedPackage::from_files(files, name_check) {
        Ok(package) => package,
        Err(err) => {
            check.push_all(FindingOrigin::Package, err.issues);
            return check;
        }
    };
    check.findings.extend(deprecated_api_version(
        check.api_version.as_deref(),
        SUPPORTED_API_VERSIONS,
    ));
    check.findings.extend(package.authoring_findings());
    match read_scenarios(fs, dir) {
        Ok(None) => check.scenarios = vec![Scenario::implied_default()],
        Ok(Some(text)) => match Scenarios::parse(&text) {
            Ok(scenarios) => match Scenarios::validate(&scenarios, package.settings_schema()) {
                Ok(()) => check.scenarios = scenarios,
                Err(issues) => check.push_all(FindingOrigin::Scenarios, issues),
            },
            Err(issues) => check.push_all(FindingOrigin::Scenarios, issues),
        },
        Err(issue) => check.push_all(FindingOrigin::Scenarios, [issue]),
    }
    check.package = Some(package);
    check
}

impl PackageCheck {
    fn push_all(&mut self, origin: FindingOrigin, issues: impl IntoIterator<Item = Issue>) {
        self.findings.extend(
            issues
                .into_iter()
                .map(|issue| Finding::error(origin, issue)),
        );
    }
}

/// The `apiVersion` string of a read package's `provider.yaml`, when it
/// parses and the field is a string.
fn raw_api_version(files: &PackageFiles) -> Option<String> {
    let text = std::str::from_utf8(files.get(MANIFEST_FILE)?).ok()?;
    let value = parse_yaml(MANIFEST_FILE, text).ok()?;
    value.get("apiVersion")?.as_str().map(str::to_owned)
}

/// The one `warning` of a package whose `apiVersion` has the status
/// [`ApiVersionStatus::Deprecated`] in `table`, at `provider.yaml` field
/// `apiVersion`; none for a current or unknown version. `table` is
/// [`SUPPORTED_API_VERSIONS`] outside tests.
fn deprecated_api_version(
    api_version: Option<&str>,
    table: &[ApiVersionSupport],
) -> Option<Finding> {
    let name = api_version?;
    let version = table.iter().find(|version| version.name == name)?;
    if version.status != ApiVersionStatus::Deprecated {
        return None;
    }
    let current: Vec<&str> = table
        .iter()
        .filter(|version| version.status == ApiVersionStatus::Current)
        .map(|version| version.name)
        .collect();
    Some(Finding::warning(
        FindingOrigin::Package,
        Issue::new(
            MANIFEST_FILE,
            Location::Field("apiVersion".to_owned()),
            IssueCode::DeprecatedApiVersion,
            format!(
                "apiVersion \"{name}\" is deprecated; new packages use: {}",
                current.join(", ")
            ),
        ),
    ))
}

/// The issue for an `apiVersion` this build does not load, at
/// `provider.yaml` field `apiVersion`.
pub(crate) fn unsupported_api_version_issue(value: Option<&str>) -> Issue {
    Issue::new(
        MANIFEST_FILE,
        Location::Field("apiVersion".to_owned()),
        IssueCode::UnsupportedApiVersion,
        unsupported_api_version_message(value),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_holds_exactly_v1alpha1_as_current() {
        assert_eq!(
            SUPPORTED_API_VERSIONS,
            [ApiVersionSupport {
                name: "stackly/v1alpha1",
                status: ApiVersionStatus::Current,
            }]
        );
        assert!(is_supported_api_version("stackly/v1alpha1"));
        assert!(!is_supported_api_version("stackly/v2"));
    }

    #[test]
    fn a_deprecated_api_version_gives_one_warning_naming_the_current_one() {
        let table = [
            ApiVersionSupport {
                name: "stackly/v2",
                status: ApiVersionStatus::Current,
            },
            ApiVersionSupport {
                name: "stackly/v1alpha1",
                status: ApiVersionStatus::Deprecated,
            },
        ];

        let finding = deprecated_api_version(Some("stackly/v1alpha1"), &table)
            .expect("a deprecated version warns");

        assert_eq!(finding.severity, Severity::Warning);
        assert_eq!(finding.origin, FindingOrigin::Package);
        assert_eq!(finding.issue.code, IssueCode::DeprecatedApiVersion);
        assert_eq!(finding.issue.file, MANIFEST_FILE);
        assert_eq!(
            finding.issue.message,
            "apiVersion \"stackly/v1alpha1\" is deprecated; new packages use: stackly/v2"
        );
        assert_eq!(deprecated_api_version(Some("stackly/v2"), &table), None);
        assert_eq!(deprecated_api_version(Some("stackly/v9"), &table), None);
        assert_eq!(deprecated_api_version(None, &table), None);
        assert_eq!(
            deprecated_api_version(Some("stackly/v1alpha1"), SUPPORTED_API_VERSIONS),
            None,
            "nothing is deprecated in this build"
        );
    }

    #[test]
    fn the_message_names_the_value_and_every_supported_version() {
        assert_eq!(
            unsupported_api_version_message(Some("stackly/v2")),
            "apiVersion \"stackly/v2\" is not supported; this Stackly supports: stackly/v1alpha1"
        );
        assert_eq!(
            unsupported_api_version_message(None),
            "apiVersion \"<not a string>\" is not supported; this Stackly supports: stackly/v1alpha1"
        );
    }

    #[test]
    fn the_message_escapes_control_characters_and_cuts_long_values() {
        let message = unsupported_api_version_message(Some("a\"b\nc"));
        assert!(
            message.starts_with("apiVersion \"a\\\"b\\nc\" is"),
            "{message}"
        );
        let long = "x".repeat(500);
        let message = unsupported_api_version_message(Some(&long));
        assert!(
            message.contains(&format!("{}...\"", "x".repeat(128))),
            "{message}"
        );
    }
}
