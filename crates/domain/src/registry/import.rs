//! Imported local provider packages and the trust decision on them (spec
//! 0037, *Model data* and *State approval*).

use jiff::Timestamp;

use crate::code::code_enum;
use crate::id::ProviderSourceId;
use crate::value::{ProviderName, ResourceRevision, RevisionLabel, Sha256Digest, SoftwareName};

code_enum! {
    /// A trust decision on provider bytes, an open code set in the registry
    /// (`provider_imports.decision` and `provider_revisions.approval`). A
    /// stored code this build does not know reads as `Corrupt`.
    pub enum TrustDecision: TrustDecision {
        /// The user approved the bytes.
        Approved => "approved",
        /// The approval was revoked.
        Revoked => "revoked",
    }
}

code_enum! {
    /// Where an imported package stands, computed by [`approval_state`] and
    /// never stored.
    pub enum ApprovalState: ApprovalState {
        /// No decision covers the stored bytes.
        Pending => "pending",
        /// The stored bytes are approved.
        Approved => "approved",
        /// The approval of the stored bytes was revoked.
        Revoked => "revoked",
    }
}

/// The last trust decision on an imported provider: the five `decided_*`
/// columns of `provider_imports`, all set or all empty.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ImportDecision {
    /// What was decided.
    pub decision: TrustDecision,
    /// The package digest of the bytes the decision covers.
    pub package_sha256: Sha256Digest,
    /// The provider revision label of those bytes.
    pub provider_revision: RevisionLabel,
    /// The permissions of those bytes, as `stackly.provider-permissions/v1`
    /// JSON object text.
    pub permissions_json: String,
    /// When it was decided.
    pub at: Timestamp,
}

impl ImportDecision {
    /// A decision on the bytes with `package_sha256`.
    #[must_use]
    pub const fn new(
        decision: TrustDecision,
        package_sha256: Sha256Digest,
        provider_revision: RevisionLabel,
        permissions_json: String,
        at: Timestamp,
    ) -> Self {
        Self {
            decision,
            package_sha256,
            provider_revision,
            permissions_json,
            at,
        }
    }
}

/// One imported provider: one `provider_imports` row (spec 0037).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ProviderImportRecord {
    /// The provider ID, such as `acme/demo`, the key of the row.
    pub provider: ProviderName,
    /// The `local` provider source row.
    pub source_id: ProviderSourceId,
    /// The stored package's `metadata.revision`.
    pub provider_revision: RevisionLabel,
    /// The stored package's software.
    pub software: SoftwareName,
    /// The stored package's manifest digest.
    pub manifest_sha256: Sha256Digest,
    /// The stored package's `versions.json` digest.
    pub versions_sha256: Sha256Digest,
    /// The digest of every file of the stored package, the name of its
    /// store folder; unique across rows.
    pub package_sha256: Sha256Digest,
    /// The package size in bytes, 1 to [`ProviderImportRecord::PACKAGE_BYTES_MAX`].
    pub package_bytes: u64,
    /// The folder path the client sent, for information only.
    pub origin_path: String,
    /// Whether the origin `provider.yaml` carried a quarantine attribute,
    /// empty when the attribute could not be read.
    pub quarantined: Option<bool>,
    /// The quarantine agent, when one was read.
    pub quarantine_agent: Option<String>,
    /// When the last import committed.
    pub imported_at: Timestamp,
    /// The last trust decision, if any. It may cover other bytes than the
    /// stored ones; [`approval_state`] says what it means for them.
    pub decision: Option<ImportDecision>,
    /// The row's revision, 1 for a new row.
    pub revision: ResourceRevision,
    /// When the row was written first.
    pub created_at: Timestamp,
    /// When the row last changed.
    pub updated_at: Timestamp,
}

impl ProviderImportRecord {
    /// The largest package, in bytes.
    pub const PACKAGE_BYTES_MAX: u64 = 4 * 1024 * 1024;
    /// The longest origin path, in bytes of UTF-8.
    pub const ORIGIN_PATH_MAX: usize = 1024;
    /// The longest quarantine agent name, in bytes of UTF-8.
    pub const QUARANTINE_AGENT_MAX: usize = 128;

