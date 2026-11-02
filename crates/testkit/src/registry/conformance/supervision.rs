//! What the process supervisor needs from the registry (spec 0005
//! "Tambahan fitur 10"): process observations, whose newest is the largest
//! row ID; config revisions, which the instance points to without a
//! revision of its own; numbered revisions that activate through the
//! aggregate, become ready once, and prune to the newest (spec 0029); and
//! several updates of one instance in one batch.

use std::collections::BTreeMap;
use std::path::PathBuf;

use jiff::Timestamp;
use stackly_domain::{
    BatchError, ConfigRevisionId, ConfigRevisionOrigin, ConfigRevisionRecord, ConfigRevisionStatus,
    ConfigValuesJson, DesiredState, Endpoint, EndpointRequests, Instance, InstanceId, IpEndpoint,
    OperationId, Pid, Port, PortRequest, ProcessIdentity, ProcessObservation, ProcessStart,
    RegistryError, ResourceRef, Transport, WriteOp,
};

use super::{Suite, at, batch, domain, expect, expect_eq, parse, rev, shifted, suffix};
use crate::registry::seeded_observation;

/// Runs every scenario the process supervisor needs.
pub(super) fn supervision(suite: &Suite<'_>) {
    unknown_instances(suite);
    observations(suite);
    config_revisions(suite);
    activated_revisions(suite);
    ready_and_pruned_revisions(suite);
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
        EndpointRequests::default(),
        Some(OperationId::generate()),
        suite.loaded_at,
    )
}

