//! Maps the catalog read model of `stackly-core` to the `catalog` DTOs
//! and errors (spec 0017, *API surface*, *Value sourcing*, *Kode error di
//! wire*).
//!
//! Every status, reason, and install verdict comes from
//! `stackly_core::catalog`; this module only copies them. An enum travels
//! by its stable code, which spec 0017 makes the same as its wire value,
//! so a core value and its DTO value never drift apart.

use serde::de::DeserializeOwned;
use stackly_core::catalog::{
    Catalog, CatalogEntry, CatalogLocation, Installability, NotInstallableReason,
    OFFICIAL_NAMESPACE, ProviderEntry, SUMMARY_VERSIONS_MAX, TargetSupport, UnavailableReason,
    category_code, entry_status, installability, version_support,
};
use stackly_core::provider::{ArtifactStatus, Issue, VersionArtifact};
use stackly_domain::{ExactVersion, ProviderName, Target};
use stackly_platform::HostFacts;
use stackly_protocol::catalog as dto;
use stackly_protocol::codes::catalog;
use stackly_protocol::{Count, ErrorBody, Revision, Timestamp};

/// The publisher of every `official` package.
const OFFICIAL_PUBLISHER: &str = "Stackly";

/// The `reason` detail of `catalog.unavailable` when the daemon has no
/// catalog folder at all.
const NOT_CONFIGURED: &str = "not_configured";

/// Reads a wire enum from its code. Every catalog DTO enum is open, so any
/// code reads; one this build does not know reads as `unrecognized`, which
/// the per variant tests below rule out for every core value.
fn wire<T: DeserializeOwned>(code: &str) -> T {
    serde_json::from_value(serde_json::Value::String(code.to_owned()))
        .expect("an open wire enum reads any string")
}

/// The OS and architecture of `target`.
fn host_target(target: Target) -> dto::HostTarget {
    dto::HostTarget {
        os: wire(target.os().as_code()),
        arch: wire(target.arch().as_code()),
    }
}

/// `bundled` and `official` for the app catalog, `development` twice for
/// `STACKLY_CATALOG_DIR`.
fn source_and_trust(location: &CatalogLocation) -> (dto::CatalogSourceKind, dto::TrustLevel) {
    match location {
        CatalogLocation::Bundled(_) => (dto::CatalogSourceKind::Bundled, dto::TrustLevel::Official),
        CatalogLocation::Development(_) => (
            dto::CatalogSourceKind::Development,
            dto::TrustLevel::Development,
        ),
    }
}

/// The facts about `catalog` for a Mac of target `host`; `outdated` is
/// [`Catalog::outdated`] at the time of the request.
#[must_use]
pub fn catalog_info(catalog: &Catalog, host: &HostFacts, outdated: bool) -> dto::CatalogInfo {
    let meta = catalog.meta();
    let (source, trust) = source_and_trust(&meta.location);
    dto::CatalogInfo {
        revision: Revision::saturating(meta.revision),
        published_at: Timestamp::from_jiff(meta.published_at),
        loaded_at: Timestamp::from_jiff(meta.loaded_at),
        content_sha256: meta.content_sha256.to_string(),
        source,
        trust,
        host: host_target(host.target),
        outdated,
        invalid_count: Count::saturating(catalog.invalid_count() as u64),
    }
}

/// The reply of `catalog.list_providers`: the facts and every entry, in
/// the catalog's display order.
#[must_use]
pub fn list_providers(
    catalog: &Catalog,
    host: &HostFacts,
    outdated: bool,
) -> dto::ListProvidersReply {
    dto::ListProvidersReply {
        catalog: catalog_info(catalog, host, outdated),
        items: catalog
            .entries()
            .iter()
            .map(|entry| provider_summary(catalog, entry, host))
            .collect(),
    }
}

/// The artifacts of a provider entry that install on `host`: `available`
/// and built for exactly that target.
fn host_artifacts(entry: &ProviderEntry, host: Target) -> impl Iterator<Item = &VersionArtifact> {
    entry.artifacts().iter().filter(move |artifact| {
        artifact.target == host && matches!(artifact.status, ArtifactStatus::Available { .. })
    })
}

