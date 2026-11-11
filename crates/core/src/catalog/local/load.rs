//! Loading imported local packages into catalog entries (spec 0037, *Read
//! model*): every import row's store copy, checked again on each load.

use std::sync::Arc;

use stackly_domain::{ProviderImportRecord, Registry, RegistryError, Versioned};
use stackly_platform::{DirectoryLayout, Filesystem};

use super::digest::package_sha256;
use super::permissions::permissions;
use crate::catalog::index::versions_sha256;
use crate::catalog::model::{
    CatalogEntry, EntryOrigin, InvalidEntry, LocalInfo, ProviderEntry, TileHint,
};
use crate::provider::{
    FindingOrigin, Issue, IssueCode, Location, NameCheck, Severity, check_package,
};

/// Builds the catalog entries of every import row, in provider order, with
/// the registry generation the rows were read at.
///
/// Each row's store copy `provider-imports/<package sha256>/` goes through
/// `check_package`. A copy that does not load (missing or damaged) is an
/// invalid entry with its `package` findings; a copy that loads but whose
/// package digest, ID, revision, manifest digest, or `versions.json` digest
/// differs from the row is an invalid entry with one `import_digest_mismatch`
/// issue. An `authoring` finding never makes a stored copy invalid, the
/// rule of pinned copies. Invalid local entries keep their row, so they stay
/// `local` and can be removed or imported again; the other entries load as
/// usual. It never writes.
///
/// # Errors
///
/// Returns the [`RegistryError`] of a failed read of the rows.
pub fn load(
    fs: &dyn Filesystem,
    registry: &dyn Registry,
    layout: &DirectoryLayout,
) -> Result<Versioned<Vec<CatalogEntry>>, RegistryError> {
    let rows = registry.provider_imports()?;
    let entries = rows
        .value
        .into_iter()
        .map(|record| load_one(fs, layout, record))
        .collect();
    Ok(Versioned::new(entries, rows.generation))
}

fn load_one(
    fs: &dyn Filesystem,
    layout: &DirectoryLayout,
    record: ProviderImportRecord,
) -> CatalogEntry {
    let dir = layout.provider_import_dir(&record.package_sha256);
    let check = check_package(fs, &dir, NameCheck::Skip);
    let invalid = |record: ProviderImportRecord, issues: Vec<Issue>| {
        CatalogEntry::Invalid(InvalidEntry {
            id: record.provider.clone(),
            category: None,
            tile: TileHint::local(record.provider.name()),
            issues,
            local: Some(Box::new(record)),
        })
    };
    // Only an error makes a store copy unloadable; a warning never does
    // (spec 0036 fix 3).
    let blocks = |finding: &crate::provider::Finding| {
        finding.origin == FindingOrigin::Package && finding.severity == Severity::Error
    };
    let Some(package) = check.package.filter(|_| !check.findings.iter().any(blocks)) else {
        let issues = check
            .findings
            .into_iter()
            .filter(blocks)
            .map(|finding| finding.issue)
            .collect();
        return invalid(record, issues);
    };
    let key = package.key();
    let matches = package_sha256(package.files()) == record.package_sha256
        && key.provider() == &record.provider
        && key.revision() == &record.provider_revision
        && *key.manifest_sha256() == record.manifest_sha256
        && package.software() == &record.software
        && versions_sha256(package.files()).as_deref()
            == Some(record.versions_sha256.to_string().as_str());
    if !matches {
        let category = Some(package.manifest().category());
        let mut entry = invalid(
            record,
            vec![Issue::new(
                "provider.yaml",
                Location::Field("metadata.id".into()),
                IssueCode::ImportDigestMismatch,
                "the stored copy does not match the import record",
            )],
        );
        if let CatalogEntry::Invalid(invalid) = &mut entry {
            invalid.category = category;
        }
        return entry;
    }
    let permissions = permissions(&package);
    CatalogEntry::Provider(ProviderEntry {
        tile: TileHint::local(package.manifest().display_name()),
        package: Arc::new(package),
        notes: Vec::new(),
        origin: EntryOrigin::Local(Box::new(LocalInfo::new(record, permissions))),
    })
}
