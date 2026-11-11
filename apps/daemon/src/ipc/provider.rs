//! `provider.validate` (spec 0036, AC-5): checks a package folder with
//! `stackly_core::provider::check_package`, the validator `cargo xtask
//! provider check`, the harness, and import share, and maps what it found.
//!
//! The handler only reads the folder through the platform filesystem. It
//! never touches the registry, the catalog, the operation engine, or the
//! audit log, and it never runs anything from the package.

use std::path::PathBuf;

use stackly_core::provider::{
    ApiVersionStatus as CoreStatus, Finding, FindingOrigin as CoreOrigin, NameCheck, PackageCheck,
    SUPPORTED_API_VERSIONS, Severity, check_package,
};
use stackly_protocol::codes::internal;
use stackly_protocol::provider::{
    ApiVersionStatus, FindingOrigin, FindingSeverity, PackageIdentity, ProviderValidateParams,
    ProviderValidation, SupportedApiVersion, VALIDATION_FINDINGS_MAX, ValidationFinding,
};
use stackly_protocol::{Count, ErrorBody};

use crate::dispatch::HandlerFuture;
use crate::state::DaemonCore;

/// Checks the package at `params.path`. A package that does not load is
/// still a successful reply with `loadable: false`.
pub fn validate(
    core: &DaemonCore,
    params: ProviderValidateParams,
) -> HandlerFuture<'_, ProviderValidation> {
    Box::pin(async move {
        let platform = core.platform().clone();
        let dir = PathBuf::from(params.path);
        let check = tokio::task::spawn_blocking(move || {
            check_package(platform.fs(), &dir, NameCheck::FolderName)
        })
        .await
        .map_err(|_| {
            Box::new(ErrorBody::new(
                &internal::UNEXPECTED,
                "the package check failed",
            ))
        })?;
        Ok(validation(&check))
    })
}

/// Maps a [`PackageCheck`] to its wire form, keeping the first
/// [`VALIDATION_FINDINGS_MAX`] findings.
pub(crate) fn validation(check: &PackageCheck) -> ProviderValidation {
    let findings: Vec<ValidationFinding> = check
        .findings
        .iter()
        .take(VALIDATION_FINDINGS_MAX)
        .map(finding)
        .collect();
    let total = check.findings.len() as u64;
    ProviderValidation {
        folder: check.folder.clone(),
        loadable: check.loadable(),
        publishable: check.publishable(),
        api_version: check.api_version.clone(),
        supported_api_versions: SUPPORTED_API_VERSIONS
            .iter()
            .map(|version| SupportedApiVersion {
                name: version.name.to_owned(),
                status: match version.status {
                    CoreStatus::Current => ApiVersionStatus::Current,
                    CoreStatus::Deprecated => ApiVersionStatus::Deprecated,
                },
            })
            .collect(),
        identity: check.package.as_ref().map(|package| {
            let key = package.key();
            PackageIdentity {
                provider: key.provider().to_string(),
                revision: key.revision().to_string(),
                manifest_sha256: key.manifest_sha256().to_string(),
                software: package.software().to_string(),
                display_name: package.manifest().display_name().to_owned(),
            }
        }),
        omitted: Count::saturating(total - findings.len() as u64),
        finding_count: Count::saturating(total),
        findings,
    }
}

fn finding(finding: &Finding) -> ValidationFinding {
    let issue = &finding.issue;
    ValidationFinding {
        severity: match finding.severity {
            Severity::Error => FindingSeverity::Error,
            Severity::Warning => FindingSeverity::Warning,
        },
        origin: match finding.origin {
            CoreOrigin::Package => FindingOrigin::Package,
            CoreOrigin::Authoring => FindingOrigin::Authoring,
            CoreOrigin::Scenarios => FindingOrigin::Scenarios,
        },
        file: issue.file.clone(),
        location: issue.location.to_string(),
        code: issue.code.as_code().to_owned(),
        message: issue.message.clone(),
        reason: issue.reason.map(str::to_owned),
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use stackly_platform_macos::MacosFilesystem;

    use super::*;

    #[test]
    fn a_hundred_and_fifty_findings_are_cut_to_a_hundred_with_the_rest_counted() {
        let mut check = check_package(
            &MacosFilesystem,
            Path::new("/nonexistent/stackly-provider-validate"),
            NameCheck::FolderName,
        );
        let first = check.findings[0].clone();
        check.findings = vec![first; 150];

        let reply = validation(&check);

        assert_eq!(reply.findings.len(), 100);
        assert_eq!(u64::from(reply.finding_count), 150);
        assert_eq!(u64::from(reply.omitted), 50);
        assert_eq!(reply.folder, "stackly-provider-validate");
        assert!(!reply.loadable && reply.identity.is_none());
    }
}
