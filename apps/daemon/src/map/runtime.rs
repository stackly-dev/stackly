//! Maps runtimes, artifacts, and resolutions to the `runtime` DTOs and
//! errors (spec 0018, *API surface*, *Kode error*).
//!
//! Every status and reason comes from `stackly_core`; this module copies
//! them. An enum travels by its stable code, which spec 0018 makes the same
//! as its wire value, so a core value and its DTO value never drift apart.

use serde::Deserialize;
use stackly_core::catalog::NotInstallableReason;
use stackly_core::provider::{ArtifactStatus, VersionArtifact};
use stackly_core::runtime::compatibility::SupportVerdict;
use stackly_core::runtime::install::{ArtifactInput, InstallInput};
use stackly_core::runtime::resolve::{ResolveError, UnresolvedReason};
use stackly_domain::{
    ArtifactRecord, CompatibilityRecordRow, CompatibilityResult, CompatibilityScope, InstallMode,
    Instance, InstanceLifecycle, Operation, Runtime, RuntimeId, SupportStatus, Target,
};
use stackly_platform::HostFacts;
use stackly_protocol::catalog::{DependencyKind, HostTarget, SupportReason};
use stackly_protocol::codes::runtime as codes;
use stackly_protocol::runtime::{
    ArtifactFormat, ArtifactInfo, CompatibilityRecordView, DependencyInfo, ExecutableSignature,
    InstallingRuntime, ProviderRevisionRef, ResolvedArtifact, RuntimeDetail, RuntimeSummary,
    RuntimeUser, SignatureInfo,
};
use stackly_protocol::{Count, ErrorBody, Timestamp};
use uuid::Uuid;

use crate::ipc::operation::dto::revision;
use crate::map::catalog::{host_target, not_installable_text, wire};

/// The wire status and reason of a verdict.
#[must_use]
pub fn support(
    status: SupportStatus,
    reason: stackly_core::catalog::SupportReason,
) -> (stackly_protocol::catalog::SupportStatus, SupportReason) {
    (wire(status.as_code()), wire(reason.as_code()))
}

/// The wire support of `verdict`.
#[must_use]
pub fn verdict_support(
    verdict: &SupportVerdict<'_>,
) -> (stackly_protocol::catalog::SupportStatus, SupportReason) {
    support(verdict.status, verdict.reason)
}

/// The resolved artifact of an `available` catalog artifact; `None` for
/// any other status, which the gate never lets through.
#[must_use]
pub fn resolved_artifact(
    provider: &str,
    software: &str,
    artifact: &VersionArtifact,
    verdict: &SupportVerdict<'_>,
) -> Option<ResolvedArtifact> {
    let ArtifactStatus::Available {
        min_os,
        format,
        sha256,
        size,
        stable,
        unpacked,
        ..
    } = &artifact.status
    else {
        return None;
    };
    let (support, support_reason) = verdict_support(verdict);
    Some(ResolvedArtifact {
        provider: provider.to_owned(),
        software: software.to_owned(),
        artifact_id: artifact.id.clone(),
        version: artifact.version.to_string(),
        line: artifact.line.to_string(),
        target: host_target(artifact.target),
        sha256: sha256.to_string(),
        size: Count::saturating(*size),
        unpacked_size: unpacked
            .as_ref()
            .map(|unpacked| Count::saturating(unpacked.bytes)),
        format: wire(format.as_code()),
        min_os: min_os.to_string(),
        stable: *stable,
        support,
        support_reason,
    })
}

/// The resolved artifact of an install input, for the reply to a request
/// the operation was started for earlier. The catalog is not read again,
/// so the support is the one every artifact without a record has.
///
/// # Errors
///
/// Returns the name of the input field that does not parse.
pub fn resolved_from_input(input: &InstallInput) -> Result<ResolvedArtifact, &'static str> {
    let ArtifactInput {
        id,
        line,
        version,
        target,
        min_os,
        format,
        sha256,
        size,
        stable,
        unpacked_size,
        ..
    } = &input.artifact;
    let target: Target = target.parse().map_err(|_| "artifact.target")?;
    Ok(ResolvedArtifact {
        provider: input.provider.clone(),
        software: input.software.clone(),
        artifact_id: id.clone(),
        version: version.clone(),
        line: line.clone(),
        target: host_target(target),
        sha256: sha256.clone(),
        size: Count::saturating(*size),
        unpacked_size: unpacked_size.map(Count::saturating),
        format: wire(format),
        min_os: min_os.clone(),
        stable: *stable,
        support: wire(SupportStatus::Unverified.as_code()),
        support_reason: wire(stackly_core::catalog::SupportReason::Untested.as_code()),
    })
}

