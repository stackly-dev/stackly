//! The handover gate of a planned daemon restart (spec 0030): which provider
//! lines have native proof that their process survives the daemon, and the
//! plan that follows from it.
//!
//! [`HANDOVER_PROVEN`] is a list in code. A manifest can never add to it, and
//! a provider enters it only with the ID of a native launchd evidence record
//! that passed (spec 0030 AC-8). [`plan_entries`] is the pure rule of AC-7:
//! it sorts the instances of a registry into `keep` and `stop_restore`, and
//! [`plan_digest`] hashes the result so a client can confirm the plan it
//! saw.

use sha2::{Digest, Sha256};
use stackly_domain::{
    Instance, InstanceId, InstanceLifecycle, InstanceName, PlanDecision, PlanReason,
    ProviderRevisionKey, Sha256Digest, plan_decision, plan_digest_text,
};

/// Native proof that the processes of one provider line survive a restart
/// of the daemon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct HandoverProof {
    /// The provider, such as `official/postgresql`.
    pub provider: &'static str,
    /// The revision line the proof covers: the major for revisions from
    /// `1.0.0`, the major and minor (`0.3`) below that, because a minor
    /// below 1.0 is not compatible by semver.
    pub line: &'static str,
    /// The ID of the evidence record that proved it.
    pub evidence: &'static str,
}

impl HandoverProof {
    /// A proof for `line` of `provider`, with its evidence ID.
    #[must_use]
    pub const fn new(provider: &'static str, line: &'static str, evidence: &'static str) -> Self {
        Self {
            provider,
            line,
            evidence,
        }
    }
}

/// The provider lines with a native proof of survival. The daemon always
/// plans with this list; only tests plan with another one, through
/// [`SupervisorConfig::handover_proven`](super::SupervisorConfig).
pub const HANDOVER_PROVEN: &[HandoverProof] = &[
    // Spec 0015 AC-16: a fake instance keeps its process through
    // `launchctl kickstart -k` and `bootout`.
    HandoverProof::new("testing/fake", "0.3", "spec-0015-ac-16"),
    HandoverProof::new("testing/fake", "0.4", "spec-0015-ac-16"),
];

/// Returns the line of a semver `revision`: its major, or major and minor
/// while the major is 0. `None` for a revision that is not
/// `major.minor.patch` with an optional `-pre` or `+build` suffix.
#[must_use]
pub fn revision_line(revision: &str) -> Option<String> {
    let core = revision.split(['-', '+']).next()?;
    let mut parts = core.split('.');
    let major = number(parts.next()?)?;
    let minor = number(parts.next()?)?;
    number(parts.next()?)?;
    if parts.next().is_some() {
        return None;
    }
    Some(if major == "0" {
        format!("{major}.{minor}")
    } else {
        major.to_owned()
    })
}

/// A decimal part of a version, without a sign or a leading zero.
fn number(part: &str) -> Option<&str> {
    let ok = !part.is_empty()
        && part.bytes().all(|byte| byte.is_ascii_digit())
        && (part == "0" || !part.starts_with('0'));
    ok.then_some(part)
}

/// Returns whether `proven` covers the provider revision `key`.
#[must_use]
pub fn is_handover_proven(proven: &[HandoverProof], key: &ProviderRevisionKey) -> bool {
    let Some(line) = revision_line(key.revision().as_str()) else {
        return false;
    };
    proven
        .iter()
        .any(|proof| proof.provider == key.provider().as_str() && proof.line == line)
}

/// One instance of a restart plan, before it is written or shown.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PlanEntry {
    /// The instance.
    pub instance_id: InstanceId,
    /// Its name now.
    pub name: InstanceName,
    /// The name of the provider it is pinned to, such as `postgresql`.
    pub provider: String,
    /// The lifecycle the decision was made from.
    pub lifecycle: InstanceLifecycle,
    /// What the restart does with it.
    pub decision: PlanDecision,
    /// Why.
    pub reason: PlanReason,
}

