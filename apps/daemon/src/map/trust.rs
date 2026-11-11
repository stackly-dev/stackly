//! Maps the local provider model of `stackly-core` to the `provider` DTOs
//! (spec 0037, *API surface*, *Value sourcing*): permissions, the import
//! facts and last decision of a local entry, revocations, and the errors
//! of the import and trust codes.
//!
//! An enum travels by its stable code, which spec 0037 makes the same as
//! its wire value; the per variant tests below rule out an `unrecognized`.

use stackly_core::catalog::local::{
    ExecutableUse, PermissionChange, Permissions, permission_changes,
};
use stackly_core::catalog::{RevokedInfo, TrustVerdict};
use stackly_domain::{ApprovalState, ProviderImportRecord, TrustDecision};
use stackly_protocol::codes::provider as codes;
use stackly_protocol::provider as dto;
use stackly_protocol::{Count, ErrorBody, Revision, Timestamp};

use super::catalog::wire;

/// The wire approval state of a domain one.
#[must_use]
pub fn approval(state: ApprovalState) -> dto::ApprovalState {
    wire(state.as_code())
}

/// The wire form of a decision kind, as an approval state.
fn decision(decision: TrustDecision) -> dto::ApprovalState {
    wire(decision.as_code())
}

/// The wire form of where a package runs a program.
fn executable_use(use_: ExecutableUse) -> dto::ExecutableUse {
    wire(use_.as_code())
}

/// The wire form of one permission change.
#[must_use]
pub fn change(change: PermissionChange) -> dto::PermissionChange {
    wire(change.as_code())
}

/// The permission summary of a package.
#[must_use]
pub fn permissions(permissions: &Permissions) -> dto::ProviderPermissions {
    dto::ProviderPermissions {
        capabilities: permissions
            .capabilities
            .iter()
            .map(|capability| wire(capability.as_code()))
            .collect(),
        managed_filesystem: permissions
            .managed_filesystem
            .iter()
            .map(|dir| dir.as_code().to_owned())
            .collect(),
        executables: permissions
            .executables
            .iter()
            .map(|executable| dto::PermissionExecutable {
                name: executable.name.clone(),
                path: executable.path.clone(),
                uses: executable
                    .uses
                    .iter()
                    .copied()
                    .map(executable_use)
                    .collect(),
            })
            .collect(),
        secrets: permissions.secrets.clone(),
        endpoints: permissions
            .endpoints
            .iter()
            .map(|endpoint| dto::PermissionEndpoint {
                name: endpoint.name.clone(),
                transport: endpoint.transport.clone(),
                purpose: endpoint.purpose.clone(),
                default_port: endpoint.default_port,
            })
            .collect(),
        downloads: permissions
            .downloads
            .iter()
            .map(|download| dto::PermissionDownload {
                artifact: download.artifact.clone(),
                version: download.version.clone(),
                os: wire(download.os.as_code()),
                arch: wire(download.arch.as_code()),
                sha256: download.sha256.to_string(),
                size: Count::saturating(download.size),
                hosts: download.hosts.clone(),
                signing_policy: download.signing_policy.clone(),
            })
            .collect(),
        open_with: permissions
            .open_with
            .iter()
            .map(|entry| dto::PermissionOpenWith {
                name: entry.name.clone(),
                kind: entry.kind.clone(),
                target: entry.target.clone(),
            })
            .collect(),
    }
}

/// What differs from the last decision of `record`, given the permissions
/// of its stored bytes: empty without a decision or while the stored bytes
/// are what was decided, and also when the decided permissions do not read.
#[must_use]
pub fn changes_since_decision(
    record: &ProviderImportRecord,
    current: &Permissions,
) -> Vec<dto::PermissionChange> {
    let Some(decided) = record.decision.as_ref() else {
        return Vec::new();
    };
    if record.approval() != ApprovalState::Pending {
        return Vec::new();
    }
    let Ok(decided_permissions) = Permissions::from_json(&decided.permissions_json) else {
        return Vec::new();
    };
    permission_changes(
        &decided_permissions,
        &decided.provider_revision,
        current,
        &record.provider_revision,
    )
    .into_iter()
    .map(change)
    .collect()
}

/// `ProviderDetail.local` of a local entry; `current` is the permissions
/// of its stored bytes, none for an invalid entry.
#[must_use]
pub fn local_info(
    record: &ProviderImportRecord,
    current: Option<&Permissions>,
) -> dto::LocalProviderInfo {
    dto::LocalProviderInfo {
        imported_at: Timestamp::from_jiff(record.imported_at),
        origin_path: record.origin_path.clone(),
        quarantined: record.quarantined,
        versions_sha256: record.versions_sha256.to_string(),
        package_sha256: record.package_sha256.to_string(),
        package_bytes: Count::saturating(record.package_bytes),
        decision: record
            .decision
            .as_ref()
            .map(|decided| dto::TrustDecisionInfo {
                decision: decision(decided.decision),
                decided_at: Timestamp::from_jiff(decided.at),
                provider_revision: decided.provider_revision.to_string(),
                package_sha256: decided.package_sha256.to_string(),
            }),
        permissions: current.map(permissions),
        changes: current
            .map(|current| changes_since_decision(record, current))
            .unwrap_or_default(),
    }
}

/// The wire form of a revocation.
#[must_use]
pub fn revocation(info: &RevokedInfo) -> dto::RevocationInfo {
    dto::RevocationInfo {
        by: wire(info.by.as_code()),
        since: Timestamp::from_jiff(info.since),
        reason: info.reason.clone(),
        catalog_revision: info.catalog_revision.map(Revision::saturating),
    }
}

