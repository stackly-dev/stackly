//! Imported local provider packages and the trust decisions on them (spec
//! 0037, *Model data*): the `provider_imports` table and the approval
//! columns of `provider_revisions`.

use rusqlite::{Connection, params};
use stackly_domain::{
    ImportDecision, ProviderImportRecord, ProviderName, ProviderRevisionKey, ProviderSourceId,
    RegistryError, ResourceRef, ResourceRevision, TrustDecision,
};

use super::column::{Cells, collect, exists, first, int, sql_error, ts};
use super::source;

const COLUMNS: &str = "provider, source_id, provider_revision, software, manifest_sha256, \
     versions_sha256, package_sha256, package_bytes, origin_path, quarantined, \
     quarantine_agent, imported_at, decision, decided_package_sha256, decided_revision, \
     decided_permissions, decided_at, revision, created_at, updated_at";

/// Every import row, ordered by provider ID.
pub(super) fn all(conn: &Connection) -> Result<Vec<ProviderImportRecord>, RegistryError> {
    collect(
        conn,
        &format!("SELECT {COLUMNS} FROM provider_imports ORDER BY provider"),
        [],
        "provider_imports",
        record,
    )
}

/// The import row of `provider`.
pub(super) fn one(
    conn: &Connection,
    provider: &ProviderName,
) -> Result<Option<ProviderImportRecord>, RegistryError> {
    first(
        conn,
        &format!("SELECT {COLUMNS} FROM provider_imports WHERE provider = ?1"),
        [provider.as_str()],
        "provider_imports",
        record,
    )
}

/// Whether a provider revision of `provider` is `approved`.
pub(super) fn has_approved_revision(
    conn: &Connection,
    provider: &ProviderName,
) -> Result<bool, RegistryError> {
    exists(
        conn,
        "SELECT 1 FROM provider_revisions WHERE provider = ?1 AND approval = ?2",
        (provider.as_str(), TrustDecision::Approved.as_code()),
    )
}

fn record(cells: Cells<'_, '_>) -> Result<ProviderImportRecord, RegistryError> {
    let bytes = cells
        .opt_u64("package_bytes")?
        .ok_or_else(|| cells.corrupt("package_bytes", "is null"))?;
    let mut record = ProviderImportRecord::new(
        cells.parse("provider")?,
        cells.parse::<ProviderSourceId>("source_id")?,
        cells.parse("provider_revision")?,
        cells.parse("software")?,
        cells.parse("manifest_sha256")?,
        cells.parse("versions_sha256")?,
        cells.parse("package_sha256")?,
        bytes,
        cells.text("origin_path")?,
        cells.ts("imported_at")?,
    )
    .with_quarantine(
        cells.opt_bool("quarantined")?,
        cells.opt_text("quarantine_agent")?,
    );
    if let Some(decision) = cells.opt_parse::<TrustDecision>("decision")? {
        let missing = |column| cells.corrupt(column, "is null while decision is set");
        record = record.with_decision(ImportDecision::new(
            decision,
            cells
                .opt_parse("decided_package_sha256")?
                .ok_or_else(|| missing("decided_package_sha256"))?,
            cells
                .opt_parse("decided_revision")?
                .ok_or_else(|| missing("decided_revision"))?,
            cells
                .opt_text("decided_permissions")?
                .ok_or_else(|| missing("decided_permissions"))?,
            cells
                .opt_ts("decided_at")?
                .ok_or_else(|| missing("decided_at"))?,
        ));
    }
    record.revision = cells.revision()?;
    record.created_at = cells.ts("created_at")?;
    record.updated_at = cells.ts("updated_at")?;
    Ok(record)
}