/// The revision after `previous`.
fn next_revision(suite: &Suite<'_>, previous: &ConfigRevisionRecord) -> ConfigRevisionRecord {
    let mut record = first_revision(suite, previous.instance);
    record.previous = Some(previous.id);
    record.number = previous.number.next();
    record.values = values(r#"{"listen":"127.0.0.1","port":5433}"#);
    record
}

/// The revision that activates after `active` with `endpoints`.
fn activated(
    suite: &Suite<'_>,
    active: &ConfigRevisionRecord,
    endpoints: EndpointRequests,
    seconds: i64,
) -> ConfigRevisionRecord {
    ConfigRevisionRecord::following(
        active,
        ConfigRevisionId::generate(),
        values(&format!(r#"{{"port":{}}}"#, 5432 + seconds)),
        endpoints,
        ConfigRevisionOrigin::Applied,
        None,
        Some(OperationId::generate()),
        shifted(suite.loaded_at, seconds),
    )
}

fn port(number: u16) -> Port {
    domain("make a port", Port::new(number))
}

fn history(suite: &Suite<'_>, check: &str, instance: InstanceId) -> Vec<u64> {
    suite
        .value(check, suite.registry.config_revisions(instance, 100))
        .iter()
        .map(|revision| revision.number.get())
        .collect()
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

/// `ActivateConfigRevision` rewrites the instance with its revision check,
/// records the next number, supersedes the active one, and refuses a
/// record that does not follow it or whose endpoints differ (spec 0029).
fn activated_revisions(suite: &Suite<'_>) {
    let stack = suite.stack("store a runtime for activated revisions");
    let main = || {
        Endpoint::Ip(IpEndpoint::new(
            parse("make an endpoint name", "main"),
            Transport::Tcp,
            parse("make an address", "127.0.0.1"),
            PortRequest::Auto,
            None,
        ))
    };
    let instance = Suite::instance(&stack).endpoints(vec![main()]).build();
    let id = instance.id();
    let first = ConfigRevisionRecord::first(
        ConfigRevisionId::generate(),
        id,
        values("{}"),
        EndpointRequests::of(&instance),
        None,
        suite.loaded_at,
    );
    suite.commit(
        "store an instance with an endpoint and its first revision",
        vec![
            WriteOp::InsertInstance(instance.clone()),
            WriteOp::InsertConfigRevision(first.clone()),
        ],
    );
    expect_eq(
        "the first revision carries the endpoint snapshot",
        active(suite, "read the first revision", id).map(|record| record.endpoints),
        Some(EndpointRequests::of(&instance)),
    );

    let fixed = EndpointRequests::new(BTreeMap::from([(
        parse("make an endpoint name", "main"),
        PortRequest::Fixed(port(5544)),
    )]));
    let mut changed = instance.clone();
    domain("set the port request", changed.set_port_requests(&fixed));
    let second = activated(suite, &first, fixed.clone(), 1);

    let mut wrong_endpoints = second.clone();
    wrong_endpoints.endpoints = EndpointRequests::of(&instance);
    let mut skipped = second.clone();
    skipped.number = second.number.next();
    let mut orphan = second.clone();
    orphan.previous = None;
    let mut superseded = second.clone();
    superseded.status = ConfigRevisionStatus::Superseded;
    for (check, record) in [
        (
            "an activated revision whose endpoints differ is refused",
            wrong_endpoints,
        ),
        (
            "an activated revision that skips a number is refused",
            skipped,
        ),
        ("an activated revision that follows none is refused", orphan),
        (
            "an activated revision that is not active is refused",
            superseded,
        ),
    ] {
        let before = suite.generation();
        let refused = suite
            .registry
            .commit(batch(vec![WriteOp::ActivateConfigRevision {
                instance: changed.clone(),
                expected: rev(1),
                record: Box::new(record),
            }]));
        expect(
            check,
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
            "a refused activation leaves the generation",
            suite.generation(),
            before,
        );
    }
    suite.rejected(
        "an activation at a stale instance revision conflicts",
        batch(vec![WriteOp::ActivateConfigRevision {
            instance: {
                let mut ahead = changed.clone();
                domain(
                    "touch again",
                    ahead.set_port_requests(&EndpointRequests::of(&changed)),
                );
                ahead
            },
            expected: rev(2),
            record: Box::new(second.clone()),
        }]),
        at(
            0,
            RegistryError::RevisionConflict {
                resource: ResourceRef::Instance(id),
                expected: rev(2),
                actual: rev(1),
            },
        ),
    );
    expect_eq(
        "refused activations leave the first revision active",
        active(suite, "read after refusals", id).map(|record| record.id),
        Some(first.id),
    );

    let committed = suite.commit(
        "activate the second revision with a fixed port",
        vec![WriteOp::ActivateConfigRevision {
            instance: changed.clone(),
            expected: rev(1),
            record: Box::new(second.clone()),
        }],
    );
    expect_eq(
        "an activation returns the instance's new revision",
        committed.revisions,
        vec![(ResourceRef::Instance(id), rev(2))],
    );
    expect_eq(
        "the activated revision reads back whole as the active one",
        active(suite, "read the second revision", id),
        Some(second.clone()),
    );
    expect_eq(
        "the activated port request reads back on the instance",
        suite
            .value("read the instance", suite.registry.instance(id))
            .map(|stored| EndpointRequests::of(&stored)),
        Some(fixed),
    );
    let all = suite.value("read the history", suite.registry.config_revisions(id, 10));
    expect_eq(
        "the history is newest number first and the old revision is superseded",
        all.iter()
            .map(|record| (record.number.get(), record.status))
            .collect::<Vec<_>>(),
        vec![
            (2, ConfigRevisionStatus::Active),
            (1, ConfigRevisionStatus::Superseded),
        ],
    );
    suite.rejected(
        "an activated revision ID is unique",
        batch(vec![WriteOp::ActivateConfigRevision {
            instance: {
                let mut again = changed.clone();
                domain(
                    "keep the port request",
                    again.set_port_requests(&EndpointRequests::of(&changed)),
                );
                again
            },
            expected: rev(2),
            record: {
                let mut taken = activated(suite, &second, EndpointRequests::of(&changed), 2);
                taken.id = second.id;
                Box::new(taken)
            },
        }]),
        at(
            0,
            RegistryError::DuplicateResource {
                resource: ResourceRef::ConfigRevision(second.id),
            },
        ),
    );
    let lonely = stored_instance(suite, "store an instance without revisions");
    let mut touched = lonely.clone();
    domain(
        "touch the instance",
        touched.set_port_requests(&EndpointRequests::of(&lonely)),
    );
    suite.rejected(
        "an activation on an instance without an active revision is refused",
        batch(vec![WriteOp::ActivateConfigRevision {
            instance: touched,
            expected: rev(1),
            record: Box::new(activated(
                suite,
                &first_revision(suite, lonely.id()),
                EndpointRequests::of(&lonely),
                3,
            )),
        }]),
        at(
            0,
            RegistryError::MissingReference {
                resource: ResourceRef::Instance(lonely.id()),
            },
        ),
    );
}

/// `MarkConfigRevisionReady` sets `ready_at` once; the last known good is
/// the highest number with it; prune keeps the newest, the active one, and
/// the last known good, and never reuses a number.
fn ready_and_pruned_revisions(suite: &Suite<'_>) {
    let stack = suite.stack("store a runtime for pruned revisions");
    let instance = Suite::instance(&stack).build();
    let id = instance.id();
    let first = first_revision(suite, id);
    suite.commit(
        "store an instance with its first revision",
        vec![
            WriteOp::InsertInstance(instance.clone()),
            WriteOp::InsertConfigRevision(first.clone()),
        ],
    );
    expect_eq(
        "an instance never ready has no last known good",
        suite.value(
            "read the last known good",
            suite.registry.last_known_good(id),
        ),
        None,
    );
    let ready_at = shifted(suite.loaded_at, 30);
    suite.commit(
        "mark the first revision ready",
        vec![
            WriteOp::MarkConfigRevisionReady {
                revision: first.id,
                at: ready_at,
            },
            WriteOp::MarkConfigRevisionReady {
                revision: first.id,
                at: shifted(ready_at, 60),
            },
        ],
    );
    expect_eq(
        "ready_at is set once and kept",
        suite
            .value(
                "read the last known good",
                suite.registry.last_known_good(id),
            )
            .map(|record| (record.id, record.ready_at)),
        Some((first.id, Some(ready_at))),
    );
    let unknown = ConfigRevisionId::generate();
    suite.rejected(
        "marking an unknown revision ready is not found",
        batch(vec![WriteOp::MarkConfigRevisionReady {
            revision: unknown,
            at: ready_at,
        }]),
        at(
            0,
            RegistryError::NotFound {
                resource: ResourceRef::ConfigRevision(unknown),
            },
        ),
    );

    let mut current = instance;
    let mut active_record = first.clone();
    for step in 1..=5_i64 {
        let mut next = current.clone();
        domain(
            "touch the instance",
            next.set_port_requests(&EndpointRequests::of(&current)),
        );
        let record = activated(suite, &active_record, EndpointRequests::of(&current), step);
        suite.commit(
            "activate one more revision and prune to three",
            vec![
                WriteOp::ActivateConfigRevision {
                    instance: next.clone(),
                    expected: current.revision(),
                    record: Box::new(record.clone()),
                },
                WriteOp::PruneConfigRevisions {
                    instance: id,
                    keep: 3,
                },
            ],
        );
        current = next;
        active_record = record;
    }
    expect_eq(
        "prune keeps the three newest numbers and the last known good",
        history(suite, "read the pruned history", id),
        vec![6, 5, 4, 1],
    );
    let survivors = suite.value(
        "read the pruned rows",
        suite.registry.config_revisions(id, 2),
    );
    expect_eq(
        "a limit returns the newest numbers only",
        survivors
            .iter()
            .map(|record| record.number.get())
            .collect::<Vec<_>>(),
        vec![6, 5],
    );
    expect_eq(
        "the oldest kept revision whose predecessor was pruned follows none",
        suite
            .value("read the history", suite.registry.config_revisions(id, 100))
            .iter()
            .find(|record| record.number.get() == 4)
            .map(|record| record.previous),
        Some(None),
    );
    let ghost = InstanceId::generate();
    suite.rejected(
        "prune of an unknown instance is refused",
        batch(vec![WriteOp::PruneConfigRevisions {
            instance: ghost,
            keep: 3,
        }]),
        at(
            0,
            RegistryError::MissingReference {
                resource: ResourceRef::Instance(ghost),
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
