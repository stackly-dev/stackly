//! Selector resolution and the support status of an artifact (spec 0018,
//! AC-1, AC-10, AC-15): `stable`, a digest, a version, or a line against the
//! artifacts of a catalog entry, the install gate's last word, and the
//! rules of `support_status`.
#![cfg(unix)]

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use stackly_core::catalog::{
    NotInstallableReason, ProviderEntry, SupportReason, TileColor, TileHint,
};
use stackly_core::provider::{NameCheck, PackageFiles, ValidatedPackage};
use stackly_core::runtime::compatibility::support_status;
use stackly_core::runtime::resolve::{ResolveError, UnresolvedReason, resolve};
use stackly_domain::{
    Arch, CompatibilityRecordRow, CompatibilityResult, CompatibilityScope, Os, OsVersion,
    ProviderRevisionKey, RuntimeSelector, SupportStatus, Target,
};
use stackly_platform::HostFacts;
use stackly_testkit::evidence::{Kind, evidence};

fn sha(byte: char) -> String {
    byte.to_string().repeat(64)
}

fn available(
    id: &str,
    line: &str,
    version: &str,
    os: &str,
    arch: &str,
    byte: char,
    extra: &str,
) -> String {
    format!(
        r#"{{ "id": "{id}", "line": "{line}", "version": "{version}", "os": "{os}", "arch": "{arch}",
        "status": "available", "minOs": "13.0", "format": "tar_gz",
        "urls": ["https://example.test/{id}.tar.gz"], "sha256": "{}", "size": 1000,
        "signingPolicy": "keep_or_adhoc", "license": "Apache-2.0",
        "licenseUrl": "https://example.test/LICENSE", "executables": ["nats-server"] {extra} }}"#,
        sha(byte)
    )
}

/// `providers/nats` with the artifacts of the test, and linux added to the
/// platforms.
fn package(artifacts: &[String]) -> ValidatedPackage {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../providers/nats");
    let mut files: BTreeMap<String, Vec<u8>> = [
        "provider.yaml",
        "schemas/settings.schema.json",
        "templates/nats.conf.tmpl",
    ]
    .into_iter()
    .map(|path| {
        (
            path.to_owned(),
            std::fs::read(dir.join(path)).expect("package file"),
        )
    })
    .collect();
    let manifest = String::from_utf8(files["provider.yaml"].clone()).expect("utf-8");
    let manifest = manifest.replace(
        "    - { os: macos, arch: aarch64 }\n",
        "    - { os: macos, arch: aarch64 }\n    - { os: macos, arch: x86_64 }\n    - { os: linux, arch: x86_64 }\n",
    );
    files.insert("provider.yaml".to_owned(), manifest.into_bytes());
    let versions = format!(
        r#"{{ "schema": "stackly.provider-versions/v1", "software": "nats", "artifacts": [{}] }}"#,
        artifacts.join(",")
    );
    files.insert("versions.json".to_owned(), versions.into_bytes());
    ValidatedPackage::from_files(PackageFiles::new("nats", files), NameCheck::FolderName)
        .unwrap_or_else(|error| panic!("the test package validates: {:?}", error.issues))
}

fn entry(artifacts: &[String]) -> ProviderEntry {
    ProviderEntry {
        package: Arc::new(package(artifacts)),
        tile: TileHint {
            mark: "Na".into(),
            tile: TileColor::Gray,
        },
        notes: Vec::new(),
    }
}

fn host() -> HostFacts {
    HostFacts::new(
        "macos-aarch64".parse().expect("a target"),
        OsVersion::major_minor(27, 0),
        Some("26A428".to_owned()),
    )
}

fn selector(text: &str) -> RuntimeSelector {
    text.parse().expect("a selector")
}

