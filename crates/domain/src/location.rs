//! Daemon settings and storage locations (spec 0032).
//!
//! A storage location is a folder that holds the data, the logs, or the
//! downloads of what Stackly creates from then on. The registry never changes
//! or deletes a location row, and an instance keeps the locations it was
//! created in, so changing the default moves no data.

use jiff::Timestamp;

use crate::code::code_enum;
use crate::error::{ValueError, ValueKind};
use crate::id::StorageLocationId;
use crate::instance::StartupPolicy;
use crate::value::ResourceRevision;

/// Longest location path, in bytes.
pub const LOCATION_PATH_MAX: usize = 4096;

/// The version of the telemetry text a new registry starts with.
pub const TELEMETRY_COPY_VERSION: u32 = 1;

code_enum! {
    /// What a storage location holds.
    pub enum LocationKind: LocationKind {
        /// The data folders of instances.
        Data => "data",
        /// The log folders of instances.
        Logs => "logs",
        /// The downloads of runtimes.
        Cache => "cache",
    }
}

code_enum! {
    /// Which update channel Stackly follows once it can update itself.
    pub enum UpdateChannel: UpdateChannel {
        /// Releases that passed every check.
        Stable => "stable",
        /// Earlier builds.
        Beta => "beta",
    }
}

code_enum! {
    /// The rule a folder broke, in the order `check_location` tests them.
    pub enum LocationRule: LocationRule {
        /// The path is relative.
        NotAbsolute => "not_absolute",
        /// The folder does not exist.
        Missing => "missing",
        /// The path is not a folder.
        NotDirectory => "not_directory",
        /// The path, or a folder above it, is a symlink.
        Symlink => "symlink",
        /// Another user owns the folder.
        NotOwned => "not_owned",
        /// Group or other can write to the folder.
        TooOpen => "too_open",
        /// macOS may block the daemon from the folder.
        ProtectedFolder => "protected_folder",
        /// The disk is not local.
        NotLocalVolume => "not_local_volume",
        /// The disk is not inside this Mac.
        NotInternalVolume => "not_internal_volume",
        /// The folder overlaps another Stackly location or root.
        OverlapsRoot => "overlaps_root",
        /// A `Stackly` folder is there and does not belong to this registry.
        ForeignMarker => "foreign_marker",
        /// The daemon cannot create the folders it needs.
        NotWritable => "not_writable",
    }
}

impl LocationRule {
    /// The sentence that tells a person what to change, in English
    /// (spec 0032, *Aturan lokasi*).
    #[must_use]
    pub const fn sentence(self) -> &'static str {
        match self {
            Self::NotAbsolute => "Choose a full folder path.",
            Self::Missing => "This folder doesn’t exist.",
            Self::NotDirectory => "Choose a folder, not a file.",
            Self::Symlink => "Choose the real folder, not an alias or link.",
            Self::NotOwned => "This folder belongs to another user.",
            Self::TooOpen => "Other users can write to this folder.",
            Self::ProtectedFolder => {
                "macOS may block stacklyd from this folder. Choose a folder in your home folder or on a disk inside this Mac."
            }
            Self::NotLocalVolume => "Choose a folder on a disk in this Mac.",
            Self::NotInternalVolume => {
                "Choose a folder on a disk inside this Mac. External disks aren’t supported yet."
            }
            Self::OverlapsRoot => "This folder overlaps another Stackly location.",
            Self::ForeignMarker => {
                "This folder already has a Stackly folder that isn’t from this Mac’s Stackly."
            }
            Self::NotWritable => "stacklyd can’t write to this folder.",
        }
    }
}

impl LocationKind {
    /// The ID of the location of this kind that comes with Stackly.
    #[must_use]
    pub const fn built_in_id(self) -> StorageLocationId {
        match self {
            Self::Data => BUILT_IN_LOCATION_IDS[0],
            Self::Logs => BUILT_IN_LOCATION_IDS[1],
            Self::Cache => BUILT_IN_LOCATION_IDS[2],
        }
    }
}

/// The fixed IDs of the three built in locations, in the order data, logs,
/// cache. The migration inserts the same values.
pub const BUILT_IN_LOCATION_IDS: [StorageLocationId; 3] = [
    StorageLocationId::from_built_in(0x0000_0000_0000_7000_8000_0000_0000_0001),
    StorageLocationId::from_built_in(0x0000_0000_0000_7000_8000_0000_0000_0002),
    StorageLocationId::from_built_in(0x0000_0000_0000_7000_8000_0000_0000_0003),
];

/// One storage location.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageLocation {
    id: StorageLocationId,
    kind: LocationKind,
    path: Option<String>,
    created_at: Timestamp,
}

impl StorageLocation {
    /// The built in location of `kind`. It has no path: it resolves through
    /// the root set, so `STACKLY_ROOT` keeps working.
    #[must_use]
    pub const fn built_in(kind: LocationKind, created_at: Timestamp) -> Self {
        Self {
            id: kind.built_in_id(),
            kind,
            path: None,
            created_at,
        }
    }

