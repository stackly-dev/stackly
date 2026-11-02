//! Daemon settings and storage locations (spec 0032): the revision rules of
//! the settings row, the refusals of a location write, and the location an
//! instance takes when it is created.
//!
//! The scenarios leave the defaults as they found them: the built in
//! locations.

use stackly_domain::{
    LocationKind, RegistryError, ResourceRef, SettingsPatch, StartupPolicy, StorageLocation,
    StorageLocationId, UpdateChannel, WriteOp,
};

use super::{Suite, at, batch, domain, expect_eq, shifted};

/// Runs every settings and location scenario.
pub(super) fn settings(suite: &Suite<'_>) {
    patches(suite);
    locations(suite);
    instance_locations(suite);
}

fn current(suite: &Suite<'_>, check: &str) -> stackly_domain::DaemonSettings {
    suite.value(check, suite.registry.daemon_settings())
}

fn custom(suite: &Suite<'_>, check: &str, kind: LocationKind, path: &str) -> StorageLocation {
    domain(
        check,
        StorageLocation::custom(StorageLocationId::generate(), kind, path, suite.loaded_at),
    )
}

fn patches(suite: &Suite<'_>) {
    const CHECK: &str = "a_settings_patch_checks_the_revision_and_changes_only_what_it_names";
    let before = current(suite, CHECK);
    let patch = SettingsPatch {
        telemetry: Some(!before.telemetry),
        update_channel: Some(UpdateChannel::Beta),
        ..SettingsPatch::default()
    };
    let at_time = shifted(suite.loaded_at, 50);

    suite.rejected(
        CHECK,
        batch(vec![WriteOp::UpdateDaemonSettings {
            patch,
            expected: before.revision.next(),
            at: at_time,
        }]),
        at(
            0,
            RegistryError::RevisionConflict {
                resource: ResourceRef::DaemonSettings,
                expected: before.revision.next(),
                actual: before.revision,
            },
        ),
    );

    suite.commit(
        CHECK,
        vec![WriteOp::UpdateDaemonSettings {
            patch,
            expected: before.revision,
            at: at_time,
        }],
    );
    let after = current(suite, CHECK);
    expect_eq(CHECK, after.revision, before.revision.next());
    expect_eq(CHECK, after.update_channel, UpdateChannel::Beta);
    expect_eq(CHECK, after.telemetry, !before.telemetry);
    expect_eq(CHECK, after.default_startup, before.default_startup);
    expect_eq(CHECK, after.updated_at, at_time);

    // The same values again change nothing, the revision included.
    suite.commit(
        CHECK,
        vec![WriteOp::UpdateDaemonSettings {
            patch,
            expected: after.revision,
            at: shifted(suite.loaded_at, 60),
        }],
    );
    expect_eq(CHECK, current(suite, CHECK), after.clone());

    suite.commit(
        CHECK,
        vec![WriteOp::UpdateDaemonSettings {
            patch: SettingsPatch {
                default_startup: Some(StartupPolicy::Manual),
                update_channel: Some(before.update_channel),
                telemetry: Some(before.telemetry),
            },
            expected: after.revision,
            at: shifted(suite.loaded_at, 70),
        }],
    );
}

