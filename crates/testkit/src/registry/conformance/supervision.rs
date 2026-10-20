//! What the process supervisor needs from the registry (spec 0005
//! "Tambahan fitur 10"): process observations, whose newest is the largest
//! row ID; config revisions, which the instance points to without a
//! revision of its own; and several updates of one instance in one batch.

use std::path::PathBuf;

use jiff::Timestamp;
use stackly_domain::{
    BatchError, ConfigRevisionId, ConfigRevisionRecord, ConfigValuesJson, DesiredState, Instance,
    InstanceId, OperationId, Pid, ProcessIdentity, ProcessObservation, ProcessStart, RegistryError,
    ResourceRef, WriteOp,
};

use super::{Suite, at, batch, domain, expect, expect_eq, parse, rev, shifted, suffix};
use crate::registry::seeded_observation;

/// Runs every scenario the process supervisor needs.
pub(super) fn supervision(suite: &Suite<'_>) {
    unknown_instances(suite);
    observations(suite);
    config_revisions(suite);
    config_survives_instance_updates(suite);
    chained_instance_updates(suite);
}

/// An observation of a made up process of `instance`, started `pid`
/// seconds after the epoch, so each `pid` names another process.
fn observation(instance: InstanceId, pid: u32, observed_at: Timestamp) -> ProcessObservation {
    observation_running(
        instance,
        pid,
        format!("/conformance/runtimes/{}/bin/postgres", suffix(instance)).into(),
        observed_at,
    )
}

fn observation_running(
    instance: InstanceId,
    pid: u32,
    executable: PathBuf,
    observed_at: Timestamp,
) -> ProcessObservation {
    let pid = domain("make a process ID", Pid::new(pid));
    ProcessObservation {
        instance,
        identity: ProcessIdentity {
            pid,
            start: domain(
                "make a process start",
                ProcessStart::new(u64::from(pid.get()), 123_456),
            ),
            executable,
            session: pid,
            boot: parse("make a boot ID", "6f9619ff-8b86-4011-b42d-00c04fc964ff"),
        },
        observed_at,
    }
}

fn values(text: &str) -> ConfigValuesJson {
    domain("make config values", ConfigValuesJson::new(text.to_owned()))
}