/// One entry as a card or a list row shows it.
#[must_use]
pub fn provider_summary(
    catalog: &Catalog,
    entry: &CatalogEntry,
    host: &HostFacts,
) -> dto::ProviderSummary {
    let status = entry_status(entry, host);
    let (_, trust) = source_and_trust(&catalog.meta().location);
    let mut versions: Vec<String> = Vec::new();
    let mut licenses: Vec<String> = Vec::new();
    let (software, description, provider_revision) = match entry {
        CatalogEntry::Provider(provider) => {
            for artifact in host_artifacts(provider, host.target) {
                let version = artifact.version.to_string();
                if !versions.contains(&version) {
                    versions.push(version);
                }
                if let ArtifactStatus::Available { license, .. } = &artifact.status
                    && !licenses.contains(license)
                {
                    licenses.push(license.clone());
                }
            }
            let manifest = provider.package.manifest();
            (
                Some(provider.package.software().to_string()),
                manifest.description().map(str::to_owned),
                Some(provider.package.key().revision().to_string()),
            )
        }
        CatalogEntry::Planned(planned) => (None, Some(planned.description.clone()), None),
        CatalogEntry::Invalid(_) => (None, None, None),
    };
    let version_count = Count::saturating(versions.len() as u64);
    versions.truncate(SUMMARY_VERSIONS_MAX);
    let tile = entry.tile();
    dto::ProviderSummary {
        provider: entry.id().to_string(),
        revision: Revision::saturating(catalog.meta().revision),
        display_name: entry.display_name(),
        software,
        category: entry
            .category()
            .map(|category| wire(category_code(category))),
        description,
        state: wire(status.state.as_code()),
        state_reason: status.reason.map(|reason| wire(reason.as_code())),
        state_note: status.note,
        provider_revision,
        publisher: (entry.id().namespace() == OFFICIAL_NAMESPACE)
            .then(|| OFFICIAL_PUBLISHER.to_owned()),
        trust,
        versions,
        version_count,
        licenses,
        mark: tile.mark.clone(),
        tile: wire(tile.tile.as_code()),
    }
}

/// The reply of `catalog.get_provider` for one entry.
#[must_use]
pub fn provider_detail(
    catalog: &Catalog,
    entry: &CatalogEntry,
    host: &HostFacts,
) -> dto::ProviderDetail {
    let summary = provider_summary(catalog, entry, host);
    let software = summary.software.clone();
    let mut detail = dto::ProviderDetail {
        summary,
        software,
        manifest_sha256: None,
        capabilities: Vec::new(),
        versions: Vec::new(),
        platform_notes: Vec::new(),
        note: None,
        issues: Vec::new(),
    };
    match entry {
        CatalogEntry::Provider(provider) => {
            let package = &provider.package;
            let declared = package.provider_revision().capabilities();
            detail.manifest_sha256 = Some(package.key().manifest_sha256().to_string());
            detail.capabilities = dto::ProviderCapability::ALL
                .into_iter()
                .map(|capability| dto::CapabilityStatus {
                    capability,
                    supported: declared.iter().any(|declared| {
                        wire::<dto::ProviderCapability>(declared.as_code()) == capability
                    }),
                })
                .collect();
            detail.versions = provider
                .versions()
                .into_iter()
                .map(|version| catalog_version(entry, provider, version, host))
                .collect();
            detail.platform_notes = provider
                .notes
                .iter()
                .map(|note| dto::PlatformNoteDto {
                    os: wire(note.os.as_code()),
                    status: wire(note.status.as_code()),
                    note: note.note.clone(),
                })
                .collect();
        }
        CatalogEntry::Planned(planned) => detail.note.clone_from(&planned.note),
        CatalogEntry::Invalid(invalid) => {
            detail.issues = invalid.issues.iter().map(catalog_issue).collect();
        }
    }
    detail
}