/// The revocation of a verdict, when it is one.
#[must_use]
pub fn verdict_revocation(verdict: &TrustVerdict) -> Option<dto::RevocationInfo> {
    match verdict {
        TrustVerdict::Revoked(info) => Some(revocation(info)),
        _ => None,
    }
}

/// `provider.import_not_found`: `provider` is not an imported provider;
/// `reason` is `not_imported` or `not_local`.
#[must_use]
pub fn import_not_found(provider: &str, reason: &'static str) -> ErrorBody {
    with_hint(
        ErrorBody::new(
            &codes::IMPORT_NOT_FOUND,
            format!("no imported provider {provider}"),
        )
        .with_detail("provider", provider)
        .with_detail("reason", reason),
    )
}

/// `provider.import_changed`: the bytes the client showed are not the ones
/// stored or read now.
#[must_use]
pub fn import_changed(provider: &str, expected: &str, actual: &str) -> ErrorBody {
    with_hint(
        ErrorBody::new(
            &codes::IMPORT_CHANGED,
            "the provider package changed since it was previewed",
        )
        .with_detail("provider", provider)
        .with_detail("expected", expected)
        .with_detail("actual", actual),
    )
}

/// `provider.import_limit`.
#[must_use]
pub fn import_limit(limit: stackly_core::catalog::local::ImportLimit) -> ErrorBody {
    with_hint(
        ErrorBody::new(&codes::IMPORT_LIMIT, "too many imported providers")
            .with_detail("limit", limit.as_code())
            .with_detail("max", limit.max().to_string()),
    )
}

/// `provider.import_unreadable`; `path` has the home folder written `~`.
#[must_use]
pub fn import_unreadable(path: &str, reason: &'static str) -> ErrorBody {
    with_hint(
        ErrorBody::new(
            &codes::IMPORT_UNREADABLE,
            "the provider folder can't be read",
        )
        .with_detail("path", path)
        .with_detail("reason", reason),
    )
}

/// `provider.store_failed`.
#[must_use]
pub fn store_failed(provider: &str, reason: &'static str) -> ErrorBody {
    with_hint(
        ErrorBody::new(
            &codes::STORE_FAILED,
            "the provider package could not be stored",
        )
        .with_detail("provider", provider)
        .with_detail("reason", reason),
    )
}

/// `provider.approval_required` for the stored bytes of `record`.
#[must_use]
pub fn approval_required(record: &ProviderImportRecord) -> ErrorBody {
    with_hint(
        ErrorBody::new(
            &codes::APPROVAL_REQUIRED,
            format!("{} is not approved", record.provider),
        )
        .with_detail("provider", record.provider.as_str())
        .with_detail("providerRevision", record.provider_revision.as_str())
        .with_detail("packageSha256", record.package_sha256.to_string()),
    )
}

/// `provider.revoked` for `provider` at `provider_revision`.
#[must_use]
pub fn revoked(provider: &str, provider_revision: &str, info: &RevokedInfo) -> ErrorBody {
    let mut body = ErrorBody::new(
        &codes::REVOKED,
        format!("{provider} {provider_revision} is revoked"),
    )
    .with_detail("provider", provider)
    .with_detail("providerRevision", provider_revision)
    .with_detail("by", info.by.as_code())
    .with_detail("since", Timestamp::from_jiff(info.since).to_string());
    if let Some(revision) = info.catalog_revision {
        body = body.with_detail("catalogRevision", revision.to_string());
    }
    with_hint(body)
}

/// `body` with the static hint of its provider code.
fn with_hint(body: ErrorBody) -> ErrorBody {
    match codes::hint(body.code(), body.details()) {
        Some(hint) => body.with_hint(hint),
        None => body,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_core_code_has_a_known_wire_value() {
        for state in ApprovalState::ALL {
            assert_ne!(
                approval(*state),
                dto::ApprovalState::Unrecognized,
                "{state}"
            );
        }
        for kind in TrustDecision::ALL {
            assert_ne!(decision(*kind), dto::ApprovalState::Unrecognized, "{kind}");
        }
        for use_ in ExecutableUse::ALL {
            assert_ne!(executable_use(use_), dto::ExecutableUse::Unrecognized);
        }
        for value in PermissionChange::ALL {
            assert_ne!(change(value), dto::PermissionChange::Unrecognized);
        }
    }

    #[test]
    fn every_error_has_its_hint_and_a_lowercase_message_without_a_period() {
        let info = RevokedInfo::by_user(jiff::Timestamp::UNIX_EPOCH);
        let bodies = [
            import_not_found("acme/demo", "not_imported"),
            import_not_found("official/nats", "not_local"),
            import_changed("acme/demo", "a", "b"),
            import_limit(stackly_core::catalog::local::ImportLimit::Count),
            import_unreadable("~/x", "permission_denied"),
            import_unreadable("~/x", "not_found"),
            store_failed("acme/demo", "no_space"),
            revoked("acme/demo", "1.0.0", &info),
        ];
        for body in bodies {
            assert!(body.hint().is_some(), "{}", body.code());
            let message = body.message();
            assert!(!message.ends_with('.'), "{message}");
            assert_eq!(
                message.chars().next(),
                message.chars().next().map(|c| c.to_ascii_lowercase())
            );
        }
    }
}
