use std::ffi::{OsStr, OsString};
use std::fmt;
use std::path::{Path, PathBuf};

use stackly_domain::{InstanceId, InstanceManifest};
use stackly_platform::{
    DirEntryInfo, DirectoryLayout, FileKind, Filesystem, Platform, PlatformError,
};

use crate::error::{CoreError, ManifestError};
use crate::instance_dir::{INSTANCE_MANIFEST_MAX_LEN, decode_manifest};
use crate::layout::PreparedLayout;

/// A file macOS Finder leaves in folders; the inventory skips it.
const FINDER_METADATA: &str = ".DS_Store";
/// The reason given for a manifest that is a symlink, folder, FIFO, or
/// device.
const NOT_A_REGULAR_FILE: &str = "not a regular file";
/// The reason given for a manifest over [`INSTANCE_MANIFEST_MAX_LEN`].
const TOO_LARGE: &str = "too large";

/// Where an instance folder lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InstanceLocation {
    /// Under `instances/`.
    Active,
    /// Under `retained/`, after a remove that kept the data.
    Retained,
}

impl InstanceLocation {
    /// Both locations, in inventory order.
    pub const ALL: [Self; 2] = [Self::Active, Self::Retained];

    /// Returns the stable code, `active` or `retained`.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Retained => "retained",
        }
    }
}

impl fmt::Display for InstanceLocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_code())
    }
}

/// One entry of `instances/` or `retained/`, classified from the disk alone.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum InventoryEntry {
    /// An instance folder whose manifest matches its name and location.
    Found {
        /// Where the folder lives.
        location: InstanceLocation,
        /// The decoded manifest, boxed because it is far larger than the
        /// other entries.
        manifest: Box<InstanceManifest>,
    },
    /// An instance folder without `instance-manifest.json`.
    MissingManifest {
        /// The instance folder.
        dir: PathBuf,
    },
    /// An instance folder whose manifest cannot be read or decoded: not a
    /// regular file, over 64 KiB, unreadable, or malformed.
    UnreadableManifest {
        /// The instance folder.
        dir: PathBuf,
        /// Why, for example `not a regular file` or `too large`.
        reason: String,
    },
    /// An instance folder whose manifest has a schema this version does not
    /// read.
    UnsupportedSchema {
        /// The instance folder.
        dir: PathBuf,
        /// The manifest's `schema` value.
        schema: String,
    },
    /// An instance folder named by a valid ID whose manifest names another.
    IdMismatch {
        /// The instance folder.
        dir: PathBuf,
        /// The ID the manifest holds.
        manifest_id: InstanceId,
    },
    /// A retained marker in `instances/`, or none in `retained/`.
    LocationMismatch {
        /// The instance folder.
        dir: PathBuf,
    },
    /// A symlink where an instance folder belongs. It is never followed.
    Symlink {
        /// The symlink itself.
        path: PathBuf,
    },
    /// Anything else: a file, a FIFO, or a folder whose name is not a
    /// lowercase instance ID.
    UnexpectedEntry {
        /// The entry's name.
        name: OsString,
        /// What the entry is.
        kind: FileKind,
    },
}

/// What [`inventory_instances`] found, in order: every entry of
/// `instances/` by name, then every entry of `retained/` by name.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Inventory {
    entries: Vec<InventoryEntry>,
}

impl Inventory {
    /// Returns every entry, in inventory order.
    #[must_use]
    pub fn entries(&self) -> &[InventoryEntry] {
        &self.entries
    }

    /// Returns the entries, consuming the inventory.
    #[must_use]
    pub fn into_entries(self) -> Vec<InventoryEntry> {
        self.entries
    }
}

/// Lists every instance folder from the disk alone, without the registry.
///
/// Each entry of `instances/` and then of `retained/`, each sorted by name,
/// is classified with the data of its manifest (see [`InventoryEntry`]).
/// `.DS_Store` is skipped. A symlink is never followed, a manifest that is
/// not a regular file is never opened, a manifest over
/// [`INSTANCE_MANIFEST_MAX_LEN`] is recorded as unreadable, and nothing on
/// disk is changed.
///
/// # Errors
///
/// Returns [`CoreError::Platform`] only when `instances/` or `retained/`
/// itself cannot be listed. A problem with one entry becomes that entry's
/// classification.
pub fn inventory_instances(
    platform: &Platform,
    prepared: &PreparedLayout,
) -> Result<Inventory, CoreError> {
    let fs = platform.fs();
    let layout = prepared.layout();
    let mut entries = Vec::new();
    for location in InstanceLocation::ALL {
        let parent = match location {
            InstanceLocation::Active => layout.instances_dir(),
            InstanceLocation::Retained => layout.retained_dir(),
        };
        let mut listing = fs.read_dir(&parent)?;
        listing.sort_by(|a, b| a.name.cmp(&b.name));
        entries.extend(
            listing
                .into_iter()
                .filter(|entry| entry.name != FINDER_METADATA)
                .map(|entry| classify(fs, layout, location, &parent, entry)),
        );
    }
    Ok(Inventory { entries })
}