    /// A location in a folder the person chose. `path` is the canonical
    /// `Stackly` subfolder.
    ///
    /// # Errors
    ///
    /// Returns a [`ValueError`] when the path is empty, longer than
    /// [`LOCATION_PATH_MAX`], relative, or holds a NUL byte.
    pub fn custom(
        id: StorageLocationId,
        kind: LocationKind,
        path: impl Into<String>,
        created_at: Timestamp,
    ) -> Result<Self, ValueError> {
        let path = path.into();
        if path.is_empty() || path.len() > LOCATION_PATH_MAX {
            return Err(ValueError::new(
                ValueKind::LocationPath,
                "must be 1 to 4096 bytes",
            ));
        }
        if !path.starts_with('/') || path.contains('\0') {
            return Err(ValueError::new(
                ValueKind::LocationPath,
                "must be an absolute path without a NUL byte",
            ));
        }
        Ok(Self {
            id,
            kind,
            path: Some(path),
            created_at,
        })
    }

    /// Rebuilds a stored location and checks that a path is there exactly
    /// when the location is not built in.
    ///
    /// # Errors
    ///
    /// Returns a [`ValueError`] when the row breaks that rule, or when
    /// [`custom`](Self::custom) refuses the path.
    pub fn from_stored(
        id: StorageLocationId,
        kind: LocationKind,
        path: Option<String>,
        built_in: bool,
        created_at: Timestamp,
    ) -> Result<Self, ValueError> {
        match (built_in, path) {
            (true, None) if id == kind.built_in_id() => Ok(Self::built_in(kind, created_at)),
            (false, Some(path)) => Self::custom(id, kind, path, created_at),
            _ => Err(ValueError::new(
                ValueKind::LocationPath,
                "a built in location has the fixed ID of its kind and no path, any other has a path",
            )),
        }
    }

    /// The ID.
    #[must_use]
    pub const fn id(&self) -> StorageLocationId {
        self.id
    }

    /// What the location holds.
    #[must_use]
    pub const fn kind(&self) -> LocationKind {
        self.kind
    }

    /// The canonical path, or `None` for a built in location.
    #[must_use]
    pub fn path(&self) -> Option<&str> {
        self.path.as_deref()
    }

    /// Whether this is the location that comes with Stackly.
    #[must_use]
    pub const fn is_built_in(&self) -> bool {
        self.path.is_none()
    }

    /// When the location was added.
    #[must_use]
    pub const fn created_at(&self) -> Timestamp {
        self.created_at
    }
}

/// The settings the daemon keeps in the registry. One row, always.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonSettings {
    /// The startup policy of an instance created without one.
    pub default_startup: StartupPolicy,
    /// The update channel.
    pub update_channel: UpdateChannel,
    /// Whether the person agreed to share usage statistics. Nothing is sent
    /// whatever the value.
    pub telemetry: bool,
    /// The version of the telemetry text the person agreed to.
    pub telemetry_copy_version: u32,
    /// The default data location.
    pub data_location: StorageLocationId,
    /// The default log location.
    pub logs_location: StorageLocationId,
    /// The default download location.
    pub cache_location: StorageLocationId,
    /// Rises with every change.
    pub revision: ResourceRevision,
    /// When a setting last changed.
    pub updated_at: Timestamp,
}

impl DaemonSettings {
    /// The settings a new registry starts with: autostart off, the stable
    /// channel, telemetry off, and the built in locations.
    #[must_use]
    pub const fn initial(at: Timestamp) -> Self {
        Self {
            default_startup: StartupPolicy::Manual,
            update_channel: UpdateChannel::Stable,
            telemetry: false,
            telemetry_copy_version: TELEMETRY_COPY_VERSION,
            data_location: LocationKind::Data.built_in_id(),
            logs_location: LocationKind::Logs.built_in_id(),
            cache_location: LocationKind::Cache.built_in_id(),
            revision: ResourceRevision::INITIAL,
            updated_at: at,
        }
    }

    /// The default location of `kind`.
    #[must_use]
    pub const fn location_of(&self, kind: LocationKind) -> StorageLocationId {
        match kind {
            LocationKind::Data => self.data_location,
            LocationKind::Logs => self.logs_location,
            LocationKind::Cache => self.cache_location,
        }
    }

    /// The settings after `patch`, with the next revision, or `None` when
    /// the patch changes nothing.
    #[must_use]
    pub fn patched(&self, patch: &SettingsPatch, at: Timestamp) -> Option<Self> {
        let mut next = self.clone();
        if let Some(startup) = patch.default_startup {
            next.default_startup = startup;
        }
        if let Some(channel) = patch.update_channel {
            next.update_channel = channel;
        }
        if let Some(telemetry) = patch.telemetry {
            next.telemetry = telemetry;
        }
        if next == *self {
            return None;
        }
        next.revision = self.revision.next();
        next.updated_at = at;
        Some(next)
    }

    /// The settings with the default location of `kind` set to `id`, with
    /// the next revision, or `None` when it already is.
    #[must_use]
    pub fn with_location(
        &self,
        kind: LocationKind,
        id: StorageLocationId,
        at: Timestamp,
    ) -> Option<Self> {
        if self.location_of(kind) == id {
            return None;
        }
        let mut next = self.clone();
        match kind {
            LocationKind::Data => next.data_location = id,
            LocationKind::Logs => next.logs_location = id,
            LocationKind::Cache => next.cache_location = id,
        }
        next.revision = self.revision.next();
        next.updated_at = at;
        Some(next)
    }
}

