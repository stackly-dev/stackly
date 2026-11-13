//! A create an environment Up starts, bound to its environment (spec 0046
//! AC-7, AC-12, AC-13): the `register` batch writes the instance, its
//! `dedicated` member with its dependency conditions, and its environment
//! lease together; compensation removes exactly those; and the binding
//! survives a rebuild from the journal input.

use stackly_core::instance::create::{
    CreateHooks, CreateWorkFactory, EnvironmentBinding, encode_bound_input, encode_input,
};
use stackly_core::operation::WorkFactory;
use stackly_domain::{
    DependencyCondition, EnvironmentId, Instance, InstanceId, JournalView, LeaseFilter,
    LeaseHolder, MemberDependency, NewJournal, OperationJournal, OperationKind, OperationResource,
    OperationState, OperationTrigger, Ownership, Registry, WriteBatch, WriteOp,
};
use stackly_platform::{ChildRole, ExitStatus};
use stackly_testkit::{
    EnvironmentBuilder, InstanceBuilder, LeaseBuilder, OperationBuilder, ProviderRevisionBuilder,
};

use crate::common::Rig;
use crate::harness::CreateRig;

/// Finishes the init of the create that runs next: the `n`th init helper,
/// with exit code `code`.
async fn end_init(rig: &Rig, n: usize, code: i32) {
    let step = rig.helper_spawn(ChildRole::InitStep, n).await;
    rig.processes()
        .exit(step.pid, Some(ExitStatus::Exited(code)));
}

/// An instance the user made, on a provider revision of its own, with a
/// name close to the ones the tests create.
fn user_instance(cr: &CreateRig, name: &str) -> (Instance, Vec<WriteOp>) {
    let provider = ProviderRevisionBuilder::new().software(cr.rig.runtime.software().as_str());
    let instance = InstanceBuilder::new(&cr.rig.runtime, provider.clone().build().key())
        .name(name)
        .build();
    (
        instance.clone(),
        vec![
            WriteOp::InsertProviderRevision(provider.record()),
            WriteOp::InsertInstance(instance),
        ],
    )
}

/// Stores an environment that already borrows `shared` with a lease, and
/// returns its ID.
fn environment_with(cr: &CreateRig, shared: &Instance, mut ops: Vec<WriteOp>) -> EnvironmentId {
    let environment = EnvironmentBuilder::new().no_members().shared(shared.id());
    let id = environment.id();
    ops.push(WriteOp::InsertEnvironment(environment.build()));
    ops.push(WriteOp::InsertLease(
        LeaseBuilder::new(shared.id()).environment(id).build(),
    ));
    cr.rig
        .registry
        .commit(WriteBatch::new(ops))
        .expect("the environment can be stored");
    id
}

fn binding(environment: EnvironmentId) -> EnvironmentBinding {
    EnvironmentBinding {
        environment,
        alias: "db".parse().expect("an alias"),
        conditions: vec![MemberDependency::new(
            "cache".parse().expect("an alias"),
            DependencyCondition::Ready,
            600,
        )],
    }
}

fn environment_leases(rig: &Rig, instance: InstanceId, environment: EnvironmentId) -> usize {
    rig.registry
        .leases(instance, LeaseFilter::All)
        .expect("it reads")
        .value
        .iter()
        .filter(|lease| {
            lease.holder() == LeaseHolder::Environment(environment) && lease.is_active()
        })
        .count()
}

// covers: AC-7, AC-13 (spec 0046)
#[tokio::test]
async fn a_bound_create_registers_its_instance_dedicated_member_and_lease_together() {
    let cr = CreateRig::new().await;
    let rig = &cr.rig;
    let (shared, ops) = user_instance(&cr, "orders-cache");
    let environment = environment_with(&cr, &shared, ops);
    let id = InstanceId::generate();

    let op = cr
        .begin_bound(id, cr.draft("orders-db"), binding(environment))
        .await
        .expect("accepted");
    // The init runs after `register`: by then the instance, its member, and
    // its lease are all stored, from one commit.
    let step = rig.helper_spawn(ChildRole::InitStep, 0).await;
    let stored = rig
        .registry
        .environment(environment)
        .expect("it reads")
        .value
        .expect("the environment");
    assert!(rig.registry.instance(id).expect("it reads").value.is_some());
    let member = stored
        .member(&"db".parse().expect("an alias"))
        .expect("the new member");
    assert_eq!(
        (member.instance_id(), member.ownership()),
        (id, Ownership::Dedicated)
    );
    assert_eq!(
        member.dependency_conditions(),
        binding(environment).conditions
    );
    assert_eq!(stored.revision().get(), 2, "one revision for the member");
    assert_eq!(environment_leases(rig, id, environment), 1);
    let desired = rig
        .registry
        .instance(id)
        .expect("it reads")
        .value
        .expect("the instance")
        .desired();
    rig.processes().exit(step.pid, Some(ExitStatus::Exited(0)));
    rig.wait_state(op, OperationState::Succeeded).await;

    assert_eq!(
        desired,
        stackly_domain::DesiredState::Stopped,
        "a lease does not change the desired state"
    );
    assert_eq!(environment_leases(rig, shared.id(), environment), 1);
}

