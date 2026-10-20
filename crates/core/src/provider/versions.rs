//! Rules of `versions.json`, schema `stackly.provider-versions/v1` (spec
//! 0009, "`versions.json`").

use std::collections::BTreeSet;

use stackly_domain::{
    Arch, ArtifactFormat, ExactVersion, Os, OsVersion, RuntimeLine, Sha256Digest, SoftwareName,
    Target,
};

use super::issue::{FileIssues, Issue, IssueCode, Location};
use super::names::is_component;
use super::package::MANIFEST_FILE;
use super::raw::{
    RawArtifact, RawMetadata, RawVersions, check_yaml_value, decode_yaml, join_path, parse_yaml,
};

/// The only `schema` of `versions.json` this engine reads.
pub const VERSIONS_SCHEMA: &str = "stackly.provider-versions/v1";

/// The validated `versions.json` of a package (spec 0009, *Tipe core*): its
/// software and every artifact, in file order (the inventory order).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionCatalog {
    software: SoftwareName,
    artifacts: Vec<VersionArtifact>,
}

impl VersionCatalog {
    /// Returns the software every artifact belongs to.
    #[must_use]
    pub const fn software(&self) -> &SoftwareName {
        &self.software
    }

    /// Returns every artifact in the order of the file.
    #[must_use]
    pub fn artifacts(&self) -> &[VersionArtifact] {
        &self.artifacts
    }
}

/// One concrete artifact of `versions.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct VersionArtifact {
    /// The artifact ID, unique in the file, such as
    /// `nats-2.15.1-macos-aarch64`.
    pub id: String,
    /// The runtime line the version belongs to.
    pub line: RuntimeLine,
    /// The exact version.
    pub version: ExactVersion,
    /// The OS and architecture it is built for.
    pub target: Target,
    /// Whether it can be installed, and the fields of that status.
    pub status: ArtifactStatus,
}

/// The status of an artifact and the fields that come with it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ArtifactStatus {
    /// A real artifact that can be downloaded and installed.
    #[non_exhaustive]
    Available {
        /// The lowest OS version the executables need.
        min_os: OsVersion,
        /// How the artifact is packed.
        format: ArtifactFormat,
        /// Candidate `https` URLs, in order of preference.
        urls: Vec<String>,
        /// The SHA-256 of the downloaded bytes.
        sha256: Sha256Digest,
        /// The download size in bytes.
        size: u64,
        /// The one folder at the root of the archive, if any.
        archive_root: Option<String>,
        /// What happens to the code signature of the executables.
        signing_policy: SigningPolicy,
        /// The SPDX license of the artifact.
        license: String,
        /// The `https` URL of the license text.
        license_url: String,
        /// Executable paths relative to the runtime root.
        executables: Vec<String>,
    },
    /// An artifact that cannot be installed yet, with the recorded reason.
    #[non_exhaustive]
    Blocked {
        /// Why it is blocked.
        reason: String,
        /// Who can unblock it.
        owner: String,
        /// Since when, written `YYYY-MM-DD`.
        since: String,
        /// The SPDX license, if known.
        license: Option<String>,
        /// The `https` URL of the license text, if known.
        license_url: Option<String>,
    },
    /// A test fixture of the `testing` namespace, never downloadable.
    Fixture {
        /// Executable paths relative to the runtime root.
        executables: Vec<String>,
    },
}

/// What the installer does with the code signature of an artifact's
/// executables (spec 0003).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SigningPolicy {
    /// Keep a valid upstream signature, or sign ad hoc when there is none.
    KeepOrAdhoc,
    /// The maintainer pipeline signs ad hoc.
    AdhocPipeline,
}

impl SigningPolicy {
    /// Returns the stable code, such as `keep_or_adhoc`.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::KeepOrAdhoc => "keep_or_adhoc",
            Self::AdhocPipeline => "adhoc_pipeline",
        }
    }

    fn from_code(code: &str) -> Option<Self> {
        [Self::KeepOrAdhoc, Self::AdhocPipeline]
            .into_iter()
            .find(|policy| policy.as_code() == code)
    }
}

