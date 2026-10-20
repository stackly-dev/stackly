//! The tables of feature 10 on SQLite (spec 0005, "Tambahan fitur 10"):
//! the stored text of a process observation, the instance column a config
//! revision fills without touching the instance's revision, the column
//! checks that stay the last guard, and stored rows that read as `Corrupt`.

use jiff::{SignedDuration, Timestamp};
use stackly_domain::{
    BatchError, ConfigRevisionId, ConfigRevisionRecord, ConfigValuesJson, Instance, InstanceId,
    OperationId, Pid, ProcessIdentity, ProcessObservation, ProcessStart, Registry, RegistryError,
    WriteOp,
};

use crate::common::Harness;

fn observation(instance: InstanceId, observed_at: Timestamp) -> ProcessObservation {
    ProcessObservation {
        instance,
        identity: ProcessIdentity {
            pid: Pid::new(4242).expect("4242 is a process ID"),
            start: ProcessStart::new(1_700_000_000, 42).expect("42 microseconds is in range"),
            executable: "/Applications/Stackly/runtimes/pg/bin/postgres".into(),
            session: Pid::new(4243).expect("4243 is a process ID"),
            boot: "6F9619FF-8B86-4011-B42D-00C04FC964FF"
                .parse()
                .expect("a hyphenated UUID in either case is a boot ID"),
        },
        observed_at,
    }
}

fn first_revision(instance: InstanceId, values: &str, at: Timestamp) -> ConfigRevisionRecord {
    ConfigRevisionRecord::first(
        ConfigRevisionId::generate(),
        instance,
        ConfigValuesJson::new(values.to_owned()).expect("the test passes object text"),
        Some(OperationId::generate()),
        at,
    )
}

/// Stores an instance of a new base.
fn stored_instance(harness: &Harness) -> Instance {
    let base = harness.base();
    let instance = harness.instance(&base, "main");
    harness.commit(vec![WriteOp::InsertInstance(instance.clone())]);
    instance
}

#[test]
fn an_observation_is_stored_in_the_column_text_of_spec_0005() {
    let harness = Harness::new();
    let instance = stored_instance(&harness);
    let observed_at = Timestamp::constant(1_767_225_600, 123_456_789);
    harness.commit(vec![WriteOp::RecordProcessObservation(observation(
        instance.id(),
        observed_at,
    ))]);
    let raw = |column: &str| {
        harness
            .raw(
                &format!("SELECT {column} FROM process_observations WHERE instance_id = ?1"),
                &instance.id().to_string(),
            )
            .unwrap()
    };

    assert_eq!(raw("typeof(pid) || ':' || pid"), "integer:4242");
    assert_eq!(raw("start_identity"), "1700000000.000042");
    assert_eq!(
        raw("executable_identity"),
        "/Applications/Stackly/runtimes/pg/bin/postgres"
    );
    assert_eq!(raw("boot_id"), "6f9619ff-8b86-4011-b42d-00c04fc964ff");
    assert_eq!(raw("session_id"), "4243");
    assert_eq!(raw("observed_at"), "2026-01-01T00:00:00.123456789Z");
    assert_eq!(
        harness
            .registry
            .latest_process_observation(instance.id())
            .unwrap()
            .value,
        Some(observation(instance.id(), observed_at))
    );
}

#[test]
fn a_config_revision_fills_the_instance_column_and_leaves_its_revision_and_updated_at() {
    let harness = Harness::new();
    let instance = stored_instance(&harness);
    let id = instance.id().to_string();
    let updated_at = harness.raw("SELECT updated_at FROM instances WHERE id = ?1", &id);
    harness.clock().advance(SignedDuration::from_secs(60));
    let record = first_revision(instance.id(), r#"{"port":5432}"#, harness.now());

    harness.commit(vec![WriteOp::InsertConfigRevision(record.clone())]);

    assert_eq!(
        harness.raw(
            "SELECT config_revision_id FROM instances WHERE id = ?1",
            &id
        ),
        Some(record.id.to_string())
    );
    assert_eq!(
        harness.raw("SELECT updated_at FROM instances WHERE id = ?1", &id),
        updated_at
    );
    assert_eq!(
        harness.raw(
            "SELECT CAST(revision AS TEXT) FROM instances WHERE id = ?1",
            &id
        ),
        Some("1".to_owned())
    );
    let config_id = record.id.to_string();
    assert_eq!(
        harness.raw(
            "SELECT config_values || ' ' || status FROM config_revisions WHERE id = ?1",
            &config_id
        ),
        Some(r#"{"port":5432} active"#.to_owned())
    );
}

#[test]
fn config_values_that_are_not_a_json_object_are_refused_by_the_column_check() {
    let harness = Harness::new();
    let instance = stored_instance(&harness);

    let err = harness.reject(vec![WriteOp::InsertConfigRevision(first_revision(
        instance.id(),
        "{x}",
        harness.now(),
    ))]);

    assert!(
        matches!(
            err,
            BatchError {
                op_index: Some(0),
                error: RegistryError::Integrity { .. },
            }
        ),
        "{err:?}"
    );
    assert_eq!(
        harness
            .registry
            .active_config_revision(instance.id())
            .unwrap()
            .value,
        None
    );
}

#[test]
fn an_observation_row_that_cannot_be_read_is_corrupt() {
    let harness = Harness::new();
    let instance = stored_instance(&harness);
    harness.commit(vec![WriteOp::RecordProcessObservation(observation(
        instance.id(),
        harness.now(),
    ))]);

    for (column, value) in [
        ("boot_id", None),
        ("boot_id", Some("not-a-boot")),
        ("session_id", None),
        ("session_id", Some("")),
        ("session_id", Some("0")),
        ("start_identity", Some("start-1")),
    ] {
        let side = harness.side();
        side.execute_batch(
            "UPDATE process_observations SET boot_id = '6f9619ff-8b86-4011-b42d-00c04fc964ff', \
             session_id = '4243', start_identity = '1700000000.000042'",
        )
        .unwrap();
        side.execute(
            &format!("UPDATE process_observations SET {column} = ?1"),
            [value],
        )
        .unwrap();

        let err = harness
            .registry
            .latest_process_observation(instance.id())
            .unwrap_err();

        assert!(
            matches!(
                &err,
                RegistryError::Corrupt { table: "process_observations", column: found, .. }
                    if *found == column
            ),
            "{column} = {value:?}: {err:?}"
        );
    }
}

#[test]
fn an_unknown_config_status_reads_as_corrupt() {
    let harness = Harness::new();
    let instance = stored_instance(&harness);
    let record = first_revision(instance.id(), "{}", harness.now());
    harness.commit(vec![WriteOp::InsertConfigRevision(record.clone())]);
    harness
        .side()
        .execute(
            "UPDATE config_revisions SET status = 'proposed' WHERE id = ?1",
            [record.id.to_string()],
        )
        .unwrap();

    let err = harness
        .registry
        .active_config_revision(instance.id())
        .unwrap_err();

    assert!(
        matches!(
            &err,
            RegistryError::Corrupt { table: "config_revisions", column: "status", detail }
                if !detail.contains("proposed")
        ),
        "{err:?}"
    );
}
