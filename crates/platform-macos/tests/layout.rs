//! The macOS adapter on the real filesystem: the default and override roots,
//! override validation, the `fs_conformance` suite for `MacosFilesystem`,
//! the capability statuses, and `prepare_layout`, `create_instance_layout`,
//! and `inventory_instances` through `MacosPlatform::with_roots`. Every
//! write goes to a `TestRoot`; the real per user folders are only ever read.
#![cfg(target_os = "macos")]

use std::fs::Permissions;
use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::Command;

use stackly_core::{
    CoreError, InstanceLocation, InventoryEntry, PreparedLayout, check_data_root_overlap,
    create_instance_layout, daemon_info, ensure_instance_run_dir, inventory_instances,
    prepare_layout, read_instance_manifest, write_instance_manifest,
};
use stackly_domain::{
    Arch, InstanceId, InstanceManifest, ManifestRuntime, NewInstanceManifest, OperationId, Os,
    ProviderRevisionKey, RetainedMarker, Rule, RuntimeId, Target,
};
use stackly_platform::{
    CapabilityStatus, FileInfo, Filesystem, Platform, PlatformCapability, PlatformError,
    RUNTIME_ROOT_MAX, UserId, UserIdentity, validate_root_override,
};
use stackly_platform_macos::{MacosFilesystem, MacosIdentity, MacosPlatform};
use stackly_testkit::{TestRoot, fs_conformance, make_fifo};

const DIGEST: &str = "da578821d15e9c0f59eb8e4672962c6f032074e802684576debc6dfc7f86815f";

/// Runs a command and returns its trimmed stdout.
fn stdout_of(program: &str, args: &[&str]) -> String {
    let output = Command::new(program)
        .args(args)
        .output()
        .expect("the macOS command runs");
    assert!(output.status.success(), "{program} {args:?}: {output:?}");
    String::from_utf8(output.stdout)
        .expect("the command prints UTF-8")
        .trim()
        .to_owned()
}

/// The user's private temporary directory, as `getconf` reports it.
fn user_temp_dir() -> PathBuf {
    PathBuf::from(stdout_of("getconf", &["DARWIN_USER_TEMP_DIR"]))
}

/// The user's home directory from the directory service, not from `HOME`.
fn user_home_dir() -> PathBuf {
    let user = stdout_of("id", &["-un"]);
    let record = stdout_of(
        "dscl",
        &[".", "-read", &format!("/Users/{user}"), "NFSHomeDirectory"],
    );
    let home = record
        .strip_prefix("NFSHomeDirectory:")
        .expect("dscl prints the attribute name first")
        .trim();
    PathBuf::from(home)
}

fn private_dir(path: &Path) {
    std::fs::create_dir(path).expect("the test root accepts a new folder");
    std::fs::set_permissions(path, Permissions::from_mode(0o700))
        .expect("the test owns the folder it just made");
}

#[test]
fn default_roots_come_from_the_user_database_and_the_user_temp_dir() {
    let platform = MacosPlatform::new(None).unwrap();
    let roots = platform.directories().roots().unwrap();

    let home = user_home_dir();
    assert_eq!(
        roots.data(),
        home.join("Library/Application Support/Stackly")
    );
    assert_eq!(roots.logs(), home.join("Library/Logs/Stackly"));
    assert_eq!(roots.cache(), home.join("Library/Caches/Stackly"));
    assert_eq!(roots.runtime(), user_temp_dir().join("stackly"));
    assert!(roots.runtime().as_os_str().len() <= RUNTIME_ROOT_MAX);
}

#[test]
fn an_override_moves_every_root_and_keeps_the_runtime_root_in_the_user_temp_dir() {
    let root = TestRoot::new().unwrap();

    let platform = MacosPlatform::new(Some(root.path().into())).unwrap();
    let roots = platform.directories().roots().unwrap();

    assert_eq!(roots.data(), root.path().join("data"));
    assert_eq!(roots.logs(), root.path().join("logs"));
    assert_eq!(roots.cache(), root.path().join("cache"));
    assert_eq!(roots.runtime().parent().unwrap(), user_temp_dir().as_path());
    assert!(roots.runtime().as_os_str().len() <= RUNTIME_ROOT_MAX);
    assert!(!roots.runtime().exists());
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}