/// Reads `metadata.software` from the text of a `provider.yaml`, for the
/// `versions.json` generator of `cargo xtask codegen`.
///
/// The YAML is parsed and checked like the loader does (no tags, merge
/// keys, or duplicate keys), and only the `metadata` mapping is decoded,
/// strictly. The rest of the manifest is not read, so a package whose
/// other parts are broken still gets its catalog generated.
///
/// # Errors
///
/// Returns the first [`Issue`] in `provider.yaml`: YAML that does not
/// parse, a tag or merge key, a missing or unknown `metadata` field, or a
/// `software` that is not a valid software name.
pub fn manifest_software(manifest: &str) -> Result<SoftwareName, Issue> {
    let file = MANIFEST_FILE;
    let value = parse_yaml(file, manifest)?;
    check_yaml_value(file, &value)?;
    let metadata = value.get("metadata").cloned().ok_or_else(|| {
        Issue::new(
            file,
            Location::Field("metadata".into()),
            IssueCode::MissingField,
            "missing field `metadata`",
        )
    })?;
    let raw: RawMetadata =
        decode_yaml(file, metadata, IssueCode::UnknownField).map_err(|mut issue| {
            if let Location::Field(path) = &issue.location {
                issue.location = Location::Field(if path == "." {
                    "metadata".to_owned()
                } else {
                    join_path("metadata", path)
                });
            }
            issue
        })?;
    raw.software.parse().map_err(|_| {
        Issue::new(
            file,
            Location::Field("metadata.software".into()),
            IssueCode::InvalidName,
            "software is not a valid software name",
        )
    })
}

/// What the catalog is checked against from the manifest.
pub(crate) struct VersionsContext<'a> {
    pub(crate) software: Option<&'a SoftwareName>,
    pub(crate) testing: bool,
    pub(crate) platforms: &'a [Target],
    /// Executable paths the manifest runs, as written, with where.
    pub(crate) executables: Vec<(String, String)>,
}

/// Checks `versions.json` and its cross rules with the manifest, and
/// returns its typed form when the file has no issue.
pub(crate) fn check_versions(
    raw: &RawVersions,
    context: &VersionsContext<'_>,
    issues: &mut FileIssues<'_>,
) -> Option<VersionCatalog> {
    let start = issues.count();
    let rule = IssueCode::VersionsRule;
    if raw.schema != VERSIONS_SCHEMA {
        issues.at("schema", rule, format!("schema must be {VERSIONS_SCHEMA}"));
    }
    let software = match raw.software.parse::<SoftwareName>() {
        Ok(software) => {
            if context
                .software
                .is_some_and(|expected| *expected != software)
            {
                issues.at(
                    "software",
                    rule,
                    "software differs from metadata.software in provider.yaml",
                );
            }
            Some(software)
        }
        Err(_) => {
            issues.at(
                "software",
                IssueCode::InvalidName,
                "software is not a valid software name",
            );
            None
        }
    };
    let mut artifacts = Vec::with_capacity(raw.artifacts.len());
    let mut ids = BTreeSet::new();
    let mut builds = BTreeSet::new();
    for (index, artifact) in raw.artifacts.iter().enumerate() {
        let path = format!("artifacts[{index}]");
        if !ids.insert(artifact.id.as_str()) {
            issues.at(format!("{path}.id"), rule, "artifact IDs must be unique");
        }
        if !builds.insert((
            artifact.version.as_str(),
            artifact.os.as_str(),
            artifact.arch.as_str(),
        )) {
            issues.at(&path, rule, "each version, os, and arch appears once");
        }
        if artifact.id.is_empty() || !artifact.id.bytes().all(super::names::is_component_byte) {
            issues.at(
                format!("{path}.id"),
                rule,
                "artifact IDs are letters, digits, and . _ -",
            );
        }
        if let Some(typed) = check_artifact(artifact, &path, context, issues) {
            artifacts.push(typed);
        }
    }
    (issues.count() == start).then_some(())?;
    Some(VersionCatalog {
        software: software?,
        artifacts,
    })
}