/// One version row of a provider: its matrix cells, the gate's verdict for
/// this Mac, and its artifacts in file order.
fn catalog_version(
    entry: &CatalogEntry,
    provider: &ProviderEntry,
    version: &ExactVersion,
    host: &HostFacts,
) -> dto::CatalogVersion {
    let artifacts: Vec<&VersionArtifact> = provider
        .artifacts()
        .iter()
        .filter(|artifact| artifact.version == *version)
        .collect();
    let install = match installability(entry, version, host) {
        Ok(Installability::Installable { .. }) => dto::InstallVerdict {
            installable: true,
            reason: None,
        },
        Ok(Installability::NotInstallable { reason, .. }) => dto::InstallVerdict {
            installable: false,
            reason: Some(wire(reason.as_code())),
        },
        // Every version of `provider.versions()` has an artifact.
        Err(_) => dto::InstallVerdict {
            installable: false,
            reason: Some(wire(NotInstallableReason::NoBuildForHost.as_code())),
        },
    };
    dto::CatalogVersion {
        version: version.to_string(),
        line: artifacts
            .first()
            .map(|artifact| artifact.line.to_string())
            .unwrap_or_default(),
        support: version_support(provider, version, host)
            .iter()
            .map(target_support)
            .collect(),
        install,
        artifacts: artifacts.into_iter().map(catalog_artifact).collect(),
    }
}

/// One matrix cell.
fn target_support(cell: &TargetSupport) -> dto::TargetSupport {
    dto::TargetSupport {
        os: wire(cell.os.as_code()),
        arches: cell
            .arches
            .iter()
            .map(|arch| wire(arch.as_code()))
            .collect(),
        status: wire(cell.status.as_code()),
        reason: cell.reason.map(|reason| wire(reason.as_code())),
        note: cell.note.clone(),
    }
}

/// The host of `url` and its first path segment, without the scheme, such
/// as `github.com/nats-io`.
fn url_source(url: &str) -> Option<String> {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let rest = rest.split(['?', '#']).next().unwrap_or_default();
    let mut parts = rest.split('/');
    let host = parts.next().filter(|host| !host.is_empty())?;
    Some(match parts.next().filter(|segment| !segment.is_empty()) {
        Some(segment) => format!("{host}/{segment}"),
        None => host.to_owned(),
    })
}

/// One artifact; the fields of a status travel only with that status.
fn catalog_artifact(artifact: &VersionArtifact) -> dto::CatalogArtifact {
    let mut out = dto::CatalogArtifact {
        id: artifact.id.clone(),
        os: wire(artifact.target.os().as_code()),
        arch: wire(artifact.target.arch().as_code()),
        status: dto::ArtifactStatus::Unrecognized,
        sha256: None,
        min_os: None,
        download_size: None,
        installed_size: None,
        license: None,
        license_url: None,
        source: None,
        blocked_reason: None,
        dependencies: None,
    };
    match &artifact.status {
        ArtifactStatus::Available {
            min_os,
            urls,
            sha256,
            size,
            license,
            license_url,
            ..
        } => {
            out.status = dto::ArtifactStatus::Available;
            out.sha256 = Some(sha256.to_string());
            out.min_os = Some(min_os.to_string());
            out.download_size = Some(Count::saturating(*size));
            out.license = Some(license.clone());
            out.license_url = Some(license_url.clone());
            out.source = urls.first().and_then(|url| url_source(url));
        }
        ArtifactStatus::Blocked {
            reason,
            license,
            license_url,
            ..
        } => {
            out.status = dto::ArtifactStatus::Blocked;
            out.blocked_reason = Some(reason.clone());
            out.license.clone_from(license);
            out.license_url.clone_from(license_url);
        }
        ArtifactStatus::Fixture { .. } => out.status = dto::ArtifactStatus::Fixture,
        // A status newer than this build of the daemon.
        _ => {}
    }
    out
}

/// One issue of an invalid entry.
fn catalog_issue(issue: &Issue) -> dto::CatalogIssue {
    dto::CatalogIssue {
        file: issue.file.clone(),
        location: issue.location.to_string(),
        code: issue.code.as_code().to_owned(),
        message: issue.message.clone(),
        reason: issue.reason.map(str::to_owned),
    }
}

