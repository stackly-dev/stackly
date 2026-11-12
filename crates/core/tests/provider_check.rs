//! The package validator and the scenarios file (spec 0036, build step 1):
//! `check_package` over copies of the repository packages in a test root,
//! read through the fake filesystem, and `read_scenarios` with every file
//! kind it refuses.

use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

use stackly_core::provider::{
    ApiVersionStatus, ApiVersionSupport, Expect, FindingOrigin, IssueCode, NameCheck, PackageCheck,
    PackageFiles, SUPPORTED_API_VERSIONS, Severity, ValidatedPackage, check_package,
    check_package_with_versions, read_package, read_scenarios,
};
use stackly_evidence::{Kind, evidence};
use stackly_testkit::{FakePlatform, TestRoot, make_fifo};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fixture(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(path)
}

/// Copies a folder tree with `std::fs`, as test setup.
fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).expect("the copy target can be created");
    for entry in fs::read_dir(from).expect("the package folder can be listed") {
        let entry = entry.expect("the entry can be read");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("the entry has a type").is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target).expect("the file can be copied");
        }
    }
}

/// A copy of a package under `<root>/providers/<folder name>`.
struct Copy {
    root: TestRoot,
    dir: PathBuf,
}

impl Copy {
    fn of(source: &Path) -> Self {
        let root = TestRoot::new().expect("a test root can be created");
        let name = source.file_name().expect("the package has a folder name");
        let dir = root.path().join("providers").join(name);
        copy_dir(source, &dir);
        Self { root, dir }
    }

    fn fake() -> Self {
        Self::of(&repo_root().join("providers/fake"))
    }

    fn replace(&self, file: &str, from: &str, to: &str) -> &Self {
        let path = self.dir.join(file);
        let text = fs::read_to_string(&path).expect("the package file reads");
        assert!(text.contains(from), "{file} holds {from:?}");
        fs::write(&path, text.replacen(from, to, 1)).expect("the package file writes");
        self
    }

    fn write(&self, file: &str, bytes: impl AsRef<[u8]>) -> &Self {
        let path = self.dir.join(file);
        fs::create_dir_all(path.parent().expect("a file has a parent"))
            .expect("the parent folder can be created");
        fs::write(path, bytes).expect("the file writes");
        self
    }

    /// Runs `check_package` through the fake filesystem and asserts that it
    /// wrote nothing and broke no fake rule.
    fn check(&self) -> PackageCheck {
        let platform = FakePlatform::new(&self.root);
        let before = platform.fs().mutations();
        let check = check_package(platform.fs(), &self.dir, NameCheck::FolderName);
        assert_eq!(
            platform.fs().mutations(),
            before,
            "checking a package writes nothing"
        );
        assert_eq!(platform.fs().violations(), 0);
        check
    }

    fn files(&self) -> PackageFiles {
        let platform = FakePlatform::new(&self.root);
        read_package(platform.fs(), &self.dir).expect("the package reads")
    }
}

/// `(origin, file, location, code)` of every finding.
fn findings(check: &PackageCheck) -> Vec<(FindingOrigin, String, String, IssueCode)> {
    check
        .findings
        .iter()
        .map(|finding| {
            assert_eq!(finding.severity, Severity::Error);
            (
                finding.origin,
                finding.issue.file.clone(),
                finding.issue.location.to_string(),
                finding.issue.code,
            )
        })
        .collect()
}

// covers: AC-2, AC-3 (spec 0036), AC-1 (spec 0041)
#[test]
fn every_repository_package_is_publishable_with_the_implied_default_scenario() {
    let ev = evidence!(
        covers = ["REQ-030"],
        kind = Kind::Fixture,
        expected = "check_package finds nothing in providers/fake, nats, postgresql, and traefik, reads apiVersion, gives fake, nats, and postgresql the implied default scenario, and reads the two scenarios of traefik",
    );
    for name in ["fake", "nats", "postgresql", "traefik"] {
        let copy = Copy::of(&repo_root().join("providers").join(name));

        let check = copy.check();

        assert!(check.findings.is_empty(), "{name}: {:#?}", check.findings);
        assert!(check.loadable() && check.publishable(), "{name}");
        assert_eq!(check.folder, name);
        assert_eq!(check.api_version.as_deref(), Some("stackly/v1alpha1"));
        let package = check.package.as_ref().expect("the package validated");
        let direct = ValidatedPackage::from_files(copy.files(), NameCheck::FolderName)
            .expect("from_files accepts the same package");
        assert_eq!(package.key(), direct.key(), "{name}");
        if name == "traefik" {
            // spec 0041 AC-1: the package names its own two scenarios.
            let names: Vec<&str> = check.scenarios.iter().map(|s| s.name.as_str()).collect();
            assert_eq!(names, ["default", "one-route"]);
            assert_eq!(check.scenarios[0].settings, serde_json::json!({}));
            assert_eq!(
                check.scenarios[1].settings,
                serde_json::json!({ "routes": [{
                    "name": "app",
                    "rule": "Host(`app.localhost`)",
                    "backendUrl": "http://127.0.0.1:9",
                }] })
            );
            assert!(check.scenarios.iter().all(|s| s.expect == Expect::Ready));
            continue;
        }
        assert_eq!(check.scenarios.len(), 1, "{name}");
        assert_eq!(check.scenarios[0].name.as_str(), "default");
        assert_eq!(check.scenarios[0].settings, serde_json::json!({}));
        assert_eq!(check.scenarios[0].expect, Expect::Ready);
    }
    ev.pass();
}

