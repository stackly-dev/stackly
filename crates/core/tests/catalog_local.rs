//! Local provider packages (spec 0037, build step 2): the package digest,
//! the permission summary and its changes, the import rules over a
//! `check_package` result, the import limits, the path rule, and the store
//! copy writer in import mode. Packages are copies of the `acme-*` fixtures
//! in a test root, read through the fake filesystem.
#![cfg(unix)]

use std::fs;
use std::os::unix::fs::{MetadataExt, symlink};
use std::path::{Path, PathBuf};

use stackly_core::catalog::local::{
    ExecutableUse, ImportCheck, ImportLimit, ImportPathError, LOCAL_BYTES_MAX, LOCAL_PROVIDERS_MAX,
    PermissionChange, Permissions, check_import_path, import_limit, package_bytes, package_sha256,
    permission_changes,
};
use stackly_core::catalog::{CatalogOptions, StoreError, StoreMode, write_package};
use stackly_core::provider::{
    FindingOrigin, IssueCode, Location, NameCheck, PackageCheck, Severity, check_package,
};
use stackly_core::{PreparedLayout, prepare_layout};
use stackly_domain::{Arch, Capability, Os, ProviderImportRecord, ProviderName, Sha256Digest};
use stackly_platform::Platform;
use stackly_testkit::{FakePlatform, Fault, FsOp, TestRoot};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/packages")
        .join(name)
}

fn repo(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(path)
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).expect("the copy target can be made");
    for entry in fs::read_dir(from).expect("the package folder lists") {
        let entry = entry.expect("an entry reads");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("the entry has a type").is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target).expect("a file copies");
        }
    }
}

/// A test root with the fake platform and copies of packages in `input/`.
struct Site {
    root: TestRoot,
    fake: FakePlatform,
    platform: Platform,
    prepared: PreparedLayout,
}

impl Site {
    fn new() -> Self {
        let root = TestRoot::new().expect("a test root can be made");
        let fake = FakePlatform::new(&root);
        let platform = fake.platform();
        let prepared = prepare_layout(&platform).expect("the fake roots prepare");
        Self {
            root,
            fake,
            platform,
            prepared,
        }
    }

    /// Copies `source` to `input/<name>` and returns the copy.
    fn copy(&self, source: &Path, name: &str) -> PathBuf {
        let dir = self.root.path().join("input").join(name);
        copy_dir(source, &dir);
        dir
    }

    /// Copies a fixture and edits one of its files.
    fn edited(
        &self,
        fixture_name: &str,
        name: &str,
        file: &str,
        edit: impl Fn(String) -> String,
    ) -> PathBuf {
        let dir = self.copy(&fixture(fixture_name), name);
        let path = dir.join(file);
        let text = fs::read_to_string(&path).expect("the file reads");
        fs::write(&path, edit(text)).expect("the file writes");
        dir
    }

    fn check(&self, dir: &Path) -> PackageCheck {
        check_package(self.platform.fs(), dir, NameCheck::Skip)
    }

    fn import(&self, dir: &Path, options: CatalogOptions) -> ImportCheck {
        ImportCheck::new(self.check(dir), options, None)
    }
}

fn no_testing() -> CatalogOptions {
    CatalogOptions::default()
}

fn testing() -> CatalogOptions {
    CatalogOptions {
        allow_testing_namespace: true,
    }
}

fn codes(check: &ImportCheck) -> Vec<(IssueCode, String)> {
    check
        .import_issues
        .iter()
        .map(|issue| {
            let location = match &issue.location {
                Location::Field(field) => field.clone(),
                other => format!("{other:?}"),
            };
            (issue.code, location)
        })
        .collect()
}

fn permissions_of(site: &Site, dir: &Path) -> (Permissions, stackly_domain::RevisionLabel) {
    let check = site.import(dir, no_testing());
    let package = check.package().expect("the package validates");
    (
        check
            .permissions
            .clone()
            .expect("a valid package has permissions"),
        package.key().revision().clone(),
    )
}