/// `PutProviderImport`: insert a row, or replace the one at `expected`,
/// keeping its decision and `created_at`.
pub(super) fn put(
    conn: &Connection,
    record: &ProviderImportRecord,
    expected: Option<ResourceRevision>,
) -> Result<(), RegistryError> {
    let resource = ResourceRef::ProviderImport(record.provider.clone());
    if record.decision.is_some() {
        return Err(RegistryError::Integrity {
            detail: "an import write never carries a decision".to_owned(),
        });
    }
    let stored = first(
        conn,
        "SELECT revision FROM provider_imports WHERE provider = ?1",
        [record.provider.as_str()],
        "provider_imports",
        |cells| cells.revision(),
    )?;
    match (expected, stored) {
        (None, Some(_)) => return Err(RegistryError::DuplicateResource { resource }),
        (Some(_), None) => return Err(RegistryError::NotFound { resource }),
        (Some(expected), Some(actual)) if expected != actual => {
            return Err(RegistryError::RevisionConflict {
                resource,
                expected,
                actual,
            });
        }
        _ => {}
    }
    source::require(conn, record.source_id)?;
    if exists(
        conn,
        "SELECT 1 FROM provider_imports WHERE package_sha256 = ?1 AND provider <> ?2",
        (record.package_sha256.to_string(), record.provider.as_str()),
    )? {
        return Err(RegistryError::Integrity {
            detail: format!(
                "another import holds the package digest {}",
                record.package_sha256
            ),
        });
    }
    let package_bytes = int(record.package_bytes, "package_bytes")?;
    let revision = int(record.revision.get(), "revision")?;
    if stored.is_some() {
        // A replace keeps `created_at` and the decision columns.
        conn.execute(
            "UPDATE provider_imports SET source_id = ?2, provider_revision = ?3, software = ?4, \
             manifest_sha256 = ?5, versions_sha256 = ?6, package_sha256 = ?7, \
             package_bytes = ?8, origin_path = ?9, quarantined = ?10, quarantine_agent = ?11, \
             imported_at = ?12, revision = ?13, updated_at = ?14 WHERE provider = ?1",
            params![
                record.provider.as_str(),
                record.source_id.to_string(),
                record.provider_revision.as_str(),
                record.software.as_str(),
                record.manifest_sha256.to_string(),
                record.versions_sha256.to_string(),
                record.package_sha256.to_string(),
                package_bytes,
                record.origin_path,
                record.quarantined,
                record.quarantine_agent,
                ts(record.imported_at),
                revision,
                ts(record.updated_at),
            ],
        )
    } else {
        conn.execute(
            "INSERT INTO provider_imports (provider, source_id, provider_revision, software, \
             manifest_sha256, versions_sha256, package_sha256, package_bytes, origin_path, \
             quarantined, quarantine_agent, imported_at, revision, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            params![
                record.provider.as_str(),
                record.source_id.to_string(),
                record.provider_revision.as_str(),
                record.software.as_str(),
                record.manifest_sha256.to_string(),
                record.versions_sha256.to_string(),
                record.package_sha256.to_string(),
                package_bytes,
                record.origin_path,
                record.quarantined,
                record.quarantine_agent,
                ts(record.imported_at),
                revision,
                ts(record.created_at),
                ts(record.updated_at),
            ],
        )
    }
    .map_err(sql_error)?;
    Ok(())
}

/// `DecideProviderImport`: the decision columns, a raised revision, and
/// `updated_at` at the decision time.
pub(super) fn decide(
    conn: &Connection,
    provider: &ProviderName,
    decision: &ImportDecision,
) -> Result<(), RegistryError> {
    let changed = conn
        .execute(
            "UPDATE provider_imports SET decision = ?2, decided_package_sha256 = ?3, \
             decided_revision = ?4, decided_permissions = ?5, decided_at = ?6, \
             revision = revision + 1, updated_at = ?6 WHERE provider = ?1",
            params![
                provider.as_str(),
                decision.decision.as_code(),
                decision.package_sha256.to_string(),
                decision.provider_revision.as_str(),
                decision.permissions_json,
                ts(decision.at),
            ],
        )
        .map_err(sql_error)?;
    if changed == 0 {
        return Err(RegistryError::NotFound {
            resource: ResourceRef::ProviderImport(provider.clone()),
        });
    }
    Ok(())
}

/// `DeleteProviderImport`.
pub(super) fn delete(conn: &Connection, provider: &ProviderName) -> Result<(), RegistryError> {
    let changed = conn
        .execute(
            "DELETE FROM provider_imports WHERE provider = ?1",
            [provider.as_str()],
        )
        .map_err(sql_error)?;
    if changed == 0 {
        return Err(RegistryError::NotFound {
            resource: ResourceRef::ProviderImport(provider.clone()),
        });
    }
    Ok(())
}

/// `SetProviderRevisionApproval`: zero or one row.
pub(super) fn set_revision_approval(
    conn: &Connection,
    key: &ProviderRevisionKey,
    approval: TrustDecision,
    at: jiff::Timestamp,
) -> Result<(), RegistryError> {
    conn.execute(
        "UPDATE provider_revisions SET approval = ?4, approved_at = ?5 \
         WHERE provider = ?1 AND revision = ?2 AND manifest_sha256 = ?3",
        params![
            key.provider().as_str(),
            key.revision().as_str(),
            key.manifest_sha256().to_string(),
            approval.as_code(),
            ts(at),
        ],
    )
    .map_err(sql_error)?;
    Ok(())
}

/// `RevokeProviderApprovals`: every `approved` revision of the provider.
pub(super) fn revoke_approvals(
    conn: &Connection,
    provider: &ProviderName,
    at: jiff::Timestamp,
) -> Result<(), RegistryError> {
    conn.execute(
        "UPDATE provider_revisions SET approval = ?3, approved_at = ?4 \
         WHERE provider = ?1 AND approval = ?2",
        params![
            provider.as_str(),
            TrustDecision::Approved.as_code(),
            TrustDecision::Revoked.as_code(),
            ts(at),
        ],
    )
    .map_err(sql_error)?;
    Ok(())
}