#[test]
fn an_empty_or_relative_override_is_rejected() {
    for (value, reason) in [("", "empty"), ("relative/root", "not absolute")] {
        let result = MacosPlatform::new(Some(value.into()));

        assert!(
            matches!(
                result,
                Err(PlatformError::InvalidRootOverride { reason: actual, .. }) if actual == reason
            ),
            "{value}: {result:?}"
        );
    }
}

#[test]
fn override_validation_rejects_symlinks_open_folders_and_the_default_data_root() {
    let root = TestRoot::new().unwrap();
    let default_data = root.path().join("Stackly");
    private_dir(&default_data);
    let inside = default_data.join("tests");
    private_dir(&inside);
    let target = root.path().join("target");
    private_dir(&target);
    let link = root.path().join("link");
    symlink(&target, &link).unwrap();
    let open = root.path().join("open");
    private_dir(&open);
    std::fs::set_permissions(&open, Permissions::from_mode(0o777)).unwrap();
    let validate =
        |path: &Path| validate_root_override(path, &MacosFilesystem, &MacosIdentity, &default_data);

    for (path, reason) in [
        (&link, "is a symlink"),
        (&open, "writable by group or others"),
        (&default_data, "inside the default data root"),
        (&inside, "inside the default data root"),
        (&root.path().join("missing"), "does not exist"),
    ] {
        let result = validate(path);
        assert!(
            matches!(
                &result,
                Err(PlatformError::InvalidRootOverride { reason: actual, .. }) if *actual == reason
            ),
            "{}: {result:?}",
            path.display()
        );
    }
    assert_eq!(validate(&target).unwrap(), target);
}

#[test]
fn macos_filesystem_passes_the_conformance_suite() {
    let root = TestRoot::new().unwrap();

    fs_conformance(&MacosFilesystem, &root);
}

#[test]
fn identity_is_the_uid_that_owns_new_files() {
    let root = TestRoot::new().unwrap();
    let file = root.path().join("owned");
    std::fs::write(&file, b"").unwrap();

    let user = MacosIdentity.current_user().unwrap();

    assert_eq!(user, UserId::Unix(std::fs::metadata(&file).unwrap().uid()));
}

#[test]
fn the_built_capabilities_are_supported_and_the_rest_name_their_feature() {
    let platform = MacosPlatform::with_roots(TestRoot::new().unwrap().roots()).unwrap();
    let info = platform.info();
    let caps = info.capabilities();

    assert_eq!(info.target().os(), Os::Macos);
    for capability in PlatformCapability::ALL {
        let status = caps.status(capability);
        match capability {
            PlatformCapability::Directories
            | PlatformCapability::Filesystem
            | PlatformCapability::Clock
            | PlatformCapability::UserIdentity
            | PlatformCapability::ProcessSupervision
            | PlatformCapability::SecretStore
            | PlatformCapability::IpcEndpoint
            | PlatformCapability::NetworkFetch
            | PlatformCapability::CodeSigning => {
                assert_eq!(status, CapabilityStatus::Supported, "{capability}");
            }
            _ => assert!(
                matches!(status, CapabilityStatus::NotSupported { reason } if reason.contains("(feature ")),
                "{capability}: {status:?}"
            ),
        }
    }
}

#[test]
fn prepare_layout_makes_a_private_layout_on_the_real_filesystem() {
    let root = TestRoot::new().unwrap();
    let platform =
        MacosPlatform::with_roots(root.roots()).expect("this Mac reports its OS version");

    let prepared = prepare_layout(&platform).unwrap();
    let again = prepare_layout(&platform).unwrap();
    let run_dir =
        ensure_instance_run_dir(&platform, &prepared, InstanceId::generate().short_key()).unwrap();

    assert_eq!(again, prepared);
    let layout = prepared.layout();
    let mut dirs: Vec<PathBuf> = layout
        .owned_roots()
        .iter()
        .map(|dir| dir.to_path_buf())
        .collect();
    dirs.extend(layout.standard_dirs());
    dirs.push(run_dir);
    let uid = std::fs::metadata(root.path()).unwrap().uid();
    for dir in &dirs {
        let info = MacosFilesystem.symlink_metadata(dir).unwrap().unwrap();
        assert_eq!(info.mode, Some(0o700), "{}", dir.display());
        assert_eq!(info.owner, Some(UserId::Unix(uid)), "{}", dir.display());
    }
}

