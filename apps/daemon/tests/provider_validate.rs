//! `provider.validate` through the real daemon (spec 0036 step 3, AC-5
//! daemon part, AC-1 method and catalog parts): the in process daemon on a
//! macOS test root checks copies of the repository packages, answers a
//! broken package with `loadable: false` instead of an error, refuses only
//! a path that is not absolute, still answers in recovery mode, and changes
//! nothing in the registry or the root. A catalog entry whose package has
//! another `apiVersion` is `invalid` with that one issue.
#![cfg(target_os = "macos")]

mod common;
#[path = "supervisor/support.rs"]
mod support;

use std::path::{Path, PathBuf};

use stackly_core::catalog::testing::CatalogBuilder;
use stackly_core::provider::read_package;
use stackly_domain::Registry as _;
use stackly_platform_macos::MacosPlatform;
use stackly_protocol::catalog::{CatalogEntryState, GetProviderParams};
use stackly_protocol::provider::{
    ApiVersionStatus, FindingOrigin, FindingSeverity, ProviderValidateParams, ProviderValidation,
};
use stackly_protocol::{Call, ErrorBody, Reply};
use stackly_testkit::evidence::{Kind, evidence};
use stackly_testkit::{SpawnCatalog, TestRoot};
use stacklyd::server::DaemonHandle;

use crate::support::{Api, Site};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fixture(file: &str) -> ProviderValidation {
    let path = repo().join("protocols/fixtures/provider").join(file);
    let bytes = std::fs::read(&path).expect("the provider fixture reads");
    serde_json::from_slice(&bytes).expect("the provider fixture decodes")
}

/// Copies a folder tree with `std::fs`, as test setup.
fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("the copy target can be created");
    for entry in std::fs::read_dir(from).expect("the folder can be listed") {
        let entry = entry.expect("the entry can be read");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("the entry has a type").is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).expect("the file can be copied");
        }
    }
}

/// A copy of `source` at `<test root>/packages/<name>`, outside the
/// daemon's own folders.
fn copy_package(site: &Site, source: &Path, name: &str) -> PathBuf {
    let dir = site.root.path().join("packages").join(name);
    copy_dir(source, &dir);
    dir
}

fn replace(file: &Path, from: &str, to: &str) {
    let text = std::fs::read_to_string(file).expect("the file reads");
    assert!(text.contains(from), "{} holds {from:?}", file.display());
    std::fs::write(file, text.replacen(from, to, 1)).expect("the file writes");
}

async fn validate(api: &Api, path: &Path) -> Result<ProviderValidation, Box<ErrorBody>> {
    let params = ProviderValidateParams {
        path: path.display().to_string(),
    };
    match api.client.call(Call::ProviderValidate(params)).await? {
        Reply::ProviderValidate(validation) => Ok(*validation),
        other => panic!("not a provider.validate reply: {other:?}"),
    }
}

/// Every file and folder under `root` with its size, sorted, skipping the
/// daemon's own log and socket, which change on their own.
fn tree(root: &Path) -> Vec<(String, u64)> {
    let mut found = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let relative = path
                .strip_prefix(root)
                .expect("under the root")
                .display()
                .to_string();
            if relative.contains("logs")
                || relative.ends_with(".sock")
                || relative.contains("-wal")
                || relative.contains("-shm")
            {
                continue;
            }
            let meta = entry.metadata().expect("the entry has metadata");
            if meta.is_dir() {
                pending.push(path);
                found.push((relative, 0));
            } else {
                found.push((relative, meta.len()));
            }
        }
    }
    found.sort();
    found
}

async fn stop(daemon: DaemonHandle) {
    daemon
        .shutdown(
            stackly_protocol::events::ShutdownReason::Requested,
            std::future::pending(),
        )
        .await;
}

