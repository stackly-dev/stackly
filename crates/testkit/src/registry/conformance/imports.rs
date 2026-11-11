//! Scenarios of the ops feature 37 adds (spec 0037, *Model data*): import
//! rows of local provider packages, their trust decision, and the approval
//! columns of provider revisions.

use stackly_domain::{
    ImportDecision, ProviderImportRecord, ProviderName, ProviderRevisionKey, ProviderSourceId,
    ProviderSourceKind, ProviderSourceRecord, RegistryError, ResourceRef, TrustDecision, WriteOp,
};

use super::{Suite, at, batch, expect_eq, parse, rev, shifted, suffix};
use crate::registry::ProviderRevisionBuilder;
use crate::registry::builders::fresh_digest;

fn local_source(suite: &Suite<'_>) -> ProviderSourceRecord {
    let id = ProviderSourceId::generate();
    ProviderSourceRecord::new(
        id,
        ProviderSourceKind::Local,
        format!("imports-{id}"),
        suite.clock_now(),
    )
}

fn import(
    suite: &Suite<'_>,
    provider: &ProviderName,
    source: ProviderSourceId,
) -> ProviderImportRecord {
    ProviderImportRecord::new(
        provider.clone(),
        source,
        parse("make a revision label", "1.0.0"),
        parse("make a software", "demo"),
        fresh_digest(),
        fresh_digest(),
        fresh_digest(),
        2048,
        "~/providers/demo".to_owned(),
        suite.clock_now(),
    )
    .with_quarantine(Some(true), Some("Safari".to_owned()))
}

fn decision(
    kind: TrustDecision,
    record: &ProviderImportRecord,
    seconds: i64,
    suite: &Suite<'_>,
) -> ImportDecision {
    ImportDecision::new(
        kind,
        record.package_sha256,
        record.provider_revision.clone(),
        r#"{"capabilities":[]}"#.to_owned(),
        shifted(suite.clock_now(), seconds),
    )
}

