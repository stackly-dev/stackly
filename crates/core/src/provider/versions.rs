//! Rules of `versions.json`, schema `stackly.provider-versions/v1` (spec
//! 0009, "`versions.json`").

use std::collections::BTreeSet;

use stackly_domain::{Arch, ExactVersion, Os, RuntimeLine, SoftwareName, Target};

use super::issue::{FileIssues, IssueCode};
use super::names::is_component;
use super::raw::{RawArtifact, RawVersions};

/// The only `schema` of `versions.json` this engine reads.
pub(crate) const VERSIONS_SCHEMA: &str = "stackly.provider-versions/v1";

/// What the catalog is checked against from the manifest.
pub(crate) struct VersionsContext<'a> {
    pub(crate) software: Option<&'a SoftwareName>,
    pub(crate) testing: bool,
    pub(crate) platforms: &'a [Target],
    /// Executable paths the manifest runs, as written, with where.
    pub(crate) executables: Vec<(String, String)>,
}

/// Checks `versions.json` and its cross rules with the manifest.
pub(crate) fn check_versions(
    raw: &RawVersions,
    context: &VersionsContext<'_>,
    issues: &mut FileIssues<'_>,
) {
    let rule = IssueCode::VersionsRule;
    if raw.schema != VERSIONS_SCHEMA {
        issues.at("schema", rule, format!("schema must be {VERSIONS_SCHEMA}"));
    }
    match raw.software.parse::<SoftwareName>() {
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
        }
        Err(_) => issues.at(
            "software",
            IssueCode::InvalidName,
            "software is not a valid software name",
        ),
    }
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
        check_artifact(artifact, &path, context, issues);
    }
}

fn check_artifact(
    artifact: &RawArtifact,
    path: &str,
    context: &VersionsContext<'_>,
    issues: &mut FileIssues<'_>,
) {
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
            return;
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
    if let Some(format) = &artifact.format
        && !matches!(format.as_str(), "tar_gz" | "zip" | "binary")
    {
        issues.at(
            at("format"),
            IssueCode::InvalidValue,
            "format must be tar_gz, zip, or binary",
        );
    }
    if let Some(policy) = &artifact.signing_policy
        && !matches!(policy.as_str(), "keep_or_adhoc" | "adhoc_pipeline")
    {
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
        ("minOs", &artifact.min_os),
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