/// `body` with the static hint of its code and details, if it has one.
fn with_catalog_hint(body: ErrorBody) -> ErrorBody {
    match catalog::hint(body.code(), body.details()) {
        Some(hint) => body.with_hint(hint),
        None => body,
    }
}

/// `catalog.unavailable` when the daemon has no catalog folder.
#[must_use]
pub fn catalog_not_configured() -> ErrorBody {
    catalog_unavailable_with(NOT_CONFIGURED)
}

/// `catalog.unavailable` for a catalog folder that cannot be used.
#[must_use]
pub fn catalog_unavailable(reason: UnavailableReason) -> ErrorBody {
    catalog_unavailable_with(reason.as_code())
}

fn catalog_unavailable_with(reason: &str) -> ErrorBody {
    with_catalog_hint(
        ErrorBody::new(
            &catalog::UNAVAILABLE,
            "the provider catalog is not available",
        )
        .with_detail("reason", reason),
    )
}

/// `catalog.provider_not_found` for the asked `provider`, with the full ID
/// to suggest when a name without its namespace matched one entry.
#[must_use]
pub fn provider_not_found(provider: &str, suggestion: Option<&ProviderName>) -> ErrorBody {
    let body = match suggestion {
        Some(suggestion) => ErrorBody::new(
            &catalog::PROVIDER_NOT_FOUND,
            format!("no provider {provider} in the catalog; did you mean {suggestion}"),
        )
        .with_detail("provider", provider)
        .with_detail("suggestion", suggestion.as_str()),
        None => ErrorBody::new(
            &catalog::PROVIDER_NOT_FOUND,
            format!("no provider {provider} in the catalog"),
        )
        .with_detail("provider", provider),
    };
    with_catalog_hint(body)
}

/// `catalog.version_not_found`, when the gate is asked about a version the
/// entry does not have.
#[must_use]
pub fn version_not_found(provider: &ProviderName, version: &ExactVersion) -> ErrorBody {
    with_catalog_hint(
        ErrorBody::new(
            &catalog::VERSION_NOT_FOUND,
            format!("{provider} has no version {version} in the catalog"),
        )
        .with_detail("provider", provider.as_str())
        .with_detail("version", version.to_string()),
    )
}

