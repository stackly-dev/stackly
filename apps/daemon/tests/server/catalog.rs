//! The catalog subsystem in process (spec 0017 AC-2 to AC-5, AC-7, AC-8,
//! AC-13): its health in every state, both `catalog` methods, their
//! errors with static hints, `daemon.starting` before the start slot
//! loaded the catalog, recovery mode, and the frame budget of a catalog
//! at every limit.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::json;
use stackly_core::catalog::testing::{CatalogBuilder, CatalogFixture};
use stackly_core::catalog::{
    ARTIFACT_ID_MAX_LEN, ARTIFACTS_MAX, BLOCKED_REASON_MAX_LEN, Catalog, CatalogEntry,
    CatalogLocation, CatalogOptions, LICENSE_MAX_LEN, LICENSE_URL_MAX_LEN, NOTE_MAX_LEN,
    PLANNED_MAX, PROVIDERS_MAX, PlannedEntry, PlatformNote, ProviderEntry, TileColor, TileHint,
};
use stackly_core::provider::{Category, NameCheck, PackageFiles, ValidatedPackage, read_package};
use stackly_domain::{Os, SupportStatus};
use stackly_protocol::catalog::{
    CatalogEntryState, CatalogSourceKind, GetProviderParams, ProviderCapability, SupportReason,
    TrustLevel,
};
use stackly_protocol::codes::{catalog, daemon, request as request_codes};
use stackly_protocol::daemon::{DaemonHealth, DaemonHealthStatus, SubsystemStatus, subsystem};
use stackly_protocol::events::ShutdownReason;
use stackly_protocol::limits::CONTROL_FRAME_MAX;
use stackly_protocol::{Call, ErrorBody, Reply};
use stackly_testkit::evidence::{Kind, evidence};
use stacklyd::catalog::{CatalogOverride, NOT_CONFIGURED_DETAIL};
use stacklyd::server::DaemonHandle;

use crate::common::{self, Raw, Site, client, error_of, request};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn nats_files(site: &Site) -> PackageFiles {
    read_package(site.platform.fs(), &repo().join("providers/nats"))
        .expect("the NATS package reads")
}

fn fake_files(site: &Site) -> PackageFiles {
    read_package(site.platform.fs(), &repo().join("providers/fake"))
        .expect("the fake package reads")
}

/// A catalog folder in the test root, written like `cargo xtask catalog
/// build` writes one.
fn write_catalog(site: &Site, builder: &CatalogBuilder) -> PathBuf {
    let dir = site.prepared.layout().roots().cache().join("catalog");
    builder
        .write(site.platform.fs(), &dir)
        .expect("the test catalog is written");
    std::fs::canonicalize(&dir).expect("the catalog folder exists")
}

fn location(dir: PathBuf, options: CatalogOptions) -> CatalogOverride {
    CatalogOverride::Location {
        location: Some(CatalogLocation::Development(dir)),
        options,
    }
}

async fn start_with_catalog(site: &Site, catalog: Option<CatalogOverride>) -> DaemonHandle {
    let mut config = common::config();
    config.catalog = catalog;
    site.start_configured(config).await
}

fn catalog_health(health: &DaemonHealth) -> (SubsystemStatus, Option<String>) {
    let catalog = health
        .subsystems
        .iter()
        .find(|subsystem| subsystem.name == subsystem::CATALOG)
        .expect("health lists the catalog subsystem");
    (catalog.status, catalog.detail.clone())
}

async fn list(site: &Site) -> Result<Reply, Box<ErrorBody>> {
    let (client, _) = client(site).await;
    client.call(Call::CatalogListProviders).await
}

async fn get(site: &Site, provider: &str) -> Result<Reply, Box<ErrorBody>> {
    let (client, _) = client(site).await;
    client
        .call(Call::CatalogGetProvider(GetProviderParams {
            provider: provider.to_owned(),
        }))
        .await
}