/// Sorts `instances` into the entries of a restart plan (spec 0030 AC-7),
/// ordered by instance ID. Instances outside the plan (`stopped`, `failed`,
/// `initializing`, `uninitialized`, `retained`) have no entry.
///
/// `supervisor_available` is false while the registry is in recovery mode,
/// when every instance is `keep`.
#[must_use]
pub fn plan_entries(
    instances: &[Instance],
    supervisor_available: bool,
    proven: &[HandoverProof],
) -> Vec<PlanEntry> {
    let mut entries: Vec<PlanEntry> = instances
        .iter()
        .filter_map(|instance| {
            let key = instance.provider_revision();
            let (decision, reason) = plan_decision(
                instance.lifecycle(),
                supervisor_available,
                is_handover_proven(proven, key),
            )?;
            Some(PlanEntry {
                instance_id: instance.id(),
                name: instance.name().clone(),
                provider: key.provider().name().to_owned(),
                lifecycle: instance.lifecycle(),
                decision,
                reason,
            })
        })
        .collect();
    entries.sort_by_key(|entry| entry.instance_id);
    entries
}

/// Returns the digest of a plan: the sha256 of its
/// `instanceId:decision:reason` lines, sorted by instance ID.
#[must_use]
pub fn plan_digest(entries: &[PlanEntry]) -> Sha256Digest {
    let lines: Vec<_> = entries
        .iter()
        .map(|entry| (entry.instance_id, entry.decision, entry.reason))
        .collect();
    let hash = Sha256::digest(plan_digest_text(&lines).as_bytes());
    Sha256Digest::from_bytes(hash.into())
}

#[cfg(test)]
mod tests {
    use stackly_testkit::{InstanceBuilder, RuntimeBuilder};

    use super::*;

    fn key(provider: &str, revision: &str) -> ProviderRevisionKey {
        ProviderRevisionKey::new(
            provider.parse().unwrap(),
            revision.parse().unwrap(),
            "ab".repeat(32).parse().unwrap(),
        )
    }

    #[test]
    fn a_major_from_one_is_the_line_and_a_zero_major_keeps_its_minor() {
        assert_eq!(revision_line("1.0.0").as_deref(), Some("1"));
        assert_eq!(revision_line("1.7.3").as_deref(), Some("1"));
        assert_eq!(revision_line("12.0.1-rc.1+build.5").as_deref(), Some("12"));
        assert_eq!(revision_line("0.3.0").as_deref(), Some("0.3"));
        assert_eq!(revision_line("0.4.1").as_deref(), Some("0.4"));
    }

    #[test]
    fn a_revision_that_is_not_semver_has_no_line() {
        for revision in [
            "1",
            "1.0",
            "1.0.0.0",
            "a.b.c",
            "01.0.0",
            "1.-1.0",
            "2026.10.1x",
            "",
        ] {
            assert_eq!(revision_line(revision), None, "{revision:?}");
        }
    }

    #[test]
    fn a_provider_is_proven_by_its_name_and_line_only() {
        let proven = [
            HandoverProof::new("official/postgresql", "1", "id-1"),
            HandoverProof::new("testing/fake", "0.3", "id-2"),
        ];
        let cases = [
            ("official/postgresql", "1.0.0", true),
            ("official/postgresql", "1.2.9", true),
            ("official/postgresql", "2.0.0", false),
            ("official/nats", "1.0.1", false),
            ("acme/postgresql", "1.0.0", false),
            ("testing/fake", "0.3.5", true),
            ("testing/fake", "0.4.0", false),
            ("testing/fake", "0.2.0", false),
            ("official/postgresql", "2026.10.1", false),
        ];
        for (provider, revision, expected) in cases {
            assert_eq!(
                is_handover_proven(&proven, &key(provider, revision)),
                expected,
                "{provider}@{revision}"
            );
        }
    }

    #[test]
    fn the_built_in_list_covers_the_fake_provider() {
        assert!(is_handover_proven(
            HANDOVER_PROVEN,
            &key("testing/fake", "0.4.0")
        ));
        assert!(!is_handover_proven(
            HANDOVER_PROVEN,
            &key("official/postgresql", "1.0.0")
        ));
    }

    fn instance(lifecycle: InstanceLifecycle, provider: &str, revision: &str) -> Instance {
        let key = key(provider, revision);
        InstanceBuilder::new(&RuntimeBuilder::new().build(), &key)
            .lifecycle(lifecycle)
            .build()
    }

