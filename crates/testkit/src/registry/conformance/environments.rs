//! Environment manifests (spec 0044 step 1): the snapshot and location
//! read back, names and locations are unique, a guard reads every
//! environment, and `DeleteEnvironment` refuses an active lease, then
//! keeps instances and moves owned secret references to the user.

use stackly_domain::{
    BatchError, DeclaredService, DependencyCondition, DependencyEdge, GuardDecision, Guarded,
    LeaseFilter, NotDeletableReason, Ownership, RegistryError, ResourceRef, SecretOwner,
    SecretUser, WriteBatch, WriteOp,
};

use super::secrets::{fresh_reference, user_record, view};
use super::{Suite, at, batch, expect, expect_eq, fail, parse, rev};
use crate::registry::{EnvironmentBuilder, LeaseBuilder};

pub(super) fn environments(suite: &Suite<'_>) {
    snapshot_and_uniqueness(suite);
    delete(suite);
}

fn service(alias: &str, depends_on: &[&str]) -> DeclaredService {
    DeclaredService::new(
        parse("make an alias", alias),
        parse("make a provider", "official/postgresql"),
        Ownership::Dedicated,
        Some(parse("make a selector", "18")),
        None,
        Some(parse("make an instance name", &format!("{alias}-pg"))),
        depends_on
            .iter()
            .map(|dep| {
                DependencyEdge::new(
                    parse("make an alias", dep),
                    DependencyCondition::Ready,
                    Some(30),
                )
            })
            .collect(),
    )
}

/// Expects `batch` to fail at op 0 with an `Integrity` error.
fn integrity(suite: &Suite<'_>, check: &str, batch: WriteBatch) {
    match suite.registry.commit(batch) {
        Ok(_) => fail(check, "the commit succeeded, expected Integrity"),
        Err(BatchError {
            op_index, error, ..
        }) => expect(
            check,
            op_index == Some(0) && matches!(error, RegistryError::Integrity { .. }),
            || format!("expected Integrity at op 0, got {error:?} at {op_index:?}"),
        ),
    }
}

fn snapshot_and_uniqueness(suite: &Suite<'_>) {
    let name = format!(
        "manifest-{}",
        super::suffix(stackly_domain::OperationId::generate())
    );
    let location = format!("/Users/example/{name}/stackly.yaml");
    let shop = EnvironmentBuilder::new()
        .name(&name)
        .manifest_location(&location)
        .declared(service("api", &["db"]))
        .declared(service("db", &[]));
    suite.commit(
        "store an environment with a manifest snapshot",
        vec![WriteOp::InsertEnvironment(shop.clone().build())],
    );
    expect_eq(
        "the snapshot and location read back",
        suite.value(
            "read the environment",
            suite.registry.environment(shop.id()),
        ),
        Some(shop.clone().build()),
    );
    integrity(
        suite,
        "an environment name is unique",
        batch(vec![WriteOp::InsertEnvironment(
            EnvironmentBuilder::new().name(&name).build(),
        )]),
    );
    integrity(
        suite,
        "a manifest location is unique",
        batch(vec![WriteOp::InsertEnvironment(
            EnvironmentBuilder::new()
                .manifest_location(&location)
                .build(),
        )]),
    );
    let mut moved = shop.clone().build();
    moved.clear_manifest_location();
    suite.commit(
        "an environment can give up its location",
        vec![WriteOp::UpdateEnvironment {
            environment: moved.clone(),
            expected: rev(1),
        }],
    );
    let taker = EnvironmentBuilder::new().manifest_location(&location);
    suite.commit(
        "a freed location can be taken",
        vec![WriteOp::InsertEnvironment(taker.clone().build())],
    );

    let wanted = shop.id();
    let guard = Box::new(move |view: &dyn stackly_domain::GuardView| {
        let all = view.environments()?;
        if all.iter().any(|environment| environment.id() == wanted) {
            Ok(GuardDecision::Write(Vec::new()))
        } else {
            Ok(GuardDecision::Refuse(Box::new(())))
        }
    });
    match suite.registry.commit_guarded(guard) {
        Ok(Guarded::Committed(_)) => {}
        Ok(Guarded::Refused(_)) => fail("a guard reads every environment", "it saw none"),
        Err(err) => fail("a guard reads every environment", &err.to_string()),
    }
}

fn delete(suite: &Suite<'_>) {
    let stack = suite.stack("store a runtime for an environment delete");
    let instance = Suite::instance(&stack).build();
    let environment = EnvironmentBuilder::new().dedicated(instance.id());
    let lease = LeaseBuilder::new(instance.id()).environment(environment.id());
    let owned = fresh_reference("environment-owned");
    let mut record = user_record(suite, &owned);
    record.owner = SecretOwner::Environment(environment.id());
    suite.commit(
        "store an environment with a lease and a secret",
        vec![
            WriteOp::InsertInstance(instance.clone()),
            WriteOp::InsertEnvironment(environment.clone().build()),
            WriteOp::InsertLease(lease.clone().build()),
            WriteOp::InsertSecretReference(record),
            WriteOp::ReplaceSecretUses {
                user: SecretUser::Environment(environment.id()),
                uses: std::collections::BTreeMap::from([(
                    parse("make a field", "adminPassword"),
                    owned.clone(),
                )]),
            },
        ],
    );
    let resource = ResourceRef::Environment(environment.id());
    suite.rejected(
        "an environment with an active lease is not deleted",
        batch(vec![WriteOp::DeleteEnvironment {
            id: environment.id(),
            expected: rev(1),
        }]),
        at(
            0,
            RegistryError::NotDeletable {
                resource: resource.clone(),
                reason: NotDeletableReason::ActiveLease,
            },
        ),
    );
    suite.commit(
        "release the lease",
        vec![WriteOp::UpdateLease(
            lease.released(suite.clock_now()).build(),
        )],
    );
    suite.rejected(
        "a delete compares the revision",
        batch(vec![WriteOp::DeleteEnvironment {
            id: environment.id(),
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
    suite.commit(
        "delete the environment",
        vec![WriteOp::DeleteEnvironment {
            id: environment.id(),
            expected: rev(1),
        }],
    );
    expect_eq(
        "the environment is gone",
        suite.value(
            "read the environment",
            suite.registry.environment(environment.id()),
        ),
        None,
    );
    expect(
        "the instance stays",
        suite
            .value("read the instance", suite.registry.instance(instance.id()))
            .is_some(),
        || "the instance is gone".to_owned(),
    );
    expect_eq(
        "the released demand row is gone",
        suite
            .value(
                "read the leases",
                suite.registry.leases(instance.id(), LeaseFilter::All),
            )
            .len(),
        0,
    );
    let moved = view(suite, "read the moved reference", &owned);
    expect_eq(
        "an owned reference moves to the user",
        moved.record.owner,
        SecretOwner::User,
    );
    expect_eq(
        "the move raises its revision",
        moved.record.revision,
        rev(2),
    );
    expect_eq("its uses are gone", moved.used_by.len(), 0);
    suite.rejected(
        "a deleted environment is not found",
        batch(vec![WriteOp::DeleteEnvironment {
            id: environment.id(),
            expected: rev(1),
        }]),
        at(0, RegistryError::NotFound { resource }),
    );
}