/// The manifest a create of instance `id` writes, with its time from the
/// platform clock and its version from `daemon_info`.
fn new_manifest(platform: &Platform, id: InstanceId) -> InstanceManifest {
    InstanceManifest::new(NewInstanceManifest {
        instance_id: id,
        name: "orders-db".parse().expect("orders-db is an instance name"),
        display_name: Some(
            "Orders database"
                .parse()
                .expect("Orders database is a display name"),
        ),
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
        created_at: platform.clock().now(),
        created_by_operation: OperationId::generate(),
        stackly_version: daemon_info().version.to_owned(),
    })
}

/// Creates an instance with `known` taken from the inventory, as the create
/// use case will before the registry exists.
fn create(platform: &Platform, prepared: &PreparedLayout) -> InstanceManifest {
    let known: Vec<InstanceId> = inventory_instances(platform, prepared)
        .expect("the prepared layout lists")
        .entries()
        .iter()
        .filter_map(|entry| match entry {
            InventoryEntry::Found { manifest, .. } => Some(manifest.instance_id()),
            _ => None,
        })
        .collect();
    let manifest = new_manifest(platform, InstanceId::generate());
    create_instance_layout(platform, prepared, &manifest, &known)
        .expect("a fresh ID creates its folder");
    manifest
}

/// The metadata of each default root, read without following symlinks.
fn default_root_metadata() -> Vec<(PathBuf, Option<FileInfo>)> {
    let roots = MacosPlatform::new(None)
        .expect("the default roots resolve")
        .directories()
        .roots()
        .expect("the default roots resolve");
    roots
        .entries()
        .iter()
        .map(|(_, root)| {
            let info = MacosFilesystem
                .symlink_metadata(root)
                .expect("a default root can be inspected");
            (root.to_path_buf(), info)
        })
        .collect()
}

#[test]
fn instances_are_created_and_inventoried_without_touching_the_default_roots() {
    let before = default_root_metadata();
    let root = TestRoot::new().unwrap();
    let platform =
        MacosPlatform::with_roots(root.roots()).expect("this Mac reports its OS version");
    let prepared = prepare_layout(&platform).unwrap();
    let mut initialized = create(&platform, &prepared);
    let mut retained = create(&platform, &prepared);
    let layout = prepared.layout();

    let now = platform.clock().now();
    initialized.record_initialized(now);
    initialized.record_write(now, daemon_info().version.to_owned());
    let active = layout.instance_paths(initialized.instance_id());
    write_instance_manifest(&platform, &active, &initialized).unwrap();
    retained.record_retained(RetainedMarker::new(now, OperationId::generate()));
    retained.record_write(now, daemon_info().version.to_owned());
    let moving = layout.instance_paths(retained.instance_id());
    write_instance_manifest(&platform, &moving, &retained).unwrap();
    std::fs::rename(
        &moving.root,
        layout.retained_instance_paths(retained.instance_id()).root,
    )
    .unwrap();
    let inventory = inventory_instances(&platform, &prepared).unwrap();

    assert_eq!(
        inventory.entries(),
        [
            InventoryEntry::Found {
                location: InstanceLocation::Active,
                manifest: Box::new(initialized.clone()),
            },
            InventoryEntry::Found {
                location: InstanceLocation::Retained,
                manifest: Box::new(retained),
            },
        ]
    );
    assert_eq!(
        read_instance_manifest(&platform, &active).unwrap(),
        initialized
    );
    let uid = std::fs::metadata(root.path()).unwrap().uid();
    for dir in [
        &active.root,
        &active.data,
        &active.config,
        &active.config_history,
        &active.temp,
        &active.logs,
    ] {
        let info = MacosFilesystem.symlink_metadata(dir).unwrap().unwrap();
        assert_eq!(info.mode, Some(0o700), "{}", dir.display());
        assert_eq!(info.owner, Some(UserId::Unix(uid)), "{}", dir.display());
    }
    let manifest = MacosFilesystem
        .symlink_metadata(&active.manifest)
        .unwrap()
        .unwrap();
    assert_eq!(manifest.mode, Some(0o600));
    assert!(!active.run.exists());
    assert_eq!(default_root_metadata(), before);
}