    #[test]
    fn instances_sort_into_keep_and_stop_restore_by_lifecycle_and_proof() {
        use InstanceLifecycle as L;
        let proven = [HandoverProof::new("testing/fake", "0.3", "id")];
        let cases = [
            (
                L::Running,
                "testing/fake",
                "0.3.0",
                Some((PlanDecision::Keep, PlanReason::HandoverProven)),
            ),
            (
                L::Starting,
                "testing/fake",
                "0.3.0",
                Some((PlanDecision::Keep, PlanReason::HandoverProven)),
            ),
            (
                L::Stopping,
                "testing/fake",
                "0.3.0",
                Some((PlanDecision::Keep, PlanReason::HandoverProven)),
            ),
            (
                L::Running,
                "official/nats",
                "1.0.1",
                Some((PlanDecision::StopRestore, PlanReason::HandoverUnproven)),
            ),
            (
                L::Starting,
                "official/nats",
                "1.0.1",
                Some((PlanDecision::StopRestore, PlanReason::HandoverUnproven)),
            ),
            (
                L::Stopping,
                "official/nats",
                "1.0.1",
                Some((PlanDecision::StopRestore, PlanReason::HandoverUnproven)),
            ),
            (
                L::Recovering,
                "official/nats",
                "1.0.1",
                Some((PlanDecision::Keep, PlanReason::RespawnPending)),
            ),
            (
                L::Unknown,
                "official/nats",
                "1.0.1",
                Some((PlanDecision::Keep, PlanReason::IdentityUnproven)),
            ),
            (
                L::Unknown,
                "testing/fake",
                "0.3.0",
                Some((PlanDecision::Keep, PlanReason::IdentityUnproven)),
            ),
            (L::Stopped, "official/nats", "1.0.1", None),
            (L::Failed, "official/nats", "1.0.1", None),
            (L::Initializing, "official/nats", "1.0.1", None),
            (L::Uninitialized, "official/nats", "1.0.1", None),
            (L::Retained, "official/nats", "1.0.1", None),
        ];
        for (lifecycle, provider, revision, expected) in cases {
            let entries = plan_entries(&[instance(lifecycle, provider, revision)], true, &proven);
            let got = entries.first().map(|entry| (entry.decision, entry.reason));
            assert_eq!(got, expected, "{lifecycle:?} {provider}@{revision}");
        }
    }

    #[test]
    fn a_registry_in_recovery_mode_keeps_every_instance() {
        let instances = [
            instance(InstanceLifecycle::Running, "official/nats", "1.0.1"),
            instance(InstanceLifecycle::Recovering, "testing/fake", "0.3.0"),
            instance(InstanceLifecycle::Stopped, "testing/fake", "0.3.0"),
        ];
        let entries = plan_entries(&instances, false, &[]);
        assert_eq!(entries.len(), 2);
        assert!(entries.iter().all(|entry| {
            (entry.decision, entry.reason)
                == (PlanDecision::Keep, PlanReason::SupervisorUnavailable)
        }));
    }

    #[test]
    fn entries_carry_the_provider_name_and_sort_by_instance_id() {
        let instances = [
            instance(InstanceLifecycle::Running, "official/nats", "1.0.1"),
            instance(InstanceLifecycle::Running, "official/postgresql", "1.0.0"),
            instance(InstanceLifecycle::Running, "testing/fake", "0.3.0"),
        ];
        let entries = plan_entries(&instances, true, &[]);
        assert!(
            entries
                .windows(2)
                .all(|pair| pair[0].instance_id < pair[1].instance_id)
        );
        let mut providers: Vec<_> = entries
            .iter()
            .map(|entry| entry.provider.as_str())
            .collect();
        providers.sort_unstable();
        assert_eq!(providers, ["fake", "nats", "postgresql"]);
    }

    #[test]
    fn the_digest_follows_decisions_and_reasons_but_not_the_order_of_the_input() {
        let a = instance(InstanceLifecycle::Running, "official/nats", "1.0.1");
        let b = instance(InstanceLifecycle::Running, "testing/fake", "0.3.0");
        let proven = [HandoverProof::new("testing/fake", "0.3", "id")];
        let forward = plan_digest(&plan_entries(&[a.clone(), b.clone()], true, &proven));
        let backward = plan_digest(&plan_entries(&[b.clone(), a.clone()], true, &proven));
        assert_eq!(forward, backward);
        let changed = plan_digest(&plan_entries(&[a, b], true, &[]));
        assert_ne!(forward, changed);
        let empty = plan_digest(&[]);
        assert_eq!(
            empty.to_string(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }
}
