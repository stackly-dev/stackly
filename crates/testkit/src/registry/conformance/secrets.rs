//! Secret and audit scenarios of spec 0005 "Tambahan fitur 11": the seven
//! ops in their check order, observations that leave the revision, a
//! delete refused with the full list of uses, a restore at revision 1
//! refused, uses replaced whole, and audit pages.

use std::collections::BTreeMap;

use jiff::Timestamp;
use stackly_domain::{
    AuditAction, AuditActor, AuditCursor, AuditEventId, AuditEventRecord, AuditQuery,
    AuditResource, AuditResult, DetailKey, InstanceId, OperationId, PlatformStoreKey,
    RegistryError, ResourceRef, SafeDetails, SecretFieldName, SecretOrigin, SecretOwner,
    SecretOwnerKind, SecretPurpose, SecretRef, SecretReferenceFilter, SecretReferenceId,
    SecretReferenceRecord, SecretReferenceView, SecretStatus, SecretUse, SecretUser, WriteOp,
};

use super::{Suite, at, batch, domain, expect, expect_eq, fail, parse, rev, shifted, suffix};
use crate::registry::EnvironmentBuilder;

/// Runs every secret and audit scenario.
pub(super) fn secrets(suite: &Suite<'_>) {
    insert_and_restore(suite);
    update_and_observe(suite);
    uses_and_delete(suite);
    lists(suite);
    audit(suite);
}

/// A reference no other scenario or stored row uses.
fn fresh_reference(label: &str) -> SecretRef {
    let text = format!(
        "local:conformance/{}/{label}",
        suffix(OperationId::generate())
    );
    domain("make a reference", SecretRef::parse(&text))
}

fn store_key(reference: &SecretRef) -> PlatformStoreKey {
    domain(
        "make a store key",
        PlatformStoreKey::new(format!("fake:stackly-secret/{}", reference.path())),
    )
}

fn field(name: &str) -> SecretFieldName {
    parse("make a field name", name)
}

/// A new row owned by the user, as `secret.set` registers one, with a
/// stored value.
fn user_record(suite: &Suite<'_>, reference: &SecretRef) -> SecretReferenceRecord {
    let mut record = SecretReferenceRecord::new(
        SecretReferenceId::generate(),
        reference.clone(),
        store_key(reference),
        SecretOwner::User,
        SecretPurpose::user(),
        suite.loaded_at,
    );
    record.status = SecretStatus::Present;
    record.status_observed_at = Some(suite.loaded_at);
    record.value_origin = Some(SecretOrigin::Entered);
    record.value_updated_at = Some(suite.loaded_at);
    record
}

/// Reads one reference row, which must exist.
fn view(suite: &Suite<'_>, check: &str, reference: &SecretRef) -> SecretReferenceView {
    suite
        .value(check, suite.registry.secret_reference(reference))
        .unwrap_or_else(|| fail(check, "the reference row is missing"))
}

