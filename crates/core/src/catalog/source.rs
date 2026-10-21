//! The row of `provider_sources` for the catalog the daemon loaded (spec
//! 0017 K-2, built by spec 0020): `pin_revision` names it in the record of
//! every revision it pins.

use stackly_domain::{
    ProviderSourceId, ProviderSourceKind, ProviderSourceRecord, Registry, RegistryError,
    WriteBatch, WriteOp,
};
use stackly_platform::Clock;

use super::model::{CatalogLocation, CatalogMeta};

/// The `location` of the catalog inside the app bundle. It is fixed, so an
/// app that moves does not make a new row.
pub const BUNDLED_LOCATION: &str = "app";
/// The `trust_root` of the bundled catalog.
pub const BUNDLED_TRUST_ROOT: &str = "app_bundle";

/// Makes sure the registry holds exactly one `provider_sources` row for the
/// catalog described by `meta` and returns its ID. A second call, and a
/// restart of the daemon, find the same row; `last_sync_at` stays empty.
///
/// # Errors
///
/// Returns the [`RegistryError`] of the read or the write, and
/// [`RegistryError::NotUtf8Path`] when the folder of a development catalog
/// is not valid UTF-8.
pub fn ensure_source(
    registry: &dyn Registry,
    clock: &dyn Clock,
    meta: &CatalogMeta,
) -> Result<ProviderSourceId, RegistryError> {
    let (kind, location, trust_root) = match &meta.location {
        CatalogLocation::Bundled(_) => (
            ProviderSourceKind::Bundled,
            BUNDLED_LOCATION.to_owned(),
            Some(BUNDLED_TRUST_ROOT.to_owned()),
        ),
        CatalogLocation::Development(path) => (
            ProviderSourceKind::Development,
            path.to_str()
                .ok_or(RegistryError::NotUtf8Path { field: "location" })?
                .to_owned(),
            None,
        ),
    };
    if let Some(found) = registry.provider_source_by_location(kind, &location)?.value {
        return Ok(found.id);
    }
    let id = ProviderSourceId::generate();
    let mut record = ProviderSourceRecord::new(id, kind, location.clone(), clock.now());
    record.trust_root = trust_root;
    match registry.commit(WriteBatch::new(vec![WriteOp::InsertProviderSource(record)])) {
        Ok(_) => Ok(id),
        // Another caller wrote the row between the read and the write.
        Err(error) if matches!(error.error, RegistryError::DuplicateResource { .. }) => registry
            .provider_source_by_location(kind, &location)?
            .value
            .map(|found| found.id)
            .ok_or(error.error),
        Err(error) => Err(error.error),
    }
}
