//! *Precheck Up* (spec 0046 AC-1, AC-2): a pure check over the plan the
//! daemon computed, run in the begin gate before any operation exists.
//! The first row that matches wins.

use stackly_domain::{NeedsSecretItem, Sha256Digest};

use super::children::PlannedUp;
use super::report::UpError;
use crate::environment::BlockerKind;
use crate::environment::Severity;
use crate::environment::register::EnvOpError;

/// The reason of an `env.instance_blocked` blocker for an instance with a
/// `needsSecret` blocker.
const NEEDS_SECRET_REASON: &str = "needs_secret";

fn is_needs_secret(kind: &BlockerKind) -> bool {
    matches!(kind, BlockerKind::InstanceBlocked { reason, .. } if reason == NEEDS_SECRET_REASON)
}

/// Checks `planned` against the digest the client reviewed: an invalid
/// manifest, a blocker other than a member's `needs_secret`, a digest that
/// differs, then every reference that is not set, from blocked members and
/// from `create` items, by text.
///
/// # Errors
///
/// Returns the [`UpError`] of the first row that matches.
pub fn check(planned: &PlannedUp, expected: Sha256Digest) -> Result<(), UpError> {
    let errors = planned.validation.findings.count_of(Severity::Error);
    if !planned.validation.is_valid() {
        return Err(UpError::Environment(EnvOpError::Invalid { errors }));
    }
    let blocked: Vec<_> = planned
        .plan
        .blockers
        .iter()
        .zip(&planned.blocked)
        .filter(|(blocker, _)| !is_needs_secret(&blocker.kind))
        .map(|(_, item)| item.clone())
        .collect();
    if !blocked.is_empty() {
        return Err(UpError::PlanBlocked { blockers: blocked });
    }
    if planned.plan.applicable && planned.plan.digest != Some(expected) {
        return Err(UpError::PlanChanged);
    }
    let mut references: Vec<NeedsSecretItem> = planned
        .plan
        .blockers
        .iter()
        .filter(|blocker| is_needs_secret(&blocker.kind))
        .filter_map(|blocker| {
            let service = blocker.service.as_ref()?;
            planned
                .instance_needs
                .iter()
                .find(|(alias, _)| alias == service)
        })
        .flat_map(|(_, items)| items.iter().cloned())
        .chain(planned.create_needs.iter().cloned())
        .collect();
    references.sort_by_key(|item| item.reference.to_string());
    references.dedup_by(|a, b| a.reference == b.reference);
    if !references.is_empty() {
        return Err(UpError::NeedsSecret { references });
    }
    if !planned.plan.applicable {
        // Only a member's `needs_secret` blocked it, and that member's
        // blocker carried no reference: still blocked.
        return Err(UpError::PlanBlocked {
            blockers: planned.blocked.clone(),
        });
    }
    Ok(())
}