/// Inserts are at revision 1 and new by ID and reference; the owner is not
/// checked; a restore needs revision 2 or more and a free ID and
/// reference.
fn insert_and_restore(suite: &Suite<'_>) {
    let reference = fresh_reference("insert");
    let record = user_record(suite, &reference);
    let resource = ResourceRef::SecretReference(reference.clone());

    let mut at_two = record.clone();
    at_two.revision = rev(2);
    suite.rejected(
        "a reference is inserted at revision 1",
        batch(vec![WriteOp::InsertSecretReference(at_two)]),
        at(
            0,
            RegistryError::InvalidRevision {
                resource: resource.clone(),
                expected: rev(1),
                actual: rev(2),
            },
        ),
    );
    let committed = suite.commit(
        "insert a reference",
        vec![WriteOp::InsertSecretReference(record.clone())],
    );
    expect_eq(
        "an inserted reference returns its revision",
        committed.revisions,
        vec![(resource.clone(), rev(1))],
    );
    expect_eq(
        "an inserted reference reads back with no uses",
        view(suite, "read the inserted reference", &reference),
        SecretReferenceView::new(record.clone(), Vec::new()),
    );

    let mut same_reference = user_record(suite, &reference);
    same_reference.purpose = parse("make a purpose", "other");
    suite.rejected(
        "a reference text is unique",
        batch(vec![WriteOp::InsertSecretReference(same_reference)]),
        at(
            0,
            RegistryError::DuplicateResource {
                resource: resource.clone(),
            },
        ),
    );
    let other = fresh_reference("same-id");
    let mut same_id = user_record(suite, &other);
    same_id.id = record.id;
    suite.rejected(
        "a reference ID is unique",
        batch(vec![WriteOp::InsertSecretReference(same_id)]),
        at(
            0,
            RegistryError::DuplicateResource {
                resource: ResourceRef::SecretReference(other),
            },
        ),
    );

    let owned = fresh_reference("owned");
    let missing_owner = InstanceId::generate();
    let mut instance_owned = user_record(suite, &owned);
    instance_owned.owner = SecretOwner::Instance(missing_owner);
    instance_owned.purpose = parse("make a purpose", "admin-password");
    instance_owned.status = SecretStatus::Missing;
    instance_owned.status_observed_at = None;
    instance_owned.value_origin = None;
    instance_owned.value_updated_at = None;
    suite.commit(
        "a reference may name an owner the registry does not hold yet",
        vec![WriteOp::InsertSecretReference(instance_owned.clone())],
    );
    expect_eq(
        "an instance owned reference reads back with its owner",
        view(suite, "read the owned reference", &owned).record,
        instance_owned,
    );

    let at_one = fresh_reference("restore-one");
    suite.rejected(
        "a restore needs revision 2 or more",
        batch(vec![WriteOp::RestoreSecretReference(user_record(
            suite, &at_one,
        ))]),
        at(
            0,
            RegistryError::InvalidRevision {
                resource: ResourceRef::SecretReference(at_one),
                expected: rev(2),
                actual: rev(1),
            },
        ),
    );
    let mut restored = record.clone();
    restored.revision = rev(2);
    suite.rejected(
        "a restore needs a free reference",
        batch(vec![WriteOp::RestoreSecretReference(restored.clone())]),
        at(0, RegistryError::DuplicateResource { resource }),
    );
    suite.commit(
        "delete the reference to restore it",
        vec![WriteOp::DeleteSecretReference {
            reference: reference.clone(),
            expected: rev(1),
        }],
    );
    expect(
        "a deleted reference reads as absent",
        suite
            .value(
                "read the deleted reference",
                suite.registry.secret_reference(&reference),
            )
            .is_none(),
        || "the row is still there".to_owned(),
    );
    let committed = suite.commit(
        "restore the deleted reference",
        vec![WriteOp::RestoreSecretReference(restored.clone())],
    );
    expect_eq(
        "a restore returns its revision",
        committed.revisions,
        vec![(ResourceRef::SecretReference(reference.clone()), rev(2))],
    );
    expect_eq(
        "a restored reference reads back as it was, at its new revision",
        view(suite, "read the restored reference", &reference).record,
        restored,
    );
}

