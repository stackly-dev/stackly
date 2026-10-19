//! Providers: immutable provider revisions, the key that names one, their
//! line compatibility, and accepted revision changes (BR-009, BR-017).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::capability::CapabilitySet;
use crate::error::{ValueError, ValueKind};
use crate::value::{ProviderName, RevisionLabel, RuntimeLine, Sha256Digest, SoftwareName};

/// Names one immutable provider revision: the provider, its revision label,
/// and the SHA-256 digest of its manifest.
///
/// An instance stores this key, never a selector, so its provider revision
/// changes only through an accepted revision change (BR-001, BR-017). In
/// JSON the provider is the `name` field:
/// `{ "name": "official/postgresql", "revision": "…", "manifestSha256": "…" }`.
///
/// Its text form, the `id` of a provider `ResourceRef` on the wire (spec
/// 0009), is `<namespace>/<name>@<revision>#<manifest_sha256>`. It is never
/// ambiguous, because neither a provider name nor a revision label holds
/// `@` or `#`, and it reads back to the same key:
///
/// ```
/// use stackly_domain::ProviderRevisionKey;
///
/// let text = format!("official/nats@2.12.1-1#{}", "ab".repeat(32));
/// let key: ProviderRevisionKey = text.parse()?;
/// assert_eq!(key.provider().as_str(), "official/nats");
/// assert_eq!(key.revision().as_str(), "2.12.1-1");
/// assert_eq!(key.to_string(), text);
/// # Ok::<(), stackly_domain::ValueError>(())
/// ```
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

impl fmt::Display for ProviderRevisionKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}@{}#{}",
            self.provider, self.revision, self.manifest_sha256
        )
    }
}

impl FromStr for ProviderRevisionKey {
    type Err = ValueError;

    /// Reads `<namespace>/<name>@<revision>#<manifest_sha256>`. A part that
    /// breaks its own rules gives that part's error.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let invalid = || {
            ValueError::new(
                ValueKind::ProviderRevisionKey,
                "must be <namespace>/<name>@<revision>#<manifest sha256>",
            )
        };
        let (provider, rest) = s.split_once('@').ok_or_else(invalid)?;
        let (revision, digest) = rest.split_once('#').ok_or_else(invalid)?;
        Ok(Self::new(
            provider.parse()?,
            revision.parse()?,
            digest.parse()?,
        ))
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
    /// Creates the compatibility from a map of each data line to the other
    /// runtime lines that may run its data, such as
    /// `compatibility.lineCompatibility` in a provider manifest.
    ///
    /// ```
    /// use std::collections::{BTreeMap, BTreeSet};
    ///
    /// use stackly_domain::{LineCompatibility, RuntimeLine};
    ///
    /// let line = |text: &str| text.parse::<RuntimeLine>().expect("valid line");
    /// let map = BTreeMap::from([(line("0.1"), BTreeSet::from([line("0.2")]))]);
    /// let compat = LineCompatibility::from_map(map);
    /// assert!(compat.allows(&line("0.1"), &line("0.2")));
    /// assert!(!compat.allows(&line("0.2"), &line("0.1")));
    /// ```
    #[must_use]
    pub const fn from_map(lines: BTreeMap<RuntimeLine, BTreeSet<RuntimeLine>>) -> Self {
        Self(lines)
    }

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

    /// Returns each data line with the other runtime lines allowed for its
    /// data, both in line order. The registry stores the map this way.
    pub fn iter(&self) -> impl Iterator<Item = (&RuntimeLine, &BTreeSet<RuntimeLine>)> {
        self.0.iter()
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

    /// Rebuilds a provider revision the registry stored. It never changes
    /// once recorded, so this is [`ProviderRevision::new`]; it exists so the
    /// registry restores every aggregate the same way.
    #[must_use]
    pub const fn restore(
        key: ProviderRevisionKey,
        software: SoftwareName,
        capabilities: CapabilitySet,
        line_compatibility: LineCompatibility,
    ) -> Self {
        Self::new(key, software, capabilities, line_compatibility)
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

    fn line(text: &str) -> RuntimeLine {
        text.parse().unwrap()
    }

    #[test]
    fn line_compatibility_from_a_map_equals_the_same_allows_chained() {
        let map = BTreeMap::from([
            (line("17"), BTreeSet::from([line("18"), line("19")])),
            (line("8.0"), BTreeSet::from([line("8.4")])),
        ]);

        let compat = LineCompatibility::from_map(map);

        assert_eq!(
            compat,
            LineCompatibility::default()
                .allow(line("17"), line("18"))
                .allow(line("17"), line("19"))
                .allow(line("8.0"), line("8.4"))
        );
        assert!(compat.allows(&line("17"), &line("19")));
        assert!(!compat.allows(&line("18"), &line("17")));
        assert!(LineCompatibility::from_map(BTreeMap::new()).allows(&line("17"), &line("17")));
    }

    #[test]
    fn iterating_line_compatibility_rebuilds_the_same_map() {
        let compat = LineCompatibility::default()
            .allow(line("8.0"), line("8.4"))
            .allow(line("17"), line("19"))
            .allow(line("17"), line("18"));

        let map: BTreeMap<_, _> = compat
            .iter()
            .map(|(data, runtimes)| (data.clone(), runtimes.clone()))
            .collect();

        assert_eq!(
            compat
                .iter()
                .map(|(data, _)| data.as_str())
                .collect::<Vec<_>>(),
            ["17", "8.0"]
        );
        assert_eq!(LineCompatibility::from_map(map), compat);
    }

    #[test]
    fn revision_key_text_reads_back_to_the_same_key() {
        let key = ProviderRevisionKey::new(
            "official/nats".parse().unwrap(),
            "2.12.1-1".parse().unwrap(),
            DIGEST.parse().unwrap(),
        );
        let text = key.to_string();
        assert_eq!(text, format!("official/nats@2.12.1-1#{DIGEST}"));
        assert_eq!(text.parse::<ProviderRevisionKey>().unwrap(), key);
        let upper = format!("official/nats@2.12.1-1#{}", DIGEST.to_uppercase());
        assert_eq!(upper.parse::<ProviderRevisionKey>().unwrap(), key);
    }

    #[test]
    fn revision_key_text_without_its_separators_or_with_a_bad_part_is_refused() {
        let kind = |text: &str| text.parse::<ProviderRevisionKey>().unwrap_err().kind();
        assert_eq!(kind("official/nats"), ValueKind::ProviderRevisionKey);
        assert_eq!(
            kind("official/nats@2.12.1-1"),
            ValueKind::ProviderRevisionKey
        );
        assert_eq!(
            kind(&format!("official/nats#{DIGEST}")),
            ValueKind::ProviderRevisionKey
        );
        assert_eq!(
            kind(&format!("nats@2.12.1-1#{DIGEST}")),
            ValueKind::ProviderName
        );
        assert_eq!(
            kind(&format!("official/nats@..#{DIGEST}")),
            ValueKind::RevisionLabel
        );
        assert_eq!(kind("official/nats@2.12.1-1#abc"), ValueKind::Sha256Digest);
        assert_eq!(
            kind(&format!("official/nats@2.12.1-1#{DIGEST}#{DIGEST}")),
            ValueKind::Sha256Digest
        );
    }

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