fn classify(
    fs: &dyn Filesystem,
    layout: &DirectoryLayout,
    location: InstanceLocation,
    parent: &Path,
    entry: DirEntryInfo,
) -> InventoryEntry {
    match entry.info.kind {
        FileKind::Dir => {}
        FileKind::Symlink => {
            return InventoryEntry::Symlink {
                path: parent.join(&entry.name),
            };
        }
        kind @ (FileKind::File | FileKind::Other) => {
            return InventoryEntry::UnexpectedEntry {
                name: entry.name,
                kind,
            };
        }
    }
    let Some(id) = folder_id(&entry.name) else {
        return InventoryEntry::UnexpectedEntry {
            name: entry.name,
            kind: FileKind::Dir,
        };
    };
    let paths = match location {
        InstanceLocation::Active => layout.instance_paths(id),
        InstanceLocation::Retained => layout.retained_instance_paths(id),
    };
    let dir = paths.root;
    let manifest = match read_manifest(fs, &paths.manifest) {
        Ok(manifest) => manifest,
        Err(Problem::Missing) => return InventoryEntry::MissingManifest { dir },
        Err(Problem::Unreadable(reason)) => {
            return InventoryEntry::UnreadableManifest { dir, reason };
        }
        Err(Problem::UnsupportedSchema(schema)) => {
            return InventoryEntry::UnsupportedSchema { dir, schema };
        }
    };
    if manifest.instance_id() != id {
        return InventoryEntry::IdMismatch {
            dir,
            manifest_id: manifest.instance_id(),
        };
    }
    let retained = manifest.retained().is_some();
    if retained != (location == InstanceLocation::Retained) {
        return InventoryEntry::LocationMismatch { dir };
    }
    InventoryEntry::Found {
        location,
        manifest: Box::new(manifest),
    }
}

/// Returns the ID a folder is named by, only when the name is the ID's
/// canonical lowercase form.
fn folder_id(name: &OsStr) -> Option<InstanceId> {
    let name = name.to_str()?;
    let id: InstanceId = name.parse().ok()?;
    (id.to_string() == name).then_some(id)
}

/// Why a manifest did not decode.
enum Problem {
    Missing,
    Unreadable(String),
    UnsupportedSchema(String),
}