/// Updates write only the value columns, with compare and swap; an
/// observation writes the status, its time, and the store key, and
/// nothing else.
fn update_and_observe(suite: &Suite<'_>) {
    let reference = fresh_reference("update");
    let record = user_record(suite, &reference);
    let resource = ResourceRef::SecretReference(reference.clone());
    suite.commit(
        "insert a reference to update",
        vec![WriteOp::InsertSecretReference(record.clone())],
    );

    let later = shifted(suite.loaded_at, 60);
    let mut changed = record.clone();
    changed.revision = rev(2);
    changed.value_origin = Some(SecretOrigin::Generated);
    changed.value_updated_at = Some(later);
    changed.status_observed_at = Some(later);
    changed.updated_at = later;
    changed.platform_store_key = domain(
        "make a store key",
        PlatformStoreKey::new(format!("fake:moved/{}", reference.path())),
    );
    let mut wrong_shape = changed.clone();
    wrong_shape.revision = rev(3);
    wrong_shape.reference = fresh_reference("nowhere");
    suite.rejected(
        "an update carries expected + 1, checked before the row is looked up",
        batch(vec![WriteOp::UpdateSecretReference {
            record: wrong_shape.clone(),
            expected: rev(1),
        }]),
        at(
            0,
            RegistryError::InvalidRevision {
                resource: ResourceRef::SecretReference(wrong_shape.reference),
                expected: rev(2),
                actual: rev(3),
            },
        ),
    );
    let unknown = fresh_reference("unknown");
    let mut elsewhere = changed.clone();
    elsewhere.reference = unknown.clone();
    suite.rejected(
        "an update of an unknown reference is not found",
        batch(vec![WriteOp::UpdateSecretReference {
            record: elsewhere,
            expected: rev(1),
        }]),
        at(
            0,
            RegistryError::NotFound {
                resource: ResourceRef::SecretReference(unknown),
            },
        ),
    );
    let mut stale = changed.clone();
    stale.revision = rev(3);
    suite.rejected(
        "an update at a stale revision conflicts",
        batch(vec![WriteOp::UpdateSecretReference {
            record: stale,
            expected: rev(2),
        }]),
        at(
            0,
            RegistryError::RevisionConflict {
                resource: resource.clone(),
                expected: rev(2),
                actual: rev(1),
            },
        ),
    );

    // The ID, owner, purpose, and creation time are not the update's.
    let mut carried = changed.clone();
    carried.id = SecretReferenceId::generate();
    carried.owner = SecretOwner::Instance(InstanceId::generate());
    carried.purpose = parse("make a purpose", "changed");
    carried.created_at = later;
    let committed = suite.commit(
        "update a reference",
        vec![WriteOp::UpdateSecretReference {
            record: carried,
            expected: rev(1),
        }],
    );
    expect_eq(
        "an update returns its revision",
        committed.revisions,
        vec![(resource.clone(), rev(2))],
    );
    expect_eq(
        "an update writes only the value columns",
        view(suite, "read the updated reference", &reference).record,
        changed.clone(),
    );

    let observed_at = shifted(suite.loaded_at, 120);
    let observed_key = store_key(&reference);
    let unknown = fresh_reference("unobserved");
    suite.rejected(
        "an observation of an unknown reference is not found",
        batch(vec![WriteOp::ObserveSecretReference {
            reference: unknown.clone(),
            status: SecretStatus::Missing,
            status_observed_at: observed_at,
            platform_store_key: observed_key.clone(),
        }]),
        at(
            0,
            RegistryError::NotFound {
                resource: ResourceRef::SecretReference(unknown),
            },
        ),
    );
    let before = suite.generation();
    let committed = suite.commit(
        "observe a reference",
        vec![WriteOp::ObserveSecretReference {
            reference: reference.clone(),
            status: SecretStatus::Unknown,
            status_observed_at: observed_at,
            platform_store_key: observed_key.clone(),
        }],
    );
    expect_eq(
        "an observation returns no revision",
        committed.revisions,
        Vec::new(),
    );
    expect_eq(
        "an observation is a commit like any other",
        committed.generation,
        before.next(),
    );
    let mut observed = changed;
    observed.status = SecretStatus::Unknown;
    observed.status_observed_at = Some(observed_at);
    observed.platform_store_key = observed_key;
    expect_eq(
        "an observation leaves the revision and updated_at",
        view(suite, "read the observed reference", &reference).record,
        observed,
    );
}

