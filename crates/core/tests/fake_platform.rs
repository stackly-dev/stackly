//! Every core use case through the public API on `FakePlatform` (spec 0004,
//! NFR-010): core reaches the OS only through the platform ports, so the
//! whole instance folder journey runs on the fakes, stays inside the test
//! root, and fails cleanly on a platform that has no adapter.
#![cfg(unix)]

use stackly_core::{
    CoreError, InstanceLocation, InventoryEntry, check_data_root_overlap, check_owned_path,
    create_instance_layout, daemon_info, ensure_instance_run_dir, inventory_instances,
    prepare_layout, read_instance_manifest, write_instance_manifest,
};
use stackly_domain::{
    Arch, InstanceId, InstanceManifest, ManifestRuntime, NewInstanceManifest, OperationId, Os,
    ProviderRevisionKey, RetainedMarker, RuntimeId, Target,
};
use stackly_platform::{Clock, PlatformCapability, PlatformError, RootKind, UnsupportedPlatform};
use stackly_testkit::evidence::{Kind, evidence};
use stackly_testkit::{FakePlatform, FsOp, TestRoot};

const DIGEST: &str = "da578821d15e9c0f59eb8e4672962c6f032074e802684576debc6dfc7f86815f";

/// The manifest a create of instance `id` writes, with its times from the
/// platform clock and its version from `daemon_info`.
fn new_manifest(fake: &FakePlatform, id: InstanceId) -> InstanceManifest {
    InstanceManifest::new(NewInstanceManifest {
        instance_id: id,
        name: "orders-db".parse().expect("orders-db is an instance name"),
        display_name: None,
        software: "postgresql".parse().expect("postgresql is a software name"),
        provider: ProviderRevisionKey::new(
            "official/postgresql"
                .parse()
                .expect("official/postgresql is a provider name"),
            "2026.10.1".parse().expect("2026.10.1 is a revision label"),
            DIGEST.parse().expect("DIGEST is 64 hex characters"),
        ),
        runtime: ManifestRuntime::managed(
            RuntimeId::generate(),
            "17.6".parse().expect("17.6 is an exact version"),
            "17".parse().expect("17 is a runtime line"),
            Target::new(Os::Macos, Arch::Aarch64),
            DIGEST.parse().expect("DIGEST is 64 hex characters"),
        ),
        created_at: fake.clock().now(),
        created_by_operation: OperationId::generate(),
        stackly_version: daemon_info().version.to_owned(),
    })
}

#[test]
fn core_use_cases_run_on_the_fake_platform() {
    let ev = evidence!(
        covers = ["NFR-010"],
        kind = Kind::Fixture,
        expected = "prepare, run folder, owned path, data root overlap, create, manifest rewrite and read, and inventory all run on FakePlatform through the platform ports, write only inside the test root, and never trip the fake's confinement",
    );
    let root = TestRoot::new().unwrap();
    let fake = FakePlatform::new(&root);
    let platform = fake.platform();

    let prepared = prepare_layout(&platform).unwrap();
    let first = InstanceId::generate();
    let second = InstanceId::generate();
    let mut initialized = new_manifest(&fake, first);
    let first_paths = create_instance_layout(&platform, &prepared, &initialized, &[]).unwrap();
    let mut retained = new_manifest(&fake, second);
    let second_paths = create_instance_layout(&platform, &prepared, &retained, &[first]).unwrap();

    let run_dir = ensure_instance_run_dir(&platform, &prepared, first.short_key()).unwrap();
    assert_eq!(
        run_dir,
        prepared.layout().instance_run_dir(first.short_key())
    );
    let owned = check_owned_path(&platform, &prepared, &first_paths.data).unwrap();
    assert_eq!(owned.root(), RootKind::Data);
    check_data_root_overlap(&platform, &second_paths.data, [&first_paths.data]).unwrap();
    let overlap = check_data_root_overlap(&platform, &first_paths.data, [&first_paths.root]);
    assert!(
        matches!(overlap, Err(CoreError::Invariant(_))),
        "{overlap:?}"
    );

    fake.clock()
        .set("2026-01-01T00:05:00Z".parse().expect("a valid timestamp"));
    let now = fake.clock().now();
    initialized.record_initialized(now);
    initialized.record_write(now, daemon_info().version.to_owned());
    write_instance_manifest(&platform, &first_paths, &initialized).unwrap();
    assert_eq!(
        read_instance_manifest(&platform, &first_paths).unwrap(),
        initialized
    );
    retained.record_retained(RetainedMarker::new(now, OperationId::generate()));
    retained.record_write(now, daemon_info().version.to_owned());
    write_instance_manifest(&platform, &second_paths, &retained).unwrap();
    // Feature 54 adds the rename port; until then the test moves the folder.
    std::fs::rename(
        &second_paths.root,
        prepared.layout().retained_instance_paths(second).root,
    )
    .unwrap();
    let mutations = fake.fs().mutations();

    let inventory = inventory_instances(&platform, &prepared).unwrap();

    assert_eq!(
        inventory.entries(),
        [
            InventoryEntry::Found {
                location: InstanceLocation::Active,
                manifest: Box::new(initialized),
            },
            InventoryEntry::Found {
                location: InstanceLocation::Retained,
                manifest: Box::new(retained),
            },
        ]
    );
    assert_eq!(fake.fs().mutations(), mutations, "inventory writes nothing");
    assert_eq!(fake.fs().violations(), 0);
    for mutation in &mutations {
        assert!(
            matches!(mutation.op, FsOp::CreateDirPrivate | FsOp::WriteFileAtomic),
            "{mutation:?}"
        );
        assert!(
            mutation.path.starts_with(root.path()) || mutation.path.starts_with(root.run_dir()),
            "{} left the test root",
            mutation.path.display()
        );
    }
    ev.pass();
}

#[test]
fn core_use_cases_report_not_supported_on_a_platform_without_an_adapter() {
    let windows = UnsupportedPlatform::new(Target::new(Os::Windows, Arch::Aarch64));

    let err = prepare_layout(&windows).unwrap_err();

    assert!(
        matches!(
            err,
            CoreError::Platform(PlatformError::NotSupported {
                capability: PlatformCapability::Directories,
                os: Os::Windows,
            })
        ),
        "{err:?}"
    );
}