/// Inserts, replaces, decides, and deletes import rows, with the checks of
/// each op, and the approval ops on provider revisions.
pub(super) fn imports(suite: &Suite<'_>) {
    let source = local_source(suite);
    suite.commit(
        "insert the local provider source",
        vec![WriteOp::InsertProviderSource(source.clone())],
    );
    let provider: ProviderName = parse(
        "make a provider name",
        &format!("acme/demo-{}", suffix(source.id)),
    );
    let resource = ResourceRef::ProviderImport(provider.clone());
    let first = import(suite, &provider, source.id);

    expect_eq(
        "a provider that is not imported reads as none",
        suite.value(
            "read an import that is not stored",
            suite.registry.provider_import(&provider),
        ),
        None,
    );
    let missing = ProviderSourceId::generate();
    let mut orphan = first.clone();
    orphan.source_id = missing;
    suite.rejected(
        "an import names a stored source",
        batch(vec![WriteOp::PutProviderImport {
            record: Box::new(orphan),
            expected: None,
        }]),
        at(
            0,
            RegistryError::MissingReference {
                resource: ResourceRef::ProviderSource(missing),
            },
        ),
    );
    suite.rejected(
        "an import write never carries a decision",
        batch(vec![WriteOp::PutProviderImport {
            record: Box::new(first.clone().with_decision(decision(
                TrustDecision::Approved,
                &first,
                0,
                suite,
            ))),
            expected: None,
        }]),
        at(
            0,
            RegistryError::Integrity {
                detail: "an import write never carries a decision".to_owned(),
            },
        ),
    );
    suite.rejected(
        "replacing an import that is not stored is not found",
        batch(vec![WriteOp::PutProviderImport {
            record: Box::new({
                let mut next = first.clone();
                next.revision = rev(2);
                next
            }),
            expected: Some(rev(1)),
        }]),
        at(
            0,
            RegistryError::NotFound {
                resource: resource.clone(),
            },
        ),
    );
    let committed = suite.commit(
        "insert an import",
        vec![WriteOp::PutProviderImport {
            record: Box::new(first.clone()),
            expected: None,
        }],
    );
    expect_eq(
        "an inserted import returns its revision",
        committed.revisions,
        vec![(resource.clone(), rev(1))],
    );
    expect_eq(
        "an import reads back as written",
        suite.value("read the import", suite.registry.provider_import(&provider)),
        Some(first.clone()),
    );
    expect_eq(
        "the import is listed",
        suite
            .value("list the imports", suite.registry.provider_imports())
            .into_iter()
            .filter(|record| record.provider == provider)
            .collect::<Vec<_>>(),
        vec![first.clone()],
    );
    suite.rejected(
        "a second insert of a provider is a duplicate",
        batch(vec![WriteOp::PutProviderImport {
            record: Box::new(first.clone()),
            expected: None,
        }]),
        at(
            0,
            RegistryError::DuplicateResource {
                resource: resource.clone(),
            },
        ),
    );
    let other: ProviderName = parse(
        "make another provider name",
        &format!("acme/other-{}", suffix(source.id)),
    );
    let mut same_bytes = import(suite, &other, source.id);
    same_bytes.package_sha256 = first.package_sha256;
    suite.rejected(
        "two imports never hold the same package digest",
        batch(vec![WriteOp::PutProviderImport {
            record: Box::new(same_bytes),
            expected: None,
        }]),
        at(
            0,
            RegistryError::Integrity {
                detail: format!(
                    "another import holds the package digest {}",
                    first.package_sha256
                ),
            },
        ),
    );

    let approved = decision(TrustDecision::Approved, &first, 60, suite);
    let committed = suite.commit(
        "approve the import",
        vec![WriteOp::DecideProviderImport {
            provider: provider.clone(),
            decision: approved.clone(),
        }],
    );
    expect_eq(
        "a decision returns no revision",
        committed.revisions,
        Vec::new(),
    );
    let mut decided = first.clone().with_decision(approved.clone());
    decided.revision = rev(2);
    decided.updated_at = approved.at;
    expect_eq(
        "a decision raises the revision and stamps updated_at",
        suite.value(
            "read the decided import",
            suite.registry.provider_import(&provider),
        ),
        Some(decided.clone()),
    );

    let mut replaced = import(suite, &provider, source.id);
    replaced.revision = rev(3);
    replaced.quarantined = None;
    replaced.quarantine_agent = None;
    suite.rejected(
        "a replace checks the revision it read",
        batch(vec![WriteOp::PutProviderImport {
            record: Box::new({
                let mut stale = replaced.clone();
                stale.revision = rev(2);
                stale
            }),
            expected: Some(rev(1)),
        }]),
        at(
            0,
            RegistryError::RevisionConflict {
                resource: resource.clone(),
                expected: rev(1),
                actual: rev(2),
            },
        ),
    );
    suite.commit(
        "replace the import with other bytes",
        vec![WriteOp::PutProviderImport {
            record: Box::new(replaced.clone()),
            expected: Some(rev(2)),
        }],
    );
    let mut kept = replaced.clone().with_decision(approved);
    kept.created_at = first.created_at;
    expect_eq(
        "a replace keeps the decision and created_at",
        suite.value(
            "read the replaced import",
            suite.registry.provider_import(&provider),
        ),
        Some(kept),
    );

    revision_approvals(suite, &provider, source.id);

    suite.commit(
        "delete the import",
        vec![WriteOp::DeleteProviderImport(provider.clone())],
    );
    expect_eq(
        "a deleted import reads as none",
        suite.value(
            "read the deleted import",
            suite.registry.provider_import(&provider),
        ),
        None,
    );
    suite.rejected(
        "deleting an import that is not stored is not found",
        batch(vec![WriteOp::DeleteProviderImport(provider.clone())]),
        at(
            0,
            RegistryError::NotFound {
                resource: resource.clone(),
            },
        ),
    );
    suite.rejected(
        "deciding on an import that is not stored is not found",
        batch(vec![WriteOp::DecideProviderImport {
            provider,
            decision: decision(TrustDecision::Revoked, &first, 0, suite),
        }]),
        at(0, RegistryError::NotFound { resource }),
    );
}