#[tokio::test]
async fn the_default_test_catalog_is_ok_and_lists_no_entries() {
    let site = Site::new();
    let daemon = site.start().await;

    let Ok(Reply::CatalogListProviders(reply)) = list(&site).await else {
        panic!("not a list reply");
    };

    assert!(reply.items.is_empty());
    assert_eq!(reply.catalog.revision.get(), 1);
    assert_eq!(reply.catalog.source, CatalogSourceKind::Development);
    assert_eq!(reply.catalog.trust, TrustLevel::Development);
    let host = site.platform.info().target();
    assert_eq!(
        serde_json::to_value(reply.catalog.host).unwrap(),
        json!({ "os": host.os().as_code(), "arch": host.arch().as_code() })
    );
    let health = daemon.health();
    assert_eq!(catalog_health(&health), (SubsystemStatus::Ok, None));
    assert_eq!(health.status, DaemonHealthStatus::Ready);
}

#[tokio::test]
async fn without_a_catalog_the_daemon_serves_catalog_unavailable_and_reports_degraded() {
    let ev = evidence!(
        covers = ["REQ-033"],
        kind = Kind::Fixture,
        expected = "without a catalog folder stacklyd starts, the catalog subsystem is unavailable naming both sources, and every catalog method answers catalog.unavailable not_configured with its hint",
    );
    let site = Site::new();
    let daemon = start_with_catalog(&site, None).await;

    for result in [list(&site).await, get(&site, "official/nats").await] {
        let error = result.expect_err("no catalog to serve");
        assert_eq!(error.code(), catalog::UNAVAILABLE.code);
        assert_eq!(error.detail("reason"), Some("not_configured"));
        assert_eq!(
            error.hint(),
            Some("run stacklyd from Stackly.app or set STACKLY_CATALOG_DIR")
        );
    }
    let health = daemon.health();
    assert_eq!(
        catalog_health(&health),
        (
            SubsystemStatus::Unavailable,
            Some(NOT_CONFIGURED_DETAIL.to_owned())
        )
    );
    assert_eq!(health.status, DaemonHealthStatus::Degraded);
    ev.pass();
}

#[tokio::test]
async fn a_catalog_folder_loads_with_every_entry_in_display_order() {
    let site = Site::new();
    let dir = write_catalog(
        &site,
        &CatalogBuilder::new()
            .package("official/nats", nats_files(&site))
            .fixture(CatalogFixture::LinuxOnly)
            .fixture(CatalogFixture::IntelOnly)
            .fixture(CatalogFixture::BlockedHost)
            .platform_note(
                "official/nats",
                "windows",
                "planned",
                "Windows builds are planned.",
            )
            .planned(
                "official/valkey",
                "Valkey",
                "cache",
                "In-memory key-value store.",
            ),
    );
    let daemon = start_with_catalog(&site, Some(location(dir, CatalogOptions::default()))).await;

    let Ok(Reply::CatalogListProviders(reply)) = list(&site).await else {
        panic!("not a list reply");
    };

    let rows: Vec<_> = reply
        .items
        .iter()
        .map(|item| (item.provider.as_str(), item.state, item.state_reason))
        .collect();
    assert_eq!(
        rows,
        [
            (
                "official/blockedhost",
                CatalogEntryState::Blocked,
                Some(SupportReason::ArtifactBlocked)
            ),
            (
                "official/intelonly",
                CatalogEntryState::Unsupported,
                Some(SupportReason::NoBuildForArch)
            ),
            (
                "official/linuxonly",
                CatalogEntryState::Unsupported,
                Some(SupportReason::NoBuildForHost)
            ),
            ("official/nats", CatalogEntryState::Available, None),
            (
                "official/valkey",
                CatalogEntryState::Planned,
                Some(SupportReason::ProviderPlanned)
            ),
        ]
    );
    assert_eq!(reply.catalog.source, CatalogSourceKind::Development);
    assert_eq!(
        catalog_health(&daemon.health()),
        (SubsystemStatus::Ok, None)
    );
    let nats = reply
        .items
        .iter()
        .find(|item| item.provider == "official/nats")
        .unwrap();
    assert_eq!(nats.software.as_deref(), Some("nats"));
    assert_eq!(nats.versions, ["2.15.1"]);
}