// covers: AC-5 (spec 0036)
#[tokio::test(flavor = "multi_thread")]
async fn a_repository_package_is_publishable_with_its_identity_and_nothing_changes() {
    let ev = evidence!(
        covers = ["REQ-030"],
        kind = Kind::Fixture,
        expected = "provider.validate answers official/nats as loadable and publishable with its identity and the supported versions, and the registry generation and the root stay the same",
    );
    let site = Site::new();
    let daemon = site.start().await;
    let api = site.api().await;
    let nats = copy_package(&site, &repo().join("providers/nats"), "nats");
    let registry = daemon.core().registry().expect("the registry is open");
    let generation = registry
        .known_instance_ids()
        .expect("the ids read")
        .generation;
    let before = tree(site.root.path());

    let reply = validate(&api, &nats)
        .await
        .expect("provider.validate answers");

    let platform = MacosPlatform::with_roots(site.root.roots()).expect("the platform starts");
    let files = read_package(stackly_platform::Platform::fs(&platform), &nats).expect("nats reads");
    let key = stackly_core::provider::ValidatedPackage::from_files(
        files,
        stackly_core::provider::NameCheck::FolderName,
    )
    .expect("nats validates")
    .key()
    .clone();
    assert!(reply.loadable && reply.publishable, "{reply:#?}");
    assert_eq!(reply.folder, "nats");
    assert_eq!(reply.api_version.as_deref(), Some("stackly/v1alpha1"));
    assert!(reply.findings.is_empty());
    assert_eq!(u64::from(reply.finding_count), 0);
    let identity = reply.identity.as_ref().expect("an identity");
    assert_eq!(identity.provider, "official/nats");
    assert_eq!(identity.revision, key.revision().to_string());
    assert_eq!(identity.manifest_sha256, key.manifest_sha256().to_string());
    assert_eq!(identity.software, "nats");
    assert_eq!(identity.display_name, "NATS Server");
    assert_eq!(reply.supported_api_versions.len(), 1);
    assert_eq!(reply.supported_api_versions[0].name, "stackly/v1alpha1");
    assert_eq!(
        reply.supported_api_versions[0].status,
        ApiVersionStatus::Current
    );
    assert_eq!(
        registry
            .known_instance_ids()
            .expect("the ids read")
            .generation,
        generation,
        "provider.validate writes nothing to the registry"
    );
    assert_eq!(tree(site.root.path()), before, "the root is unchanged");
    stop(daemon).await;
    ev.pass();
}

// covers: AC-1, AC-5 (spec 0036)
#[tokio::test(flavor = "multi_thread")]
async fn the_replies_match_the_contract_fixtures() {
    let ev = evidence!(
        covers = ["AC-030", "REQ-030"],
        kind = Kind::Fixture,
        expected = "provider.validate answers apiVersion stackly/v2 with exactly the unsupported_api_version fixture and the fake-command fixture with exactly the authoring fixture",
    );
    let site = Site::new();
    let daemon = site.start().await;
    let api = site.api().await;
    let mytool = copy_package(&site, &repo().join("providers/fake"), "mytool");
    replace(
        &mytool.join("provider.yaml"),
        "apiVersion: stackly/v1alpha1",
        "apiVersion: stackly/v2",
    );
    let fake_command = copy_package(
        &site,
        &repo().join("crates/core/tests/fixtures/packages/fake-command"),
        "fake-command",
    );

    let unsupported = validate(&api, &mytool).await.expect("an answer");
    let authoring = validate(&api, &fake_command).await.expect("an answer");

    assert_eq!(
        unsupported,
        fixture("validation-unsupported-api-version.json")
    );
    assert_eq!(authoring, fixture("validation-authoring.json"));
    stop(daemon).await;
    ev.pass();
}

// covers: AC-5 (spec 0036)
#[tokio::test(flavor = "multi_thread")]
async fn a_folder_that_is_missing_or_holds_too_much_is_still_a_reply() {
    let site = Site::new();
    let daemon = site.start().await;
    let api = site.api().await;

    let missing = validate(&api, &site.root.path().join("packages/nowhere"))
        .await
        .expect("a missing folder is a reply");
    assert!(!missing.loadable && !missing.publishable);
    assert_eq!(missing.folder, "nowhere");
    assert!(missing.identity.is_none() && missing.api_version.is_none());
    assert_eq!(missing.findings[0].code, "missing_file");
    assert_eq!(missing.findings[0].origin, FindingOrigin::Package);
    assert_eq!(missing.findings[0].severity, FindingSeverity::Error);

    let crowded = copy_package(&site, &repo().join("providers/fake"), "fake");
    for n in 0..150 {
        std::fs::write(crowded.join(format!("extra-{n:03}.txt")), b"x").expect("a file writes");
    }
    let crowded = validate(&api, &crowded).await.expect("a reply");
    assert!(!crowded.loadable);
    assert_eq!(crowded.findings.len(), 100);
    assert_eq!(u64::from(crowded.finding_count), 101);
    assert_eq!(u64::from(crowded.omitted), 1);
    assert!(
        crowded
            .findings
            .iter()
            .all(|finding| finding.code == "unexpected_entry")
    );
    stop(daemon).await;
}