/// `SetProviderRevisionApproval` touches one key, `RevokeProviderApprovals`
/// every approved revision of the provider, and
/// `provider_has_approved_revision` follows them.
fn revision_approvals(suite: &Suite<'_>, provider: &ProviderName, source: ProviderSourceId) {
    let builder = |label: &str| {
        ProviderRevisionBuilder::new()
            .provider(provider.as_str())
            .revision(label)
            .software("demo")
    };
    let old = builder("1.0.0").record().with_source_id(source);
    let new = builder("1.1.0").record().with_source_id(source);
    let other = ProviderRevisionBuilder::new()
        .columns(None, None, Some(TrustDecision::Approved))
        .record();
    let key = |record: &stackly_domain::ProviderRevisionRecord| -> ProviderRevisionKey {
        record.revision.key().clone()
    };
    suite.commit(
        "record three provider revisions",
        vec![
            WriteOp::InsertProviderRevision(old.clone()),
            WriteOp::InsertProviderRevision(new.clone()),
            WriteOp::InsertProviderRevision(other.clone()),
        ],
    );
    let has = |check: &str| {
        suite.value(
            check,
            suite.registry.provider_has_approved_revision(provider),
        )
    };
    expect_eq("no revision is approved yet", has("read before"), false);

    let approved_at = shifted(suite.clock_now(), 120);
    let missing = builder("9.9.9").key();
    suite.commit(
        "approve one revision, and a key that is not recorded",
        vec![
            WriteOp::SetProviderRevisionApproval {
                key: key(&old),
                approval: TrustDecision::Approved,
                at: approved_at,
            },
            WriteOp::SetProviderRevisionApproval {
                key: missing.clone(),
                approval: TrustDecision::Approved,
                at: approved_at,
            },
            WriteOp::SetProviderRevisionApproval {
                key: key(&new),
                approval: TrustDecision::Approved,
                at: approved_at,
            },
        ],
    );
    let read = |check: &str, record: &stackly_domain::ProviderRevisionRecord| {
        let stored = suite
            .value(check, suite.registry.provider_revision(&key(record)))
            .unwrap_or_else(|| super::fail(check, "the revision is not recorded"));
        (stored.approval, stored.approved_at)
    };
    expect_eq(
        "an approval sets the code and the time",
        read("read the approved revision", &old),
        (Some(TrustDecision::Approved), Some(approved_at)),
    );
    expect_eq(
        "a key that is not recorded writes nothing",
        suite.value(
            "read the missing key",
            suite.registry.provider_revision(&missing),
        ),
        None,
    );
    expect_eq(
        "an approved revision is found",
        has("read after approve"),
        true,
    );

    let revoked_at = shifted(suite.clock_now(), 180);
    suite.commit(
        "revoke every approval of the provider",
        vec![WriteOp::RevokeProviderApprovals {
            provider: provider.clone(),
            at: revoked_at,
        }],
    );
    expect_eq(
        "a revoke marks every approved revision of the provider",
        (
            read("read the old revision", &old),
            read("read the new revision", &new),
        ),
        (
            (Some(TrustDecision::Revoked), Some(revoked_at)),
            (Some(TrustDecision::Revoked), Some(revoked_at)),
        ),
    );
    expect_eq(
        "a revoke leaves other providers alone",
        read("read the other provider", &other).0,
        Some(TrustDecision::Approved),
    );
    expect_eq(
        "no approved revision is left",
        has("read after revoke"),
        false,
    );
}