/// The first config revision of `instance`, written by a made up
/// operation.
fn first_revision(suite: &Suite<'_>, instance: InstanceId) -> ConfigRevisionRecord {
    ConfigRevisionRecord::first(
        ConfigRevisionId::generate(),
        instance,
        values(r#"{"listen":"127.0.0.1","port":5432}"#),
        Some(OperationId::generate()),
        suite.loaded_at,
    )
}

/// The revision after `previous`.
fn next_revision(suite: &Suite<'_>, previous: &ConfigRevisionRecord) -> ConfigRevisionRecord {
    let mut record = first_revision(suite, previous.instance);
    record.previous = Some(previous.id);
    record.values = values(r#"{"listen":"127.0.0.1","port":5433}"#);
    record
}

fn latest(suite: &Suite<'_>, check: &str, instance: InstanceId) -> Option<ProcessObservation> {
    suite.value(check, suite.registry.latest_process_observation(instance))
}

fn active(suite: &Suite<'_>, check: &str, instance: InstanceId) -> Option<ConfigRevisionRecord> {
    suite.value(check, suite.registry.active_config_revision(instance))
}

/// Stores a new instance of a new stack.
fn stored_instance(suite: &Suite<'_>, check: &str) -> Instance {
    let stack = suite.stack(check);
    let instance = Suite::instance(&stack).build();
    suite.commit(check, vec![WriteOp::InsertInstance(instance.clone())]);
    instance
}

/// Both reads of an unknown instance fail with `NotFound`, and both writes
/// with `MissingReference`.
fn unknown_instances(suite: &Suite<'_>) {
    let unknown = InstanceId::generate();
    let not_found = RegistryError::NotFound {
        resource: ResourceRef::Instance(unknown),
    };
    expect_eq(
        "the newest observation of an unknown instance is not found",
        suite.registry.latest_process_observation(unknown),
        Err(not_found.clone()),
    );
    expect_eq(
        "the active config revision of an unknown instance is not found",
        suite.registry.active_config_revision(unknown),
        Err(not_found),
    );
    let missing = RegistryError::MissingReference {
        resource: ResourceRef::Instance(unknown),
    };
    suite.rejected(
        "an observation of an unknown instance is refused",
        batch(vec![WriteOp::RecordProcessObservation(observation(
            unknown,
            4001,
            suite.loaded_at,
        ))]),
        at(0, missing.clone()),
    );
    suite.rejected(
        "a config revision of an unknown instance is refused",
        batch(vec![WriteOp::InsertConfigRevision(first_revision(
            suite, unknown,
        ))]),
        at(0, missing),
    );
}

/// The newest observation is the one recorded last, whatever its
/// `observed_at`; observations raise no revision.
fn observations(suite: &Suite<'_>) {
    let stack = suite.stack("store a runtime for observations");
    let instance = Suite::instance(&stack).build();
    let id = instance.id();
    let observed_at = shifted(suite.loaded_at, 5);

    suite.rejected(
        "an observation before the insert of its instance in a batch is refused",
        batch(vec![
            WriteOp::RecordProcessObservation(observation(id, 4101, observed_at)),
            WriteOp::InsertInstance(instance.clone()),
        ]),
        at(
            0,
            RegistryError::MissingReference {
                resource: ResourceRef::Instance(id),
            },
        ),
    );
    suite.commit(
        "store an instance without observations",
        vec![WriteOp::InsertInstance(instance)],
    );
    expect_eq(
        "an instance without observations has no newest one",
        latest(suite, "read the newest observation", id),
        None,
    );

    let first = observation(id, 4102, observed_at);
    let second = observation(id, 4103, observed_at);
    let committed = suite.commit(
        "record two observations at the same time",
        vec![
            WriteOp::RecordProcessObservation(first),
            WriteOp::RecordProcessObservation(second.clone()),
        ],
    );
    expect_eq(
        "observations add no committed revision",
        committed.revisions,
        Vec::new(),
    );
    expect_eq(
        "of two observations at the same time the one recorded last is the newest",
        latest(suite, "read the newest of two", id),
        Some(second),
    );

    let earlier = observation(id, 4104, shifted(observed_at, -3600));
    suite.commit(
        "record an observation after the clock went back",
        vec![WriteOp::RecordProcessObservation(earlier.clone())],
    );
    expect_eq(
        "an observation recorded after the clock went back is still the newest",
        latest(suite, "read the newest after the clock went back", id),
        Some(earlier),
    );

    let long = PathBuf::from(format!("/{}", "x".repeat(1023)));
    let longest = observation_running(id, 4105, long.clone(), observed_at);
    suite.commit(
        "record an observation with a 1024 character executable",
        vec![WriteOp::RecordProcessObservation(longest.clone())],
    );
    expect_eq(
        "a 1024 character executable reads back whole",
        latest(suite, "read the longest executable", id),
        Some(longest.clone()),
    );
    let mut too_long = long.into_os_string();
    too_long.push("x");
    let before = suite.generation();
    let refused = suite
        .registry
        .commit(batch(vec![WriteOp::RecordProcessObservation(
            observation_running(id, 4106, too_long.into(), observed_at),
        )]));
    expect(
        "an executable over 1024 characters is refused by the column check",
        matches!(
            &refused,
            Err(BatchError {
                op_index: Some(0),
                error: RegistryError::Integrity { .. },
            })
        ),
        || format!("expected Integrity at op 0, got {refused:?}"),
    );
    expect_eq(
        "a refused observation leaves the generation",
        suite.generation(),
        before,
    );
    expect_eq(
        "a refused observation leaves the newest one",
        latest(suite, "read the newest after a refusal", id),
        Some(longest),
    );

    let seeded_at = shifted(observed_at, 60);
    suite.seed.seed_observations(id, 1, seeded_at);
    expect_eq(
        "a seeded observation reads back with its fixed values",
        latest(suite, "read the seeded observation", id),
        Some(seeded_observation(id, seeded_at)),
    );
}

/// A config revision is new by ID, belongs to a stored instance, follows a
/// revision of the same instance or none while the instance has none, and
/// becomes the active one without raising the instance's revision.
fn config_revisions(suite: &Suite<'_>) {
    let stack = suite.stack("store a runtime for config revisions");
    let instance = Suite::instance(&stack).build();
    let id = instance.id();
    let mut first = first_revision(suite, id);
    first.rendered_sha256 = Some(parse("make a digest", &"5e".repeat(32)));
    first.created_at = shifted(suite.loaded_at, 7);

    suite.rejected(
        "a config revision before the insert of its instance in a batch is refused",
        batch(vec![
            WriteOp::InsertConfigRevision(first.clone()),
            WriteOp::InsertInstance(instance.clone()),
        ]),
        at(
            0,
            RegistryError::MissingReference {
                resource: ResourceRef::Instance(id),
            },
        ),
    );
    let committed = suite.commit(
        "store an instance with its first config revision",
        vec![
            WriteOp::InsertInstance(instance.clone()),
            WriteOp::InsertConfigRevision(first.clone()),
        ],
    );
    expect_eq(
        "a config revision adds no committed revision",
        committed.revisions,
        vec![(ResourceRef::Instance(id), rev(1))],
    );
    expect_eq(
        "the first config revision is the active one and reads back whole",
        active(suite, "read the first active revision", id),
        Some(first.clone()),
    );
    expect_eq(
        "a config revision leaves the instance's revision",
        suite
            .value("read the configured instance", suite.registry.instance(id))
            .map(|stored| stored.revision()),
        Some(rev(1)),
    );

    let other = stored_instance(suite, "store another instance for config revisions");
    let mut taken = first_revision(suite, other.id());
    taken.id = first.id;
    suite.rejected(
        "a config revision ID is unique",
        batch(vec![WriteOp::InsertConfigRevision(taken)]),
        at(
            0,
            RegistryError::DuplicateResource {
                resource: ResourceRef::ConfigRevision(first.id),
            },
        ),
    );
    let mut taken_unknown = first_revision(suite, InstanceId::generate());
    taken_unknown.id = first.id;
    suite.rejected(
        "a duplicate ID is found before an unknown instance",
        batch(vec![WriteOp::InsertConfigRevision(taken_unknown)]),
        at(
            0,
            RegistryError::DuplicateResource {
                resource: ResourceRef::ConfigRevision(first.id),
            },
        ),
    );
    let mut foreign = first_revision(suite, other.id());
    foreign.previous = Some(first.id);
    suite.rejected(
        "a config revision cannot follow a revision of another instance",
        batch(vec![WriteOp::InsertConfigRevision(foreign)]),
        at(
            0,
            RegistryError::MissingReference {
                resource: ResourceRef::ConfigRevision(first.id),
            },
        ),
    );
    let unknown = ConfigRevisionId::generate();
    let mut dangling = first_revision(suite, other.id());
    dangling.previous = Some(unknown);
    suite.rejected(
        "a config revision cannot follow an unknown revision",
        batch(vec![WriteOp::InsertConfigRevision(dangling)]),
        at(
            0,
            RegistryError::MissingReference {
                resource: ResourceRef::ConfigRevision(unknown),
            },
        ),
    );
    suite.rejected(
        "a config revision that follows none is refused while one is active",
        batch(vec![WriteOp::InsertConfigRevision(first_revision(
            suite, id,
        ))]),
        at(
            0,
            RegistryError::DuplicateResource {
                resource: ResourceRef::ConfigRevision(first.id),
            },
        ),
    );

    let ghost = InstanceId::generate();
    suite.rejected(
        "a failed batch leaves no config revision behind",
        batch(vec![
            WriteOp::InsertConfigRevision(first_revision(suite, other.id())),
            WriteOp::RecordProcessObservation(observation(ghost, 4201, suite.loaded_at)),
        ]),
        at(
            1,
            RegistryError::MissingReference {
                resource: ResourceRef::Instance(ghost),
            },
        ),
    );
    expect_eq(
        "a failed batch leaves the instance without an active config revision",
        active(suite, "read the config after a failed batch", other.id()),
        None,
    );
    let unborn = Suite::instance(&stack).build();
    let unborn_id = unborn.id();
    let unborn_config = first_revision(suite, unborn_id);
    suite.rejected(
        "a failed create leaves neither the instance nor its config revision",
        batch(vec![
            WriteOp::InsertInstance(unborn),
            WriteOp::InsertConfigRevision(unborn_config.clone()),
            WriteOp::InsertConfigRevision(unborn_config.clone()),
        ]),
        at(
            2,
            RegistryError::DuplicateResource {
                resource: ResourceRef::ConfigRevision(unborn_config.id),
            },
        ),
    );
    expect_eq(
        "the config revision of a failed create is not found",
        suite.registry.active_config_revision(unborn_id),
        Err(RegistryError::NotFound {
            resource: ResourceRef::Instance(unborn_id),
        }),
    );

    let second = next_revision(suite, &first);
    let committed = suite.commit(
        "store the revision after the first",
        vec![WriteOp::InsertConfigRevision(second.clone())],
    );
    expect_eq(
        "the next config revision adds no committed revision",
        committed.revisions,
        Vec::new(),
    );
    expect_eq(
        "the revision after the first is the active one",
        active(suite, "read the second active revision", id),
        Some(second.clone()),
    );
    suite.rejected(
        "a config revision that follows none names the active one when refused",
        batch(vec![WriteOp::InsertConfigRevision(first_revision(
            suite, id,
        ))]),
        at(
            0,
            RegistryError::DuplicateResource {
                resource: ResourceRef::ConfigRevision(second.id),
            },
        ),
    );
}

/// An instance update never touches the active config revision, in the
/// same batch as the revision or in a later one.
fn config_survives_instance_updates(suite: &Suite<'_>) {
    let stack = suite.stack("store a runtime for config and updates");
    let instance = Suite::instance(&stack).build();
    let id = instance.id();
    let config = first_revision(suite, id);
    let mut updated = instance.clone();
    updated.set_desired(DesiredState::Running);
    let committed = suite.commit(
        "store an instance, its config, and an update in one batch",
        vec![
            WriteOp::InsertInstance(instance),
            WriteOp::InsertConfigRevision(config.clone()),
            WriteOp::UpdateInstance {
                instance: updated.clone(),
                expected: rev(1),
            },
        ],
    );
    expect_eq(
        "only the instance ops return revisions",
        committed.revisions,
        vec![
            (ResourceRef::Instance(id), rev(1)),
            (ResourceRef::Instance(id), rev(2)),
        ],
    );
    expect_eq(
        "an update after the config revision in a batch keeps it active",
        active(suite, "read the config after a batch update", id),
        Some(config.clone()),
    );
    let mut renamed = updated;
    renamed.rename(parse("make a name", &format!("configured-{}", suffix(id))));
    suite.commit(
        "update the configured instance",
        vec![WriteOp::UpdateInstance {
            instance: renamed.clone(),
            expected: rev(2),
        }],
    );
    expect_eq(
        "a later update keeps the active config revision",
        active(suite, "read the config after a later update", id),
        Some(config),
    );
    expect_eq(
        "the updated instance reads back",
        suite.value("read the configured instance", suite.registry.instance(id)),
        Some(suite.expected_instance(&renamed)),
    );
}

/// Several updates of one instance follow each other in a batch, each at
/// the revision the op before it wrote, and each returns its revision.
fn chained_instance_updates(suite: &Suite<'_>) {
    let original = stored_instance(suite, "store an instance for chained updates");
    let id = original.id();
    let mut running = original.clone();
    running.set_desired(DesiredState::Running);
    let mut renamed = running.clone();
    renamed.rename(parse("make a name", &format!("chained-{}", suffix(id))));

    let committed = suite.commit(
        "two updates of one instance in one batch apply in order",
        vec![
            WriteOp::UpdateInstance {
                instance: running.clone(),
                expected: rev(1),
            },
            WriteOp::UpdateInstance {
                instance: renamed.clone(),
                expected: rev(2),
            },
        ],
    );
    expect_eq(
        "chained updates return one revision per op, in order",
        committed.revisions,
        vec![
            (ResourceRef::Instance(id), rev(2)),
            (ResourceRef::Instance(id), rev(3)),
        ],
    );
    expect_eq(
        "chained updates read back as the last one",
        suite.value("read the chained instance", suite.registry.instance(id)),
        Some(suite.expected_instance(&renamed)),
    );

    let mut stopped = renamed.clone();
    stopped.set_desired(DesiredState::Stopped);
    let mut stale = renamed.clone();
    stale.rename(parse("make a name", &format!("stale-{}", suffix(id))));
    suite.rejected(
        "a chained update at the revision before the op ahead of it conflicts",
        batch(vec![
            WriteOp::UpdateInstance {
                instance: stopped,
                expected: rev(3),
            },
            WriteOp::UpdateInstance {
                instance: stale,
                expected: rev(3),
            },
        ]),
        at(
            1,
            RegistryError::RevisionConflict {
                resource: ResourceRef::Instance(id),
                expected: rev(3),
                actual: rev(4),
            },
        ),
    );
    expect_eq(
        "a failed chain leaves the instance at its last revision",
        suite.value(
            "read the instance after a failed chain",
            suite.registry.instance(id),
        ),
        Some(suite.expected_instance(&renamed)),
    );
}
