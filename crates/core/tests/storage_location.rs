//! The location rules (spec 0032, *Aturan lokasi*): each rule is refused
//! with its own code, a folder that breaks several reports the first in the
//! order of the table, and the folders of a location are made private with a
//! marker.
#![cfg(unix)]

use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};

use stackly_core::storage::{
    FolderFact, LOCATION_DIR, LocationContext, LocationVerdict, MARKER_FILE, Marker,
    check_location, gather_location_facts, prepare_location,
};
use stackly_domain::{LocationKind, LocationRule, StorageLocationId};
use stackly_platform::{Platform, UserId};
use stackly_testkit::{FakePlatform, FakeVolume, TestRoot};

struct Site {
    _root: TestRoot,
    fake: FakePlatform,
    platform: Platform,
    base: PathBuf,
    home: PathBuf,
}

impl Site {
    fn new() -> Self {
        let root = TestRoot::new().expect("a test root can be made");
        let fake = FakePlatform::new(&root);
        let platform = fake.platform();
        let base = root.path().join("disk");
        let home = root.path().join("home");
        fs::create_dir_all(&base).expect("the base folder can be made");
        fs::create_dir_all(&home).expect("the home folder can be made");
        Self {
            _root: root,
            fake,
            platform,
            base,
            home,
        }
    }

    fn folder(&self, name: &str) -> PathBuf {
        let path = self.base.join(name);
        fs::create_dir_all(&path).expect("a folder can be made");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("the mode can be set");
        path
    }

    fn context(&self) -> LocationContext {
        LocationContext::new(Some(self.home.clone()), Vec::new(), Vec::new())
    }

    fn judge(
        &self,
        kind: LocationKind,
        path: &Path,
        context: &LocationContext,
    ) -> Result<LocationVerdict, LocationRule> {
        let facts = gather_location_facts(&self.platform, path).expect("the facts can be read");
        check_location(kind, &facts, context)
    }

    fn rule(&self, path: &Path) -> LocationRule {
        self.judge(LocationKind::Data, path, &self.context())
            .expect_err("the folder breaks a rule")
    }
}

#[test]
fn a_plain_folder_on_an_internal_disk_passes() {
    let site = Site::new();
    let folder = site.folder("work");

    assert_eq!(
        site.judge(LocationKind::Data, &folder, &site.context()),
        Ok(LocationVerdict::New)
    );
}

#[test]
fn a_relative_path_is_not_absolute() {
    let site = Site::new();

    assert_eq!(
        site.rule(Path::new("work/folder")),
        LocationRule::NotAbsolute
    );
}

#[test]
fn a_folder_that_is_not_there_is_missing() {
    let site = Site::new();

    assert_eq!(site.rule(&site.base.join("nothing")), LocationRule::Missing);
}

#[test]
fn a_file_is_not_a_directory() {
    let site = Site::new();
    let file = site.base.join("file");
    fs::write(&file, b"x").unwrap();

    assert_eq!(site.rule(&file), LocationRule::NotDirectory);
}

#[test]
fn a_link_to_a_folder_is_a_symlink_and_so_is_a_folder_below_one() {
    let site = Site::new();
    let real = site.folder("real");
    let link = site.base.join("alias");
    symlink(&real, &link).unwrap();
    fs::create_dir_all(real.join("inner")).unwrap();

    assert_eq!(site.rule(&link), LocationRule::Symlink);
    assert_eq!(site.rule(&link.join("inner")), LocationRule::Symlink);
}

#[test]
fn a_folder_of_another_user_is_not_owned() {
    let site = Site::new();
    let folder = site.folder("theirs");
    site.fake.identity().set_current(UserId::Unix(1));

    assert_eq!(site.rule(&folder), LocationRule::NotOwned);
}

#[test]
fn a_folder_others_can_write_to_is_too_open() {
    let site = Site::new();
    let group = site.folder("group");
    let other = site.folder("other");
    fs::set_permissions(&group, fs::Permissions::from_mode(0o775)).unwrap();
    fs::set_permissions(&other, fs::Permissions::from_mode(0o757)).unwrap();

    assert_eq!(site.rule(&group), LocationRule::TooOpen);
    assert_eq!(site.rule(&other), LocationRule::TooOpen);
}

#[test]
fn the_folders_macos_guards_are_protected() {
    let site = Site::new();
    for parts in [
        vec!["Desktop"],
        vec!["Documents", "projects"],
        vec!["Downloads"],
        vec!["Library", "Mobile Documents"],
        vec!["Library", "CloudStorage", "drive"],
    ] {
        let folder = parts
            .iter()
            .fold(site.home.clone(), |dir, part| dir.join(part));
        fs::create_dir_all(&folder).unwrap();

        assert_eq!(
            site.rule(&folder),
            LocationRule::ProtectedFolder,
            "{parts:?}"
        );
    }
    let fine = site.home.join("Work");
    fs::create_dir_all(&fine).unwrap();
    assert_eq!(
        site.judge(LocationKind::Logs, &fine, &site.context()),
        Ok(LocationVerdict::New)
    );
}

#[test]
fn a_network_share_is_not_a_local_volume_and_an_external_disk_is_not_internal() {
    let site = Site::new();
    let share = site.folder("share");
    let usb = site.folder("usb");
    site.fake
        .fs()
        .set_volume(&share, FakeVolume::DEFAULT.with_kind("smbfs", false));
    site.fake
        .fs()
        .set_volume(&usb, FakeVolume::DEFAULT.external());

    assert_eq!(site.rule(&share), LocationRule::NotLocalVolume);
    assert_eq!(site.rule(&usb), LocationRule::NotInternalVolume);
}