/// A partial change of the settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SettingsPatch {
    /// The startup policy of instances created from now on.
    pub default_startup: Option<StartupPolicy>,
    /// The update channel.
    pub update_channel: Option<UpdateChannel>,
    /// Whether the person agrees to share usage statistics.
    pub telemetry: Option<bool>,
}

impl SettingsPatch {
    /// Whether the patch sets no field.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.default_startup.is_none() && self.update_channel.is_none() && self.telemetry.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at() -> Timestamp {
        "2026-11-03T08:00:00Z".parse().unwrap()
    }

    #[test]
    fn the_built_in_ids_are_fixed_version_seven_values_in_kind_order() {
        let texts: Vec<_> = LocationKind::ALL
            .iter()
            .map(|kind| kind.built_in_id().to_string())
            .collect();
        assert_eq!(
            texts,
            [
                "00000000-0000-7000-8000-000000000001",
                "00000000-0000-7000-8000-000000000002",
                "00000000-0000-7000-8000-000000000003",
            ]
        );
        for (kind, id) in LocationKind::ALL.iter().zip(BUILT_IN_LOCATION_IDS) {
            assert_eq!(kind.built_in_id(), id);
            assert_eq!(id.to_string().parse::<StorageLocationId>().unwrap(), id);
        }
    }

    #[test]
    fn a_built_in_location_has_no_path_and_a_custom_one_needs_one() {
        let built_in = StorageLocation::built_in(LocationKind::Data, at());
        assert!(built_in.is_built_in());
        assert_eq!(built_in.path(), None);

        let id = StorageLocationId::generate();
        let custom =
            StorageLocation::custom(id, LocationKind::Logs, "/Volumes/x/Stackly", at()).unwrap();
        assert!(!custom.is_built_in());
        assert_eq!(custom.path(), Some("/Volumes/x/Stackly"));

        for bad in ["", "relative/path", "/a\0b"] {
            assert!(StorageLocation::custom(id, LocationKind::Data, bad, at()).is_err());
        }
        assert!(
            StorageLocation::custom(
                id,
                LocationKind::Data,
                "/".repeat(LOCATION_PATH_MAX + 1),
                at()
            )
            .is_err()
        );
    }

    #[test]
    fn a_stored_row_is_built_in_exactly_when_it_has_no_path() {
        let other = StorageLocationId::generate();
        let built_in_id = LocationKind::Cache.built_in_id();
        assert!(
            StorageLocation::from_stored(built_in_id, LocationKind::Cache, None, true, at())
                .is_ok()
        );
        assert!(
            StorageLocation::from_stored(other, LocationKind::Cache, None, true, at()).is_err()
        );
        assert!(
            StorageLocation::from_stored(
                other,
                LocationKind::Cache,
                Some("/x/Stackly".to_owned()),
                true,
                at()
            )
            .is_err()
        );
        assert!(
            StorageLocation::from_stored(other, LocationKind::Cache, None, false, at()).is_err()
        );
    }

    #[test]
    fn a_patch_that_changes_nothing_gives_no_new_settings() {
        let settings = DaemonSettings::initial(at());
        let same = SettingsPatch {
            default_startup: Some(StartupPolicy::Manual),
            telemetry: Some(false),
            ..SettingsPatch::default()
        };
        assert_eq!(settings.patched(&same, at()), None);

        let change = SettingsPatch {
            telemetry: Some(true),
            update_channel: Some(UpdateChannel::Beta),
            ..SettingsPatch::default()
        };
        let next = settings.patched(&change, at()).unwrap();
        assert!(next.telemetry);
        assert_eq!(next.update_channel, UpdateChannel::Beta);
        assert_eq!(next.revision.get(), 2);
        assert_eq!(next.default_startup, StartupPolicy::Manual);
    }

    #[test]
    fn a_default_location_changes_one_kind_and_raises_the_revision() {
        let settings = DaemonSettings::initial(at());
        let id = StorageLocationId::generate();
        assert_eq!(
            settings.with_location(LocationKind::Data, LocationKind::Data.built_in_id(), at()),
            None
        );
        let next = settings
            .with_location(LocationKind::Data, id, at())
            .unwrap();
        assert_eq!(next.location_of(LocationKind::Data), id);
        assert_eq!(
            next.location_of(LocationKind::Logs),
            LocationKind::Logs.built_in_id()
        );
        assert_eq!(next.revision.get(), 2);
    }

    #[test]
    fn rule_codes_follow_the_check_order_of_the_spec() {
        let codes: Vec<_> = LocationRule::ALL
            .iter()
            .map(|rule| rule.as_code())
            .collect();
        assert_eq!(codes.len(), 12);
        assert_eq!(codes[0], "not_absolute");
        assert_eq!(codes[11], "not_writable");
        assert!(LocationRule::NotAbsolute < LocationRule::NotWritable);
    }
}
