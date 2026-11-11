//! The trust gate of local provider entries (spec 0037, *Gate trust*):
//! whether Stackly may download, check, initialize, or start anything that
//! comes from an entry or a pinned revision.
//!
//! A catalog entry is trusted through the bundle's code signature. A local
//! entry is trusted only when the user approved exactly its stored bytes.
//! The display reads the snapshot; the gate re-reads the registry, and a
//! registry that cannot be read refuses, never trusts.

use jiff::Timestamp;
use stackly_domain::{
    ApprovalState, Generation, GuardView, ProviderImportRecord, ProviderName, ProviderRevisionKey,
    ProviderRevisionRecord, Registry, RegistryError, Sha256Digest, TrustDecision,
};

use super::model::{Catalog, LocalInfo, ProviderEntry};

/// What the gate decides for an entry or a pinned revision.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum TrustVerdict {
    /// Stackly may install from it and create instances of it.
    Trusted,
    /// A local entry whose stored bytes no decision covers.
    ApprovalRequired,
    /// The approval was revoked, by the user or by the installed catalog.
    Revoked(RevokedInfo),
}

impl TrustVerdict {
    /// Returns whether the gate is open.
    #[must_use]
    pub const fn is_trusted(&self) -> bool {
        matches!(self, Self::Trusted)
    }
}

/// Who revoked a revision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RevokedBy {
    /// The user, through `provider.revoke` or `provider.remove`.
    User,
    /// The `revoked` list of the installed catalog.
    Catalog,
}

impl RevokedBy {
    /// Returns the stable code, the `by` detail of `provider.revoked`.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Catalog => "catalog",
        }
    }
}

/// When and by whom a revision was revoked.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct RevokedInfo {
    /// Who revoked it.
    pub by: RevokedBy,
    /// Since when: the decision time, or midnight UTC of the catalog date.
    pub since: Timestamp,
    /// The catalog's reason, for a catalog revocation.
    pub reason: Option<String>,
    /// The revision of the catalog that revokes it, for a catalog
    /// revocation.
    pub catalog_revision: Option<u64>,
}

impl RevokedInfo {
    /// A revocation by the user at `since`.
    #[must_use]
    pub const fn by_user(since: Timestamp) -> Self {
        Self {
            by: RevokedBy::User,
            since,
            reason: None,
            catalog_revision: None,
        }
    }
}

/// The revocation of the installed catalog that matches `key`, as the
/// verdict carries it: by the catalog, since midnight UTC of its date, with
/// its reason and the catalog revision.
#[must_use]
pub fn catalog_revocation(catalog: &Catalog, key: &ProviderRevisionKey) -> Option<RevokedInfo> {
    catalog.revocation(key).map(|revocation| RevokedInfo {
        by: RevokedBy::Catalog,
        since: revocation.since_instant(),
        reason: Some(revocation.reason.clone()),
        catalog_revision: Some(catalog.meta().revision),
    })
}

/// The verdict of a local import row: `pending` needs an approval, a
/// `revoked` decision on the stored bytes is a revocation by the user at
/// the decision time, and `approved` is trusted.
#[must_use]
pub fn record_trust(record: &ProviderImportRecord) -> TrustVerdict {
    match record.approval() {
        ApprovalState::Pending => TrustVerdict::ApprovalRequired,
        ApprovalState::Approved => TrustVerdict::Trusted,
        ApprovalState::Revoked => TrustVerdict::Revoked(RevokedInfo::by_user(
            record
                .decision
                .as_ref()
                .map_or(record.updated_at, |decision| decision.at),
        )),
    }
}

/// The verdict of an entry from the snapshot alone, without the installed
/// catalog's revocations: for the read model's own status rules.
#[must_use]
pub(crate) fn snapshot_trust(entry: &ProviderEntry) -> TrustVerdict {
    entry
        .local()
        .map_or(TrustVerdict::Trusted, |info: &LocalInfo| {
            record_trust(&info.record)
        })
}

/// The verdict of an entry for display (list, detail,
/// `CatalogVersion.install`), from the snapshot: a matching revocation of
/// the installed catalog first, then for a local entry the approval state
/// of its import row as the snapshot read it. A catalog entry without a
/// revocation is always trusted.
#[must_use]
pub fn entry_trust(catalog: &Catalog, entry: &ProviderEntry) -> TrustVerdict {
    if let Some(revoked) = catalog_revocation(catalog, entry.package.key()) {
        return TrustVerdict::Revoked(revoked);
    }
    snapshot_trust(entry)
}