// covers: AC-1, AC-2 (spec 0037)
#[test]
fn the_demo_package_is_importable_with_its_digests_and_permissions() {
    let site = Site::new();
    let dir = site.copy(&fixture("acme-demo"), "acme-demo");

    let check = site.import(&dir, no_testing());

    assert!(
        check.valid(),
        "{:?} {:?}",
        check.findings(),
        check.import_issues
    );
    let package = check.package().expect("it validates");
    assert_eq!(package.key().provider().as_str(), "acme/demo");
    assert_eq!(check.package_sha256, Some(package_sha256(package.files())));
    assert_eq!(check.package_bytes, package_bytes(package.files()));
    assert!(check.package_bytes > 0);
    let permissions = check.permissions.expect("it has permissions");
    assert_eq!(
        permissions.capabilities,
        [Capability::MultiInstance, Capability::Persistence]
    );
    let dirs: Vec<&str> = permissions
        .managed_filesystem
        .iter()
        .map(|dir| dir.as_code())
        .collect();
    assert_eq!(
        dirs,
        ["config", "data", "logs", "run", "temp"],
        "ordered by code"
    );
    let executables: Vec<(&str, &str, Vec<ExecutableUse>)> = permissions
        .executables
        .iter()
        .map(|executable| {
            (
                executable.name.as_str(),
                executable.path.as_str(),
                executable.uses.clone(),
            )
        })
        .collect();
    assert_eq!(
        executables,
        [
            (
                "runtime",
                "bin/demo-server",
                vec![ExecutableUse::Start, ExecutableUse::VersionCheck]
            ),
            (
                "demoCli",
                "bin/demo-cli",
                vec![ExecutableUse::Probe, ExecutableUse::OpenWith]
            ),
        ]
    );
    assert_eq!(permissions.secrets, ["password"]);
    assert_eq!(permissions.endpoints.len(), 1);
    assert_eq!(permissions.endpoints[0].name, "client");
    assert_eq!(permissions.endpoints[0].transport, "tcp");
    assert_eq!(permissions.endpoints[0].purpose, "client");
    assert_eq!(permissions.endpoints[0].default_port, Some(7400));
    assert_eq!(permissions.downloads.len(), 1);
    let download = &permissions.downloads[0];
    assert_eq!(download.artifact, "demo-1.0.0-macos-aarch64");
    assert_eq!(download.version, "1.0.0");
    assert_eq!((download.os, download.arch), (Os::Macos, Arch::Aarch64));
    assert_eq!(download.size, 1_048_576);
    assert_eq!(
        download.hosts,
        ["downloads.example.test", "mirror.example.test"]
    );
    assert_eq!(download.signing_policy, "keep_or_adhoc");
    assert_eq!(permissions.open_with.len(), 1);
    assert_eq!(permissions.open_with[0].name, "shell");
    assert_eq!(permissions.open_with[0].kind, "terminal");
    assert_eq!(permissions.open_with[0].target.as_deref(), Some("demoCli"));
}