/// `catalog.not_installable`, when the gate refuses `version` of
/// `provider` on `host`.
#[must_use]
pub fn not_installable(
    provider: &ProviderName,
    version: &ExactVersion,
    host: Target,
    reason: NotInstallableReason,
    available_targets: &[Target],
) -> ErrorBody {
    let available: Vec<String> = available_targets.iter().map(ToString::to_string).collect();
    with_catalog_hint(
        ErrorBody::new(
            &catalog::NOT_INSTALLABLE,
            format!("{provider} {version} can't be installed on {host}"),
        )
        .with_detail("provider", provider.as_str())
        .with_detail("version", version.to_string())
        .with_detail("target", host.to_string())
        .with_detail("availableTargets", available.join(","))
        .with_detail("reason", reason.as_code()),
    )
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    use serde::de::DeserializeOwned;
    use stackly_core::catalog::testing::CatalogFixture;
    use stackly_core::catalog::{
        CatalogMeta, EntryState, InvalidEntry, PlannedEntry, PlatformNote, SupportReason,
        TileColor, TileHint,
    };
    use stackly_core::provider::{
        Category, IssueCode, Location, NameCheck, PackageFiles, ValidatedPackage,
    };
    use stackly_domain::{Arch, Capability, Os, SupportStatus};

    use super::*;

    fn host() -> HostFacts {
        HostFacts::new(
            "macos-aarch64".parse().unwrap(),
            stackly_domain::OsVersion::major_minor(27, 0),
            None,
        )
    }

    fn fixture<T: DeserializeOwned>(file: &str) -> T {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../protocols/fixtures/catalog")
            .join(file);
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap()
    }

    fn nats_package() -> ValidatedPackage {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../providers/nats");
        let files: BTreeMap<String, Vec<u8>> = [
            "provider.yaml",
            "versions.json",
            "schemas/settings.schema.json",
            "templates/nats.conf.tmpl",
        ]
        .into_iter()
        .map(|path| (path.to_owned(), std::fs::read(dir.join(path)).unwrap()))
        .collect();
        ValidatedPackage::from_files(PackageFiles::new("nats", files), NameCheck::FolderName)
            .unwrap()
    }

    fn tile(mark: &str, tile: TileColor) -> TileHint {
        TileHint {
            mark: mark.to_owned(),
            tile,
        }
    }

    fn provider(
        package: ValidatedPackage,
        hint: TileHint,
        notes: Vec<PlatformNote>,
    ) -> CatalogEntry {
        CatalogEntry::Provider(ProviderEntry {
            package: Arc::new(package),
            tile: hint,
            notes,
        })
    }

    fn planned(
        id: &str,
        name: &str,
        category: Category,
        description: &str,
        hint: TileHint,
        note: Option<&str>,
    ) -> CatalogEntry {
        CatalogEntry::Planned(PlannedEntry {
            id: id.parse().unwrap(),
            display_name: name.to_owned(),
            category,
            description: description.to_owned(),
            note: note.map(str::to_owned),
            tile: hint,
        })
    }

    /// The catalog `protocols/fixtures/catalog/` describes, built from the
    /// real packages, so the fixtures the CLI and Swift tests read are
    /// what this daemon sends.
    fn fixture_catalog() -> Catalog {
        let info: dto::CatalogInfo =
            fixture::<dto::ListProvidersReply>("list-providers.json").catalog;
        let meta = CatalogMeta {
            revision: info.revision.get(),
            published_at: info.published_at.as_jiff(),
            content_sha256: info.content_sha256.parse().unwrap(),
            location: CatalogLocation::Bundled(PathBuf::from(
                "/Applications/Stackly.app/Contents/Resources/catalog",
            )),
            loaded_at: info.loaded_at.as_jiff(),
        };
        let windows = PlatformNote {
            os: Os::Windows,
            status: SupportStatus::Planned,
            note: "Windows builds are planned for a later release.".to_owned(),
        };
        let invalid = |id: &str, category, mark: &str, issue: Issue| {
            CatalogEntry::Invalid(InvalidEntry {
                id: id.parse().unwrap(),
                category,
                tile: tile(mark, TileColor::Gray),
                issues: vec![issue],
            })
        };
        let entries = [
            provider(nats_package(), tile("Na", TileColor::Green), vec![windows]),
            provider(
                CatalogFixture::IntelOnly.package(),
                tile("Na", TileColor::Gray),
                Vec::new(),
            ),
            provider(
                CatalogFixture::BlockedHost.package(),
                tile("Na", TileColor::Gray),
                Vec::new(),
            ),
            invalid(
                "official/broken",
                Some(Category::Cache),
                "Br",
                Issue::new(
                    "catalog.json",
                    Location::Field("providers[1].manifestSha256".to_owned()),
                    IssueCode::CatalogDigestMismatch,
                    "manifestSha256 does not match the package files",
                ),
            ),
            invalid(
                "official/zenith",
                None,
                "Ze",
                Issue::new(
                    "catalog.json",
                    Location::Field("providers[4]".to_owned()),
                    IssueCode::CatalogLimit,
                    "the catalog packages are larger than 16 MiB together",
                )
                .with_reason("total_bytes"),
            ),
            planned(
                "official/postgresql",
                "PostgreSQL",
                Category::Database,
                "Relational database. Multiple majors and multiple clusters per version, each with its own data area.",
                tile("Pg", TileColor::Blue),
                Some("Planned for a later release."),
            ),
            planned(
                "official/mysql",
                "MySQL",
                Category::Database,
                "Relational server with isolated datadir, socket and tmpdir per instance. Never bound to an existing system service.",
                tile("My", TileColor::Teal),
                None,
            ),
            planned(
                "official/clickhouse",
                "ClickHouse",
                Category::Analytics,
                "Columnar analytics database. Single node with HTTP and native TCP endpoints.",
                tile("Ch", TileColor::Yellow),
                None,
            ),
            planned(
                "official/valkey",
                "Valkey",
                Category::Cache,
                "In-memory key-value store. Standalone, with RDB or AOF persistence per instance.",
                tile("Vk", TileColor::Red),
                None,
            ),
            planned(
                "official/traefik",
                "Traefik",
                Category::Proxy,
                "Reverse proxy with file-based routes. Entrypoints bind to loopback by default.",
                tile("Tr", TileColor::Cyan),
                None,
            ),
        ];
        Catalog::for_tests_with_meta(meta, &entries)
    }

    fn detail_of(catalog: &Catalog, id: &str) -> dto::ProviderDetail {
        let entry = catalog.entry(&id.parse().unwrap()).unwrap();
        provider_detail(catalog, entry, &host())
    }

    #[test]
    fn the_list_reply_of_the_fixture_catalog_is_the_shared_fixture() {
        let catalog = fixture_catalog();

        let reply = list_providers(&catalog, &host(), false);

        assert_eq!(reply, fixture("list-providers.json"));
    }

    #[test]
    fn every_detail_reply_of_the_fixture_catalog_is_its_shared_fixture() {
        let catalog = fixture_catalog();
        for (id, file) in [
            ("official/nats", "provider-nats.json"),
            ("official/intelonly", "provider-intelonly.json"),
            ("official/blockedhost", "provider-blockedhost.json"),
            ("official/broken", "provider-broken.json"),
            ("official/zenith", "provider-zenith.json"),
            ("official/postgresql", "provider-postgresql.json"),
        ] {
            assert_eq!(
                detail_of(&catalog, id),
                fixture::<dto::ProviderDetail>(file),
                "{id}"
            );
        }
    }

    #[test]
    fn a_development_catalog_is_development_twice_and_never_official() {
        let catalog = Catalog::for_tests(&[provider(
            nats_package(),
            tile("Na", TileColor::Green),
            Vec::new(),
        )]);

        let reply = list_providers(&catalog, &host(), true);

        assert_eq!(reply.catalog.source, dto::CatalogSourceKind::Development);
        assert_eq!(reply.catalog.trust, dto::TrustLevel::Development);
        assert_eq!(reply.items[0].trust, dto::TrustLevel::Development);
        assert!(reply.catalog.outdated);
        assert_eq!(reply.catalog.invalid_count.get(), 0);
    }

    #[test]
    fn capabilities_follow_the_capability_set_in_the_fixed_order() {
        let catalog = fixture_catalog();
        let detail = detail_of(&catalog, "official/nats");
        let declared = nats_package().provider_revision().capabilities().clone();

        let listed: Vec<_> = detail.capabilities.iter().map(|c| c.capability).collect();
        assert_eq!(listed, dto::ProviderCapability::ALL);
        for status in &detail.capabilities {
            let code = serde_json::to_value(status.capability).unwrap();
            let capability = Capability::ALL
                .iter()
                .copied()
                .find(|capability| capability.as_code() == code)
                .unwrap();
            assert_eq!(status.supported, declared.contains(capability), "{code}");
        }
        assert!(!detail.capabilities[2].supported, "NATS has no reload");
        assert!(
            detail_of(&catalog, "official/postgresql")
                .capabilities
                .is_empty()
        );
    }

    #[test]
    fn a_summary_lists_at_most_16_versions_and_counts_them_all() {
        let package = nats_package();
        let text =
            String::from_utf8(package.files().get("versions.json").unwrap().to_vec()).unwrap();
        let mut value: serde_json::Value = serde_json::from_str(&text).unwrap();
        let first = value["artifacts"][0].clone();
        let artifacts: Vec<_> = (0..20)
            .map(|patch| {
                let mut artifact = first.clone();
                artifact["id"] = format!("nats-2.15.{patch}-macos-aarch64").into();
                artifact["version"] = format!("2.15.{patch}").into();
                if patch > 0 {
                    artifact.as_object_mut().unwrap().remove("stable");
                }
                artifact
            })
            .collect();
        value["artifacts"] = artifacts.into();
        let mut files: BTreeMap<String, Vec<u8>> = package
            .files()
            .iter()
            .map(|(path, bytes)| (path.to_owned(), bytes.to_vec()))
            .collect();
        files.insert(
            "versions.json".to_owned(),
            serde_json::to_vec(&value).unwrap(),
        );
        let many =
            ValidatedPackage::from_files(PackageFiles::new("nats", files), NameCheck::FolderName)
                .unwrap();
        let catalog =
            Catalog::for_tests(&[provider(many, tile("Na", TileColor::Green), Vec::new())]);

        let summary = &list_providers(&catalog, &host(), false).items[0];

        assert_eq!(summary.versions.len(), 16);
        assert_eq!(summary.versions[0], "2.15.0");
        assert_eq!(summary.version_count.get(), 20);
        assert_eq!(summary.licenses, ["Apache-2.0"]);
    }

    #[test]
    fn the_source_of_an_artifact_is_its_first_url_host_and_path_segment() {
        assert_eq!(
            url_source("https://github.com/nats-io/nats-server/releases/x.tar.gz").as_deref(),
            Some("github.com/nats-io")
        );
        assert_eq!(
            url_source("https://example.com").as_deref(),
            Some("example.com")
        );
        assert_eq!(
            url_source("https://example.com/").as_deref(),
            Some("example.com")
        );
        assert_eq!(
            url_source("https://example.com?x=1").as_deref(),
            Some("example.com")
        );
        assert_eq!(url_source("https://").as_deref(), None);
    }

    #[test]
    fn every_core_value_has_a_known_wire_value() {
        for os in Os::ALL {
            assert_ne!(
                wire::<dto::TargetOs>(os.as_code()),
                dto::TargetOs::Unrecognized
            );
        }
        for arch in Arch::ALL {
            assert_ne!(
                wire::<dto::TargetArch>(arch.as_code()),
                dto::TargetArch::Unrecognized
            );
        }
        for category in [
            Category::Database,
            Category::Messaging,
            Category::Analytics,
            Category::Cache,
            Category::Proxy,
            Category::Testing,
        ] {
            assert_ne!(
                wire::<dto::ProviderCategory>(category_code(category)),
                dto::ProviderCategory::Unrecognized
            );
        }
        for color in TileColor::ALL {
            assert_ne!(
                wire::<dto::TileColor>(color.as_code()),
                dto::TileColor::Unrecognized
            );
        }
        for status in SupportStatus::ALL {
            assert_ne!(
                wire::<dto::SupportStatus>(status.as_code()),
                dto::SupportStatus::Unrecognized
            );
        }
        for reason in SupportReason::ALL {
            // provisional: F18 step 10 gives the three reasons of the
            // compatibility rules their wire values.
            if matches!(
                reason,
                SupportReason::RequiresNewerOs
                    | SupportReason::ProviderTested
                    | SupportReason::RuntimeTested
            ) {
                continue;
            }
            assert_ne!(
                wire::<dto::SupportReason>(reason.as_code()),
                dto::SupportReason::Unrecognized
            );
        }
        for state in EntryState::ALL {
            assert_ne!(
                wire::<dto::CatalogEntryState>(state.as_code()),
                dto::CatalogEntryState::Unrecognized
            );
        }
        for capability in Capability::ALL {
            assert_ne!(
                wire::<dto::ProviderCapability>(capability.as_code()),
                dto::ProviderCapability::Unrecognized
            );
        }
    }

    fn details(body: &ErrorBody) -> Vec<(&str, &str)> {
        body.details()
            .iter()
            .map(|detail| (detail.key(), detail.value()))
            .collect()
    }

    #[test]
    fn a_missing_catalog_maps_to_catalog_unavailable_with_its_reason_and_hint() {
        let body = catalog_not_configured();
        assert_eq!(body.code(), "catalog.unavailable");
        assert_eq!(body.message(), "the provider catalog is not available");
        assert_eq!(details(&body), [("reason", "not_configured")]);
        assert_eq!(
            body.hint(),
            Some("run stacklyd from Stackly.app or set STACKLY_CATALOG_DIR")
        );
        for (reason, code) in [
            (UnavailableReason::Missing, "missing"),
            (UnavailableReason::IndexInvalid, "index_invalid"),
            (UnavailableReason::UnsupportedSchema, "unsupported_schema"),
        ] {
            let body = catalog_unavailable(reason);
            assert_eq!(details(&body), [("reason", code)]);
            assert!(body.hint().is_some(), "{code}");
        }
    }

    #[test]
    fn an_unknown_provider_maps_to_provider_not_found_with_a_suggestion_only_when_there_is_one() {
        let nats: ProviderName = "official/nats".parse().unwrap();
        let suggested = provider_not_found("nats", Some(&nats));
        assert_eq!(suggested.code(), "catalog.provider_not_found");
        assert_eq!(
            suggested.message(),
            "no provider nats in the catalog; did you mean official/nats"
        );
        assert_eq!(
            details(&suggested),
            [("provider", "nats"), ("suggestion", "official/nats")]
        );
        assert_eq!(
            suggested.hint(),
            Some("use the full provider ID, including its namespace")
        );

        let plain = provider_not_found("official/postgres", None);
        assert_eq!(
            plain.message(),
            "no provider official/postgres in the catalog"
        );
        assert_eq!(details(&plain), [("provider", "official/postgres")]);
        assert_eq!(
            plain.hint(),
            Some("run stackly provider list to see the catalog")
        );
    }

    #[test]
    fn a_gate_refusal_maps_to_not_installable_with_all_five_details() {
        let provider: ProviderName = "official/intelonly".parse().unwrap();
        let version: ExactVersion = "2.15.1".parse().unwrap();
        let targets: Vec<Target> = vec!["macos-x86_64".parse().unwrap()];

        let body = not_installable(
            &provider,
            &version,
            host().target,
            NotInstallableReason::NoBuildForArch,
            &targets,
        );

        assert_eq!(body.code(), "catalog.not_installable");
        assert_eq!(
            body.message(),
            "official/intelonly 2.15.1 can't be installed on macos-aarch64"
        );
        assert_eq!(
            details(&body),
            [
                ("provider", "official/intelonly"),
                ("version", "2.15.1"),
                ("target", "macos-aarch64"),
                ("availableTargets", "macos-x86_64"),
                ("reason", "no_build_for_arch"),
            ]
        );
        assert!(body.hint().is_some());
        let fixture_frame: serde_json::Value = serde_json::from_slice(
            &std::fs::read(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../protocols/fixtures/ipc/frame-error-catalog-not-installable.json"),
            )
            .unwrap(),
        )
        .unwrap();
        let expected: ErrorBody =
            serde_json::from_value(fixture_frame["content"]["error"].clone()).unwrap();
        assert_eq!(body, expected);
    }

    #[test]
    fn a_missing_version_maps_to_version_not_found() {
        let provider: ProviderName = "official/nats".parse().unwrap();
        let body = version_not_found(&provider, &"9.9.9".parse().unwrap());

        assert_eq!(body.code(), "catalog.version_not_found");
        assert_eq!(
            body.message(),
            "official/nats has no version 9.9.9 in the catalog"
        );
        assert_eq!(
            details(&body),
            [("provider", "official/nats"), ("version", "9.9.9")]
        );
        assert!(body.hint().is_some());
    }

    #[test]
    fn every_gate_refusal_of_a_version_matches_its_install_verdict() {
        let catalog = fixture_catalog();
        for (id, installable, reason) in [
            ("official/nats", true, None),
            (
                "official/intelonly",
                false,
                Some(dto::SupportReason::NoBuildForArch),
            ),
            (
                "official/blockedhost",
                false,
                Some(dto::SupportReason::ArtifactBlocked),
            ),
        ] {
            let detail = detail_of(&catalog, id);
            assert_eq!(
                detail.versions[0].install,
                dto::InstallVerdict {
                    installable,
                    reason
                },
                "{id}"
            );
        }
    }
}