// covers: AC-7, AC-12 (spec 0046)
#[tokio::test]
async fn a_bound_create_that_fails_removes_only_its_own_instance_member_and_lease() {
    let cr = CreateRig::new().await;
    let rig = &cr.rig;
    let (shared, mut ops) = user_instance(&cr, "orders-cache");
    let (lookalike, more) = user_instance(&cr, "orders-db-2");
    ops.extend(more);
    let environment = environment_with(&cr, &shared, ops);
    let before = rig
        .registry
        .environment(environment)
        .expect("it reads")
        .value
        .expect("the environment");
    let runtime_before = rig
        .registry
        .runtime(rig.runtime.id())
        .expect("it reads")
        .value;
    let id = InstanceId::generate();

    let op = cr
        .begin_bound(id, cr.draft("orders-db"), binding(environment))
        .await
        .expect("accepted");
    end_init(rig, 0, 1).await;
    let failed = rig.wait_state(op, OperationState::Failed).await;
    assert_eq!(Rig::failure_code(&failed), "supervisor.init_failed");

    assert!(rig.registry.instance(id).expect("it reads").value.is_none());
    assert_eq!(environment_leases(rig, id, environment), 0);
    let after = rig
        .registry
        .environment(environment)
        .expect("it reads")
        .value
        .expect("the environment stays");
    assert_eq!(
        after.members(),
        before.members(),
        "only the new member went"
    );
    assert_eq!(after.revision().get(), before.revision().get() + 2);
    assert_eq!(environment_leases(rig, shared.id(), environment), 1);
    for kept in [&shared, &lookalike] {
        assert_eq!(
            rig.registry
                .instance(kept.id())
                .expect("it reads")
                .value
                .as_ref(),
            Some(kept),
            "the user's instance {} is untouched",
            kept.name()
        );
    }
    assert_eq!(
        rig.registry
            .runtime(rig.runtime.id())
            .expect("it reads")
            .value,
        runtime_before
    );
}

// covers: AC-7 (spec 0046)
#[tokio::test]
async fn a_bound_create_of_an_environment_that_is_gone_fails_and_leaves_nothing() {
    let cr = CreateRig::new().await;
    let rig = &cr.rig;
    let id = InstanceId::generate();

    let op = cr
        .begin_bound(
            id,
            cr.draft("orders-db"),
            binding(EnvironmentId::generate()),
        )
        .await
        .expect("accepted");
    let failed = rig.wait_state(op, OperationState::Failed).await;

    assert_eq!(Rig::failure_code(&failed), "env.not_found");
    assert!(rig.registry.instance(id).expect("it reads").value.is_none());
    assert!(!rig.paths(id).root.exists(), "no instance folder");
}

// covers: AC-7 (spec 0046)
#[tokio::test]
async fn a_create_rebuilt_from_its_journal_input_keeps_its_binding() {
    let cr = CreateRig::new().await;
    let rig = &cr.rig;
    let (shared, ops) = user_instance(&cr, "orders-cache");
    let environment = environment_with(&cr, &shared, ops);
    let id = InstanceId::generate();
    let draft = cr.draft("orders-db");

    let unbound = encode_input(id, &draft).expect("it fits");
    assert_eq!(
        encode_bound_input(id, &draft, None).expect("it fits"),
        unbound,
        "without a binding the input is the one a client create writes"
    );
    let input = encode_bound_input(id, &draft, Some(&binding(environment))).expect("it fits");
    let operation = OperationBuilder::new()
        .kind(OperationKind::CreateInstance)
        .resource(OperationResource::Instance(id))
        .build();
    let journal = OperationJournal::new(NewJournal {
        operation_id: operation.id(),
        request: None,
        trigger: OperationTrigger::background(),
        resumable: false,
        input: input.clone(),
        expected_revision: None,
        parent: None,
    })
    .expect("a journal");
    let factory = CreateWorkFactory::new(cr.deps(CreateHooks::default()));
    let rebuilt = factory
        .rebuild(
            &operation,
            &JournalView::new(journal, Vec::new(), Vec::new(), Vec::new()),
        )
        .expect("a bound create rebuilds");

    let op = cr
        .begin_boxed(id, "orders-db".parse().expect("a name"), input, rebuilt)
        .await
        .expect("accepted");
    end_init(rig, 0, 0).await;
    rig.wait_state(op, OperationState::Succeeded).await;

    let stored = rig
        .registry
        .environment(environment)
        .expect("it reads")
        .value
        .expect("the environment");
    assert_eq!(
        stored
            .member(&"db".parse().expect("an alias"))
            .map(stackly_domain::EnvironmentMember::instance_id),
        Some(id)
    );
    assert_eq!(environment_leases(rig, id, environment), 1);
}
