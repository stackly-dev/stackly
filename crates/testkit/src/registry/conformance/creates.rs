//! Scenarios of the ops feature 20 adds (spec 0005, *Tambahan fitur 20*):
//! provider sources, `ProviderRevisionRecord.source_id`, and
//! `DeleteInstanceCreatedBy`.

use std::collections::BTreeMap;

use stackly_domain::{
    ConfigRevisionId, ConfigRevisionRecord, ConfigValuesJson, InstanceLifecycle,
    NotDeletableReason, OperationId, PlatformStoreKey, ProviderRevisionKey, ProviderSourceId,
    ProviderSourceKind, ProviderSourceRecord, RegistryError, ResourceRef, SecretFieldName,
    SecretOwner, SecretPurpose, SecretRef, SecretReferenceId, SecretReferenceRecord, SecretUser,
    WriteOp,
};

use super::{Suite, at, batch, domain, expect_eq, parse, rev, suffix};
use crate::registry::ProviderRevisionBuilder;

fn fresh_source(suite: &Suite<'_>) -> ProviderSourceRecord {
    let id = ProviderSourceId::generate();
    ProviderSourceRecord::new(
        id,
        ProviderSourceKind::Development,
        format!("/conformance/catalog/{id}"),
        suite.clock_now(),
    )
}

/// Inserts, reads by location, and the two unique rules of a provider
/// source; a provider revision may name one and must name a stored one.
pub(super) fn provider_sources(suite: &Suite<'_>) {
    let source = fresh_source(suite);
    let resource = ResourceRef::ProviderSource(source.id);

    let mut at_two = source.clone();
    at_two.revision = rev(2);
    suite.rejected(
        "a provider source is inserted at revision 1",
        batch(vec![WriteOp::InsertProviderSource(at_two)]),
        at(
            0,
            RegistryError::InvalidRevision {
                resource: resource.clone(),
                expected: rev(1),
                actual: rev(2),
            },
        ),
    );
    expect_eq(
        "a source that is not stored reads as none",
        suite.value(
            "read a source that is not stored",
            suite
                .registry
                .provider_source_by_location(source.kind, &source.location),
        ),
        None,
    );
    let committed = suite.commit(
        "insert a provider source",
        vec![WriteOp::InsertProviderSource(source.clone())],
    );
    expect_eq(
        "an inserted provider source returns its revision",
        committed.revisions,
        vec![(resource.clone(), rev(1))],
    );
    expect_eq(
        "a provider source reads back by kind and location",
        suite.value(
            "read the provider source",
            suite
                .registry
                .provider_source_by_location(source.kind, &source.location),
        ),
        Some(source.clone()),
    );
    expect_eq(
        "the same location under another kind is another source",
        suite.value(
            "read the other kind",
            suite
                .registry
                .provider_source_by_location(ProviderSourceKind::Bundled, &source.location),
        ),
        None,
    );

    suite.rejected(
        "a provider source ID is unique",
        batch(vec![WriteOp::InsertProviderSource(source.clone())]),
        at(0, RegistryError::DuplicateResource { resource }),
    );
    let mut twin = fresh_source(suite);
    twin.location.clone_from(&source.location);
    suite.rejected(
        "a provider source kind and location are unique, and the error names the holder",
        batch(vec![WriteOp::InsertProviderSource(twin)]),
        at(
            0,
            RegistryError::DuplicateResource {
                resource: ResourceRef::ProviderSource(source.id),
            },
        ),
    );
    let mut bundled = fresh_source(suite);
    bundled.kind = ProviderSourceKind::Bundled;
    bundled.location.clone_from(&source.location);
    suite.commit(
        "the same location under another kind is allowed",
        vec![WriteOp::InsertProviderSource(bundled)],
    );

    let missing = ProviderSourceId::generate();
    let record = ProviderRevisionBuilder::new().record();
    let key: ProviderRevisionKey = record.revision.key().clone();
    suite.rejected(
        "a provider revision names a source that is stored",
        batch(vec![WriteOp::InsertProviderRevision(
            record.clone().with_source_id(missing),
        )]),
        at(
            0,
            RegistryError::MissingReference {
                resource: ResourceRef::ProviderSource(missing),
            },
        ),
    );
    suite.commit(
        "a provider revision records its source",
        vec![WriteOp::InsertProviderRevision(
            record.clone().with_source_id(source.id),
        )],
    );
    expect_eq(
        "a provider revision reads back with its source",
        suite
            .value(
                "read the provider revision",
                suite.registry.provider_revision(&key),
            )
            .map(|stored| stored.source_id),
        Some(Some(source.id)),
    );
    suite.rejected(
        "a duplicate provider revision is refused before its source is checked",
        batch(vec![WriteOp::InsertProviderRevision(
            record.with_source_id(missing),
        )]),
        at(
            0,
            RegistryError::DuplicateResource {
                resource: ResourceRef::ProviderRevision(Box::new(key)),
            },
        ),
    );
    let plain = ProviderRevisionBuilder::new().record();
    let plain_key = plain.revision.key().clone();
    suite.commit(
        "a provider revision may have no source",
        vec![WriteOp::InsertProviderRevision(plain)],
    );
    expect_eq(
        "a provider revision without a source reads back without one",
        suite
            .value(
                "read the plain provider revision",
                suite.registry.provider_revision(&plain_key),
            )
            .map(|stored| stored.source_id),
        Some(None),
    );
}