    /// A new row at revision 1 without a decision, imported `at`.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        provider: ProviderName,
        source_id: ProviderSourceId,
        provider_revision: RevisionLabel,
        software: SoftwareName,
        manifest_sha256: Sha256Digest,
        versions_sha256: Sha256Digest,
        package_sha256: Sha256Digest,
        package_bytes: u64,
        origin_path: String,
        at: Timestamp,
    ) -> Self {
        Self {
            provider,
            source_id,
            provider_revision,
            software,
            manifest_sha256,
            versions_sha256,
            package_sha256,
            package_bytes,
            origin_path,
            quarantined: None,
            quarantine_agent: None,
            imported_at: at,
            decision: None,
            revision: ResourceRevision::INITIAL,
            created_at: at,
            updated_at: at,
        }
    }

    /// Records what the quarantine attribute of the origin showed.
    #[must_use]
    pub fn with_quarantine(mut self, quarantined: Option<bool>, agent: Option<String>) -> Self {
        self.quarantined = quarantined;
        self.quarantine_agent = agent;
        self
    }

    /// Carries a decision.
    #[must_use]
    pub fn with_decision(mut self, decision: ImportDecision) -> Self {
        self.decision = Some(decision);
        self
    }

    /// Returns where the stored bytes stand, as [`approval_state`] does.
    #[must_use]
    pub fn approval(&self) -> ApprovalState {
        approval_state(self)
    }
}

/// Where the stored bytes of an import stand (spec 0037, *State approval*):
/// `pending` without a decision or when the decision covers other bytes,
/// otherwise what was decided. A decision binds bytes, not the order of
/// imports, so importing the decided bytes again brings the decision back.
#[must_use]
pub fn approval_state(record: &ProviderImportRecord) -> ApprovalState {
    match &record.decision {
        Some(decision) if decision.package_sha256 == record.package_sha256 => {
            match decision.decision {
                TrustDecision::Approved => ApprovalState::Approved,
                TrustDecision::Revoked => ApprovalState::Revoked,
            }
        }
        _ => ApprovalState::Pending,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    fn at() -> Timestamp {
        "2026-11-11T00:00:00Z".parse().unwrap()
    }

    fn record(package: &str) -> ProviderImportRecord {
        ProviderImportRecord::new(
            "acme/demo".parse().unwrap(),
            "0199f000-0000-7000-8000-000000000010".parse().unwrap(),
            "1.0.0".parse().unwrap(),
            "demo".parse().unwrap(),
            A.parse().unwrap(),
            A.parse().unwrap(),
            package.parse().unwrap(),
            1024,
            "~/providers/demo".to_owned(),
            at(),
        )
    }

    fn decision(decision: TrustDecision, package: &str) -> ImportDecision {
        ImportDecision::new(
            decision,
            package.parse().unwrap(),
            "1.0.0".parse().unwrap(),
            "{}".to_owned(),
            at(),
        )
    }

    #[test]
    fn an_import_without_a_decision_is_pending() {
        assert_eq!(approval_state(&record(A)), ApprovalState::Pending);
    }

    #[test]
    fn a_decision_on_the_stored_bytes_is_the_state() {
        let approved = record(A).with_decision(decision(TrustDecision::Approved, A));
        let revoked = record(A).with_decision(decision(TrustDecision::Revoked, A));

        assert_eq!(approval_state(&approved), ApprovalState::Approved);
        assert_eq!(revoked.approval(), ApprovalState::Revoked);
    }

    #[test]
    fn a_decision_on_other_bytes_leaves_the_stored_bytes_pending() {
        for kind in TrustDecision::ALL {
            let record = record(B).with_decision(decision(*kind, A));
            assert_eq!(approval_state(&record), ApprovalState::Pending, "{kind}");
        }
    }

    #[test]
    fn trust_decision_and_approval_state_codes_round_trip() {
        for kind in TrustDecision::ALL {
            assert_eq!(kind.as_code().parse::<TrustDecision>().unwrap(), *kind);
        }
        for state in ApprovalState::ALL {
            assert_eq!(state.as_code().parse::<ApprovalState>().unwrap(), *state);
        }
        assert!("trusted".parse::<TrustDecision>().is_err());
        assert_eq!(
            ApprovalState::ALL
                .iter()
                .map(|state| state.as_code())
                .collect::<Vec<_>>(),
            ["pending", "approved", "revoked"]
        );
    }
}