fn catalog() -> ProviderEntry {
    entry(&[
        available("n-2.14.0", "2.14", "2.14.0", "macos", "aarch64", 'a', ""),
        available("n-2.15.0", "2.15", "2.15.0", "macos", "aarch64", 'b', ""),
        available("n-2.15.1", "2.15", "2.15.1", "macos", "aarch64", 'c', r#", "stable": true"#),
        available("n-2.15.1-linux", "2.15", "2.15.1", "linux", "x86_64", 'd', ""),
        r#"{ "id": "n-3.0.0", "line": "3.0", "version": "3.0.0", "os": "macos", "arch": "aarch64",
            "status": "blocked", "license": "Apache-2.0", "licenseUrl": "https://example.test/LICENSE",
            "blocked": { "reason": "closure audit fails", "owner": "maintainer", "since": "2026-10-01" } }"#
            .to_owned(),
        available("n-4.0.0", "4.0", "4.0.0", "macos", "aarch64", 'e', "")
            .replace(r#""minOs": "13.0""#, r#""minOs": "99.0""#),
    ])
}

fn version_of(entry: &ProviderEntry, text: &str, host: &HostFacts) -> Result<String, ResolveError> {
    resolve(entry, &selector(text), host).map(|found| found.artifact.version.to_string())
}

#[test]
fn stable_picks_the_one_stable_artifact_for_the_host() {
    let ev = evidence!(
        covers = ["REQ-003", "VAL-003"],
        kind = Kind::Fixture,
        expected = "stable resolves to the artifact marked stable for the host target, and fails no_stable_artifact when there is none",
    );
    let catalog = catalog();

    assert_eq!(
        version_of(&catalog, "stable", &host()),
        Ok("2.15.1".to_owned())
    );

    let none = entry(&[available(
        "n-2.14.0", "2.14", "2.14.0", "macos", "aarch64", 'a', "",
    )]);
    assert_eq!(
        version_of(&none, "stable", &host()),
        Err(ResolveError::Unresolved {
            reason: UnresolvedReason::NoStableArtifact,
            known_lines: vec!["2.14".parse().expect("a line")],
        })
    );
    ev.pass();
}

#[test]
fn an_exact_version_and_a_line_with_one_version_resolve_without_any_ordering() {
    let catalog = catalog();

    assert_eq!(
        version_of(&catalog, "2.14.0", &host()),
        Ok("2.14.0".to_owned())
    );
    assert_eq!(
        version_of(&catalog, "2.14", &host()),
        Ok("2.14.0".to_owned())
    );
    assert_eq!(
        version_of(&catalog, "2.15.0", &host()),
        Ok("2.15.0".to_owned())
    );
}

#[test]
fn a_line_with_two_versions_is_ambiguous_and_lists_them_in_catalog_order() {
    let result = version_of(&catalog(), "2.15", &host());

    assert_eq!(
        result,
        Err(ResolveError::Ambiguous {
            versions: vec![
                "2.15.0".parse().expect("a version"),
                "2.15.1".parse().expect("a version")
            ]
        })
    );
}

#[test]
fn text_that_is_both_a_version_and_a_line_is_ambiguous() {
    let both = entry(&[
        available("n-8.4", "8", "8.4", "macos", "aarch64", 'a', ""),
        available("n-8.4.3", "8.4", "8.4.3", "macos", "aarch64", 'b', ""),
    ]);

    let result = version_of(&both, "8.4", &host());

    assert!(
        matches!(result, Err(ResolveError::Ambiguous { ref versions }) if versions.len() == 2),
        "{result:?}"
    );
}

#[test]
fn text_that_matches_nothing_lists_the_known_lines() {
    let result = version_of(&catalog(), "9.9", &host());

    match result {
        Err(ResolveError::Unresolved {
            reason: UnresolvedReason::NoMatch,
            known_lines,
        }) => {
            let lines: Vec<String> = known_lines.iter().map(ToString::to_string).collect();
            assert_eq!(lines, ["2.14", "2.15", "3.0", "4.0"]);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_digest_selects_that_artifact_and_never_a_substitute() {
    let ev = evidence!(
        covers = ["REQ-003", "VAL-003"],
        kind = Kind::Fixture,
        expected = "sha256:<digest> of the host artifact resolves to it; the digest of a linux artifact of the same version fails not_installable no_build_for_host and the host artifact is never installed instead",
    );
    let catalog = catalog();

    assert_eq!(
        version_of(&catalog, &format!("sha256:{}", sha('a')), &host()),
        Ok("2.14.0".to_owned())
    );
    assert_eq!(
        version_of(&catalog, &format!("sha256:{}", sha('9')), &host()),
        Err(ResolveError::Unresolved {
            reason: UnresolvedReason::NoMatch,
            known_lines: ["2.14", "2.15", "3.0", "4.0"]
                .iter()
                .map(|line| line.parse().expect("a line"))
                .collect()
        })
    );
    let linux = resolve(
        &catalog,
        &selector(&format!("sha256:{}", sha('d'))),
        &host(),
    );
    assert_eq!(
        linux,
        Err(ResolveError::NotInstallable {
            reason: NotInstallableReason::NoBuildForHost,
            target: "macos-aarch64".parse().expect("a target"),
            available_targets: vec![
                "linux-x86_64".parse().expect("a target"),
                "macos-aarch64".parse().expect("a target"),
            ],
        })
    );
    ev.pass();
}

#[test]
fn a_digest_of_another_architecture_of_the_host_os_is_no_build_for_arch() {
    let intel = entry(&[
        available(
            "n-2.15.1-arm",
            "2.15",
            "2.15.1",
            "macos",
            "aarch64",
            'a',
            "",
        ),
        available(
            "n-2.15.1-intel",
            "2.15",
            "2.15.1",
            "macos",
            "x86_64",
            'b',
            "",
        ),
    ]);

    let result = resolve(&intel, &selector(&format!("sha256:{}", sha('b'))), &host());

    assert!(
        matches!(
            result,
            Err(ResolveError::NotInstallable {
                reason: NotInstallableReason::NoBuildForArch,
                ..
            })
        ),
        "{result:?}"
    );
}

#[test]
fn the_gate_refuses_a_blocked_artifact_and_one_that_needs_a_newer_macos() {
    let catalog = catalog();

    let blocked = version_of(&catalog, "3.0.0", &host());
    let newer = version_of(&catalog, "4.0.0", &host());

    assert_eq!(
        blocked,
        Err(ResolveError::NotInstallable {
            reason: NotInstallableReason::ArtifactBlocked,
            target: "macos-aarch64".parse().expect("a target"),
            available_targets: Vec::new(),
        })
    );
    assert!(
        matches!(
            newer,
            Err(ResolveError::NotInstallable {
                reason: NotInstallableReason::RequiresNewerOs,
                ..
            })
        ),
        "{newer:?}"
    );
    let old_mac = HostFacts::new(host().target, OsVersion::major_minor(12, 0), None);
    assert!(
        matches!(
            version_of(&catalog, "2.14.0", &old_mac),
            Err(ResolveError::NotInstallable {
                reason: NotInstallableReason::RequiresNewerOs,
                ..
            })
        ),
        "minOs 13.0 on macOS 12"
    );
}

// ---- support_status ----

fn artifact_of(entry: &ProviderEntry, id: &str) -> stackly_core::provider::VersionArtifact {
    entry
        .artifacts()
        .iter()
        .find(|artifact| artifact.id == id)
        .expect("the artifact exists")
        .clone()
}

fn record(
    scope: CompatibilityScope,
    revision: Option<ProviderRevisionKey>,
    result: CompatibilityResult,
    os_version: &str,
    arch: Arch,
) -> CompatibilityRecordRow {
    CompatibilityRecordRow::new(
        scope,
        revision,
        Os::Macos,
        os_version.parse().expect("an OS version"),
        None,
        arch,
        result,
        vec!["ready".to_owned()],
        Vec::new(),
        None,
        "2026-10-18T11:07:19.973Z".parse().expect("a time"),
        "runtime/run/record".to_owned(),
    )
    .expect("a valid record")
}

#[test]
fn the_support_status_rules_apply_in_order_and_supported_needs_a_provider_record() {
    let ev = evidence!(
        covers = ["BR-025"],
        kind = Kind::Fixture,
        expected = "blocked, fixture, other target, newer macOS, provider record, runtime record, and untested give the statuses and reasons of the spec, supported only from a passing provider record of the same revision, macOS major, and architecture",
    );
    let catalog = catalog();
    let revision = catalog.package.key().clone();
    let other = ProviderRevisionKey::new(
        revision.provider().clone(),
        "other".parse().expect("a label"),
        *revision.manifest_sha256(),
    );
    let arm = artifact_of(&catalog, "n-2.15.1");
    let verdict = |artifact, records: &[CompatibilityRecordRow], host: &HostFacts| {
        let found = support_status(&artifact, records, Some(&revision), host);
        (found.status, found.reason)
    };

    // 1 and 3 and 4.
    assert_eq!(
        verdict(artifact_of(&catalog, "n-3.0.0"), &[], &host()),
        (SupportStatus::Blocked, SupportReason::ArtifactBlocked)
    );
    assert_eq!(
        verdict(artifact_of(&catalog, "n-2.15.1-linux"), &[], &host()),
        (SupportStatus::Unsupported, SupportReason::NoBuildForHost)
    );
    assert_eq!(
        verdict(artifact_of(&catalog, "n-4.0.0"), &[], &host()),
        (SupportStatus::Unsupported, SupportReason::RequiresNewerOs)
    );
    // 7: no record.
    assert_eq!(
        verdict(arm.clone(), &[], &host()),
        (SupportStatus::Unverified, SupportReason::Untested)
    );
    // 6: a runtime record that passes on this macOS major.
    let runtime = record(
        CompatibilityScope::Runtime,
        None,
        CompatibilityResult::Pass,
        "27.0",
        Arch::Aarch64,
    );
    assert_eq!(
        verdict(arm.clone(), std::slice::from_ref(&runtime), &host()),
        (SupportStatus::Experimental, SupportReason::RuntimeTested)
    );
    // A failing record, another macOS major, or another architecture do not count.
    for wrong in [
        record(
            CompatibilityScope::Runtime,
            None,
            CompatibilityResult::Fail,
            "27.0",
            Arch::Aarch64,
        ),
        record(
            CompatibilityScope::Runtime,
            None,
            CompatibilityResult::Pass,
            "26.0",
            Arch::Aarch64,
        ),
        record(
            CompatibilityScope::Runtime,
            None,
            CompatibilityResult::Pass,
            "27.0",
            Arch::X86_64,
        ),
    ] {
        assert_eq!(
            verdict(arm.clone(), &[wrong], &host()),
            (SupportStatus::Unverified, SupportReason::Untested)
        );
    }
    // 5: a passing provider record of this revision; another revision is only a runtime pass.
    let provider = record(
        CompatibilityScope::Provider,
        Some(revision.clone()),
        CompatibilityResult::Pass,
        "27.1",
        Arch::Aarch64,
    );
    let foreign = record(
        CompatibilityScope::Provider,
        Some(other),
        CompatibilityResult::Pass,
        "27.0",
        Arch::Aarch64,
    );
    assert_eq!(
        verdict(arm.clone(), std::slice::from_ref(&provider), &host()),
        (SupportStatus::Supported, SupportReason::ProviderTested)
    );
    assert_eq!(
        verdict(arm.clone(), &[foreign, runtime], &host()),
        (SupportStatus::Experimental, SupportReason::RuntimeTested)
    );
    // The record that decided is returned.
    let found = support_status(
        &arm,
        std::slice::from_ref(&provider),
        Some(&revision),
        &host(),
    );
    assert_eq!(found.record, Some(&provider));
    ev.pass();
}

#[test]
fn a_fixture_artifact_is_unverified_and_the_target_of_the_host_is_a_target_value() {
    let fixture = entry(&[
        r#"{ "id": "n-fixture", "line": "2.15", "version": "2.15.1", "os": "macos", "arch": "aarch64",
            "status": "available", "minOs": "13.0", "format": "tar_gz",
            "urls": ["https://example.test/a.tar.gz"], "sha256": "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
            "size": 10, "signingPolicy": "keep_or_adhoc", "license": "Apache-2.0",
            "licenseUrl": "https://example.test/LICENSE", "executables": ["nats-server"] }"#
            .to_owned(),
    ]);
    let found = support_status(&artifact_of(&fixture, "n-fixture"), &[], None, &host());

    assert_eq!(
        (found.status, found.reason),
        (SupportStatus::Unverified, SupportReason::Untested)
    );
    let _: Target = host().target;
}