// covers: AC-2 (spec 0036)
#[test]
fn the_fake_command_fixture_is_loadable_but_not_publishable() {
    let ev = evidence!(
        covers = ["REQ-030"],
        kind = Kind::Fixture,
        expected = "the fake-command fixture loads through from_files unchanged, and check_package reports its init_port_issues as authoring errors, so it is loadable but not publishable",
    );
    let copy = Copy::of(&fixture("packages/fake-command"));

    let check = copy.check();

    let package = check.package.as_ref().expect("fake-command still loads");
    let authoring: Vec<_> = package
        .init_port_issues()
        .into_iter()
        .map(|issue| {
            (
                FindingOrigin::Authoring,
                issue.file,
                issue.location.to_string(),
                issue.code,
            )
        })
        .collect();
    assert_eq!(authoring.len(), 1);
    assert_eq!(authoring[0].3, IssueCode::TokenNotAllowedHere);
    assert_eq!(findings(&check), authoring);
    assert_eq!(package.authoring_findings().len(), 1);
    assert!(check.loadable());
    assert!(!check.publishable());
    assert_eq!(check.scenarios.len(), 1);
    ev.pass();
}

// covers: AC-1 (spec 0036)
#[test]
fn another_api_version_gives_one_finding_and_decodes_nothing_else() {
    let ev = evidence!(
        covers = ["AC-030", "REQ-030"],
        kind = Kind::Fixture,
        expected = "a package with apiVersion stackly/v2 gets exactly one unsupported_api_version finding at provider.yaml apiVersion that names stackly/v1alpha1, even with a broken settings schema and template, in check_package and in from_files, without a write",
    );
    let copy = Copy::fake();
    copy.replace(
        "provider.yaml",
        "apiVersion: stackly/v1alpha1",
        "apiVersion: stackly/v2",
    )
    .replace(
        "provider.yaml",
        "kind: Provider",
        "kind: Provider\nfuture: {}",
    )
    .write("schemas/settings.schema.json", "{ not json")
    .write("templates/fake.json.tmpl", "{{#each}}");

    let check = copy.check();

    assert_eq!(
        findings(&check),
        [(
            FindingOrigin::Package,
            "provider.yaml".to_owned(),
            "apiVersion".to_owned(),
            IssueCode::UnsupportedApiVersion
        )]
    );
    assert_eq!(
        check.findings[0].issue.message,
        "apiVersion \"stackly/v2\" is not supported; this Stackly supports: stackly/v1alpha1"
    );
    assert_eq!(check.api_version.as_deref(), Some("stackly/v2"));
    assert!(check.package.is_none() && check.scenarios.is_empty());
    assert!(!check.loadable() && !check.publishable());
    let err = ValidatedPackage::from_files(copy.files(), NameCheck::Skip)
        .expect_err("from_files refuses it the same way");
    assert_eq!(err.issues, [check.findings[0].issue.clone()]);
    let supported: Vec<&str> = SUPPORTED_API_VERSIONS.iter().map(|v| v.name).collect();
    assert_eq!(supported, ["stackly/v1alpha1"]);
    ev.pass();
}

// covers: AC-1 (spec 0036)
#[test]
fn an_api_version_that_is_not_a_string_or_is_missing_is_reported() {
    let copy = Copy::fake();
    copy.replace(
        "provider.yaml",
        "apiVersion: stackly/v1alpha1",
        "apiVersion: [stackly/v1alpha1]",
    );
    let check = copy.check();
    assert_eq!(check.findings.len(), 1);
    assert_eq!(
        check.findings[0].issue.message,
        "apiVersion \"<not a string>\" is not supported; this Stackly supports: stackly/v1alpha1"
    );
    assert_eq!(check.api_version, None);

    let copy = Copy::fake();
    copy.replace("provider.yaml", "apiVersion: stackly/v1alpha1\n", "");
    let check = copy.check();
    assert_eq!(
        findings(&check),
        [(
            FindingOrigin::Package,
            "provider.yaml".to_owned(),
            "apiVersion".to_owned(),
            IssueCode::MissingField
        )]
    );
}