/// Checks one artifact and returns its typed form when it has no issue.
fn check_artifact(
    artifact: &RawArtifact,
    path: &str,
    context: &VersionsContext<'_>,
    issues: &mut FileIssues<'_>,
) -> Option<VersionArtifact> {
    let start = issues.count();
    let rule = IssueCode::VersionsRule;
    let at = |field: &str| format!("{path}.{field}");
    let line = artifact.line.parse::<RuntimeLine>();
    let version = artifact.version.parse::<ExactVersion>();
    if line.is_err() {
        issues.at(
            at("line"),
            rule,
            "line must be a runtime line such as 17 or 8.4",
        );
    }
    if version.is_err() {
        issues.at(
            at("version"),
            rule,
            "version must be an exact version such as 17.6",
        );
    }
    if let (Ok(line), Ok(version)) = (&line, &version)
        && !line.contains(version)
    {
        issues.at(
            at("version"),
            rule,
            "version must start with its line followed by a dot",
        );
    }
    let os = artifact.os.parse::<Os>();
    let arch = artifact.arch.parse::<Arch>();
    if os.is_err() {
        issues.at(
            at("os"),
            IssueCode::InvalidValue,
            "os must be a domain code such as macos",
        );
    }
    if arch.is_err() {
        issues.at(
            at("arch"),
            IssueCode::InvalidValue,
            "arch must be a domain code such as aarch64",
        );
    }
    if let (Ok(os), Ok(arch)) = (os, arch)
        && !context.platforms.contains(&Target::new(os, arch))
    {
        issues.at(
            path,
            rule,
            "the artifact's os and arch are not in compatibility.platforms",
        );
    }
    let present = [
        ("minOs", artifact.min_os.is_some()),
        ("format", artifact.format.is_some()),
        ("urls", artifact.urls.is_some()),
        ("sha256", artifact.sha256.is_some()),
        ("size", artifact.size.is_some()),
        ("archiveRoot", artifact.archive_root.is_some()),
        ("signingPolicy", artifact.signing_policy.is_some()),
        ("license", artifact.license.is_some()),
        ("licenseUrl", artifact.license_url.is_some()),
        ("executables", artifact.executables.is_some()),
        ("blocked", artifact.blocked.is_some()),
    ];
    let (required, forbidden): (&[&str], &[&str]) = match artifact.status.as_str() {
        "available" => (
            &[
                "minOs",
                "format",
                "urls",
                "sha256",
                "size",
                "signingPolicy",
                "license",
                "licenseUrl",
                "executables",
            ],
            &["blocked"],
        ),
        "blocked" => (
            &["blocked"],
            &[
                "minOs",
                "format",
                "urls",
                "sha256",
                "size",
                "archiveRoot",
                "signingPolicy",
                "executables",
            ],
        ),
        "fixture" => {
            if !context.testing {
                issues.at(
                    at("status"),
                    IssueCode::FixtureOutsideTesting,
                    "fixture artifacts belong only to the testing namespace",
                );
            }
            (
                &["executables"],
                &[
                    "urls",
                    "sha256",
                    "size",
                    "format",
                    "minOs",
                    "signingPolicy",
                    "blocked",
                ],
            )
        }
        _ => {
            issues.at(
                at("status"),
                IssueCode::InvalidValue,
                "status must be available, blocked, or fixture",
            );
            return None;
        }
    };
    for (field, set) in present {
        if required.contains(&field) && !set {
            issues.at(
                at(field),
                rule,
                format!("{} artifacts need {field}", artifact.status),
            );
        }
        if forbidden.contains(&field) && set {
            issues.at(
                at(field),
                rule,
                format!("{} artifacts cannot have {field}", artifact.status),
            );
        }
    }
    let format = artifact
        .format
        .as_ref()
        .map(|format| format.parse::<ArtifactFormat>());
    if matches!(format, Some(Err(_))) {
        issues.at(
            at("format"),
            IssueCode::InvalidValue,
            "format must be tar_gz, zip, or binary",
        );
    }
    let min_os = artifact
        .min_os
        .as_ref()
        .map(|min_os| min_os.parse::<OsVersion>());
    if matches!(min_os, Some(Err(_))) {
        issues.at(
            at("minOs"),
            IssueCode::InvalidValue,
            "minOs must be an os version such as 13.0",
        );
    }
    let signing_policy = artifact
        .signing_policy
        .as_deref()
        .map(SigningPolicy::from_code);
    if matches!(signing_policy, Some(None)) {
        issues.at(
            at("signingPolicy"),
            IssueCode::InvalidValue,
            "signingPolicy must be keep_or_adhoc or adhoc_pipeline",
        );
    }
    if let Some(sha256) = &artifact.sha256
        && (sha256.len() != 64
            || !sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
    {
        issues.at(
            at("sha256"),
            rule,
            "sha256 must be 64 lowercase hex characters",
        );
    }
    if let Some(urls) = &artifact.urls
        && (urls.is_empty() || urls.iter().any(|url| !url.starts_with("https://")))
    {
        issues.at(at("urls"), rule, "urls must hold at least one https URL");
    }
    if let Some(url) = &artifact.license_url
        && !url.starts_with("https://")
    {
        issues.at(at("licenseUrl"), rule, "licenseUrl must be an https URL");
    }
    for (field, value) in [
        ("archiveRoot", &artifact.archive_root),
        ("license", &artifact.license),
    ] {
        if value
            .as_ref()
            .is_some_and(|text| text.is_empty() || text.chars().any(char::is_control))
        {
            issues.at(
                at(field),
                rule,
                format!("{field} must be non-empty text without control characters"),
            );
        }
    }
    if let Some(blocked) = &artifact.blocked {
        if blocked.reason.trim().is_empty() || blocked.owner.trim().is_empty() {
            issues.at(at("blocked"), rule, "blocked needs a reason and an owner");
        }
        if !is_date(&blocked.since) {
            issues.at(
                at("blocked.since"),
                rule,
                "since must be a date written YYYY-MM-DD",
            );
        }
    }
    if let Some(executables) = &artifact.executables {
        if executables.is_empty() {
            issues.at(
                at("executables"),
                rule,
                "executables must list at least one path",
            );
        }
        for (index, executable) in executables.iter().enumerate() {
            if !executable.split('/').all(is_component) {
                issues.at(
                    format!("{path}.executables[{index}]"),
                    rule,
                    "executables are relative paths of literal components",
                );
            }
        }
        if matches!(artifact.status.as_str(), "available" | "fixture") {
            for (needed, from) in &context.executables {
                if !executables.contains(needed) {
                    issues.at(
                        at("executables"),
                        IssueCode::ExecutableNotInArtifact,
                        format!("{from} runs {needed}, which this artifact does not list"),
                    );
                }
            }
        }
    }
    if issues.count() != start {
        return None;
    }
    let status = match artifact.status.as_str() {
        "available" => ArtifactStatus::Available {
            min_os: min_os?.ok()?,
            format: format?.ok()?,
            urls: artifact.urls.clone()?,
            sha256: artifact.sha256.as_deref()?.parse().ok()?,
            size: artifact.size?,
            archive_root: artifact.archive_root.clone(),
            signing_policy: signing_policy??,
            license: artifact.license.clone()?,
            license_url: artifact.license_url.clone()?,
            executables: artifact.executables.clone()?,
        },
        "blocked" => {
            let blocked = artifact.blocked.as_ref()?;
            ArtifactStatus::Blocked {
                reason: blocked.reason.clone(),
                owner: blocked.owner.clone(),
                since: blocked.since.clone(),
                license: artifact.license.clone(),
                license_url: artifact.license_url.clone(),
            }
        }
        _ => ArtifactStatus::Fixture {
            executables: artifact.executables.clone()?,
        },
    };
    Some(VersionArtifact {
        id: artifact.id.clone(),
        line: line.ok()?,
        version: version.ok()?,
        target: Target::new(os.ok()?, arch.ok()?),
        status,
    })
}

/// Whether text is a date written `YYYY-MM-DD`.
fn is_date(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(index, b)| index == 4 || index == 7 || b.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use stackly_domain::{Arch, Os};

    use super::super::issue::Issues;
    use super::super::package::{NameCheck, PackageFiles};
    use super::super::validate::ValidatedPackage;
    use super::*;

    macro_rules! package_file {
        ($package:literal, $path:literal) => {
            (
                $path.to_owned(),
                include_bytes!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../providers/",
                    $package,
                    "/",
                    $path
                ))
                .to_vec(),
            )
        };
    }

    fn nats() -> ValidatedPackage {
        let files = BTreeMap::from([
            package_file!("nats", "provider.yaml"),
            package_file!("nats", "versions.json"),
            package_file!("nats", "schemas/settings.schema.json"),
            package_file!("nats", "templates/nats.conf.tmpl"),
        ]);
        ValidatedPackage::from_files(PackageFiles::new("nats", files), NameCheck::FolderName)
            .unwrap()
    }

    fn fake() -> ValidatedPackage {
        let files = BTreeMap::from([
            package_file!("fake", "provider.yaml"),
            package_file!("fake", "versions.json"),
            package_file!("fake", "schemas/settings.schema.json"),
            package_file!("fake", "templates/fake.json.tmpl"),
        ]);
        ValidatedPackage::from_files(PackageFiles::new("fake", files), NameCheck::FolderName)
            .unwrap()
    }

    /// Checks a `versions.json` text against a macOS Apple Silicon
    /// manifest of software `nats` and returns the catalog and the codes.
    fn check(json: &str, testing: bool) -> (Option<VersionCatalog>, Vec<(String, IssueCode)>) {
        let raw: RawVersions = serde_json::from_str(json).unwrap();
        let software: SoftwareName = "nats".parse().unwrap();
        let platforms = [Target::new(Os::Macos, Arch::Aarch64)];
        let context = VersionsContext {
            software: Some(&software),
            testing,
            platforms: &platforms,
            executables: vec![("nats-server".to_owned(), "runtime.executable".to_owned())],
        };
        let mut issues = Issues::new();
        let catalog = check_versions(
            &raw,
            &context,
            &mut FileIssues::new("versions.json", &mut issues),
        );
        let codes = issues
            .into_vec()
            .into_iter()
            .map(|issue| (issue.location.to_string(), issue.code))
            .collect();
        (catalog, codes)
    }

    const AVAILABLE: &str = r#"{ "id": "nats-2.15.1-macos-aarch64", "line": "2.15", "version": "2.15.1", "os": "macos", "arch": "aarch64", "status": "available", "minOs": "13.0", "format": "tar_gz", "urls": ["https://example.invalid/nats.tar.gz"], "sha256": "ed0fe39222ec2e317c4d0e1a9cdc16c63ec523d94f9355d24a902a4db2655ffd", "size": 6785953, "signingPolicy": "keep_or_adhoc", "license": "Apache-2.0", "licenseUrl": "https://example.invalid/LICENSE", "executables": ["nats-server"] }"#;

    fn versions_with(artifacts: &[&str]) -> String {
        format!(
            r#"{{ "schema": "stackly.provider-versions/v1", "software": "nats", "artifacts": [{}] }}"#,
            artifacts.join(",")
        )
    }

    #[test]
    fn the_nats_package_exposes_its_typed_artifacts_in_file_order() {
        let package = nats();
        let versions = package.versions();

        assert_eq!(versions.software().as_str(), "nats");
        let [artifact] = versions.artifacts() else {
            panic!("nats has one artifact");
        };
        assert_eq!(artifact.id, "nats-2.15.1-macos-aarch64");
        assert_eq!(artifact.line.as_str(), "2.15");
        assert_eq!(artifact.version.as_str(), "2.15.1");
        assert_eq!(artifact.target, Target::new(Os::Macos, Arch::Aarch64));
        let ArtifactStatus::Available {
            min_os,
            format,
            urls,
            sha256,
            size,
            archive_root,
            signing_policy,
            license,
            license_url,
            executables,
        } = &artifact.status
        else {
            panic!("the nats artifact is available");
        };
        assert_eq!(min_os.to_string(), "13.0");
        assert_eq!(*format, ArtifactFormat::TarGz);
        assert_eq!(urls.len(), 1);
        assert_eq!(
            sha256.to_string(),
            "ed0fe39222ec2e317c4d0e1a9cdc16c63ec523d94f9355d24a902a4db2655ffd"
        );
        assert_eq!(*size, 6_785_953);
        assert_eq!(
            archive_root.as_deref(),
            Some("nats-server-v2.15.1-darwin-arm64")
        );
        assert_eq!(*signing_policy, SigningPolicy::KeepOrAdhoc);
        assert_eq!(license, "Apache-2.0");
        assert!(license_url.starts_with("https://"));
        assert_eq!(executables, &["nats-server"]);
    }

    #[test]
    fn fixture_artifacts_keep_only_their_executables() {
        let package = fake();
        let [artifact] = package.versions().artifacts() else {
            panic!("fake has one artifact");
        };

        assert_eq!(
            artifact.status,
            ArtifactStatus::Fixture {
                executables: vec!["stackly-fake-server".to_owned()]
            }
        );
    }

    #[test]
    fn artifacts_keep_the_order_of_the_file() {
        let second = AVAILABLE
            .replace("nats-2.15.1-macos-aarch64", "nats-2.14.0-macos-aarch64")
            .replace(r#""2.15""#, r#""2.14""#)
            .replace("2.15.1", "2.14.0");

        let (catalog, codes) = check(&versions_with(&[AVAILABLE, &second]), false);

        assert_eq!(codes, []);
        let ids: Vec<&str> = catalog
            .as_ref()
            .unwrap()
            .artifacts()
            .iter()
            .map(|artifact| artifact.id.as_str())
            .collect();
        assert_eq!(
            ids,
            ["nats-2.15.1-macos-aarch64", "nats-2.14.0-macos-aarch64"]
        );
    }

    #[test]
    fn blocked_artifacts_keep_their_reason_owner_date_and_license() {
        let blocked = r#"{ "id": "nats-2.16.0-macos-aarch64", "line": "2.16", "version": "2.16.0", "os": "macos", "arch": "aarch64", "status": "blocked", "license": "Apache-2.0", "blocked": { "reason": "upstream build is not signed", "owner": "runtime maintainer", "since": "2026-10-01" } }"#;

        let (catalog, codes) = check(&versions_with(&[blocked]), false);

        assert_eq!(codes, []);
        let catalog = catalog.unwrap();
        let ArtifactStatus::Blocked {
            reason,
            owner,
            since,
            license,
            license_url,
        } = &catalog.artifacts()[0].status
        else {
            panic!("the artifact is blocked");
        };
        assert_eq!(reason, "upstream build is not signed");
        assert_eq!(owner, "runtime maintainer");
        assert_eq!(since, "2026-10-01");
        assert_eq!(license.as_deref(), Some("Apache-2.0"));
        assert_eq!(*license_url, None);
    }

    #[test]
    fn min_os_must_be_an_os_version() {
        for min_os in ["13.04", "thirteen", "13.", "1.2.3.4"] {
            let artifact =
                AVAILABLE.replace(r#""minOs": "13.0""#, &format!(r#""minOs": "{min_os}""#));

            let (catalog, codes) = check(&versions_with(&[&artifact]), false);

            assert_eq!(
                codes,
                [("artifacts[0].minOs".to_owned(), IssueCode::InvalidValue)],
                "{min_os}"
            );
            assert!(catalog.is_none());
        }
    }

    #[test]
    fn format_and_signing_policy_stay_closed_sets() {
        let tar = AVAILABLE.replace(r#""tar_gz""#, r#""tar""#);
        let policy = AVAILABLE.replace(r#""keep_or_adhoc""#, r#""resign""#);

        assert_eq!(
            check(&versions_with(&[&tar]), false).1,
            [("artifacts[0].format".to_owned(), IssueCode::InvalidValue)]
        );
        assert_eq!(
            check(&versions_with(&[&policy]), false).1,
            [(
                "artifacts[0].signingPolicy".to_owned(),
                IssueCode::InvalidValue
            )]
        );
    }

    #[test]
    fn a_file_with_any_issue_gives_no_typed_catalog() {
        let missing_sha = AVAILABLE.replace(
            r#""sha256": "ed0fe39222ec2e317c4d0e1a9cdc16c63ec523d94f9355d24a902a4db2655ffd", "#,
            "",
        );

        let (catalog, codes) = check(
            &versions_with(&[
                AVAILABLE.replace("nats-2.15.1", "nats-2.15.1b").as_str(),
                &missing_sha,
            ]),
            false,
        );

        assert!(!codes.is_empty());
        assert!(catalog.is_none());
    }

    const METADATA: &str = "metadata:\n  id: official/nats\n  software: nats\n  displayName: NATS\n  revision: \"1\"\n  category: messaging\n";

    #[test]
    fn the_software_comes_from_metadata_without_reading_the_rest() {
        let manifest = format!("apiVersion: stackly/v9\n{METADATA}lifecycle: broken\n");

        let software = manifest_software(&manifest).unwrap();

        assert_eq!(software.as_str(), "nats");
    }

    #[test]
    fn metadata_problems_are_reported_under_metadata() {
        let unknown = manifest_software(&format!("{METADATA}  shell: true\n")).unwrap_err();
        let missing = manifest_software("metadata:\n  id: official/nats\n").unwrap_err();
        let absent = manifest_software("kind: Provider\n").unwrap_err();
        let invalid =
            manifest_software(&METADATA.replace("software: nats", "software: Nats")).unwrap_err();

        assert_eq!(unknown.code, IssueCode::UnknownField);
        assert_eq!(unknown.location, Location::Field("metadata.shell".into()));
        assert_eq!(missing.code, IssueCode::MissingField);
        assert_eq!(
            missing.location,
            Location::Field("metadata.software".into())
        );
        assert_eq!(absent.location, Location::Field("metadata".into()));
        assert_eq!(invalid.code, IssueCode::InvalidName);
        assert_eq!(
            invalid.location,
            Location::Field("metadata.software".into())
        );
    }

    #[test]
    fn yaml_tags_in_the_manifest_are_rejected_before_metadata_is_read() {
        let issue = manifest_software(&format!("{METADATA}x: !tag y\n")).unwrap_err();

        assert_eq!(issue.code, IssueCode::InvalidValue);
        assert_eq!(issue.file, "provider.yaml");
    }
}
