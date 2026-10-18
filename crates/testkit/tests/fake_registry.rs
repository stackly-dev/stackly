//! Behavior of `FakeRegistry`: the shared `registry_conformance` suite,
//! fault injection, commit stamps from its clock, and seeding.
#![cfg(unix)]

use std::sync::Arc;

use jiff::{SignedDuration, Timestamp};
use stackly_domain::{
    BatchError, InstanceFilter, Registry, RegistryError, ResourceRef, RetentionPolicy, WriteBatch,
    WriteOp,
};
use stackly_platform::Clock;
use stackly_testkit::{
    BackupSeed, FakeClock, FakeRegistry, InstanceBuilder, ProviderRevisionBuilder, RegistrySeed,
    RuntimeBuilder, WrittenAt, registry_conformance,
};

fn fake() -> (FakeClock, FakeRegistry) {
    let clock = FakeClock::new();
    let registry = FakeRegistry::new(Arc::new(clock.clone()), clock.now());
    (clock, registry)
}

/// Stores a provider revision, its artifact, and a runtime.
fn store_runtime(registry: &FakeRegistry) -> (ProviderRevisionBuilder, RuntimeBuilder) {
    let provider = ProviderRevisionBuilder::new();
    let runtime = RuntimeBuilder::new();
    registry
        .commit(WriteBatch::new(vec![
            WriteOp::InsertProviderRevision(provider.clone().record()),
            WriteOp::InsertArtifact(runtime.artifact()),
            WriteOp::InsertRuntime(runtime.clone().build()),
        ]))
        .expect("an empty fake registry stores a new runtime");
    (provider, runtime)
}

#[test]
fn fake_registry_passes_the_conformance_suite() {
    let (clock, registry) = fake();

    registry_conformance(&registry, &registry, &clock);
}

#[test]
fn the_conformance_suite_passes_on_a_registry_that_already_holds_data() {
    let (clock, registry) = fake();
    store_runtime(&registry);
    registry.commit(WriteBatch::new(Vec::new())).unwrap();

    registry_conformance(&registry, &registry, &clock);
}

#[test]
#[should_panic(expected = "registry_conformance check failed")]
fn the_conformance_suite_fails_when_instances_read_back_at_another_open_time() {
    let clock = FakeClock::new();
    let opened_earlier = clock
        .now()
        .checked_sub(SignedDuration::from_secs(1))
        .unwrap();
    let registry = FakeRegistry::new(Arc::new(clock.clone()), opened_earlier);

    registry_conformance(&registry, &registry, &clock);
}

#[test]
fn an_injected_fault_fails_the_next_commit_only_and_applies_nothing() {
    let (_clock, registry) = fake();
    let runtime = RuntimeBuilder::new()
        .external("/conformance/external/fault")
        .build();
    registry.fail_next_commit(RegistryError::NoSpace);

    let failed = registry.commit(WriteBatch::new(vec![WriteOp::InsertRuntime(
        runtime.clone(),
    )]));
    let retried = registry.commit(WriteBatch::new(vec![WriteOp::InsertRuntime(
        runtime.clone(),
    )]));

    assert_eq!(failed, Err(BatchError::batch(RegistryError::NoSpace)));
    assert_eq!(retried.unwrap().generation.get(), 1);
    assert_eq!(registry.runtime(runtime.id()).unwrap().value, Some(runtime));
}

#[test]
fn commits_stamp_rows_with_one_clock_reading_and_updates_keep_created_at() {
    let (clock, registry) = fake();
    let start = clock.now();
    let (_provider, runtime) = store_runtime(&registry);
    let resource = ResourceRef::Runtime(runtime.id());
    clock.advance(SignedDuration::from_secs(60));

    registry
        .commit(WriteBatch::new(vec![WriteOp::UpdateRuntime {
            runtime: runtime.clone().revision(2).build(),
            expected: stackly_domain::ResourceRevision::INITIAL,
        }]))
        .unwrap();

    assert_eq!(
        registry.written_at(&resource),
        Some(WrittenAt {
            created_at: start,
            updated_at: clock.now(),
        })
    );
    assert_eq!(
        registry.written_at(&ResourceRef::Artifact(runtime.artifact().sha256)),
        Some(WrittenAt {
            created_at: start,
            updated_at: start,
        })
    );
}

#[test]
fn an_instance_keeps_its_own_created_at_and_reads_back_at_the_load_time() {
    let (clock, registry) = fake();
    let (provider, runtime) = store_runtime(&registry);
    let created_at: Timestamp = "2025-06-01T08:30:00.123456789Z".parse().unwrap();
    let instance = InstanceBuilder::new(&runtime.clone().build(), &provider.key())
        .created_at(created_at)
        .build();

    registry
        .commit(WriteBatch::new(vec![WriteOp::InsertInstance(
            instance.clone(),
        )]))
        .unwrap();

    let stamp = registry
        .written_at(&ResourceRef::Instance(instance.id()))
        .unwrap();
    assert_eq!(stamp.created_at, created_at);
    assert_eq!(stamp.updated_at, clock.now());
    let read = registry.instance(instance.id()).unwrap().value.unwrap();
    assert_eq!(read.created_at(), created_at);
    assert_eq!(read.readiness().observed_at(), clock.now());
}

#[test]
fn seeding_never_raises_the_generation() {
    let (clock, registry) = fake();
    let (provider, runtime) = store_runtime(&registry);
    let instance = InstanceBuilder::new(&runtime.clone().build(), &provider.key()).build();
    registry
        .commit(WriteBatch::new(vec![WriteOp::InsertInstance(
            instance.clone(),
        )]))
        .unwrap();
    let before = registry.known_instance_ids().unwrap().generation;

    registry.seed_backup(BackupSeed::new(clock.now()).with_runtime(runtime.id()));
    registry.seed_audit_events(3, clock.now());
    registry.seed_observations(instance.id(), 2, clock.now());

    assert_eq!(registry.known_instance_ids().unwrap().generation, before);
    let references = registry.runtime_references(runtime.id()).unwrap().value;
    assert_eq!(references.recovery_snapshots.len(), 1);
    let report = registry
        .prune(&RetentionPolicy::DEFAULT, clock.now())
        .unwrap();
    assert_eq!(report.total(), 0);
}

#[test]
#[should_panic(expected = "a seeded backup refers to a runtime the registry holds")]
fn seeding_a_backup_of_an_unknown_runtime_panics() {
    let (clock, registry) = fake();

    registry.seed_backup(BackupSeed::new(clock.now()).with_runtime(RuntimeBuilder::new().id()));
}

#[test]
fn builders_make_a_new_id_and_a_unique_default_name_per_call() {
    let runtime = RuntimeBuilder::new().build();
    let provider = ProviderRevisionBuilder::new().key();
    let a = InstanceBuilder::new(&runtime, &provider).build();
    let b = InstanceBuilder::new(&runtime, &provider).build();

    assert_ne!(a.id(), b.id());
    assert_ne!(a.name(), b.name());
    assert_ne!(RuntimeBuilder::new().id(), runtime.id());
    assert_ne!(ProviderRevisionBuilder::new().key(), provider);
    assert_eq!(a.software(), runtime.software());
}

#[test]
fn a_list_read_carries_the_generation_of_the_registry() {
    let (_clock, registry) = fake();
    store_runtime(&registry);

    let list = registry.instances(InstanceFilter::default()).unwrap();

    assert_eq!(list.generation.get(), 1);
    assert!(list.value.is_empty());
}