// covers: AC-2 (spec 0036)
#[test]
fn a_package_that_cannot_be_read_or_validated_has_package_findings_only() {
    let copy = Copy::fake();
    fs::remove_file(copy.dir.join("versions.json")).unwrap();
    copy.write("tests/scenarios.yaml", "not: [valid");

    let check = copy.check();

    assert_eq!(
        findings(&check),
        [(
            FindingOrigin::Package,
            "versions.json".to_owned(),
            ".".to_owned(),
            IssueCode::MissingFile
        )]
    );
    assert!(check.package.is_none() && check.scenarios.is_empty());
    assert_eq!(check.api_version, None);

    let copy = Copy::fake();
    copy.replace("provider.yaml", "id: testing/fake", "id: testing/other");
    let check = copy.check();
    assert_eq!(check.findings.len(), 1);
    assert_eq!(check.findings[0].origin, FindingOrigin::Package);
    assert_eq!(
        check.findings[0].issue.code,
        IssueCode::DirectoryNameMismatch
    );
    assert_eq!(check.api_version.as_deref(), Some("stackly/v1alpha1"));
}

// covers: AC-3 (spec 0036)
#[test]
fn a_valid_scenarios_file_is_read_and_never_changes_the_digest() {
    let ev = evidence!(
        covers = ["REQ-030"],
        kind = Kind::Fixture,
        expected = "a valid tests/scenarios.yaml gives its scenarios in order, read_package never reads it, and adding or changing it keeps manifest_sha256",
    );
    let plain = Copy::fake();
    let without = plain.check();
    let copy = Copy::fake();
    copy.write(
        "tests/scenarios.yaml",
        fs::read(fixture("scenarios/fake.yaml")).unwrap(),
    );

    let check = copy.check();

    assert!(check.publishable(), "{:#?}", check.findings);
    let names: Vec<&str> = check.scenarios.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["default", "slow-ready", "crash-on-start"]);
    assert_eq!(check.scenarios[2].expect, Expect::StartFails);
    assert_eq!(
        check.scenarios[2].settings,
        serde_json::json!({ "readyDelayMs": 60000, "crashAfterMs": 300 })
    );
    let key = check.package.as_ref().unwrap().key().clone();
    assert_eq!(&key, without.package.as_ref().unwrap().key());
    assert!(copy.files().get("tests/scenarios.yaml").is_none());

    copy.replace("tests/scenarios.yaml", "1500", "2500");
    assert_eq!(copy.check().package.unwrap().key(), &key);
    ev.pass();
}

// covers: AC-3 (spec 0036)
#[test]
fn every_broken_scenario_rule_is_a_scenarios_error() {
    let ev = evidence!(
        covers = ["REQ-030"],
        kind = Kind::Fixture,
        expected = "a repeated name, a name that is not an instance name, a secret key, an out of range value, and an unknown key in tests/scenarios.yaml are scenarios errors at their location, and the package stays loadable",
    );
    let copy = Copy::fake();
    copy.write(
        "tests/scenarios.yaml",
        fs::read(fixture("scenarios/invalid.yaml")).unwrap(),
    );

    let check = copy.check();

    let file = "tests/scenarios.yaml".to_owned();
    let at = |location: &str, code| {
        (
            FindingOrigin::Scenarios,
            file.clone(),
            location.to_owned(),
            code,
        )
    };
    assert_eq!(
        findings(&check),
        [
            at("scenarios[1].name", IssueCode::DuplicateKey),
            at("scenarios[2].name", IssueCode::InvalidName),
        ],
        "parse issues stop before the settings are validated"
    );

    copy.write(
        "tests/scenarios.yaml",
        "schema: stackly.provider-scenarios/v1\nscenarios:\n  - name: default\n  - name: secret\n    settings: { authToken: \"local:instance/0192f000-0000-7000-8000-000000000001/x\" }\n  - name: out-of-range\n    settings: { readyDelayMs: -1, nope: 1 }\n    expect: create_fails\n",
    );
    let check = copy.check();
    assert_eq!(
        findings(&check),
        [
            at(
                "scenarios[1].settings.authToken",
                IssueCode::SecretNotAllowedHere
            ),
            at("scenarios[2].settings.nope", IssueCode::UnknownSetting),
            at(
                "scenarios[2].settings.readyDelayMs",
                IssueCode::SettingsValue
            ),
        ]
    );
    assert!(check.loadable() && !check.publishable());
    assert!(check.package.is_some() && check.scenarios.is_empty());
    for finding in &check.findings {
        assert!(
            !finding.issue.message.contains("local:instance"),
            "a message repeats a value: {}",
            finding.issue.message
        );
    }
    ev.pass();
}