/// Uses are replaced whole for one user, need their user and references,
/// and block a delete, which lists every one of them in order.
fn uses_and_delete(suite: &Suite<'_>) {
    let stack = suite.stack("store a runtime for secret uses");
    let instance = Suite::instance(&stack).build();
    let environment = EnvironmentBuilder::new().build();
    suite.commit(
        "store a user of each kind",
        vec![
            WriteOp::InsertInstance(instance.clone()),
            WriteOp::InsertEnvironment(environment.clone()),
        ],
    );
    let admin = fresh_reference("admin");
    let replica = fresh_reference("replica");
    suite.commit(
        "insert references to use",
        vec![
            WriteOp::InsertSecretReference(user_record(suite, &admin)),
            WriteOp::InsertSecretReference(user_record(suite, &replica)),
        ],
    );
    let on_instance = SecretUser::Instance(instance.id());
    let on_environment = SecretUser::Environment(environment.id());

    let missing_instance = SecretUser::Instance(InstanceId::generate());
    suite.rejected(
        "uses need their instance",
        batch(vec![WriteOp::ReplaceSecretUses {
            user: missing_instance,
            uses: BTreeMap::from([(field("adminPassword"), admin.clone())]),
        }]),
        at(
            0,
            RegistryError::MissingReference {
                resource: ResourceRef::Instance(instance_of(missing_instance)),
            },
        ),
    );
    let missing_environment = EnvironmentBuilder::new().build();
    suite.rejected(
        "uses need their environment",
        batch(vec![WriteOp::ReplaceSecretUses {
            user: SecretUser::Environment(missing_environment.id()),
            uses: BTreeMap::new(),
        }]),
        at(
            0,
            RegistryError::MissingReference {
                resource: ResourceRef::Environment(missing_environment.id()),
            },
        ),
    );
    let unknown = fresh_reference("unknown-use");
    suite.rejected(
        "uses need their references",
        batch(vec![WriteOp::ReplaceSecretUses {
            user: on_instance,
            uses: BTreeMap::from([
                (field("adminPassword"), admin.clone()),
                (field("replicaPassword"), unknown.clone()),
            ]),
        }]),
        at(
            0,
            RegistryError::MissingReference {
                resource: ResourceRef::SecretReference(unknown),
            },
        ),
    );
    expect_eq(
        "a refused batch leaves no uses",
        suite.value("read the uses", suite.registry.secret_uses_by(on_instance)),
        Vec::new(),
    );

    let later = Suite::instance(&stack).build();
    let later_reference = fresh_reference("same-batch");
    let committed = suite.commit(
        "uses may follow their instance and reference in the same batch",
        vec![
            WriteOp::InsertSecretReference(user_record(suite, &later_reference)),
            WriteOp::InsertInstance(later.clone()),
            WriteOp::ReplaceSecretUses {
                user: SecretUser::Instance(later.id()),
                uses: BTreeMap::from([(field("token"), later_reference.clone())]),
            },
        ],
    );
    expect_eq(
        "only the reference and the instance return a revision, not the uses",
        committed.revisions.len(),
        2,
    );

    suite.commit(
        "replace the uses of an instance",
        vec![WriteOp::ReplaceSecretUses {
            user: on_instance,
            uses: BTreeMap::from([
                (field("replicaPassword"), replica.clone()),
                (field("adminPassword"), admin.clone()),
                (field("monitorPassword"), replica.clone()),
            ]),
        }],
    );
    suite.commit(
        "replace them again, whole",
        vec![WriteOp::ReplaceSecretUses {
            user: on_instance,
            uses: BTreeMap::from([
                (field("adminPassword"), admin.clone()),
                (field("replicaPassword"), admin.clone()),
            ]),
        }],
    );
    suite.commit(
        "an environment uses the same reference",
        vec![WriteOp::ReplaceSecretUses {
            user: on_environment,
            uses: BTreeMap::from([(field("db_password"), admin.clone())]),
        }],
    );
    let instance_uses = vec![
        SecretUse::new(admin.clone(), on_instance, field("adminPassword")),
        SecretUse::new(admin.clone(), on_instance, field("replicaPassword")),
    ];
    expect_eq(
        "a replace keeps only the new uses, ordered by field",
        suite.value(
            "read the instance's uses",
            suite.registry.secret_uses_by(on_instance),
        ),
        instance_uses.clone(),
    );
    expect_eq(
        "a resource without uses has none",
        suite.value(
            "read the uses of an unknown instance",
            suite.registry.secret_uses_by(missing_instance),
        ),
        Vec::new(),
    );
    let mut used_by = vec![SecretUse::new(
        admin.clone(),
        on_environment,
        field("db_password"),
    )];
    used_by.extend(instance_uses);
    expect_eq(
        "a view lists uses with environments before instances",
        view(suite, "read the used reference", &admin).used_by,
        used_by.clone(),
    );
    expect_eq(
        "a reference that lost its uses lists none",
        view(suite, "read the unused reference", &replica).used_by,
        Vec::new(),
    );

    let admin_resource = ResourceRef::SecretReference(admin.clone());
    let unknown = fresh_reference("undeleted");
    suite.rejected(
        "a delete of an unknown reference is not found",
        batch(vec![WriteOp::DeleteSecretReference {
            reference: unknown.clone(),
            expected: rev(1),
        }]),
        at(
            0,
            RegistryError::NotFound {
                resource: ResourceRef::SecretReference(unknown),
            },
        ),
    );
    suite.rejected(
        "a delete checks the revision before the uses",
        batch(vec![WriteOp::DeleteSecretReference {
            reference: admin.clone(),
            expected: rev(2),
        }]),
        at(
            0,
            RegistryError::RevisionConflict {
                resource: admin_resource,
                expected: rev(2),
                actual: rev(1),
            },
        ),
    );
    suite.rejected(
        "a used reference cannot be deleted, and every use is named",
        batch(vec![WriteOp::DeleteSecretReference {
            reference: admin.clone(),
            expected: rev(1),
        }]),
        at(
            0,
            RegistryError::ReferenceInUse {
                reference: admin.clone(),
                used_by,
            },
        ),
    );
    suite.rejected(
        "a use added earlier in the same batch blocks a delete",
        batch(vec![
            WriteOp::ReplaceSecretUses {
                user: SecretUser::Instance(later.id()),
                uses: BTreeMap::from([(field("token"), replica.clone())]),
            },
            WriteOp::DeleteSecretReference {
                reference: replica.clone(),
                expected: rev(1),
            },
        ]),
        at(
            1,
            RegistryError::ReferenceInUse {
                reference: replica.clone(),
                used_by: vec![SecretUse::new(
                    replica.clone(),
                    SecretUser::Instance(later.id()),
                    field("token"),
                )],
            },
        ),
    );
    let committed = suite.commit(
        "empty maps delete every use",
        vec![
            WriteOp::ReplaceSecretUses {
                user: on_instance,
                uses: BTreeMap::new(),
            },
            WriteOp::ReplaceSecretUses {
                user: on_environment,
                uses: BTreeMap::new(),
            },
            WriteOp::DeleteSecretReference {
                reference: admin.clone(),
                expected: rev(1),
            },
        ],
    );
    expect_eq(
        "uses and deletes return no revision",
        committed.revisions,
        Vec::new(),
    );
    expect(
        "an unused reference is deleted",
        suite
            .value(
                "read the deleted reference",
                suite.registry.secret_reference(&admin),
            )
            .is_none(),
        || "the row is still there".to_owned(),
    );
}