#[tokio::test]
async fn the_detail_carries_capabilities_the_matrix_and_the_gate_verdict() {
    let ev = evidence!(
        covers = ["BR-016", "BR-025"],
        kind = Kind::Fixture,
        expected = "catalog.get_provider for NATS and fake lists eight capabilities equal to the package capability set, three cells per version, never supported, and the gate's verdict per version",
    );
    let site = Site::new();
    let dir = write_catalog(
        &site,
        &CatalogBuilder::new()
            .allow_testing()
            .package("official/nats", nats_files(&site))
            .package("testing/fake", fake_files(&site)),
    );
    let options = CatalogOptions {
        allow_testing_namespace: true,
    };
    let _daemon = start_with_catalog(&site, Some(location(dir, options))).await;

    for (id, files) in [
        ("official/nats", nats_files(&site)),
        ("testing/fake", fake_files(&site)),
    ] {
        let Ok(Reply::CatalogGetProvider(detail)) = get(&site, id).await else {
            panic!("not a detail reply for {id}");
        };
        let package = ValidatedPackage::from_files(files, NameCheck::FolderName).unwrap();
        let declared = package.provider_revision().capabilities();
        let listed: Vec<_> = detail.capabilities.iter().map(|c| c.capability).collect();
        assert_eq!(listed, ProviderCapability::ALL, "{id}");
        for status in &detail.capabilities {
            let code = serde_json::to_value(status.capability).unwrap();
            let held = declared
                .iter()
                .any(|capability| capability.as_code() == code);
            assert_eq!(status.supported, held, "{id} {code}");
        }
        for version in &detail.versions {
            assert_eq!(version.support.len(), 3, "{id}");
            assert!(
                version
                    .support
                    .iter()
                    .all(|cell| cell.status != stackly_protocol::catalog::SupportStatus::Supported),
                "{id}"
            );
        }
        if id == "testing/fake" {
            assert_eq!(
                detail.versions[0].install.reason,
                Some(SupportReason::FixtureOnly)
            );
        } else {
            assert!(detail.versions[0].install.installable);
            assert_eq!(
                detail.versions[0].artifacts[0]
                    .sha256
                    .as_ref()
                    .map(String::len),
                Some(64)
            );
        }
    }
    ev.pass();
}

#[tokio::test]
async fn a_broken_or_foreign_entry_is_invalid_while_the_others_stay_available() {
    let site = Site::new();
    let dir = write_catalog(
        &site,
        &CatalogBuilder::new()
            .allow_testing()
            .package("official/nats", nats_files(&site))
            .fixture(CatalogFixture::IntelOnly)
            .package("testing/fake", fake_files(&site)),
    );
    let template = dir.join("packages/official/intelonly/templates/nats.conf.tmpl");
    let mut text = std::fs::read_to_string(&template).unwrap();
    text.push_str("# changed after the build\n");
    std::fs::write(&template, text).unwrap();
    let daemon = start_with_catalog(&site, Some(location(dir, CatalogOptions::default()))).await;

    let Ok(Reply::CatalogListProviders(reply)) = list(&site).await else {
        panic!("not a list reply");
    };

    let states: BTreeMap<_, _> = reply
        .items
        .iter()
        .map(|item| (item.provider.as_str(), item.state))
        .collect();
    assert_eq!(states["official/nats"], CatalogEntryState::Available);
    assert_eq!(states["official/intelonly"], CatalogEntryState::Invalid);
    assert_eq!(states["testing/fake"], CatalogEntryState::Invalid);
    assert_eq!(reply.catalog.invalid_count.get(), 2);
    assert_eq!(
        catalog_health(&daemon.health()),
        (
            SubsystemStatus::Degraded,
            Some("2 catalog providers failed validation".to_owned())
        )
    );
    for (id, code, location) in [
        (
            "official/intelonly",
            "catalog_digest_mismatch",
            "providers[0].manifestSha256",
        ),
        ("testing/fake", "namespace_not_allowed", "providers[2].id"),
    ] {
        let Ok(Reply::CatalogGetProvider(detail)) = get(&site, id).await else {
            panic!("not a detail reply for {id}");
        };
        let issue = &detail.issues[0];
        assert_eq!(
            (
                issue.file.as_str(),
                issue.code.as_str(),
                issue.location.as_str()
            ),
            ("catalog.json", code, location),
            "{id}"
        );
        assert_eq!(issue.reason, None);
        assert!(detail.capabilities.is_empty());
        assert!(detail.versions.is_empty());
    }
}