// covers: AC-3 (spec 0036)
#[test]
fn read_scenarios_refuses_large_linked_special_and_non_utf8_files() {
    let ev = evidence!(
        covers = ["REQ-030"],
        kind = Kind::Fixture,
        expected = "tests/scenarios.yaml above 64 KiB, a symlink, a FIFO, a folder, a tests symlink, and a file that is not UTF-8 are refused with their code, and a missing file or folder means no scenarios",
    );
    let code = |copy: &Copy| {
        let platform = FakePlatform::new(&copy.root);
        let issue = read_scenarios(platform.fs(), &copy.dir).expect_err("the file is refused");
        assert_eq!(issue.file, "tests/scenarios.yaml");
        issue.code
    };

    let copy = Copy::fake();
    let platform = FakePlatform::new(&copy.root);
    assert_eq!(read_scenarios(platform.fs(), &copy.dir).unwrap(), None);
    fs::create_dir(copy.dir.join("tests")).unwrap();
    assert_eq!(read_scenarios(platform.fs(), &copy.dir).unwrap(), None);

    let mut large = String::from("schema: stackly.provider-scenarios/v1\n#");
    large.push_str(&"x".repeat(65 * 1024));
    copy.write("tests/scenarios.yaml", &large);
    assert_eq!(code(&copy), IssueCode::FileTooLarge);
    assert_eq!(copy.check().findings[0].issue.code, IssueCode::FileTooLarge);

    copy.write("tests/scenarios.yaml", b"schema: \xff\n");
    assert_eq!(code(&copy), IssueCode::NotUtf8);

    fs::remove_file(copy.dir.join("tests/scenarios.yaml")).unwrap();
    let elsewhere = copy.root.path().join("elsewhere.yaml");
    fs::copy(fixture("scenarios/fake.yaml"), &elsewhere).unwrap();
    symlink(&elsewhere, copy.dir.join("tests/scenarios.yaml")).unwrap();
    assert_eq!(code(&copy), IssueCode::SymlinkNotAllowed);

    fs::remove_file(copy.dir.join("tests/scenarios.yaml")).unwrap();
    make_fifo(&copy.dir.join("tests/scenarios.yaml")).unwrap();
    assert_eq!(code(&copy), IssueCode::NotARegularFile);

    fs::remove_file(copy.dir.join("tests/scenarios.yaml")).unwrap();
    fs::create_dir(copy.dir.join("tests/scenarios.yaml")).unwrap();
    assert_eq!(code(&copy), IssueCode::NotARegularFile);

    let linked = Copy::fake();
    let real_tests = linked.root.path().join("real-tests");
    fs::create_dir(&real_tests).unwrap();
    fs::copy(
        fixture("scenarios/fake.yaml"),
        real_tests.join("scenarios.yaml"),
    )
    .unwrap();
    symlink(&real_tests, linked.dir.join("tests")).unwrap();
    assert_eq!(code(&linked), IssueCode::SymlinkNotAllowed);
    assert!(!linked.check().publishable());
    ev.pass();
}

// covers: AC-1, AC-2 (spec 0036)
#[test]
fn a_package_on_a_deprecated_version_still_loads_and_publishes_with_one_warning() {
    let ev = evidence!(
        covers = ["REQ-030"],
        kind = Kind::Fixture,
        expected = "with a version table that marks stackly/v1alpha1 deprecated and stackly/v2 current, check_package on providers/fake gives one deprecated_api_version warning with origin package, and the package is still loadable and publishable with its scenarios",
    );
    let copy = Copy::fake();
    let table = [
        ApiVersionSupport {
            name: "stackly/v1alpha1",
            status: ApiVersionStatus::Deprecated,
        },
        ApiVersionSupport {
            name: "stackly/v2",
            status: ApiVersionStatus::Current,
        },
    ];
    let platform = FakePlatform::new(&copy.root);

    let check =
        check_package_with_versions(platform.fs(), &copy.dir, NameCheck::FolderName, &table);

    assert_eq!(check.findings.len(), 1, "{:#?}", check.findings);
    let finding = &check.findings[0];
    assert_eq!(finding.severity, Severity::Warning);
    assert_eq!(finding.origin, FindingOrigin::Package);
    assert_eq!(finding.issue.code, IssueCode::DeprecatedApiVersion);
    assert!(
        check.loadable(),
        "a warning never makes a package unloadable"
    );
    assert!(check.publishable(), "a warning never blocks publishing");
    assert!(check.package.is_some());
    assert!(!check.scenarios.is_empty());
    ev.pass();
}