fn instance_of(user: SecretUser) -> InstanceId {
    match user {
        SecretUser::Instance(id) => id,
        SecretUser::Environment(_) => fail("pick an instance user", "an environment"),
    }
}

/// The list reads by owner kind and status, in reference order.
fn lists(suite: &Suite<'_>) {
    let tag = suffix(OperationId::generate());
    let reference = |label: &str| {
        domain(
            "make a reference",
            SecretRef::parse(&format!("local:conformance-list/{tag}/{label}")),
        )
    };
    let (b, a, c) = (reference("b"), reference("a"), reference("c"));
    let present_user = user_record(suite, &b);
    let mut missing_user = user_record(suite, &a);
    missing_user.status = SecretStatus::Missing;
    let mut instance = user_record(suite, &c);
    instance.owner = SecretOwner::Instance(InstanceId::generate());
    instance.purpose = parse("make a purpose", "admin-password");
    suite.commit(
        "insert references to list",
        vec![
            WriteOp::InsertSecretReference(present_user.clone()),
            WriteOp::InsertSecretReference(missing_user.clone()),
            WriteOp::InsertSecretReference(instance.clone()),
        ],
    );
    let listed = |check: &str, filter: SecretReferenceFilter| -> Vec<SecretRef> {
        suite
            .value(check, suite.registry.secret_references(filter))
            .into_iter()
            .map(|view| view.record.reference)
            .filter(|listed| listed.as_str().starts_with("local:conformance-list/"))
            .filter(|listed| listed.as_str().contains(&tag))
            .collect()
    };
    expect_eq(
        "every reference is listed in reference order",
        listed("list every reference", SecretReferenceFilter::default()),
        vec![a.clone(), b.clone(), c.clone()],
    );
    expect_eq(
        "a list filters by owner kind",
        listed(
            "list user references",
            SecretReferenceFilter {
                owner_kind: Some(SecretOwnerKind::User),
                status: None,
            },
        ),
        vec![a.clone(), b.clone()],
    );
    expect_eq(
        "a list filters by status",
        listed(
            "list present user references",
            SecretReferenceFilter {
                owner_kind: Some(SecretOwnerKind::User),
                status: Some(SecretStatus::Present),
            },
        ),
        vec![b],
    );
    let all = suite.value(
        "list every reference",
        suite
            .registry
            .secret_references(SecretReferenceFilter::default()),
    );
    let mut sorted: Vec<SecretRef> = all
        .iter()
        .map(|view| view.record.reference.clone())
        .collect();
    sorted.sort();
    expect_eq(
        "the whole list is in reference order",
        all.into_iter()
            .map(|view| view.record.reference)
            .collect::<Vec<_>>(),
        sorted,
    );
}