fn locations(suite: &Suite<'_>) {
    const CHECK: &str = "a_default_location_needs_a_known_location_of_its_kind";
    let built_in: Vec<_> = suite
        .value(CHECK, suite.registry.storage_locations())
        .into_iter()
        .filter(StorageLocation::is_built_in)
        .map(|location| location.kind())
        .collect();
    expect_eq(
        CHECK,
        built_in,
        vec![LocationKind::Data, LocationKind::Logs, LocationKind::Cache],
    );

    let data = custom(
        suite,
        CHECK,
        LocationKind::Data,
        "/conformance/work/Stackly",
    );
    suite.commit(CHECK, vec![WriteOp::InsertStorageLocation(data.clone())]);
    let listed = suite.value(CHECK, suite.registry.storage_locations());
    expect_eq(CHECK, listed.last(), Some(&data));

    suite.rejected(
        CHECK,
        batch(vec![WriteOp::InsertStorageLocation(data.clone())]),
        at(
            0,
            RegistryError::DuplicateResource {
                resource: ResourceRef::StorageLocation(data.id()),
            },
        ),
    );
    let twin = custom(
        suite,
        CHECK,
        LocationKind::Data,
        "/conformance/work/Stackly",
    );
    suite.rejected(
        CHECK,
        batch(vec![WriteOp::InsertStorageLocation(twin)]),
        at(
            0,
            RegistryError::DuplicateResource {
                resource: ResourceRef::StorageLocation(data.id()),
            },
        ),
    );
    // The same folder for another kind is another location.
    let logs = custom(
        suite,
        CHECK,
        LocationKind::Logs,
        "/conformance/work/Stackly",
    );
    suite.commit(CHECK, vec![WriteOp::InsertStorageLocation(logs)]);

    let settings = current(suite, CHECK);
    let at_time = shifted(suite.loaded_at, 80);
    let unknown = StorageLocationId::generate();
    suite.rejected(
        CHECK,
        batch(vec![WriteOp::SetDefaultLocation {
            kind: LocationKind::Data,
            id: unknown,
            expected: settings.revision,
            at: at_time,
        }]),
        at(
            0,
            RegistryError::MissingReference {
                resource: ResourceRef::StorageLocation(unknown),
            },
        ),
    );
    suite.rejected(
        CHECK,
        batch(vec![WriteOp::SetDefaultLocation {
            kind: LocationKind::Logs,
            id: data.id(),
            expected: settings.revision,
            at: at_time,
        }]),
        at(
            0,
            RegistryError::WrongLocationKind {
                location: ResourceRef::StorageLocation(data.id()),
                expected: LocationKind::Logs,
            },
        ),
    );
    suite.rejected(
        CHECK,
        batch(vec![WriteOp::SetDefaultLocation {
            kind: LocationKind::Data,
            id: data.id(),
            expected: settings.revision.next(),
            at: at_time,
        }]),
        at(
            0,
            RegistryError::RevisionConflict {
                resource: ResourceRef::DaemonSettings,
                expected: settings.revision.next(),
                actual: settings.revision,
            },
        ),
    );

    suite.commit(
        CHECK,
        vec![WriteOp::SetDefaultLocation {
            kind: LocationKind::Data,
            id: data.id(),
            expected: settings.revision,
            at: at_time,
        }],
    );
    let moved = current(suite, CHECK);
    expect_eq(CHECK, moved.data_location, data.id());
    expect_eq(CHECK, moved.revision, settings.revision.next());
    expect_eq(CHECK, moved.updated_at, at_time);
    expect_eq(CHECK, moved.logs_location, settings.logs_location);

    // The same default again writes nothing.
    suite.commit(
        CHECK,
        vec![WriteOp::SetDefaultLocation {
            kind: LocationKind::Data,
            id: data.id(),
            expected: moved.revision,
            at: shifted(suite.loaded_at, 90),
        }],
    );
    expect_eq(CHECK, current(suite, CHECK), moved.clone());

    suite.commit(
        CHECK,
        vec![WriteOp::SetDefaultLocation {
            kind: LocationKind::Data,
            id: LocationKind::Data.built_in_id(),
            expected: moved.revision,
            at: shifted(suite.loaded_at, 100),
        }],
    );
}

