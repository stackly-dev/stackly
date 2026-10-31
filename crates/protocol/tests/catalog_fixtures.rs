//! Checks the catalog DTO fixtures in `protocols/fixtures/catalog/`, which
//! the CLI goldens and the Swift catalog tests read (spec 0017 AC-13), and
//! the catalog frames in `protocols/fixtures/ipc/` that carry them.

use std::collections::BTreeSet;
use std::path::PathBuf;

use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use stackly_protocol::catalog::{
    ArtifactStatus, CatalogEntryState, CatalogSourceKind, DependencyKind, ListProvidersReply,
    ProviderCapability, ProviderCategory, ProviderDetail, SupportReason, SupportStatus, TargetArch,
    TargetOs, TileColor, TrustLevel,
};
use stackly_protocol::{ErrorFrame, Reply, ServerFrame, codes};

#[derive(Debug, Deserialize)]
struct Index {
    fixtures: Vec<Entry>,
}

#[derive(Debug, Deserialize)]
struct Entry {
    file: String,
    #[serde(rename = "type")]
    type_name: String,
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../protocols/fixtures")
}

fn read(path: &str) -> Vec<u8> {
    let path = fixtures_dir().join(path);
    std::fs::read(&path).unwrap_or_else(|err| panic!("cannot read {}: {err}", path.display()))
}

fn decode<T: DeserializeOwned>(path: &str) -> T {
    serde_json::from_slice(&read(path)).unwrap_or_else(|err| panic!("{path}: {err}"))
}

fn index() -> Vec<Entry> {
    decode::<Index>("catalog/index.json").fixtures
}

fn reply(file: &str) -> Reply {
    let ServerFrame::Response(response) = decode(&format!("ipc/{file}")) else {
        panic!("{file}: expected a response");
    };
    response.reply
}

fn detail(file: &str) -> ProviderDetail {
    decode(&format!("catalog/{file}"))
}

fn list() -> ListProvidersReply {
    decode("catalog/list-providers.json")
}

/// Decodes `bytes` as `T`, encodes it again, and checks the JSON is the
/// same.
fn assert_roundtrip<T>(file: &str)
where
    T: DeserializeOwned + serde::Serialize,
{
    let bytes = read(&format!("catalog/{file}"));
    let value: T = serde_json::from_slice(&bytes).unwrap_or_else(|err| panic!("{file}: {err}"));
    let written = serde_json::to_value(&value).expect("a decoded value always encodes");
    let fixture: Value = serde_json::from_slice(&bytes).expect("a fixture is JSON");
    assert_eq!(written, fixture, "{file}: Rust writes another shape");
}

#[test]
fn every_catalog_fixture_roundtrips_as_its_type() {
    for entry in index() {
        match entry.type_name.as_str() {
            "ListProvidersReply" => assert_roundtrip::<ListProvidersReply>(&entry.file),
            "ProviderDetail" => assert_roundtrip::<ProviderDetail>(&entry.file),
            other => panic!("{}: no decoder for type {other}", entry.file),
        }
    }
}