/// Audit rows are new by ID, read back whole, and page from the newest
/// `(occurred_at, id)` down.
fn audit(suite: &Suite<'_>) {
    // Far ahead of any other row, so these are the newest pages.
    let base = domain(
        "make an audit time",
        "2200-01-01T00:00:00Z".parse::<Timestamp>(),
    );
    let detail_key: DetailKey = parse("make a detail key", "reference");
    let mut rows = Vec::new();
    for n in 0..5_i64 {
        // Rows 3 and 4 share their time; their IDs order them.
        let occurred_at = shifted(base, n.min(3));
        let mut record = AuditEventRecord::new(
            AuditEventId::generate(),
            if n % 2 == 0 {
                AuditActor::User
            } else {
                AuditActor::Daemon
            },
            AuditAction::from_static("secret.reveal"),
            if n == 4 {
                AuditResult::Failed
            } else {
                AuditResult::Succeeded
            },
            occurred_at,
        );
        if n == 1 {
            record.resource = Some(AuditResource::SecretReference(SecretReferenceId::generate()));
            record.operation = Some(OperationId::generate());
            record.safe_details = domain(
                "make audit details",
                SafeDetails::new([
                    (detail_key.clone(), "local:erp/admin".to_owned()),
                    (parse("make a detail key", "clientKind"), "cli".to_owned()),
                    (parse("make a detail key", "errorCode"), String::new()),
                ]),
            );
        }
        rows.push(record);
    }
    let ops = rows
        .iter()
        .rev()
        .cloned()
        .map(WriteOp::InsertAuditEvent)
        .collect();
    let committed = suite.commit("insert audit rows", ops);
    expect_eq(
        "audit rows return no revision",
        committed.revisions,
        Vec::new(),
    );
    suite.rejected(
        "an audit row ID is unique",
        batch(vec![WriteOp::InsertAuditEvent(rows[0].clone())]),
        at(
            0,
            RegistryError::DuplicateResource {
                resource: ResourceRef::AuditEvent(rows[0].id),
            },
        ),
    );

    let page = |check: &str, limit: u16, before: Option<AuditCursor>| {
        suite.value(
            check,
            suite.registry.audit_events(AuditQuery { limit, before }),
        )
    };
    let first = page("read the first audit page", 2, None);
    expect_eq(
        "the first page holds the newest rows, ties broken by ID",
        first.items.clone(),
        vec![rows[4].clone(), rows[3].clone()],
    );
    expect_eq(
        "a full page points at its last row",
        first.next,
        Some(AuditCursor::of(&rows[3])),
    );
    let second = page("read the second audit page", 2, first.next);
    expect_eq(
        "the next page starts after the cursor and reads every field back",
        second.items,
        vec![rows[2].clone(), rows[1].clone()],
    );
    let rest = page("read the rest", 500, second.next);
    expect_eq(
        "the last page goes on from the cursor",
        rest.items.first(),
        Some(&rows[0]),
    );
    expect_eq(
        "a page that is not full is the last",
        rest.next.is_none(),
        rest.items.len() < 500,
    );
    for limit in [0, AuditQuery::LIMIT_MAX + 1] {
        let check = format!("an audit page limit of {limit} is an invalid query");
        match suite.registry.audit_events(AuditQuery {
            limit,
            before: None,
        }) {
            Err(err) => expect(
                &check,
                matches!(err, RegistryError::InvalidQuery { .. }),
                || format!("{err:?}"),
            ),
            Ok(page) => fail(&check, &format!("{} rows", page.value.items.len())),
        }
    }
}