#[test]
fn permissions_round_trip_through_their_stored_json() {
    let site = Site::new();
    let dir = site.copy(&fixture("acme-demo"), "acme-demo");
    let (permissions, _) = permissions_of(&site, &dir);

    let json = permissions.to_json();

    assert!(
        json.starts_with(
            r#"{"capabilities":["multi_instance","persistence"],"managedFilesystem":"#
        )
    );
    assert_eq!(Permissions::from_json(&json).unwrap(), permissions);
    assert!(Permissions::from_json(r#"{"capabilities":[]}"#).is_err());
    assert!(
        Permissions::from_json(&json.replace("multi_instance", "teleport")).is_err(),
        "an unknown code is refused"
    );
}

// covers: AC-6 (spec 0037)
#[test]
fn every_byte_change_changes_the_package_digest_and_the_changes_name_what_moved() {
    let site = Site::new();
    let demo = site.copy(&fixture("acme-demo"), "acme-demo");
    let next = site.copy(&fixture("acme-demo-next"), "acme-demo-next");
    let template = site.copy(&fixture("acme-demo-template"), "acme-demo-template");
    let versions_only = site.edited("acme-demo", "acme-demo-versions", "versions.json", |text| {
        text.replace(&"1".repeat(64), &"3".repeat(64))
    });
    let digest = |dir: &Path| {
        site.import(dir, no_testing())
            .package_sha256
            .expect("valid")
    };
    let manifest = |dir: &Path| {
        *site
            .import(dir, no_testing())
            .package()
            .expect("valid")
            .key()
            .manifest_sha256()
    };
    let digests = [
        digest(&demo),
        digest(&next),
        digest(&template),
        digest(&versions_only),
    ];
    for (index, a) in digests.iter().enumerate() {
        for b in &digests[index + 1..] {
            assert_ne!(a, b);
        }
    }
    assert_eq!(
        manifest(&demo),
        manifest(&versions_only),
        "versions.json is outside the manifest digest"
    );

    let (decided, decided_revision) = permissions_of(&site, &demo);
    let changes = |dir: &Path| {
        let (current, revision) = permissions_of(&site, dir);
        permission_changes(&decided, &decided_revision, &current, &revision)
    };
    assert_eq!(
        changes(&next),
        [
            PermissionChange::Revision,
            PermissionChange::Downloads,
            PermissionChange::DownloadHosts
        ]
    );
    assert_eq!(changes(&template), [PermissionChange::Content]);
    assert_eq!(changes(&versions_only), [PermissionChange::Downloads]);
    let codes: Vec<&str> = PermissionChange::ALL
        .iter()
        .map(|change| change.as_code())
        .collect();
    assert_eq!(
        codes,
        [
            "revision",
            "executables",
            "downloads",
            "download_hosts",
            "capabilities",
            "filesystem",
            "secrets",
            "endpoints",
            "open_with",
            "content"
        ]
    );
}

// covers: AC-2 (spec 0037)
#[test]
fn reserved_namespaces_the_testing_category_http_urls_and_fixture_artifacts_are_refused() {
    let site = Site::new();
    for namespace in ["official", "stackly"] {
        let dir = site.edited("acme-demo", namespace, "provider.yaml", |text| {
            text.replace("id: acme/demo", &format!("id: {namespace}/demo"))
        });
        let check = site.import(&dir, no_testing());
        assert!(!check.valid(), "{namespace}");
        assert!(
            check.package().is_some(),
            "{namespace}: {:?}",
            check.findings()
        );
        assert_eq!(
            codes(&check),
            [(IssueCode::NamespaceNotAllowed, "metadata.id".to_owned())],
            "{namespace}"
        );
    }
    // The validator wants category testing exactly with namespace testing.
    let testing_dir = site.edited("acme-demo", "testing", "provider.yaml", |text| {
        text.replace("id: acme/demo", "id: testing/demo")
            .replace("category: database", "category: testing")
    });
    assert_eq!(
        codes(&site.import(&testing_dir, no_testing())),
        [
            (IssueCode::NamespaceNotAllowed, "metadata.id".to_owned()),
            (IssueCode::InvalidValue, "metadata.category".to_owned())
        ]
    );
    assert!(site.import(&testing_dir, testing()).valid());
    let http = site.edited("acme-demo", "http", "versions.json", |text| {
        text.replace(
            "https://mirror.example.test/",
            "http://mirror.example.test/",
        )
    });
    // The loader already refuses an http URL; the import rule stays as a
    // second line for a package that reaches it another way.
    let check = site.import(&http, no_testing());
    assert!(!check.valid());
    assert!(
        check
            .findings()
            .iter()
            .any(|finding| finding.issue.code == IssueCode::VersionsRule
                && finding.issue.location == Location::Field("artifacts[0].urls".into())),
        "{:?}",
        check.findings()
    );
    let userinfo = site.edited("acme-demo", "userinfo", "versions.json", |text| {
        text.replace(
            "https://mirror.example.test/",
            "https://user@mirror.example.test/",
        )
    });
    let check = site.import(&userinfo, no_testing());
    assert!(check.package().is_some(), "{:?}", check.findings());
    assert_eq!(
        codes(&check),
        [(IssueCode::VersionsRule, "artifacts[0].urls[1]".to_owned())],
        "a URL that is not an HttpsUrl of spec 0018 is refused"
    );

    let fake = site.copy(&repo("providers/fake"), "fake");
    let refused = site.import(&fake, no_testing());
    let fake_codes = codes(&refused);
    assert!(
        fake_codes.contains(&(IssueCode::NamespaceNotAllowed, "metadata.id".to_owned()))
            && fake_codes.contains(&(IssueCode::InvalidValue, "metadata.category".to_owned()))
            && fake_codes
                .iter()
                .any(|(code, at)| *code == IssueCode::VersionsRule && at.ends_with(".status")),
        "{fake_codes:?}"
    );
    let accepted = site.import(&fake, testing());
    assert!(
        accepted.valid(),
        "the test option accepts the testing namespace, category, and fixture artifacts: {:?}",
        accepted.import_issues
    );
}

// covers: AC-2 (spec 0037)
#[test]
fn more_than_32_artifacts_break_the_catalog_limit() {
    let site = Site::new();
    let dir = site.edited("acme-demo", "many", "versions.json", |text| {
        let mut json: serde_json::Value = serde_json::from_str(&text).unwrap();
        let artifact = json["artifacts"][0].clone();
        let artifacts = (0..33)
            .map(|index| {
                let mut copy = artifact.clone();
                copy["id"] = format!("demo-1.0.{index}-macos-aarch64").into();
                copy["version"] = format!("1.0.{index}").into();
                if index > 0 {
                    copy.as_object_mut().unwrap().remove("stable");
                }
                copy
            })
            .collect();
        json["artifacts"] = serde_json::Value::Array(artifacts);
        serde_json::to_string_pretty(&json).unwrap()
    });

    let check = site.import(&dir, no_testing());
    assert!(check.package().is_some(), "{:?}", check.findings());

    let limit: Vec<_> = check
        .import_issues
        .iter()
        .filter(|issue| issue.code == IssueCode::CatalogLimit)
        .map(|issue| (issue.reason, issue.location.clone()))
        .collect();
    assert_eq!(
        limit,
        [(Some("artifacts"), Location::Field("artifacts".into()))],
        "{:?}",
        check.import_issues
    );
    assert!(!check.valid());
}

// covers: AC-2 (spec 0037)
#[test]
fn package_and_authoring_errors_refuse_and_scenarios_findings_are_dropped() {
    let site = Site::new();
    let port_in_init = site.edited("acme-demo", "init-port", "provider.yaml", |text| {
        text.replace(
            "    - { primitive: renderConfig, file: demoConf }\n",
            "    - { primitive: renderConfig, file: demoConf }\n    - primitive: runExecutable\n      executable: \"${runtime.executable}\"\n      args: [\"--port=${endpoint.client.port}\"]\n",
        )
    });
    let check = site.import(&port_in_init, no_testing());
    assert!(check.check.loadable(), "an authoring finding still loads");
    assert!(
        check
            .findings()
            .iter()
            .any(|finding| finding.origin == FindingOrigin::Authoring
                && finding.severity == Severity::Error)
    );
    assert!(!check.valid(), "init_port_issues refuses the import");

    let api = site.edited("acme-demo", "api", "provider.yaml", |text| {
        text.replace("apiVersion: stackly/v1alpha1", "apiVersion: stackly/v9")
    });
    let check = site.import(&api, no_testing());
    assert!(check.package().is_none());
    assert!(check.permissions.is_none() && check.package_sha256.is_none());
    assert!(
        check
            .findings()
            .iter()
            .any(|finding| finding.issue.code == IssueCode::UnsupportedApiVersion)
    );
    assert!(!check.valid());

    let scenarios = site.copy(&fixture("acme-demo"), "scenarios");
    fs::create_dir_all(scenarios.join("tests")).unwrap();
    fs::write(scenarios.join("tests/scenarios.yaml"), "not: [valid").unwrap();
    let raw = site.check(&scenarios);
    assert!(
        raw.findings
            .iter()
            .any(|finding| finding.origin == FindingOrigin::Scenarios),
        "the raw check reports the broken scenarios file"
    );
    let check = ImportCheck::new(raw, no_testing(), None);
    assert!(
        check.valid(),
        "an import never uses tests/: {:?}",
        check.findings()
    );
    assert!(check.check.scenarios.is_empty());
}

fn row(name: &str, bytes: u64) -> ProviderImportRecord {
    let digest: Sha256Digest = format!("{:0>64}", format!("{:x}", name.len() + 1))
        .parse()
        .expect("the fixture value parses");
    ProviderImportRecord::new(
        name.parse().expect("the fixture value parses"),
        "0199f000-0000-7000-8000-000000000010"
            .parse()
            .expect("the fixture value parses"),
        "1.0.0".parse().expect("the fixture value parses"),
        "demo".parse().expect("the fixture value parses"),
        digest,
        digest,
        digest,
        bytes,
        "~/p".to_owned(),
        "2026-11-11T00:00:00Z"
            .parse()
            .expect("the fixture value parses"),
    )
}

// covers: AC-2 (spec 0037)
#[test]
fn the_import_limits_count_32_providers_and_16_mib_with_a_replace_counting_the_difference() {
    let full: Vec<ProviderImportRecord> = (0..LOCAL_PROVIDERS_MAX)
        .map(|index| row(&format!("acme/p{index}"), 1))
        .collect();
    let new: ProviderName = "acme/new".parse().unwrap();
    assert_eq!(import_limit(&full, &new, 1), Some(ImportLimit::Count));
    assert_eq!(
        import_limit(&full, &"acme/p3".parse().unwrap(), 1),
        None,
        "replacing one of the 32 adds none"
    );
    assert_eq!(import_limit(&full[..31], &new, 1), None);

    let big = vec![row("acme/big", LOCAL_BYTES_MAX - 10)];
    assert_eq!(import_limit(&big, &new, 10), None);
    assert_eq!(import_limit(&big, &new, 11), Some(ImportLimit::Bytes));
    assert_eq!(
        import_limit(&big, &"acme/big".parse().unwrap(), LOCAL_BYTES_MAX),
        None,
        "a replace counts only its own new size"
    );
    assert_eq!(
        (ImportLimit::Count.as_code(), ImportLimit::Count.max()),
        ("count", 32)
    );
    assert_eq!(
        (ImportLimit::Bytes.as_code(), ImportLimit::Bytes.max()),
        ("bytes", 16 * 1024 * 1024)
    );
}

// covers: AC-2 (spec 0037)
#[test]
fn an_import_path_is_absolute_utf8_short_and_never_normalized() {
    assert_eq!(
        check_import_path(Path::new("/Users/me/acme-demo")),
        Ok("/Users/me/acme-demo")
    );
    assert_eq!(
        check_import_path(Path::new("acme-demo")),
        Err(ImportPathError::NotAbsolute)
    );
    for dotted in ["/Users/me/./acme", "/Users/me/../acme", "/Users/me/acme/.."] {
        assert_eq!(
            check_import_path(Path::new(dotted)),
            Err(ImportPathError::NotNormalized),
            "{dotted}"
        );
    }
    let long = format!("/{}", "a".repeat(1024));
    assert_eq!(
        check_import_path(Path::new(&long)),
        Err(ImportPathError::TooLong)
    );
    {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;
        assert_eq!(
            check_import_path(Path::new(OsStr::from_bytes(b"/opt/\xff"))),
            Err(ImportPathError::NotUtf8)
        );
    }
}

// covers: AC-13 (spec 0037)
#[test]
fn a_package_folder_that_is_a_symlink_is_read_as_nothing() {
    let site = Site::new();
    let real = site.copy(&fixture("acme-demo"), "acme-demo");
    let link = site.root.path().join("input/link");
    symlink(&real, &link).unwrap();

    let check = site.check(&link);

    assert!(
        check.package.is_none(),
        "a symlinked folder does not validate"
    );
}

/// Stores the demo package with the import mode, returning the folder.
fn store(
    site: &Site,
    files: &stackly_core::provider::PackageFiles,
    digest: &Sha256Digest,
) -> Result<stackly_core::catalog::Stored, StoreError> {
    let layout = site.prepared.layout();
    write_package(
        &site.platform,
        &site.prepared,
        &layout.provider_imports_dir(),
        &layout.provider_import_dir(digest),
        files,
        StoreMode::Import(digest),
    )
}

// covers: AC-3 (spec 0037)
#[test]
fn the_store_writes_a_private_copy_that_reads_back_with_the_package_digest() {
    let site = Site::new();
    let dir = site.copy(&fixture("acme-demo"), "acme-demo");
    let check = site.import(&dir, no_testing());
    let package = check.package().expect("it validates");
    let digest = check.package_sha256.expect("it has a digest");
    let before = fs::read(dir.join("provider.yaml")).unwrap();

    let stored = store(&site, package.files(), &digest).expect("the copy is written");

    assert!(stored.created_dir && stored.wrote_files);
    let target = site.prepared.layout().provider_import_dir(&digest);
    assert_eq!(fs::metadata(&target).unwrap().mode() & 0o777, 0o700);
    assert_eq!(
        fs::metadata(target.join("templates")).unwrap().mode() & 0o777,
        0o700
    );
    for (path, bytes) in package.files().iter() {
        let file = target.join(path);
        assert_eq!(fs::read(&file).unwrap(), bytes, "{path}");
        assert_eq!(fs::metadata(&file).unwrap().mode() & 0o777, 0o600, "{path}");
    }
    assert_eq!(
        fs::read(dir.join("provider.yaml")).unwrap(),
        before,
        "the origin is untouched"
    );

    let mutations = site.fake.fs().mutations();
    let again = store(&site, package.files(), &digest).expect("the same bytes store again");
    assert!(!again.created_dir && !again.wrote_files);
    assert_eq!(
        site.fake.fs().mutations(),
        mutations,
        "nothing is written twice"
    );
}

// covers: AC-3 (spec 0037)
#[test]
fn the_store_repairs_a_changed_copy_and_reports_a_full_disk_and_a_wrong_digest() {
    let site = Site::new();
    let dir = site.copy(&fixture("acme-demo"), "acme-demo");
    let check = site.import(&dir, no_testing());
    let package = check.package().expect("it validates");
    let digest = check.package_sha256.expect("it has a digest");
    store(&site, package.files(), &digest).expect("the first copy is written");
    let target = site.prepared.layout().provider_import_dir(&digest);
    fs::write(target.join("versions.json"), b"{}").unwrap();

    // A store copy changed after its import is written again by an import
    // of the same bytes (finding 3 of the milestone 1 verify).
    let repaired = store(&site, package.files(), &digest).expect("the same bytes repair it");
    assert!(repaired.wrote_files && !repaired.created_dir);
    assert_eq!(
        fs::read(target.join("versions.json")).unwrap(),
        package.files().get("versions.json").unwrap(),
        "an import compares versions.json too and writes it again"
    );

    let other: Sha256Digest = "ab".repeat(32).parse().unwrap();
    let mismatch = store(&site, package.files(), &other).unwrap_err();
    assert_eq!(mismatch, StoreError::DigestMismatch);

    let fresh = Site::new();
    fresh
        .fake
        .fs()
        .fail_next(FsOp::WriteFileAtomic, Fault::NoSpace);
    assert_eq!(
        store(&fresh, package.files(), &digest).unwrap_err(),
        StoreError::NoSpace
    );
}

// covers: AC-12 (spec 0037), VAL-018
#[test]
fn a_manifest_cannot_declare_itself_trusted() {
    let site = Site::new();
    let dir = site.edited("acme-demo", "trusted", "provider.yaml", |text| {
        text.replace(
            "  executeRuntimeBinary: true\n",
            "  executeRuntimeBinary: true\n  trusted: true\n",
        )
    });

    let check = site.import(&dir, no_testing());

    assert!(!check.valid());
    assert!(
        check
            .findings()
            .iter()
            .any(|finding| finding.issue.code == IssueCode::UnknownField),
        "{:?}",
        check.findings()
    );
}