/// Reads one manifest, looking at it first so a symlink, FIFO, device, or
/// oversized file is never opened.
fn read_manifest(fs: &dyn Filesystem, path: &Path) -> Result<InstanceManifest, Problem> {
    match fs.symlink_metadata(path) {
        Ok(None) => return Err(Problem::Missing),
        Ok(Some(info)) if info.kind != FileKind::File => {
            return Err(Problem::Unreadable(NOT_A_REGULAR_FILE.to_owned()));
        }
        Ok(Some(info)) if info.len > INSTANCE_MANIFEST_MAX_LEN => {
            return Err(Problem::Unreadable(TOO_LARGE.to_owned()));
        }
        Ok(Some(_)) => {}
        Err(err) => return Err(Problem::Unreadable(err.to_string())),
    }
    // The file may change between the look and the read; the read itself
    // never follows a symlink, never reads a FIFO, and stops at the limit.
    let bytes = fs
        .read_file_limited(path, INSTANCE_MANIFEST_MAX_LEN)
        .map_err(|err| match err {
            PlatformError::NotFound { .. } => Problem::Missing,
            PlatformError::NotARegularFile { .. } => {
                Problem::Unreadable(NOT_A_REGULAR_FILE.to_owned())
            }
            PlatformError::FileTooLarge { .. } => Problem::Unreadable(TOO_LARGE.to_owned()),
            other => Problem::Unreadable(other.to_string()),
        })?;
    decode_manifest(&bytes).map_err(|err| match err {
        ManifestError::UnsupportedSchema { schema } => Problem::UnsupportedSchema(schema),
        other => Problem::Unreadable(other.to_string()),
    })
}

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use std::os::unix::fs::symlink;
    use std::time::SystemTime;

    use stackly_domain::{OperationId, RetainedMarker};
    use stackly_testkit::evidence::{Kind, evidence};
    use stackly_testkit::{FakePlatform, TestRoot, make_fifo};

    use super::*;
    use crate::instance_dir::{create_instance_layout, encode_manifest, write_instance_manifest};
    use crate::test_support::{manifest_for, prepared};
    use crate::{daemon_info, read_instance_manifest};

    const V1: &str = include_str!("../../domain/tests/fixtures/instance-manifest-v1.json");
    const V2: &str = include_str!("../../domain/tests/fixtures/instance-manifest-v2.json");

    fn known_ids(platform: &Platform, prepared: &PreparedLayout) -> Vec<InstanceId> {
        inventory_instances(platform, prepared)
            .unwrap()
            .entries()
            .iter()
            .filter_map(|entry| match entry {
                InventoryEntry::Found { manifest, .. } => Some(manifest.instance_id()),
                _ => None,
            })
            .collect()
    }

    /// Creates an instance the way the create use case will: `known` comes
    /// from the inventory.
    fn create(fake: &FakePlatform, prepared: &PreparedLayout, id: InstanceId) -> InstanceManifest {
        let platform = fake.platform();
        let manifest = manifest_for(&platform, id);
        let known = known_ids(&platform, prepared);
        create_instance_layout(&platform, prepared, &manifest, &known).unwrap();
        manifest
    }

    /// Marks the instance retained and moves its folder, as feature 54 will
    /// do through a rename the filesystem port does not have yet.
    fn retain(fake: &FakePlatform, prepared: &PreparedLayout, manifest: &mut InstanceManifest) {
        let platform = fake.platform();
        let now = platform.clock().now();
        let id = manifest.instance_id();
        manifest.record_retained(RetainedMarker::new(now, OperationId::generate()));
        manifest.record_write(now, daemon_info().version.to_owned());
        let active = prepared.layout().instance_paths(id);
        write_instance_manifest(&platform, &active, manifest).unwrap();
        std::fs::rename(
            &active.root,
            prepared.layout().retained_instance_paths(id).root,
        )
        .unwrap();
    }

    /// Every path under `dir` with its kind, length, and modification time,
    /// read without following symlinks.
    fn tree(dir: &Path) -> Vec<(PathBuf, std::fs::FileType, u64, SystemTime)> {
        let mut found = Vec::new();
        let mut pending = vec![dir.to_path_buf()];
        while let Some(current) = pending.pop() {
            for entry in std::fs::read_dir(&current).unwrap() {
                let path = entry.unwrap().path();
                let metadata = std::fs::symlink_metadata(&path).unwrap();
                if metadata.is_dir() {
                    pending.push(path.clone());
                }
                found.push((
                    path,
                    metadata.file_type(),
                    metadata.len(),
                    metadata.modified().unwrap(),
                ));
            }
        }
        found.sort_by(|a, b| a.0.cmp(&b.0));
        found
    }

    #[test]
    fn inventory_reads_folders_without_the_registry() {
        let ev = evidence!(
            covers = ["REQ-053"],
            kind = Kind::Fixture,
            expected = "after creating two instances, initializing one, and retaining the other, the inventory lists both from their folders alone, in order and in the right location, without writing anything",
        );
        let root = TestRoot::new().unwrap();
        let (fake, prepared) = prepared(&root);
        let platform = fake.platform();
        let mut initialized = create(&fake, &prepared, InstanceId::generate());
        let mut retained = create(&fake, &prepared, InstanceId::generate());
        fake.clock().set("2026-01-01T00:05:00Z".parse().unwrap());
        let now = platform.clock().now();
        initialized.record_initialized(now);
        initialized.record_write(now, daemon_info().version.to_owned());
        let paths = prepared.layout().instance_paths(initialized.instance_id());
        write_instance_manifest(&platform, &paths, &initialized).unwrap();
        retain(&fake, &prepared, &mut retained);
        let mutations = fake.fs().mutations();

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
                    manifest: Box::new(retained.clone()),
                },
            ]
        );
        assert_eq!(fake.fs().mutations(), mutations);
        assert_eq!(fake.fs().violations(), 0);
        let InventoryEntry::Found { manifest, .. } = &inventory.entries()[0] else {
            panic!("the first entry is found");
        };
        assert_eq!(manifest.data().initialized_at(), Some(now));
        ev.pass();
    }

    #[test]
    fn inventory_lists_active_then_retained_folders_each_in_name_order() {
        let root = TestRoot::new().unwrap();
        let (fake, prepared) = prepared(&root);
        let platform = fake.platform();
        let mut ids: Vec<InstanceId> = (0..5).map(|_| InstanceId::generate()).collect();
        ids.sort_by_key(|id| std::cmp::Reverse(id.to_string()));
        let mut manifests: Vec<InstanceManifest> =
            ids.iter().map(|id| create(&fake, &prepared, *id)).collect();
        for manifest in manifests.iter_mut().step_by(2) {
            retain(&fake, &prepared, manifest);
        }

        let inventory = inventory_instances(&platform, &prepared).unwrap();

        let mut expected: Vec<(InstanceLocation, String)> = manifests
            .iter()
            .map(|manifest| {
                let location = if manifest.retained().is_some() {
                    InstanceLocation::Retained
                } else {
                    InstanceLocation::Active
                };
                (location, manifest.instance_id().to_string())
            })
            .collect();
        expected
            .sort_by_key(|(location, id)| (*location == InstanceLocation::Retained, id.clone()));
        let actual: Vec<(InstanceLocation, String)> = inventory
            .entries()
            .iter()
            .map(|entry| match entry {
                InventoryEntry::Found { location, manifest } => {
                    (*location, manifest.instance_id().to_string())
                }
                other => panic!("expected only found entries, got {other:?}"),
            })
            .collect();
        assert_eq!(actual, expected);
    }

    #[test]
    fn dirty_inventory_classifies_every_entry_without_touching_the_disk() {
        let root = TestRoot::new().unwrap();
        let (fake, prepared) = prepared(&root);
        let platform = fake.platform();
        let layout = prepared.layout();
        let mut expected_active: Vec<(OsString, InventoryEntry)> = Vec::new();
        let mut expected_retained: Vec<(OsString, InventoryEntry)> = Vec::new();
        let active_dir = |id: InstanceId| {
            let paths = layout.instance_paths(id);
            std::fs::create_dir(&paths.root).unwrap();
            paths
        };
        let name_of = |path: &Path| path.file_name().unwrap().to_owned();
        let manifest_bytes =
            |id: InstanceId| encode_manifest(&manifest_for(&platform, id)).unwrap();

        // A folder without a manifest.
        let missing = active_dir(InstanceId::generate());
        expected_active.push((
            name_of(&missing.root),
            InventoryEntry::MissingManifest {
                dir: missing.root.clone(),
            },
        ));

        // A manifest that is not JSON.
        let broken = active_dir(InstanceId::generate());
        std::fs::write(&broken.manifest, b"{ not json").unwrap();
        let reason = decode_manifest(b"{ not json").unwrap_err().to_string();
        expected_active.push((
            name_of(&broken.root),
            InventoryEntry::UnreadableManifest {
                dir: broken.root.clone(),
                reason,
            },
        ));

        // A valid manifest padded to one byte over 64 KiB.
        let id = InstanceId::generate();
        let oversize = active_dir(id);
        let mut padded = manifest_bytes(id);
        padded.resize(64 * 1024 + 1, b' ');
        std::fs::write(&oversize.manifest, padded).unwrap();
        expected_active.push((
            name_of(&oversize.root),
            InventoryEntry::UnreadableManifest {
                dir: oversize.root.clone(),
                reason: "too large".to_owned(),
            },
        ));

        // A FIFO, a symlink to a valid manifest, and a folder in place of
        // the manifest: none is opened.
        let fifo = active_dir(InstanceId::generate());
        make_fifo(&fifo.manifest).unwrap();
        let id = InstanceId::generate();
        let linked = active_dir(id);
        let elsewhere = root.path().join("manifest-elsewhere.json");
        std::fs::write(&elsewhere, manifest_bytes(id)).unwrap();
        symlink(&elsewhere, &linked.manifest).unwrap();
        let folder = active_dir(InstanceId::generate());
        std::fs::create_dir(&folder.manifest).unwrap();
        for paths in [&fifo, &linked, &folder] {
            expected_active.push((
                name_of(&paths.root),
                InventoryEntry::UnreadableManifest {
                    dir: paths.root.clone(),
                    reason: "not a regular file".to_owned(),
                },
            ));
        }

        // A v2 manifest, with fields v1 does not know.
        let v2 = active_dir(InstanceId::generate());
        std::fs::write(&v2.manifest, V2).unwrap();
        expected_active.push((
            name_of(&v2.root),
            InventoryEntry::UnsupportedSchema {
                dir: v2.root.clone(),
                schema: "stackly.instance-manifest/v2".to_owned(),
            },
        ));

        // A folder named by one ID holding the manifest of another.
        let other = InstanceId::generate();
        let mismatch = active_dir(InstanceId::generate());
        std::fs::write(&mismatch.manifest, manifest_bytes(other)).unwrap();
        expected_active.push((
            name_of(&mismatch.root),
            InventoryEntry::IdMismatch {
                dir: mismatch.root.clone(),
                manifest_id: other,
            },
        ));

        // A retained marker under instances/.
        let id = InstanceId::generate();
        let marked = active_dir(id);
        let mut manifest = manifest_for(&platform, id);
        manifest.record_retained(RetainedMarker::new(
            platform.clock().now(),
            OperationId::generate(),
        ));
        std::fs::write(&marked.manifest, encode_manifest(&manifest).unwrap()).unwrap();
        expected_active.push((
            name_of(&marked.root),
            InventoryEntry::LocationMismatch {
                dir: marked.root.clone(),
            },
        ));

        // A symlink to another instance folder.
        let link = layout
            .instances_dir()
            .join(InstanceId::generate().to_string());
        symlink(&missing.root, &link).unwrap();
        expected_active.push((
            name_of(&link),
            InventoryEntry::Symlink { path: link.clone() },
        ));

        // Folders that are not named by a lowercase ID, and a plain file.
        let upper = InstanceId::generate().to_string().to_uppercase();
        for name in ["not-an-id", upper.as_str()] {
            std::fs::create_dir(layout.instances_dir().join(name)).unwrap();
            expected_active.push((
                name.into(),
                InventoryEntry::UnexpectedEntry {
                    name: name.into(),
                    kind: FileKind::Dir,
                },
            ));
        }
        std::fs::write(layout.instances_dir().join("notes.txt"), b"hi").unwrap();
        expected_active.push((
            "notes.txt".into(),
            InventoryEntry::UnexpectedEntry {
                name: "notes.txt".into(),
                kind: FileKind::File,
            },
        ));

        // Finder metadata, skipped in both folders.
        std::fs::write(layout.instances_dir().join(".DS_Store"), b"").unwrap();
        std::fs::write(layout.retained_dir().join(".DS_Store"), b"").unwrap();

        // Under retained/: a folder without a marker, and the shared v1
        // fixture, which has one.
        let id = InstanceId::generate();
        let unmarked = layout.retained_instance_paths(id);
        std::fs::create_dir(&unmarked.root).unwrap();
        std::fs::write(&unmarked.manifest, manifest_bytes(id)).unwrap();
        expected_retained.push((
            name_of(&unmarked.root),
            InventoryEntry::LocationMismatch {
                dir: unmarked.root.clone(),
            },
        ));
        let fixture = decode_manifest(V1.as_bytes()).unwrap();
        let kept = layout.retained_instance_paths(fixture.instance_id());
        std::fs::create_dir(&kept.root).unwrap();
        std::fs::write(&kept.manifest, V1).unwrap();
        assert_eq!(read_instance_manifest(&platform, &kept).unwrap(), fixture);
        expected_retained.push((
            name_of(&kept.root),
            InventoryEntry::Found {
                location: InstanceLocation::Retained,
                manifest: Box::new(fixture),
            },
        ));

        let disk = tree(root.path());
        let mutations = fake.fs().mutations();

        let inventory = inventory_instances(&platform, &prepared).unwrap();

        expected_active.sort_by(|a, b| a.0.cmp(&b.0));
        expected_retained.sort_by(|a, b| a.0.cmp(&b.0));
        let expected: Vec<InventoryEntry> = expected_active
            .into_iter()
            .chain(expected_retained)
            .map(|(_, entry)| entry)
            .collect();
        assert_eq!(inventory.entries(), expected.as_slice());
        assert_eq!(fake.fs().mutations(), mutations);
        assert_eq!(tree(root.path()), disk);
        assert_eq!(fake.fs().violations(), 0);
    }

    #[test]
    fn inventory_fails_only_when_a_listing_folder_cannot_be_read() {
        let root = TestRoot::new().unwrap();
        let (fake, prepared) = prepared(&root);
        std::fs::remove_dir(prepared.layout().retained_dir()).unwrap();

        let result = inventory_instances(&fake.platform(), &prepared);

        assert!(
            matches!(
                result,
                Err(CoreError::Platform(PlatformError::NotFound { .. }))
            ),
            "{result:?}"
        );
    }

    #[test]
    fn locations_have_stable_codes() {
        assert_eq!(
            InstanceLocation::ALL.map(InstanceLocation::as_code),
            ["active", "retained"]
        );
        assert_eq!(InstanceLocation::Retained.to_string(), "retained");
    }
}