#[test]
fn an_existing_folder_and_an_aliased_data_root_are_refused_on_the_real_filesystem() {
    let root = TestRoot::new().unwrap();
    let platform =
        MacosPlatform::with_roots(root.roots()).expect("this Mac reports its OS version");
    let prepared = prepare_layout(&platform).unwrap();
    let manifest = create(&platform, &prepared);
    let paths = prepared.layout().instance_paths(manifest.instance_id());

    let again = create_instance_layout(&platform, &prepared, &manifest, &[]);
    let upper = prepared
        .layout()
        .instances_dir()
        .join(manifest.instance_id().to_string().to_uppercase())
        .join("DATA");
    let alias = root.path().join("alias");
    symlink(&paths.root, &alias).unwrap();

    assert_eq!(
        again,
        Err(CoreError::AlreadyExists {
            path: paths.root.clone()
        })
    );
    for candidate in [alias.join("data"), upper] {
        let result = check_data_root_overlap(&platform, &candidate, [&paths.data]);
        assert_eq!(
            result.as_ref().map_err(CoreError::rule),
            Err(Some(Rule::Br002)),
            "{}: {result:?}",
            candidate.display()
        );
    }
}

#[test]
fn unreadable_manifests_are_classified_and_never_opened_on_the_real_filesystem() {
    let root = TestRoot::new().unwrap();
    let platform =
        MacosPlatform::with_roots(root.roots()).expect("this Mac reports its OS version");
    let prepared = prepare_layout(&platform).unwrap();
    let layout = prepared.layout();
    let mut ids: [InstanceId; 3] = std::array::from_fn(|_| InstanceId::generate());
    ids.sort_by_key(ToString::to_string);
    let [fifo, linked, oversize] = ids.map(|id| layout.instance_paths(id));
    for paths in [&fifo, &linked, &oversize] {
        std::fs::create_dir(&paths.root).unwrap();
    }
    make_fifo(&fifo.manifest).unwrap();
    // A valid manifest moved out of the folder and linked back in.
    write_instance_manifest(&platform, &linked, &new_manifest(&platform, ids[1])).unwrap();
    let elsewhere = root.path().join("manifest-elsewhere.json");
    std::fs::rename(&linked.manifest, &elsewhere).unwrap();
    symlink(&elsewhere, &linked.manifest).unwrap();
    let mut bytes = std::fs::read(&elsewhere).unwrap();
    bytes.resize(64 * 1024 + 1, b' ');
    std::fs::write(&oversize.manifest, &bytes).unwrap();

    let inventory = inventory_instances(&platform, &prepared).unwrap();
    let direct = read_instance_manifest(&platform, &fifo);

    let unreadable = |paths: &stackly_platform::InstancePaths, reason: &str| {
        InventoryEntry::UnreadableManifest {
            dir: paths.root.clone(),
            reason: reason.to_owned(),
        }
    };
    assert_eq!(
        inventory.entries(),
        [
            unreadable(&fifo, "not a regular file"),
            unreadable(&linked, "not a regular file"),
            unreadable(&oversize, "too large"),
        ]
    );
    assert!(
        matches!(
            direct,
            Err(CoreError::Platform(PlatformError::NotARegularFile { .. }))
        ),
        "{direct:?}"
    );
}

#[test]
fn a_failed_manifest_write_keeps_the_old_target_and_leaves_no_temporary_file() {
    let root = TestRoot::new().unwrap();
    let platform =
        MacosPlatform::with_roots(root.roots()).expect("this Mac reports its OS version");
    let prepared = prepare_layout(&platform).unwrap();
    let manifest = new_manifest(&platform, InstanceId::generate());
    let paths = prepared.layout().instance_paths(manifest.instance_id());
    std::fs::create_dir(&paths.root).unwrap();
    // A rename over a non empty folder fails, like a full disk would.
    std::fs::create_dir(&paths.manifest).unwrap();
    std::fs::write(paths.manifest.join("keep"), b"k").unwrap();

    let result = write_instance_manifest(&platform, &paths, &manifest);

    assert!(matches!(result, Err(CoreError::Platform(_))), "{result:?}");
    assert!(paths.manifest.join("keep").is_file());
    let leftovers: Vec<_> = std::fs::read_dir(&paths.root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .filter(|name| name.to_string_lossy().contains(".tmp-"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}