#[test]
fn a_stackly_folder_that_would_hold_a_root_or_another_location_overlaps() {
    let site = Site::new();
    let folder = site.folder("work");
    let canonical = fs::canonicalize(&folder).unwrap();
    let target = canonical.join(LOCATION_DIR);

    let inside_root = LocationContext::new(None, vec![target.join("deeper")], Vec::new());
    let in_root = LocationContext::new(None, vec![canonical.clone()], Vec::new());
    let other = LocationContext::new(
        None,
        Vec::new(),
        vec![(
            StorageLocationId::generate(),
            LocationKind::Logs,
            target.clone(),
        )],
    );

    for context in [inside_root, in_root, other] {
        assert_eq!(
            site.judge(LocationKind::Data, &folder, &context),
            Err(LocationRule::OverlapsRoot)
        );
    }
}

#[test]
fn a_stackly_folder_without_a_known_marker_is_foreign_and_a_known_one_is_used_again() {
    let site = Site::new();
    let folder = site.folder("work");
    let canonical = fs::canonicalize(&folder).unwrap();
    let target = canonical.join(LOCATION_DIR);
    fs::create_dir(&target).unwrap();

    assert_eq!(site.rule(&folder), LocationRule::ForeignMarker);

    let id = StorageLocationId::generate();
    fs::write(
        target.join(MARKER_FILE),
        serde_json::to_vec(&Marker {
            id,
            kind: "data".to_owned(),
        })
        .unwrap(),
    )
    .unwrap();
    let known = LocationContext::new(
        None,
        Vec::new(),
        vec![(id, LocationKind::Data, target.clone())],
    );

    // Not in the registry, another kind, or another ID: still foreign.
    assert_eq!(site.rule(&folder), LocationRule::ForeignMarker);
    assert_eq!(
        site.judge(LocationKind::Logs, &folder, &known),
        Err(LocationRule::OverlapsRoot)
    );
    assert_eq!(
        site.judge(LocationKind::Data, &folder, &known),
        Ok(LocationVerdict::Reuse(id))
    );
}

#[test]
fn a_folder_that_breaks_several_rules_reports_the_first_in_the_table_order() {
    let site = Site::new();
    // Too open, on an external disk, and under a protected folder.
    let folder = site.home.join("Documents").join("far");
    fs::create_dir_all(&folder).unwrap();
    fs::set_permissions(&folder, fs::Permissions::from_mode(0o777)).unwrap();
    site.fake
        .fs()
        .set_volume(&folder, FakeVolume::DEFAULT.external());
    assert_eq!(site.rule(&folder), LocationRule::TooOpen);

    fs::set_permissions(&folder, fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(site.rule(&folder), LocationRule::ProtectedFolder);

    let outside = site.folder("far");
    site.fake
        .fs()
        .set_volume(&outside, FakeVolume::DEFAULT.external());
    fs::create_dir(outside.join(LOCATION_DIR)).unwrap();
    assert_eq!(site.rule(&outside), LocationRule::NotInternalVolume);
}

#[test]
fn the_facts_of_a_missing_stackly_folder_say_absent() {
    let site = Site::new();
    let folder = site.folder("work");
    let facts = gather_location_facts(&site.platform, &folder).unwrap();

    assert_eq!(facts.folder, FolderFact::Absent);
    assert!(facts.exists && facts.is_dir && facts.owned && !facts.open_to_write);
}

#[test]
fn preparing_a_location_makes_private_folders_and_a_marker_and_leaves_content_alone() {
    let site = Site::new();
    let folder = site.folder("work");
    let id = StorageLocationId::generate();
    let fs_port = site.platform.fs();

    let location = prepare_location(fs_port, &folder, LocationKind::Data, id).unwrap();

    assert_eq!(location, folder.join(LOCATION_DIR));
    let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&location), 0o700);
    assert_eq!(mode(&location.join("instances")), 0o700);
    assert!(!location.join("downloads").exists());
    let marker: Marker =
        serde_json::from_slice(&fs::read(location.join(MARKER_FILE)).unwrap()).unwrap();
    assert_eq!(marker.id, id);
    assert_eq!(marker.kind, "data");
    assert_eq!(mode(&location.join(MARKER_FILE)), 0o600);

    fs::write(location.join("instances").join("kept"), b"data").unwrap();
    prepare_location(fs_port, &folder, LocationKind::Data, id).unwrap();
    assert_eq!(
        fs::read(location.join("instances").join("kept")).unwrap(),
        b"data"
    );

    let cache = site.folder("cache");
    let made = prepare_location(
        fs_port,
        &cache,
        LocationKind::Cache,
        StorageLocationId::generate(),
    )
    .unwrap();
    assert!(made.join("downloads").is_dir());
    assert!(!made.join("instances").exists());
}

#[test]
fn a_folder_the_daemon_cannot_write_to_is_not_writable() {
    let site = Site::new();
    let folder = site.folder("locked");
    fs::set_permissions(&folder, fs::Permissions::from_mode(0o500)).unwrap();
    let writable = fs::File::create(folder.join("probe")).is_ok();

    let result = prepare_location(
        site.platform.fs(),
        &folder,
        LocationKind::Logs,
        StorageLocationId::generate(),
    );

    fs::set_permissions(&folder, fs::Permissions::from_mode(0o700)).unwrap();
    if writable {
        // The tests run as a user the mode does not bind (root).
        return;
    }
    assert_eq!(result, Err(LocationRule::NotWritable));
}
