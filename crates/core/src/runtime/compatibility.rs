//! How supported an artifact is on this Mac (spec 0018, *Compatibility
//! record*; PRD BR-025): `supported` only from a passing record of the same
//! provider revision, macOS major, and architecture, and `experimental`
//! from a passing runtime record for this host.

use stackly_domain::{
    CompatibilityRecordRow, CompatibilityResult, CompatibilityScope, ProviderRevisionKey,
    SupportStatus,
};
use stackly_platform::HostFacts;

use crate::catalog::SupportReason;
use crate::provider::{ArtifactStatus, VersionArtifact};

/// A status with the reason for it and the record that decided it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupportVerdict<'a> {
    /// The status.
    pub status: SupportStatus,
    /// Why.
    pub reason: SupportReason,
    /// The record that decided the status, for a rule that needed one.
    pub record: Option<&'a CompatibilityRecordRow>,
}

/// Decides the support status of `artifact` on `host`.
///
/// `records` are the compatibility records of the artifact. The catalog
/// does not decode them yet, so spec 0017 passes none; an installed runtime
/// passes the rows its registry copy holds. `revision` is the provider
/// revision whose package holds the artifact.
///
/// The rules apply in order, and the first that fits decides:
///
/// 1. a `blocked` artifact is `blocked` (`artifact_blocked`);
/// 2. a `fixture` artifact is `unverified` (`fixture_only`);
/// 3. another target is `unsupported` (`no_build_for_arch` for the same OS,
///    else `no_build_for_host`);
/// 4. a `minOs` above the host's is `unsupported` (`requires_newer_os`);
/// 5. a passing `provider` record for this revision, macOS major, and
///    architecture is `supported` (`provider_tested`);
/// 6. a passing `runtime` record for this macOS major and architecture is
///    `experimental` (`runtime_tested`);
/// 7. anything else is `unverified` (`untested`).
#[must_use]
pub fn support_status<'a>(
    artifact: &VersionArtifact,
    records: &'a [CompatibilityRecordRow],
    revision: Option<&ProviderRevisionKey>,
    host: &HostFacts,
) -> SupportVerdict<'a> {
    let plain = |status, reason| SupportVerdict {
        status,
        reason,
        record: None,
    };
    let min_os = match &artifact.status {
        ArtifactStatus::Blocked { .. } => {
            return plain(SupportStatus::Blocked, SupportReason::ArtifactBlocked);
        }
        ArtifactStatus::Fixture { .. } => {
            return plain(SupportStatus::Unverified, SupportReason::FixtureOnly);
        }
        ArtifactStatus::Available { min_os, .. } => min_os,
    };
    if artifact.target != host.target {
        let reason = if artifact.target.os() == host.target.os() {
            SupportReason::NoBuildForArch
        } else {
            SupportReason::NoBuildForHost
        };
        return plain(SupportStatus::Unsupported, reason);
    }
    if *min_os > host.os_version {
        return plain(SupportStatus::Unsupported, SupportReason::RequiresNewerOs);
    }
    let on_host = |record: &&CompatibilityRecordRow| {
        record.result == CompatibilityResult::Pass
            && record.os == host.target.os()
            && record.arch == host.target.arch()
            && record.os_version.major() == host.os_version.major()
    };
    let provider = records.iter().filter(on_host).find(|record| {
        record.scope == CompatibilityScope::Provider
            && record.provider_revision.as_ref() == revision
    });
    if let Some(record) = provider.filter(|_| revision.is_some()) {
        return SupportVerdict {
            status: SupportStatus::Supported,
            reason: SupportReason::ProviderTested,
            record: Some(record),
        };
    }
    let runtime = records
        .iter()
        .filter(on_host)
        .find(|record| record.scope == CompatibilityScope::Runtime);
    if let Some(record) = runtime {
        return SupportVerdict {
            status: SupportStatus::Experimental,
            reason: SupportReason::RuntimeTested,
            record: Some(record),
        };
    }
    plain(SupportStatus::Unverified, SupportReason::Untested)
}