#[test]
fn every_file_in_the_catalog_fixture_folder_is_listed_in_the_index() {
    let listed: BTreeSet<String> = index().into_iter().map(|entry| entry.file).collect();
    let present: BTreeSet<String> = std::fs::read_dir(fixtures_dir().join("catalog"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .filter(|name| name != "index.json")
        .collect();
    assert_eq!(listed, present);
}

#[test]
fn the_catalog_frames_carry_the_catalog_fixtures() {
    let Reply::CatalogListProviders(listed) = reply("frame-response-catalog-list-providers.json")
    else {
        panic!("expected a catalog.list_providers reply");
    };
    assert_eq!(listed, list());
    let Reply::CatalogGetProvider(nats) = reply("frame-response-catalog-get-provider.json") else {
        panic!("expected a catalog.get_provider reply");
    };
    assert_eq!(*nats, detail("provider-nats.json"));
}

#[test]
fn every_detail_summary_is_its_list_item() {
    let list = list();
    for entry in index() {
        if entry.type_name != "ProviderDetail" {
            continue;
        }
        let detail = detail(&entry.file);
        assert!(
            list.items.contains(&detail.summary),
            "{}: the summary differs from the list item",
            entry.file
        );
        assert_eq!(detail.software, detail.summary.software, "{}", entry.file);
    }
    assert_eq!(
        list.catalog.invalid_count.get(),
        list.items
            .iter()
            .filter(|item| item.state == CatalogEntryState::Invalid)
            .count() as u64
    );
}

#[test]
fn software_is_set_for_providers_and_null_for_planned_and_invalid_entries() {
    for item in list().items {
        let has_package = !matches!(
            item.state,
            CatalogEntryState::Planned | CatalogEntryState::Invalid
        );
        assert_eq!(item.software.is_some(), has_package, "{}", item.provider);
        assert_eq!(
            item.provider_revision.is_some(),
            has_package,
            "{}",
            item.provider
        );
    }
}

#[test]
fn a_catalog_limit_issue_has_a_reason_and_a_digest_issue_has_none() {
    let limit = &detail("provider-zenith.json").issues[0];
    assert_eq!(
        (limit.code.as_str(), limit.reason.as_deref()),
        ("catalog_limit", Some("total_bytes"))
    );
    let digest = &detail("provider-broken.json").issues[0];
    assert_eq!(
        (digest.code.as_str(), digest.reason.as_deref()),
        ("catalog_digest_mismatch", None)
    );
    assert_eq!(detail("provider-zenith.json").summary.category, None);
}

#[test]
fn every_version_row_has_one_cell_per_os_in_matrix_order() {
    for entry in index() {
        if entry.type_name != "ProviderDetail" {
            continue;
        }
        let detail = detail(&entry.file);
        for version in &detail.versions {
            let oses: Vec<TargetOs> = version.support.iter().map(|cell| cell.os).collect();
            assert_eq!(oses, TargetOs::MATRIX, "{} {}", entry.file, version.version);
            assert_eq!(
                version.install.installable,
                version.install.reason.is_none(),
                "{}",
                entry.file
            );
        }
        let capabilities: Vec<ProviderCapability> = detail
            .capabilities
            .iter()
            .map(|capability| capability.capability)
            .collect();
        assert!(
            capabilities.is_empty() || capabilities == ProviderCapability::ALL,
            "{}",
            entry.file
        );
        for version in &detail.versions {
            for cell in &version.support {
                // `supported` only ever follows a passing provider record (BR-025).
                if cell.status == SupportStatus::Supported {
                    assert_eq!(
                        cell.reason,
                        Some(SupportReason::ProviderTested),
                        "{}",
                        entry.file
                    );
                }
            }
        }
    }
}

#[test]
fn only_available_artifacts_carry_a_sha256() {
    for entry in index() {
        if entry.type_name != "ProviderDetail" {
            continue;
        }
        for version in detail(&entry.file).versions {
            for artifact in version.artifacts {
                assert_eq!(
                    artifact.sha256.is_some(),
                    artifact.status == ArtifactStatus::Available,
                    "{}",
                    artifact.id
                );
            }
        }
    }
}

#[test]
fn foreign_values_of_the_catalog_enums_decode_as_unrecognized() {
    let Reply::CatalogGetProvider(detail) =
        reply("frame-response-catalog-get-provider-unknown-values.json")
    else {
        panic!("expected a catalog.get_provider reply");
    };
    let summary = &detail.summary;
    assert_eq!(summary.category, Some(ProviderCategory::Unrecognized));
    assert_eq!(summary.state, CatalogEntryState::Unrecognized);
    assert_eq!(summary.state_reason, Some(SupportReason::Unrecognized));
    assert_eq!(summary.trust, TrustLevel::Unrecognized);
    assert_eq!(summary.tile, TileColor::Unrecognized);
    assert_eq!(
        detail.capabilities.last().map(|c| c.capability),
        Some(ProviderCapability::Unrecognized)
    );
    let version = &detail.versions[0];
    assert_eq!(version.support[0].arches, [TargetArch::Unrecognized]);
    assert_eq!(version.support[0].status, SupportStatus::Unrecognized);
    assert_eq!(version.support[1].os, TargetOs::Unrecognized);
    assert_eq!(version.install.reason, Some(SupportReason::Unrecognized));
    let artifact = &version.artifacts[0];
    assert_eq!(
        (artifact.os, artifact.arch, artifact.status),
        (
            TargetOs::Unrecognized,
            TargetArch::Unrecognized,
            ArtifactStatus::Unrecognized
        )
    );
    assert_eq!(
        artifact.dependencies.as_ref().map(|d| d[0].kind),
        Some(DependencyKind::Unrecognized)
    );
    assert_eq!(detail.platform_notes[0].os, TargetOs::Unrecognized);

    let Reply::CatalogListProviders(listed) =
        reply("frame-response-catalog-list-providers-unknown-values.json")
    else {
        panic!("expected a catalog.list_providers reply");
    };
    assert_eq!(listed.catalog.source, CatalogSourceKind::Unrecognized);
    assert_eq!(listed.catalog.trust, TrustLevel::Unrecognized);
    assert_eq!(
        (listed.catalog.host.os, listed.catalog.host.arch),
        (TargetOs::Unrecognized, TargetArch::Unrecognized)
    );
}

#[test]
fn every_catalog_error_fixture_carries_the_hint_of_its_code_and_details() {
    let mut codes_seen = BTreeSet::new();
    for file in [
        "frame-error-catalog-unavailable.json",
        "frame-error-catalog-provider-not-found.json",
        "frame-error-catalog-not-installable.json",
    ] {
        let ServerFrame::Error(ErrorFrame { error, .. }) = decode(&format!("ipc/{file}")) else {
            panic!("{file}: expected an error frame");
        };
        assert_eq!(
            error.hint(),
            codes::catalog::hint(error.code(), error.details()),
            "{file}"
        );
        codes_seen.insert(error.code().to_owned());
    }
    assert_eq!(codes_seen.len(), 3);
}