/// The runtimes an instance runs on, retained ones included.
#[must_use]
pub fn runtime_user(instance: &Instance) -> RuntimeUser {
    RuntimeUser {
        instance_id: instance.id().as_uuid(),
        name: instance.name().to_string(),
        retained: instance.lifecycle() == InstanceLifecycle::Retained,
    }
}

/// The summary of `runtime`. `display_name` is the catalog's name for its
/// software, `users` the instances on it, and `dependents` the runtimes
/// that depend on it.
#[must_use]
pub fn runtime_summary(
    runtime: &Runtime,
    display_name: String,
    support: (stackly_protocol::catalog::SupportStatus, SupportReason),
    users: Vec<RuntimeUser>,
    dependents: Vec<Uuid>,
) -> RuntimeSummary {
    let (mode, artifact_sha256) = match runtime.install() {
        InstallMode::Managed { artifact_sha256 } => ("managed", Some(artifact_sha256.to_string())),
        InstallMode::External {
            observed_sha256, ..
        } => ("external", observed_sha256.map(|digest| digest.to_string())),
    };
    RuntimeSummary {
        id: runtime.id().as_uuid(),
        revision: revision(runtime.revision()),
        software: runtime.software().to_string(),
        display_name,
        version: runtime.version().to_string(),
        line: runtime.line().to_string(),
        target: host_target(runtime.target()),
        install_mode: wire(mode),
        state: wire(runtime.state().as_code()),
        artifact_sha256,
        installed_at: runtime.installed_at().map(Timestamp::from_jiff),
        last_verified_at: runtime.last_verified_at().map(Timestamp::from_jiff),
        support: support.0,
        support_reason: support.1,
        used_by: users,
        dependent_runtimes: dependents,
    }
}

/// The detail of a runtime: its summary, folder, and what the registry
/// kept of the artifact it came from.
#[must_use]
pub fn runtime_detail(
    summary: RuntimeSummary,
    path: String,
    artifact: Option<&ArtifactRecord>,
) -> RuntimeDetail {
    let info = artifact.map(artifact_info);
    let executables = info
        .as_ref()
        .and_then(|info| info.signature.as_ref())
        .map(|signature| {
            signature
                .executables
                .iter()
                .map(|executable| executable.path.clone())
                .collect()
        })
        .unwrap_or_default();
    let compatibility = artifact
        .map(|artifact| {
            artifact
                .compatibility
                .iter()
                .map(|row| compatibility_view(row, artifact))
                .collect()
        })
        .unwrap_or_default();
    RuntimeDetail {
        summary,
        path,
        artifact: info,
        executables,
        compatibility,
    }
}

