//! The list, the status of one reference, and the probes that keep the
//! cached status true (spec 0011, *Status dan probe*, AC-8).

use jiff::Timestamp;
use stackly_domain::{
    InstanceBlocker, PlatformStoreKey, SecretRef, SecretReferenceFilter, SecretReferenceView,
    SecretStatus, WriteBatch, WriteOp,
};
use stackly_platform::SecretPresence;

use super::{SecretChange, SecretService, StoreStatus};
use crate::secret::error::SecretError;

/// The references and the store, as `secret.list` and `secret.status`
/// report them: never a value.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct SecretListing {
    /// The reference rows with their uses, ordered by reference text.
    pub items: Vec<SecretReferenceView>,
    /// The store, from its cached state.
    pub store: StoreStatus,
}

/// How many references a probe of every reference found in each status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct ProbeReport {
    /// The store holds a value.
    pub present: usize,
    /// The store holds none.
    pub missing: usize,
    /// The store could not be checked.
    pub unknown: usize,
}

impl SecretService {
    /// Lists the references `filter` selects from the cached status, with
    /// the cached store state; it never waits on the store (AC-8).
    ///
    /// # Errors
    ///
    /// Returns [`SecretError::Registry`].
    pub fn list(&self, filter: SecretReferenceFilter) -> Result<SecretListing, SecretError> {
        let items = Self::read(self.registry().secret_references(filter))?;
        Ok(SecretListing {
            items,
            store: self.store_status(),
        })
    }

    /// Probes one reference again and returns it with the store state, as
    /// `secret.status` does (AC-8).
    ///
    /// # Errors
    ///
    /// Returns [`SecretError::NotFound`] for an unregistered reference and
    /// [`SecretError::Registry`].
    pub fn status(&self, reference: &SecretRef) -> Result<SecretListing, SecretError> {
        let view = self.probe_one(reference)?;
        Ok(SecretListing {
            items: vec![view],
            store: self.store_status(),
        })
    }

    /// Checks whether the store holds an item for one reference, from its
    /// attributes and without a dialog, and records what it saw: status
    /// `present`, `missing`, or `unknown` when the store could not be
    /// checked, with the store key it computed. The revision stays. It
    /// holds the reference like `put` and `delete`, so an old observation
    /// never overwrites a newer value. A status that changed is reported.
    ///
    /// When the stored key differs from the computed one (a registry copied
    /// from another root), the computed key is probed and recorded, and a
    /// warning names both; the old key is never read.
    ///
    /// # Errors
    ///
    /// Returns [`SecretError::NotFound`] for an unregistered reference and
    /// [`SecretError::Registry`].
    pub fn probe_one(&self, reference: &SecretRef) -> Result<SecretReferenceView, SecretError> {
        let guard = self.locks.lock(reference);
        let observed = self.probe_held(reference);
        drop(guard);
        let (view, change) = observed?;
        if let Some(change) = change {
            self.report(&change);
        }
        Ok(view)
    }

    fn probe_held(
        &self,
        reference: &SecretRef,
    ) -> Result<(SecretReferenceView, Option<SecretChange>), SecretError> {
        let view = Self::read(self.registry().secret_reference(reference))?.ok_or_else(|| {
            SecretError::NotFound {
                reference: reference.clone(),
            }
        })?;
        let (status, key) = self.observe(&view)?;
        let at = self.now();
        let ops = vec![Self::observation(&view, status, key.clone(), at)];
        self.registry()
            .commit(WriteBatch::new(ops))
            .map_err(|failed| SecretError::from(failed.error))?;
        Ok(Self::observed(view, status, key, at))
    }