fn instance_locations(suite: &Suite<'_>) {
    const CHECK: &str = "an_instance_is_stored_in_the_default_locations_or_the_ones_it_names";
    let stack = suite.stack(CHECK);
    let built_in_data = LocationKind::Data.built_in_id();
    let built_in_logs = LocationKind::Logs.built_in_id();
    let usage_of = |id: StorageLocationId| {
        suite
            .value(CHECK, suite.registry.location_usage())
            .into_iter()
            .find(|(known, _)| *known == id)
            .map_or(0, |(_, count)| count)
    };

    let plain = Suite::instance(&stack).build();
    let data_before = usage_of(built_in_data);
    suite.commit(CHECK, vec![WriteOp::InsertInstance(plain.clone())]);
    expect_eq(CHECK, usage_of(built_in_data), data_before + 1);
    let stored = suite
        .value(CHECK, suite.registry.instance_paths(plain.id()))
        .unwrap_or_default();
    expect_eq(
        CHECK,
        stored
            .iter()
            .map(|entry| entry.location())
            .collect::<Vec<_>>(),
        vec![
            Some(built_in_data),
            Some(built_in_data),
            Some(built_in_data),
            Some(built_in_data),
            Some(built_in_data),
            Some(built_in_logs),
            None,
        ],
    );

    let data = custom(
        suite,
        CHECK,
        LocationKind::Data,
        "/conformance/disk/Stackly",
    );
    let logs = custom(
        suite,
        CHECK,
        LocationKind::Logs,
        "/conformance/disk/Stackly",
    );
    suite.commit(
        CHECK,
        vec![
            WriteOp::InsertStorageLocation(data.clone()),
            WriteOp::InsertStorageLocation(logs.clone()),
        ],
    );
    let named = Suite::instance(&stack).build();
    suite.commit(
        CHECK,
        vec![WriteOp::InsertInstanceAt {
            instance: named.clone(),
            data: data.id(),
            logs: logs.id(),
        }],
    );
    let stored = suite
        .value(CHECK, suite.registry.instance_paths(named.id()))
        .unwrap_or_default();
    expect_eq(
        CHECK,
        stored
            .iter()
            .filter_map(|entry| entry.location())
            .filter(|location| *location == data.id() || *location == logs.id())
            .count(),
        6,
    );
    expect_eq(CHECK, usage_of(data.id()), 1);
    expect_eq(CHECK, usage_of(logs.id()), 1);

    // The default decides for an insert that names nothing.
    let settings = current(suite, CHECK);
    suite.commit(
        CHECK,
        vec![WriteOp::SetDefaultLocation {
            kind: LocationKind::Data,
            id: data.id(),
            expected: settings.revision,
            at: shifted(suite.loaded_at, 110),
        }],
    );
    let follower = Suite::instance(&stack).build();
    suite.commit(CHECK, vec![WriteOp::InsertInstance(follower.clone())]);
    let stored = suite
        .value(CHECK, suite.registry.instance_paths(follower.id()))
        .unwrap_or_default();
    expect_eq(
        CHECK,
        stored.first().and_then(|entry| entry.location()),
        Some(data.id()),
    );
    let settings = current(suite, CHECK);
    suite.commit(
        CHECK,
        vec![WriteOp::SetDefaultLocation {
            kind: LocationKind::Data,
            id: built_in_data,
            expected: settings.revision,
            at: shifted(suite.loaded_at, 120),
        }],
    );

    // A pair with a missing location or the wrong kind is refused.
    let wrong = Suite::instance(&stack).build();
    suite.rejected(
        CHECK,
        batch(vec![WriteOp::InsertInstanceAt {
            instance: wrong.clone(),
            data: logs.id(),
            logs: logs.id(),
        }]),
        at(
            0,
            RegistryError::WrongLocationKind {
                location: ResourceRef::StorageLocation(logs.id()),
                expected: LocationKind::Data,
            },
        ),
    );
    let unknown = StorageLocationId::generate();
    suite.rejected(
        CHECK,
        batch(vec![WriteOp::InsertInstanceAt {
            instance: wrong,
            data: data.id(),
            logs: unknown,
        }]),
        at(
            0,
            RegistryError::MissingReference {
                resource: ResourceRef::StorageLocation(unknown),
            },
        ),
    );
}