/// The verdict of an entry for the gate: like [`entry_trust`], but a local
/// entry's import row is read again from the registry. A row that is gone,
/// or now holds other bytes than the entry, needs an approval. A catalog
/// entry reads nothing.
///
/// # Errors
///
/// Returns the [`RegistryError`] of a failed read; the caller refuses the
/// request, never trusts it.
pub fn require_entry_trust(
    catalog: &Catalog,
    registry: &dyn Registry,
    entry: &ProviderEntry,
) -> Result<TrustVerdict, RegistryError> {
    if let Some(revoked) = catalog_revocation(catalog, entry.package.key()) {
        return Ok(TrustVerdict::Revoked(revoked));
    }
    let Some(info) = entry.local() else {
        return Ok(TrustVerdict::Trusted);
    };
    let stored = registry.provider_import(&info.record.provider)?.value;
    Ok(fresh_trust(stored.as_ref(), &info.record.package_sha256))
}

/// The verdict of a local import read again from the registry for bytes an
/// operation froze: the row of `provider` must still hold `package_sha256`
/// and its approval. The `install_runtime` work calls it right before the
/// first exec of a binary from a local entry.
///
/// # Errors
///
/// Returns the [`RegistryError`] of a failed read; the caller fails, never
/// trusts.
pub fn stored_trust(
    registry: &dyn Registry,
    provider: &ProviderName,
    package_sha256: &Sha256Digest,
) -> Result<(TrustVerdict, Generation), RegistryError> {
    let stored = registry.provider_import(provider)?;
    Ok((
        fresh_trust(stored.value.as_ref(), package_sha256),
        stored.generation,
    ))
}

/// The verdict of a pinned revision, from its registry record, which the
/// caller has just read: a matching catalog revocation first (none without
/// a loaded catalog), then a `revoked` approval is a revocation by the user
/// at `approved_at`; anything else is trusted, since a pinned copy never
/// downloads.
#[must_use]
pub fn pinned_trust(
    catalog: Option<&Catalog>,
    key: &ProviderRevisionKey,
    record: &ProviderRevisionRecord,
) -> TrustVerdict {
    if let Some(revoked) = catalog.and_then(|catalog| catalog_revocation(catalog, key)) {
        return TrustVerdict::Revoked(revoked);
    }
    match record.approval {
        Some(TrustDecision::Revoked) => TrustVerdict::Revoked(RevokedInfo::by_user(
            record.approved_at.unwrap_or(Timestamp::UNIX_EPOCH),
        )),
        Some(TrustDecision::Approved) | None => TrustVerdict::Trusted,
    }
}

/// The verdict of a local entry inside a guarded commit: the import row of
/// `provider` read through the guard's view must still hold
/// `package_sha256` and its approval. Called by the commit guards of
/// `create_instance` and `install_runtime`; a verdict other than
/// [`TrustVerdict::Trusted`] fails the operation.
///
/// # Errors
///
/// Returns the [`RegistryError`] of a failed read.
pub fn guard_trust(
    view: &dyn GuardView,
    provider: &ProviderName,
    package_sha256: &Sha256Digest,
) -> Result<TrustVerdict, RegistryError> {
    let stored = view.provider_import(provider)?;
    Ok(fresh_trust(stored.as_ref(), package_sha256))
}

/// The verdict of a pinned revision inside a guarded commit, from its
/// `provider_revisions` approval read through the guard's view. A revision
/// that is not recorded is trusted: the create that pins it records it.
///
/// # Errors
///
/// Returns the [`RegistryError`] of a failed read.
pub fn guard_pinned_trust(
    view: &dyn GuardView,
    catalog: Option<&Catalog>,
    key: &ProviderRevisionKey,
) -> Result<TrustVerdict, RegistryError> {
    Ok(match view.provider_revision(key)? {
        Some(record) => pinned_trust(catalog, key, &record),
        None => catalog
            .and_then(|catalog| catalog_revocation(catalog, key))
            .map_or(TrustVerdict::Trusted, TrustVerdict::Revoked),
    })
}

/// The verdict of an import row read just now, for an entry built from
/// `package_sha256`.
fn fresh_trust(
    stored: Option<&ProviderImportRecord>,
    package_sha256: &Sha256Digest,
) -> TrustVerdict {
    match stored {
        Some(record) if record.package_sha256 == *package_sha256 => record_trust(record),
        _ => TrustVerdict::ApprovalRequired,
    }
}
