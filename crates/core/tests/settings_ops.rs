//! The prechecks of the settings operations and the inventory of custom
//! data locations (spec 0032).
#![cfg(unix)]

use std::sync::Arc;

use stackly_core::settings::{
    LocationTarget, SetLocationRequest, SettingsCheck, SettingsError, set_location_precheck,
    update_precheck,
};
use stackly_core::storage::prepare_location;
use stackly_core::{
    InventoryEntry, create_instance_layout, daemon_info, inventory_instances, prepare_layout,
};
use stackly_domain::{
    Arch, InstanceId, InstanceManifest, LocationKind, ManifestRuntime, NewInstanceManifest,
    OperationId, Os, ProviderRevisionKey, Registry, ResourceRevision, RuntimeId, SettingsPatch,
    StartupPolicy, StorageLocationId, Target, UpdateChannel, WriteBatch, WriteOp,
};
use stackly_platform::Clock;
use stackly_testkit::{FakePlatform, FakeRegistry, TestRoot};

const DIGEST: &str = "da578821d15e9c0f59eb8e4672962c6f032074e802684576debc6dfc7f86815f";

fn registry(fake: &FakePlatform) -> FakeRegistry {
    FakeRegistry::new(Arc::new(fake.clock().clone()), fake.clock().now())
}

fn patch(telemetry: bool) -> SettingsPatch {
    SettingsPatch {
        telemetry: Some(telemetry),
        ..SettingsPatch::default()
    }
}

// covers: AC-16, AC-17
#[test]
fn an_update_checks_the_revision_then_whether_anything_changes() {
    let root = TestRoot::new().unwrap();
    let fake = FakePlatform::new(&root);
    let registry = registry(&fake);
    let revision = registry.daemon_settings().unwrap().value.revision;

    assert_eq!(
        update_precheck(&registry, &patch(true), revision).unwrap(),
        SettingsCheck::Proceed
    );
    assert!(matches!(
        update_precheck(&registry, &patch(false), revision).unwrap(),
        SettingsCheck::Unchanged(settings) if settings.revision == revision
    ));
    let stale = revision.next();
    assert!(matches!(
        update_precheck(&registry, &patch(true), stale),
        Err(SettingsError::RevisionMismatch { expected, actual })
            if expected == stale && actual == revision
    ));

    registry
        .commit(WriteBatch::new(vec![WriteOp::UpdateDaemonSettings {
            patch: SettingsPatch {
                update_channel: Some(UpdateChannel::Beta),
                default_startup: Some(StartupPolicy::AtLogin),
                ..patch(true)
            },
            expected: revision,
            at: fake.clock().now(),
        }]))
        .unwrap();
    assert!(matches!(
        update_precheck(&registry, &patch(true), revision),
        Err(SettingsError::RevisionMismatch { .. })
    ));
}

// covers: AC-19
#[test]
fn a_location_change_to_the_built_in_default_is_already_done() {
    let root = TestRoot::new().unwrap();
    let fake = FakePlatform::new(&root);
    let registry = registry(&fake);
    let revision = registry.daemon_settings().unwrap().value.revision;
    let request = |target, expected: ResourceRevision| {
        SetLocationRequest::new(LocationKind::Logs, target, expected)
    };

    assert!(matches!(
        set_location_precheck(&registry, &request(LocationTarget::BuiltIn, revision)).unwrap(),
        SettingsCheck::Unchanged(_)
    ));
    assert_eq!(
        set_location_precheck(
            &registry,
            &request(LocationTarget::Path("/Volumes/x".into()), revision)
        )
        .unwrap(),
        SettingsCheck::Proceed
    );
    assert!(matches!(
        set_location_precheck(
            &registry,
            &request(LocationTarget::BuiltIn, revision.next())
        ),
        Err(SettingsError::RevisionMismatch { .. })
    ));
}

fn manifest(fake: &FakePlatform, id: InstanceId) -> InstanceManifest {
    InstanceManifest::new(NewInstanceManifest {
        instance_id: id,
        name: "orders-db".parse().expect("a valid fixture value"),
        display_name: None,
        software: "postgresql".parse().expect("a valid fixture value"),
        provider: ProviderRevisionKey::new(
            "official/postgresql"
                .parse()
                .expect("a valid fixture value"),
            "2026.10.1".parse().expect("a valid fixture value"),
            DIGEST.parse().expect("a valid fixture value"),
        ),
        runtime: ManifestRuntime::managed(
            RuntimeId::generate(),
            "17.6".parse().expect("a valid fixture value"),
            "17".parse().expect("a valid fixture value"),
            Target::new(Os::Macos, Arch::Aarch64),
            DIGEST.parse().expect("a valid fixture value"),
        ),
        created_at: fake.clock().now(),
        created_by_operation: OperationId::generate(),
        stackly_version: daemon_info().version.to_owned(),
    })
}

// covers: AC-19
#[test]
fn the_inventory_lists_the_instances_of_a_custom_data_location_and_skips_a_gone_disk() {
    let root = TestRoot::new().unwrap();
    let fake = FakePlatform::new(&root);
    let platform = fake.platform();
    let prepared = prepare_layout(&platform).unwrap();
    let folder = root.path().join("disk");
    std::fs::create_dir_all(&folder).unwrap();
    let location = StorageLocationId::generate();
    let made = prepare_location(&platform, &folder, LocationKind::Data, location).unwrap();
    prepared
        .locations()
        .add_location(location, LocationKind::Data, made.clone());

    let builtin = InstanceId::generate();
    create_instance_layout(&platform, &prepared, &manifest(&fake, builtin), &[]).unwrap();
    let custom = InstanceId::generate();
    prepared
        .locations()
        .note_instance(custom, location, LocationKind::Logs.built_in_id());
    let paths =
        create_instance_layout(&platform, &prepared, &manifest(&fake, custom), &[builtin]).unwrap();
    assert!(paths.root.starts_with(made.join("instances")));

    let found = |entries: &[InventoryEntry]| -> Vec<InstanceId> {
        entries
            .iter()
            .filter_map(|entry| match entry {
                InventoryEntry::Found { manifest, .. } => Some(manifest.instance_id()),
                _ => None,
            })
            .collect()
    };
    let inventory = inventory_instances(&platform, &prepared).unwrap();
    assert_eq!(found(inventory.entries()), [builtin, custom]);

    std::fs::remove_dir_all(&made).unwrap();
    let inventory = inventory_instances(&platform, &prepared).unwrap();
    assert_eq!(found(inventory.entries()), [builtin]);
}