/// What the registry knows about an artifact. A signature or dependency
/// text that does not parse reads as absent: the registry wrote it, so the
/// text is the install's own, and a newer or older shape is not an error.
fn artifact_info(artifact: &ArtifactRecord) -> ArtifactInfo {
    let signature = artifact
        .signature_info
        .as_ref()
        .and_then(|json| serde_json::from_str::<SignatureJson>(json.as_str()).ok())
        .map(|json| SignatureInfo {
            policy: wire(&json.policy),
            adhoc_signed_at_install: json.adhoc_signed_at_install,
            mach_o_files: Count::saturating(json.mach_o_files),
            executables: json
                .executables
                .into_iter()
                .map(|executable| ExecutableSignature {
                    path: executable.path,
                    kind: wire(&executable.kind),
                    team_id: executable.team_id,
                })
                .collect(),
        });
    let manifest = artifact
        .dependency_manifest
        .as_ref()
        .and_then(|json| serde_json::from_str::<DependencyJson>(json.as_str()).ok())
        .unwrap_or_default();
    ArtifactInfo {
        catalog_id: artifact.catalog_id.clone(),
        sha256: artifact.sha256.to_string(),
        size: artifact.size.map(Count::saturating),
        unpacked_size: None,
        format: artifact.format.map(|format| wire(format.as_code())),
        minimum_os: artifact.minimum_os.clone(),
        build: artifact
            .build
            .map(|build| Count::saturating(u64::from(build))),
        source: artifact.source.clone(),
        signature,
        dependencies: manifest
            .dependencies
            .into_iter()
            .map(|dependency| DependencyInfo {
                name: dependency.name,
                kind: wire::<DependencyKind>(&dependency.kind),
                install_name: dependency.install_name,
                version: dependency.version,
                license: dependency.license,
                source_url: dependency.source_url,
            })
            .collect(),
        warnings: manifest.warnings,
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SignatureJson {
    policy: String,
    #[serde(default)]
    adhoc_signed_at_install: bool,
    #[serde(default)]
    mach_o_files: u64,
    #[serde(default)]
    executables: Vec<ExecutableJson>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExecutableJson {
    path: String,
    kind: String,
    #[serde(default)]
    team_id: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct DependencyJson {
    #[serde(default)]
    dependencies: Vec<DependencyEntryJson>,
    #[serde(default)]
    warnings: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DependencyEntryJson {
    name: String,
    kind: String,
    install_name: String,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    license: Option<String>,
    #[serde(default)]
    source_url: Option<String>,
}

fn compatibility_view(
    row: &CompatibilityRecordRow,
    artifact: &ArtifactRecord,
) -> CompatibilityRecordView {
    CompatibilityRecordView {
        scope: wire(row.scope.as_code()),
        result: wire(row.result.as_code()),
        provider_revision: row
            .provider_revision
            .as_ref()
            .map(|key| ProviderRevisionRef {
                id: key.provider().to_string(),
                revision: key.revision().to_string(),
                manifest_sha256: key.manifest_sha256().to_string(),
            }),
        os: wire(row.os.as_code()),
        os_version: row.os_version.to_string(),
        os_build: row.os_build.clone(),
        arch: wire(row.arch.as_code()),
        checks: row.checks.clone(),
        known_limitations: row.known_limitations.clone(),
        summary: row.summary.clone(),
        tested_at: Timestamp::from_jiff(row.tested_at),
        evidence: row.evidence.clone(),
        source_url: artifact.source.clone(),
        sha256: artifact.sha256.to_string(),
    }
}

/// The support of an installed runtime whose artifact is no longer in the
/// catalog: a passing record of the registry's copy for this host decides.
#[must_use]
pub fn registry_support(
    rows: &[CompatibilityRecordRow],
    host: &HostFacts,
) -> (stackly_protocol::catalog::SupportStatus, SupportReason) {
    let tested = rows.iter().any(|row| {
        row.scope == CompatibilityScope::Runtime
            && row.result == CompatibilityResult::Pass
            && row.os == host.target.os()
            && row.arch == host.target.arch()
            && row.os_version.major() == host.os_version.major()
    });
    if tested {
        support(
            SupportStatus::Experimental,
            stackly_core::catalog::SupportReason::RuntimeTested,
        )
    } else {
        support(
            SupportStatus::Unverified,
            stackly_core::catalog::SupportReason::Untested,
        )
    }
}

/// The install of an unfinished `install_runtime` operation.
#[must_use]
pub fn installing_runtime(
    operation: &Operation,
    runtime_id: RuntimeId,
    input: &InstallInput,
    display_name: String,
) -> Option<InstallingRuntime> {
    let target: Target = input.artifact.target.parse().ok()?;
    Some(InstallingRuntime {
        operation_id: operation.id().as_uuid(),
        runtime_id: runtime_id.as_uuid(),
        provider: input.provider.clone(),
        software: input.software.clone(),
        display_name,
        version: input.artifact.version.clone(),
        target: host_target(target),
        sha256: input.artifact.sha256.clone(),
    })
}

/// `body` with the static hint of its `runtime.*` code, unless it carries
/// one.
#[must_use]
pub fn with_runtime_hint(body: ErrorBody) -> ErrorBody {
    if body.hint().is_some() {
        return body;
    }
    match codes::hint(body.code(), body.details()) {
        Some(hint) => body.with_hint(hint),
        None => body,
    }
}

/// The error of a selector that did not resolve to an installable
/// artifact. `selector` is the text as sent, which `catalog.not_installable`
/// names as the version when the gate gave no version of its own.
#[must_use]
pub fn resolve_error(error: &ResolveError, provider: &str, selector: &str) -> ErrorBody {
    match error {
        ResolveError::Unresolved {
            reason,
            known_lines,
        } => with_runtime_hint(
            ErrorBody::new(
                &codes::SELECTOR_UNRESOLVED,
                format!("{provider} has no version matching {selector}"),
            )
            .with_detail("provider", provider)
            .with_detail("selector", selector)
            .with_detail("reason", unresolved_code(*reason))
            .with_detail(
                "knownLines",
                known_lines
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(","),
            ),
        ),
        ResolveError::Ambiguous { versions } => with_runtime_hint(
            ErrorBody::new(
                &codes::SELECTOR_AMBIGUOUS,
                format!("{selector} matches more than one version of {provider}"),
            )
            .with_detail("selector", selector)
            .with_detail(
                "versions",
                versions
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(","),
            ),
        ),
        ResolveError::NotInstallable {
            reason,
            target,
            available_targets,
        } => not_installable_text(provider, selector, *target, *reason, available_targets),
        // A reason newer than this build of the daemon.
        _ => ErrorBody::new(
            &stackly_protocol::codes::internal::UNEXPECTED,
            "the selector could not be resolved",
        ),
    }
}

fn unresolved_code(reason: UnresolvedReason) -> &'static str {
    reason.as_code()
}

/// `catalog.not_installable` for an entry the gate does not judge: a
/// planned provider or a package that failed validation. The version is
/// the selector text and no target has a build.
#[must_use]
pub fn entry_not_installable(
    provider: &str,
    selector: &str,
    host: Target,
    reason: NotInstallableReason,
) -> ErrorBody {
    not_installable_text(provider, selector, host, reason, &[])
}

/// `runtime.selector_changed`.
#[must_use]
pub fn selector_changed(expected: &str, resolved: &str) -> ErrorBody {
    with_runtime_hint(
        ErrorBody::new(
            &codes::SELECTOR_CHANGED,
            "the selector now resolves to another artifact",
        )
        .with_detail("expectedSha256", expected)
        .with_detail("resolvedSha256", resolved),
    )
}

/// The wire target of `target`.
#[must_use]
pub fn target(target: Target) -> HostTarget {
    host_target(target)
}

/// The wire artifact format of a catalog format code.
#[must_use]
pub fn format(code: &str) -> ArtifactFormat {
    wire(code)
}

#[cfg(test)]
mod tests {
    use stackly_domain::{ExactVersion, RuntimeLine};

    use super::*;

    fn detail(body: &ErrorBody, key: &str) -> Option<String> {
        body.details()
            .iter()
            .find(|detail| detail.key() == key)
            .map(|detail| detail.value().to_owned())
    }

    // covers: AC-1
    #[test]
    fn every_runtime_code_core_fails_with_is_in_the_catalog_with_a_hint() {
        let catalogued: Vec<&str> = codes::ALL.iter().map(|spec| spec.code).collect();
        for code in stackly_core::runtime::codes::ALL {
            assert!(
                catalogued.contains(&code.as_str()),
                "{code} is not in the runtime area of the catalog"
            );
            assert!(
                codes::hint(code.as_str(), &[]).is_some(),
                "{code} has no hint"
            );
        }
        assert_eq!(
            catalogued.len(),
            stackly_core::runtime::codes::ALL.len(),
            "the catalog lists a runtime code core never raises"
        );
    }

    #[test]
    fn an_unresolved_selector_names_its_reason_and_the_known_lines_with_a_hint() {
        let error = ResolveError::Unresolved {
            reason: UnresolvedReason::NoStableArtifact,
            known_lines: vec![
                "17".parse::<RuntimeLine>().unwrap(),
                "18".parse::<RuntimeLine>().unwrap(),
            ],
        };

        let body = resolve_error(&error, "official/postgresql", "stable");

        assert_eq!(body.code(), "runtime.selector_unresolved");
        assert_eq!(
            detail(&body, "reason").as_deref(),
            Some("no_stable_artifact")
        );
        assert_eq!(detail(&body, "knownLines").as_deref(), Some("17,18"));
        assert!(body.hint().is_some());
    }

    #[test]
    fn an_ambiguous_selector_lists_the_versions_it_could_mean() {
        let error = ResolveError::Ambiguous {
            versions: vec![
                "18.1".parse::<ExactVersion>().unwrap(),
                "18.6".parse::<ExactVersion>().unwrap(),
            ],
        };

        let body = resolve_error(&error, "official/postgresql", "18");

        assert_eq!(body.code(), "runtime.selector_ambiguous");
        assert_eq!(detail(&body, "versions").as_deref(), Some("18.1,18.6"));
    }

    #[test]
    fn a_hint_is_added_only_to_a_runtime_code_without_one() {
        let hinted = with_runtime_hint(ErrorBody::new(&codes::QUARANTINED, "quarantined"));
        assert_eq!(
            hinted.hint(),
            Some(
                "Stackly never removes quarantine. Remove this runtime and install it again from Stackly."
            )
        );
        let own = with_runtime_hint(
            ErrorBody::new(&codes::QUARANTINED, "quarantined").with_hint("my own"),
        );
        assert_eq!(own.hint(), Some("my own"));
        let other = with_runtime_hint(ErrorBody::new(
            &stackly_protocol::codes::internal::UNEXPECTED,
            "bug",
        ));
        assert_eq!(other.hint(), None);
    }

    #[test]
    fn a_registry_without_a_record_for_this_host_is_unverified() {
        let host = HostFacts::new(
            "macos-aarch64".parse().unwrap(),
            stackly_domain::OsVersion::major_minor(27, 0),
            None,
        );

        let (status, reason) = registry_support(&[], &host);

        assert_eq!(status, stackly_protocol::catalog::SupportStatus::Unverified);
        assert_eq!(reason, SupportReason::Untested);
    }
}