#[tokio::test]
async fn an_unusable_catalog_folder_answers_catalog_unavailable_with_its_reason() {
    for (reason, write) in [
        ("missing", None),
        ("index_invalid", Some("{ not json")),
        (
            "unsupported_schema",
            Some(r#"{ "schema": "stackly.catalog/v2" }"#),
        ),
    ] {
        let site = Site::new();
        let dir = site.prepared.layout().roots().cache().join("catalog");
        std::fs::create_dir(&dir).unwrap();
        if let Some(text) = write {
            std::fs::write(dir.join("catalog.json"), text).unwrap();
        }
        let dir = std::fs::canonicalize(&dir).unwrap();
        let daemon =
            start_with_catalog(&site, Some(location(dir, CatalogOptions::default()))).await;

        let error = list(&site).await.expect_err("an unusable catalog");

        assert_eq!(error.code(), catalog::UNAVAILABLE.code, "{reason}");
        assert_eq!(error.detail("reason"), Some(reason));
        assert!(error.hint().is_some(), "{reason}");
        let (status, detail) = catalog_health(&daemon.health());
        assert_eq!(status, SubsystemStatus::Unavailable, "{reason}");
        let detail = detail.expect("an unavailable catalog says why");
        assert!(
            detail.starts_with(&format!("catalog {reason}: ")),
            "{detail}"
        );
    }
}

#[tokio::test]
async fn get_provider_suggests_the_full_id_for_a_bare_name_and_refuses_a_bad_id() {
    let site = Site::new();
    let dir = write_catalog(
        &site,
        &CatalogBuilder::new().package("official/nats", nats_files(&site)),
    );
    let _daemon = start_with_catalog(&site, Some(location(dir, CatalogOptions::default()))).await;

    let missing = get(&site, "official/postgres").await.unwrap_err();
    assert_eq!(missing.code(), catalog::PROVIDER_NOT_FOUND.code);
    assert_eq!(
        missing.message(),
        "no provider official/postgres in the catalog"
    );
    assert_eq!(missing.detail("suggestion"), None);
    assert_eq!(
        missing.hint(),
        Some("run stackly provider list to see the catalog")
    );

    let bare = get(&site, "nats").await.unwrap_err();
    assert_eq!(
        bare.message(),
        "no provider nats in the catalog; did you mean official/nats"
    );
    assert_eq!(bare.detail("suggestion"), Some("official/nats"));
    assert_eq!(
        bare.hint(),
        Some("use the full provider ID, including its namespace")
    );

    let mut raw = Raw::ready(&site.socket()).await;
    raw.send(&request(
        "bad",
        "catalog.get_provider",
        Some(json!({ "provider": "Official/NATS" })),
    ))
    .await;
    let refused = error_of(&raw.answer("bad").await);
    assert_eq!(refused.code(), request_codes::INVALID_PARAMS.code);
    assert_eq!(refused.fields()[0].path(), "provider");
}

#[tokio::test]
async fn the_catalog_methods_answer_daemon_starting_until_the_start_slot_loaded_the_catalog() {
    let site = Site::new();
    let (daemon, gate) = site.start_held().await;

    let error = list(&site)
        .await
        .expect_err("the catalog is not loaded yet");
    assert_eq!(error.code(), daemon::STARTING.code);
    assert_eq!(error.detail("subsystem"), Some(subsystem::CATALOG));
    assert!(error.retryable());
    assert_eq!(
        catalog_health(&daemon.health()),
        (
            SubsystemStatus::Unavailable,
            Some("the catalog is loading".to_owned())
        )
    );

    gate.send_replace(true);
    daemon.started().await;
    assert!(matches!(
        list(&site).await,
        Ok(Reply::CatalogListProviders(_))
    ));
}

#[tokio::test]
async fn the_catalog_is_served_while_the_registry_is_in_recovery_mode() {
    let site = Site::new();
    let first = site.start().await;
    first
        .shutdown(ShutdownReason::Requested, std::future::pending())
        .await;
    let db = site.prepared.layout().registry_db();
    let mut bytes = std::fs::read(&db).unwrap();
    bytes[60..64].copy_from_slice(&999_u32.to_be_bytes());
    std::fs::write(&db, &bytes).unwrap();
    let dir = write_catalog(
        &site,
        &CatalogBuilder::new().package("official/nats", nats_files(&site)),
    );

    let daemon = start_with_catalog(&site, Some(location(dir, CatalogOptions::default()))).await;

    assert!(daemon.health().recovery.is_some());
    let Ok(Reply::CatalogListProviders(reply)) = list(&site).await else {
        panic!("the catalog is served in recovery mode");
    };
    assert_eq!(reply.items.len(), 1);
    assert!(matches!(
        get(&site, "official/nats").await,
        Ok(Reply::CatalogGetProvider(_))
    ));
}

/// `prefix` padded with `x` to exactly `len` bytes.
fn filled(prefix: &str, len: usize) -> String {
    let mut text = prefix.to_owned();
    while text.len() < len {
        text.push('x');
    }
    text.truncate(len);
    text
}

/// A copy of the NATS package with the ID `official/<name>`, a display
/// name and description at their character limits in 4 byte characters,
/// and 32 versions with one artifact each at every text limit; every
/// fourth artifact is blocked.
fn package_at_limits(nats: &PackageFiles, name: &str) -> ValidatedPackage {
    let wide = |count: usize| "\u{1F600}".repeat(count);
    let manifest = String::from_utf8(
        nats.get("provider.yaml")
            .expect("the NATS package has a manifest")
            .to_vec(),
    )
    .expect("the NATS manifest is UTF-8")
    .replace("id: official/nats", &format!("id: official/{name}"))
    .replace(
        "displayName: NATS Server",
        &format!("displayName: \"{}\"", wide(64)),
    )
    .replace(
        "description: NATS messaging server with JetStream, one server per instance.",
        &format!("description: \"{}\"", wide(512)),
    );
    let versions: serde_json::Value = serde_json::from_slice(
        nats.get("versions.json")
            .expect("the NATS package has versions"),
    )
    .expect("the NATS versions.json is JSON");
    let template = versions["artifacts"][0].clone();
    let artifacts: Vec<serde_json::Value> = (0..ARTIFACTS_MAX)
        .map(|n| {
            let mut artifact = template.clone();
            artifact["id"] = filled(&format!("nats-{n}-"), ARTIFACT_ID_MAX_LEN).into();
            artifact["version"] = format!("2.{n}.0").into();
            artifact["line"] = format!("2.{n}").into();
            artifact["license"] = filled(&format!("License-{n}-"), LICENSE_MAX_LEN).into();
            artifact["licenseUrl"] = filled("https://example.com/", LICENSE_URL_MAX_LEN).into();
            // The records the template carries are not in any catalog reply
            // yet, so they add nothing to the frame budget; fill them to
            // their limit when step 10 of spec 0018 puts them on the wire.
            if n > 0 {
                artifact
                    .as_object_mut()
                    .expect("an artifact is a JSON object")
                    .remove("stable");
            }
            if n % 4 == 3 {
                let object = artifact
                    .as_object_mut()
                    .expect("an artifact is a JSON object");
                for field in [
                    "minOs",
                    "format",
                    "urls",
                    "sha256",
                    "size",
                    "archiveRoot",
                    "signingPolicy",
                    "executables",
                    "unpackedSize",
                    "entryCount",
                    "dependencies",
                    "compatibility",
                ] {
                    object.remove(field);
                }
                object.insert("status".into(), "blocked".into());
                object.insert(
                    "blocked".into(),
                    json!({
                        "reason": filled("blocked ", BLOCKED_REASON_MAX_LEN),
                        "owner": "runtime maintainer",
                        "since": "2026-10-01",
                    }),
                );
            }
            artifact
        })
        .collect();
    let mut versions = versions;
    versions["artifacts"] = artifacts.into();
    let mut files: BTreeMap<String, Vec<u8>> = nats
        .iter()
        .map(|(path, bytes)| (path.to_owned(), bytes.to_vec()))
        .collect();
    files.insert("provider.yaml".into(), manifest.into_bytes());
    files.insert(
        "versions.json".into(),
        serde_json::to_vec(&versions).expect("JSON serializes"),
    );
    ValidatedPackage::from_files(PackageFiles::new(name, files), NameCheck::FolderName)
        .unwrap_or_else(|err| panic!("{name}: {:?}", err.issues))
}

#[tokio::test]
async fn a_catalog_at_every_limit_fits_list_and_detail_replies_in_a_control_frame() {
    let ev = evidence!(
        covers = ["NFR-014"],
        kind = Kind::Fixture,
        expected = "a catalog with 64 providers of 32 artifacts and 64 planned entries, every text at its limit, gives a list reply and the largest detail reply below 1 MiB",
    );
    let site = Site::new();
    let nats = nats_files(&site);
    let note = |os| PlatformNote {
        os,
        status: SupportStatus::Planned,
        note: filled("note ", NOTE_MAX_LEN),
    };
    let mut entries: Vec<CatalogEntry> = (0..PROVIDERS_MAX)
        .map(|n| {
            CatalogEntry::Provider(ProviderEntry {
                package: Arc::new(package_at_limits(&nats, &filled(&format!("p{n}-"), 63))),
                tile: TileHint {
                    mark: "Pp".into(),
                    tile: TileColor::Blue,
                },
                notes: vec![note(Os::Linux), note(Os::Windows)],
            })
        })
        .collect();
    entries.extend((0..PLANNED_MAX).map(|n| {
        CatalogEntry::Planned(PlannedEntry {
            id: format!("official/{}", filled(&format!("q{n}-"), 63))
                .parse()
                .unwrap(),
            display_name: "\u{1F600}".repeat(64),
            category: Category::Database,
            description: "\u{1F600}".repeat(512),
            note: Some(filled("planned ", NOTE_MAX_LEN)),
            tile: TileHint {
                mark: "Qq".into(),
                tile: TileColor::Teal,
            },
        })
    }));
    let catalog = Catalog::for_tests(&entries);
    let id = catalog.entries()[0].id().to_string();
    let _daemon = start_with_catalog(&site, Some(CatalogOverride::Preloaded(catalog))).await;
    let mut raw = Raw::ready(&site.socket()).await;

    raw.send(&request("list", "catalog.list_providers", None))
        .await;
    let listed = raw.answer("list").await;
    raw.send(&request(
        "detail",
        "catalog.get_provider",
        Some(json!({ "provider": id })),
    ))
    .await;
    let detail = raw.answer("detail").await;

    assert_eq!(listed["type"], "response", "{listed}");
    assert_eq!(
        listed["content"]["reply"]["content"]["items"]
            .as_array()
            .unwrap()
            .len(),
        128
    );
    assert_eq!(detail["type"], "response", "{detail}");
    let versions = &detail["content"]["reply"]["content"]["versions"];
    assert_eq!(versions.as_array().unwrap().len(), ARTIFACTS_MAX);
    for frame in [&listed, &detail] {
        let len = serde_json::to_vec(frame).unwrap().len();
        assert!(len < CONTROL_FRAME_MAX as usize, "{len} bytes");
    }
    ev.pass();
}