fn reference(label: &str) -> SecretRef {
    domain(
        "make a reference",
        SecretRef::parse(&format!(
            "local:conformance/{}/{label}",
            suffix(OperationId::generate())
        )),
    )
}

/// `DeleteInstanceCreatedBy`: the checks in order, then everything that
/// goes with the instance goes, and what it held is free again.
pub(super) fn delete_created_instance(suite: &Suite<'_>) {
    let stack = suite.stack("store a stack for the delete scenarios");
    let instance = Suite::instance(&stack).build();
    let id = instance.id();
    let operation = instance.created_by();
    let resource = ResourceRef::Instance(id);

    let secret = reference("delete");
    let secret_record = SecretReferenceRecord::new(
        SecretReferenceId::generate(),
        secret.clone(),
        domain(
            "make a store key",
            PlatformStoreKey::new(format!("fake:stackly-secret/{}", secret.path())),
        ),
        SecretOwner::User,
        SecretPurpose::user(),
        suite.clock_now(),
    );
    let config = ConfigRevisionRecord::first(
        ConfigRevisionId::generate(),
        id,
        domain(
            "make config values",
            ConfigValuesJson::new(r#"{"token":"x"}"#.to_owned()),
        ),
        Some(operation),
        suite.clock_now(),
    );
    let uses: BTreeMap<SecretFieldName, SecretRef> =
        BTreeMap::from([(parse("make a field name", "token"), secret.clone())]);
    suite.commit(
        "store an instance with a config revision and a secret use",
        vec![
            WriteOp::InsertSecretReference(secret_record),
            WriteOp::InsertInstance(instance.clone()),
            WriteOp::InsertConfigRevision(config),
            WriteOp::ReplaceSecretUses {
                user: SecretUser::Instance(id),
                uses,
            },
        ],
    );

    let delete = |expected, operation| WriteOp::DeleteInstanceCreatedBy {
        id,
        expected,
        operation,
    };
    let unknown = stackly_domain::InstanceId::generate();
    suite.rejected(
        "an instance that is not stored cannot be deleted",
        batch(vec![WriteOp::DeleteInstanceCreatedBy {
            id: unknown,
            expected: rev(1),
            operation,
        }]),
        at(
            0,
            RegistryError::NotFound {
                resource: ResourceRef::Instance(unknown),
            },
        ),
    );
    suite.rejected(
        "the revision is checked before the owner",
        batch(vec![delete(rev(2), OperationId::generate())]),
        at(
            0,
            RegistryError::RevisionConflict {
                resource: resource.clone(),
                expected: rev(2),
                actual: rev(1),
            },
        ),
    );
    suite.rejected(
        "an instance another operation created cannot be deleted",
        batch(vec![delete(rev(1), OperationId::generate())]),
        at(
            0,
            RegistryError::NotDeletable {
                resource: resource.clone(),
                reason: NotDeletableReason::CreatedByOtherOperation,
            },
        ),
    );

    let running = Suite::instance(&stack)
        .lifecycle(InstanceLifecycle::Running)
        .build();
    suite.commit(
        "store a running instance",
        vec![WriteOp::InsertInstance(running.clone())],
    );
    suite.rejected(
        "an instance that may be running cannot be deleted",
        batch(vec![WriteOp::DeleteInstanceCreatedBy {
            id: running.id(),
            expected: rev(1),
            operation: running.created_by(),
        }]),
        at(
            0,
            RegistryError::NotDeletable {
                resource: ResourceRef::Instance(running.id()),
                reason: NotDeletableReason::Active,
            },
        ),
    );

    let committed = suite.commit(
        "delete the instance its operation created",
        vec![delete(rev(1), operation)],
    );
    expect_eq(
        "a delete returns no revision",
        committed.revisions,
        Vec::new(),
    );
    expect_eq(
        "the instance is gone",
        suite.value("read the instance", suite.registry.instance(id)),
        None,
    );
    expect_eq(
        "its paths are gone",
        suite.value("read its paths", suite.registry.instance_paths(id)),
        None,
    );
    expect_eq(
        "its secret uses are gone",
        suite.value(
            "read its secret uses",
            suite.registry.secret_uses_by(SecretUser::Instance(id)),
        ),
        Vec::new(),
    );
    expect_eq(
        "its config revision is gone",
        suite.registry.active_config_revision(id).map(|_| ()),
        Err(RegistryError::NotFound {
            resource: resource.clone(),
        }),
    );
    suite.commit(
        "the secret reference can be deleted once nothing uses it",
        vec![WriteOp::DeleteSecretReference {
            reference: secret,
            expected: rev(1),
        }],
    );
    suite.commit(
        "the same instance can be stored again",
        vec![WriteOp::InsertInstance(instance)],
    );
}