// covers: AC-5 (spec 0036)
#[tokio::test(flavor = "multi_thread")]
async fn only_a_path_that_is_not_absolute_is_an_error() {
    let site = Site::new();
    let daemon = site.start().await;
    let api = site.api().await;
    for path in ["providers/fake", "", "/a\0b"] {
        let error = api
            .client
            .call(Call::ProviderValidate(ProviderValidateParams {
                path: path.to_owned(),
            }))
            .await
            .expect_err("the path is refused");
        assert_eq!(error.code(), "request.invalid_params", "{path:?}");
        assert_eq!(
            error.fields().first().map(|field| field.path()),
            Some("path")
        );
    }
    stop(daemon).await;
}

// covers: AC-5 (spec 0036)
#[tokio::test(flavor = "multi_thread")]
async fn provider_validate_answers_in_recovery_mode() {
    let site = Site::new();
    let package = copy_package(&site, &repo().join("providers/fake"), "fake");
    let db = site.prepared.layout().registry_db();
    let mut bytes = std::fs::read(&db).expect("the registry file reads");
    bytes[60..64].copy_from_slice(&999_u32.to_be_bytes());
    std::fs::write(&db, &bytes).expect("the registry file is written");
    let daemon = site.start().await;
    let api = site.api().await;

    let reply = validate(&api, &package)
        .await
        .expect("provider.validate is allowed in recovery mode");

    assert!(reply.publishable, "{reply:#?}");
    assert_eq!(std::fs::read(&db).expect("the registry reads"), bytes);
    stop(daemon).await;
}

// covers: AC-1 (spec 0036)
#[test]
fn a_catalog_entry_with_another_api_version_is_invalid_with_that_one_issue() {
    let ev = evidence!(
        covers = ["AC-030", "REQ-030"],
        kind = Kind::Fixture,
        expected = "a catalog entry whose provider.yaml has apiVersion stackly/v2 is invalid with exactly one unsupported_api_version issue that names stackly/v1alpha1",
    );
    let root = TestRoot::new().unwrap();
    let platform =
        MacosPlatform::with_roots(root.roots()).expect("this Mac reports its OS version");
    let nats = read_package(
        stackly_platform::Platform::fs(&platform),
        &repo().join("providers/nats"),
    )
    .expect("the NATS package reads");
    let dir = root.path().join("catalog");
    CatalogBuilder::new()
        .package("official/nats", nats)
        .write(stackly_platform::Platform::fs(&platform), &dir)
        .expect("the test catalog is written");
    replace(
        &dir.join("packages/official/nats/provider.yaml"),
        "apiVersion: stackly/v1alpha1",
        "apiVersion: stackly/v2",
    );
    let daemon = common::start_daemon_with(&root, SpawnCatalog::Dir(dir));

    let reply = common::runtime().block_on(async {
        let (client, _) = common::client(daemon.socket()).await;
        client
            .call(Call::CatalogGetProvider(GetProviderParams {
                provider: "official/nats".to_owned(),
            }))
            .await
            .expect("the entry is answered")
    });

    let Reply::CatalogGetProvider(detail) = reply else {
        panic!("not a detail reply");
    };
    assert_eq!(detail.summary.state, CatalogEntryState::Invalid);
    let issues: Vec<_> = detail
        .issues
        .iter()
        .map(|issue| {
            (
                issue.file.as_str(),
                issue.location.as_str(),
                issue.code.as_str(),
                issue.message.as_str(),
            )
        })
        .collect();
    assert_eq!(
        issues,
        [(
            "provider.yaml",
            "apiVersion",
            "unsupported_api_version",
            "apiVersion \"stackly/v2\" is not supported; this Stackly supports: stackly/v1alpha1"
        )]
    );
    assert_eq!(daemon.stop().unwrap().code(), Some(0));
    ev.pass();
}
