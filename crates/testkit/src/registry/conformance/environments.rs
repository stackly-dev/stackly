//! Environment manifests (spec 0044 step 1): the snapshot and location
//! read back, names and locations are unique, a guard reads every
//! environment, and `DeleteEnvironment` refuses an active lease, then
//! keeps instances and moves owned secret references to the user. Spec
//! 0046 step 2: the last operation, the last Up report, the members'
//! dependency conditions, and the write stamp read back.

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
    up_report(suite);
    delete(suite);
}

/// The last Up report, the last operation, and the dependency conditions
/// of a member read back as written, and every write moves the stamp.
fn up_report(suite: &Suite<'_>) {
    const CHECK: &str = "environment_up_report_round_trips";
    let stack = suite.stack("store a stack for the up report");
    let instance = Suite::instance(&stack).build();
    let builder = EnvironmentBuilder::new()
        .no_members()
        .dedicated(instance.id());
    suite.commit(
        "store an environment with one member",
        vec![
            WriteOp::InsertInstance(instance.clone()),
            WriteOp::InsertEnvironment(builder.clone().build()),
        ],
    );
    let read = |check: &str| {
        suite
            .value(check, suite.registry.environment(builder.id()))
            .unwrap_or_else(|| fail(CHECK, "the environment is gone"))
    };
    let first = read("read the new environment");
    expect(CHECK, first.updated_at().is_some(), || {
        "a stored environment has a write stamp".to_owned()
    });
    expect_eq(CHECK, first.last_up(), None);

    // Each mutation raises the revision by 1, and an update carries one
    // revision step, so one batch holds one update per mutation.
    let operation = stackly_domain::OperationId::generate();
    let mut next = first.clone();
    next.begin_up(operation, suite.clock_now());
    let begun = next.clone();
    let alias = next.members()[0].alias().clone();
    next.set_member_conditions(
        &alias,
        vec![stackly_domain::MemberDependency::new(
            parse("make an alias", "cache"),
            DependencyCondition::Ready,
            600,
        )],
    )
    .unwrap_or_else(|err| fail(CHECK, &err.to_string()));
    let conditioned = next.clone();
    let report = stackly_domain::UpReport::new(
        operation,
        stackly_domain::UpState::Failed,
        suite.clock_now(),
        Some(suite.clock_now()),
        vec![
            stackly_domain::ServiceReport::new(alias.clone(), stackly_domain::UpPhase::Failed)
                .with_stage(stackly_domain::UpStage::Start)
                .with_code(parse("make a code", "supervisor.readiness_timeout"))
                .with_operation(stackly_domain::OperationId::generate()),
            stackly_domain::ServiceReport::new(
                parse("make an alias", "api"),
                stackly_domain::UpPhase::Skipped,
            )
            .with_dependency(alias, stackly_domain::SkipReason::Failed),
            stackly_domain::ServiceReport::new(
                parse("make an alias", "web"),
                stackly_domain::UpPhase::NeedsSecret,
            )
            .with_references(vec![parse("make a reference", "local:shop/db/admin")]),
        ],
    );
    next.finish_up(report);
    suite.commit(
        "write the up report",
        vec![
            WriteOp::UpdateEnvironment {
                environment: begun.clone(),
                expected: first.revision(),
            },
            WriteOp::UpdateEnvironment {
                environment: conditioned.clone(),
                expected: begun.revision(),
            },
            WriteOp::UpdateEnvironment {
                environment: next.clone(),
                expected: conditioned.revision(),
            },
        ],
    );
    let stored = read("read the reported environment");
    expect_eq(CHECK, &stored, &next);
    expect(CHECK, stored.updated_at().is_some(), || {
        "an updated environment keeps a write stamp".to_owned()
    });
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
