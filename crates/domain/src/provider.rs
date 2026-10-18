//! Providers: the revision key that names one immutable provider revision.

use serde::{Deserialize, Serialize};

use crate::value::{ProviderName, RevisionLabel, Sha256Digest};

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