    /// Probes every registered reference, as the daemon does when it
    /// starts, and records every status in one commit; the log holds only
    /// the count per status (AC-8). Every reference is held for the whole
    /// probe.
    ///
    /// # Errors
    ///
    /// Returns [`SecretError::Registry`].
    pub fn probe_all(&self) -> Result<ProbeReport, SecretError> {
        let listed = Self::read(
            self.registry()
                .secret_references(SecretReferenceFilter::default()),
        )?;
        let references: Vec<SecretRef> = listed
            .into_iter()
            .map(|view| view.record.reference)
            .collect();
        let guard = self.locks.lock_all(&references);
        // Read again under the locks: a delete may have come in between.
        let views = Self::read(
            self.registry()
                .secret_references(SecretReferenceFilter::default()),
        )?;
        let mut report = ProbeReport::default();
        let at = self.now();
        let mut ops = Vec::new();
        let mut changes = Vec::new();
        for view in views {
            if !references.contains(&view.record.reference) {
                continue;
            }
            let (status, key) = self.observe(&view)?;
            match status {
                SecretStatus::Present => report.present += 1,
                SecretStatus::Missing => report.missing += 1,
                SecretStatus::Unknown => report.unknown += 1,
            }
            ops.push(Self::observation(&view, status, key.clone(), at));
            changes.push(Self::observed(view, status, key, at).1);
        }
        if !ops.is_empty() {
            self.registry()
                .commit(WriteBatch::new(ops))
                .map_err(|failed| SecretError::from(failed.error))?;
        }
        drop(guard);
        for change in changes.into_iter().flatten() {
            self.report(&change);
        }
        tracing::info!(
            present = report.present,
            missing = report.missing,
            unknown = report.unknown,
            "probed every secret reference"
        );
        Ok(report)
    }

    /// Returns whether a blocker is gone for good: every reference of a
    /// needs secret blocker is `present` in the cached status, which the
    /// daemon checks when `secret.changed` arrives. Any other blocker is
    /// not the secret service's to clear.
    ///
    /// # Errors
    ///
    /// Returns [`SecretError::Registry`].
    pub fn clears_blocker(&self, blocker: &InstanceBlocker) -> Result<bool, SecretError> {
        let InstanceBlocker::NeedsSecret { references, .. } = blocker else {
            return Ok(false);
        };
        for item in references {
            let view = Self::read(self.registry().secret_reference(&item.reference))?;
            if view.is_none_or(|view| view.record.status != SecretStatus::Present) {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Probes the store for one row with the key computed now.
    fn observe(
        &self,
        view: &SecretReferenceView,
    ) -> Result<(SecretStatus, PlatformStoreKey), SecretError> {
        let reference = &view.record.reference;
        let key = self.store_key(reference)?;
        if key != view.record.platform_store_key {
            tracing::warn!(
                %reference,
                stored_key = %view.record.platform_store_key,
                computed_key = %key,
                "a secret reference names another store key; it is probed under the computed key"
            );
        }
        let status = match self.store().probe(reference) {
            Ok(SecretPresence::Present) => SecretStatus::Present,
            Ok(SecretPresence::Missing) => SecretStatus::Missing,
            Err(_) => SecretStatus::Unknown,
        };
        Ok((status, key))
    }

    /// The op that records an observation of a row made `at`.
    pub(super) fn observation(
        view: &SecretReferenceView,
        status: SecretStatus,
        key: PlatformStoreKey,
        at: Timestamp,
    ) -> WriteOp {
        WriteOp::ObserveSecretReference {
            reference: view.record.reference.clone(),
            status,
            status_observed_at: at,
            platform_store_key: key,
        }
    }

    /// The row after an observation made `at`, and the change to report
    /// when its status changed.
    pub(super) fn observed(
        mut view: SecretReferenceView,
        status: SecretStatus,
        key: PlatformStoreKey,
        at: Timestamp,
    ) -> (SecretReferenceView, Option<SecretChange>) {
        let changed = view.record.status != status;
        view.record.status = status;
        view.record.status_observed_at = Some(at);
        view.record.platform_store_key = key;
        let change = changed.then(|| Self::change_of(&view));
        (view, change)
    }
}
