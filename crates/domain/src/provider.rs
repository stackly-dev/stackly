//! Providers: immutable provider revisions, the key that names one, their
//! line compatibility, and accepted revision changes (BR-009, BR-017).

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::capability::CapabilitySet;
use crate::value::{ProviderName, RevisionLabel, RuntimeLine, Sha256Digest, SoftwareName};

/// Names one immutable provider revision: the provider, its revision label,
/// and the SHA-256 digest of its manifest.
///
/// An instance stores this key, never a selector, so its provider revision
/// changes only through an accepted revision change (BR-001, BR-017). In
/// JSON the provider is the `name` field:
/// `{ "name": "official/postgresql", "revision": "…", "manifestSha256": "…" }`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderRevisionKey {
    #[serde(rename = "name")]
    provider: ProviderName,
    revision: RevisionLabel,
    manifest_sha256: Sha256Digest,
}

impl ProviderRevisionKey {
    /// Creates the key of one provider revision.
    #[must_use]
    pub const fn new(
        provider: ProviderName,
        revision: RevisionLabel,
        manifest_sha256: Sha256Digest,
    ) -> Self {
        Self {
            provider,
            revision,
            manifest_sha256,
        }
    }

    /// Returns the provider, such as `official/postgresql`.
    #[must_use]
    pub const fn provider(&self) -> &ProviderName {
        &self.provider
    }

    /// Returns the revision label.
    #[must_use]
    pub const fn revision(&self) -> &RevisionLabel {
        &self.revision
    }

    /// Returns the SHA-256 digest of the provider manifest.
    #[must_use]
    pub const fn manifest_sha256(&self) -> &Sha256Digest {
        &self.manifest_sha256
    }
}

/// Which runtime lines may run data that another line initialized, from
/// the provider manifest (feature 9).
///
/// The same line is always allowed. An empty map allows only the same line
/// (BR-009).
///
/// ```
/// use stackly_domain::{LineCompatibility, RuntimeLine};
///
/// let line = |text: &str| text.parse::<RuntimeLine>().expect("valid line");
/// let compat = LineCompatibility::default().allow(line("8.0"), line("8.4"));
/// assert!(compat.allows(&line("8.0"), &line("8.4")));
/// assert!(!compat.allows(&line("8.4"), &line("8.0")));
/// assert!(LineCompatibility::default().allows(&line("17"), &line("17")));
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct LineCompatibility(BTreeMap<RuntimeLine, BTreeSet<RuntimeLine>>);

impl LineCompatibility {
    /// Returns the map with `runtime_line` also allowed for data that
    /// `data_line` initialized.
    #[must_use]
    pub fn allow(mut self, data_line: RuntimeLine, runtime_line: RuntimeLine) -> Self {
        self.0.entry(data_line).or_default().insert(runtime_line);
        self
    }

    /// Returns whether a runtime of `runtime_line` may run data that
    /// `data_line` initialized.
    #[must_use]
    pub fn allows(&self, data_line: &RuntimeLine, runtime_line: &RuntimeLine) -> bool {
        data_line == runtime_line
            || self
                .0
                .get(data_line)
                .is_some_and(|lines| lines.contains(runtime_line))
    }
}

/// One provider revision. It never changes once it exists (BR-017).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderRevision {
    key: ProviderRevisionKey,
    software: SoftwareName,
    capabilities: CapabilitySet,
    line_compatibility: LineCompatibility,
}

impl ProviderRevision {
    /// Creates a provider revision from its manifest (feature 9).
    #[must_use]
    pub const fn new(
        key: ProviderRevisionKey,
        software: SoftwareName,
        capabilities: CapabilitySet,
        line_compatibility: LineCompatibility,
    ) -> Self {
        Self {
            key,
            software,
            capabilities,
            line_compatibility,
        }
    }

    /// Returns the key that names this revision.
    #[must_use]
    pub const fn key(&self) -> &ProviderRevisionKey {
        &self.key
    }

    /// Returns the software it provides.
    #[must_use]
    pub const fn software(&self) -> &SoftwareName {
        &self.software
    }

    /// Returns what it supports.
    #[must_use]
    pub const fn capabilities(&self) -> &CapabilitySet {
        &self.capabilities
    }

    /// Returns which runtime lines may run data of another line.
    #[must_use]
    pub const fn line_compatibility(&self) -> &LineCompatibility {
        &self.line_compatibility
    }
}

/// A provider revision change the user accepted, with the digest of the
/// config diff they saw and approved (computed by feature 59).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AcceptedRevisionChange {
    from: ProviderRevisionKey,
    to: ProviderRevisionKey,
    accepted_diff_sha256: Sha256Digest,
}

impl AcceptedRevisionChange {
    /// Records that the user accepted moving from `from` to `to` after
    /// seeing the diff with digest `accepted_diff_sha256`.
    #[must_use]
    pub const fn new(
        from: ProviderRevisionKey,
        to: ProviderRevisionKey,
        accepted_diff_sha256: Sha256Digest,
    ) -> Self {
        Self {
            from,
            to,
            accepted_diff_sha256,
        }
    }

    /// Returns the revision the change starts from.
    #[must_use]
    pub const fn from(&self) -> &ProviderRevisionKey {
        &self.from
    }

    /// Returns the revision the change moves to.
    #[must_use]
    pub const fn to(&self) -> &ProviderRevisionKey {
        &self.to
    }

    /// Returns the digest of the accepted config diff.
    #[must_use]
    pub const fn accepted_diff_sha256(&self) -> &Sha256Digest {
        &self.accepted_diff_sha256
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIGEST: &str = "cb95f16d56a9ab7b938a03732ddf4ab691346124747fbb811ac6c09928f1fa6b";

    #[test]
    fn revision_key_serializes_the_provider_as_name() {
        let key = ProviderRevisionKey::new(
            "official/postgresql".parse().unwrap(),
            "2026.10.1".parse().unwrap(),
            DIGEST.parse().unwrap(),
        );
        let json = serde_json::to_value(&key).unwrap();

        assert_eq!(
            json,
            serde_json::json!({
                "name": "official/postgresql",
                "revision": "2026.10.1",
                "manifestSha256": DIGEST,
            })
        );
        assert_eq!(
            serde_json::from_value::<ProviderRevisionKey>(json).unwrap(),
            key
        );
    }
}
