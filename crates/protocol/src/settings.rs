//! Daemon settings and storage locations (spec 0032): the settings the
//! daemon keeps in the registry, the three places new instance data, logs,
//! and downloads go, and the methods that read and change them.
//!
//! Changing a location only affects what is created from then on. Nothing
//! here moves or deletes data.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use typeshare::typeshare;
use uuid::Uuid;

use crate::error::FieldError;
use crate::instance::StartupPolicyKind;
use crate::json::{FieldCode, Validate};
use crate::logs::LogRetention;
use crate::wire::{Count, Revision, Timestamp};

/// Longest folder path `storage.set_location` accepts, in bytes.
pub const LOCATION_PATH_MAX: usize = 4096;

/// Which update channel Stackly follows once it can update itself. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum UpdateChannel {
    /// Releases that passed every check.
    Stable,
    /// Earlier builds.
    Beta,
    /// A channel this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// What a storage location holds. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LocationKind {
    /// The data folders of new instances.
    Data,
    /// The log folders of new instances.
    Logs,
    /// The downloads of runtimes.
    Cache,
    /// A kind this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// The rule a folder broke, in the order the daemon checks them (spec 0032,
/// *Aturan lokasi*). Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LocationRule {
    /// The path is relative.
    NotAbsolute,
    /// The folder does not exist.
    Missing,
    /// The path is not a folder.
    NotDirectory,
    /// The path, or a folder above it, is a symlink.
    Symlink,
    /// Another user owns the folder.
    NotOwned,
    /// Group or other can write to the folder.
    TooOpen,
    /// The folder is one macOS may block the daemon from.
    ProtectedFolder,
    /// The disk is not local.
    NotLocalVolume,
    /// The disk is not inside this Mac.
    NotInternalVolume,
    /// The folder overlaps another Stackly location or root.
    OverlapsRoot,
    /// A `Stackly` folder is there and is not one of this registry.
    ForeignMarker,
    /// The daemon cannot create the folders it needs.
    NotWritable,
    /// A rule this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

impl LocationRule {
    /// The snake_case code that `storage.location_invalid` carries in its
    /// `rule` detail.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::NotAbsolute => "not_absolute",
            Self::Missing => "missing",
            Self::NotDirectory => "not_directory",
            Self::Symlink => "symlink",
            Self::NotOwned => "not_owned",
            Self::TooOpen => "too_open",
            Self::ProtectedFolder => "protected_folder",
            Self::NotLocalVolume => "not_local_volume",
            Self::NotInternalVolume => "not_internal_volume",
            Self::OverlapsRoot => "overlaps_root",
            Self::ForeignMarker => "foreign_marker",
            Self::NotWritable => "not_writable",
            Self::Unrecognized => "unrecognized",
        }
    }

    /// The sentence that tells a person what to change, in English. It is
    /// the message of `storage.location_invalid`.
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
            Self::Unrecognized => "This folder can’t be used.",
        }
    }
}

/// Where `storage.set_location` points a kind. Tagged `type` and `content`.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", content = "content", rename_all = "camelCase")]
pub enum LocationTarget {
    /// A folder; the daemon uses its `Stackly` subfolder.
    Path(String),
    /// The location that comes with Stackly.
    BuiltIn,
}

/// One storage location as a client shows it.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StorageLocationView {
    /// The location.
    pub id: Uuid,
    /// What it holds.
    pub kind: LocationKind,
    /// The resolved path, never empty.
    pub path: String,
    /// Whether it is the location that comes with Stackly.
    pub built_in: bool,
    /// Whether new instances, or new downloads, use it now.
    pub is_default: bool,
    /// How many instances use it; 0 for the cache.
    pub instance_count: Count,
    /// Whether the folder exists now.
    pub reachable: bool,
}

/// The default location of each kind.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DefaultLocations {
    /// Where the data of new instances goes.
    pub data: StorageLocationView,
    /// Where the logs of new instances go.
    pub logs: StorageLocationView,
    /// Where new downloads go.
    pub cache: StorageLocationView,
}

/// Reply of `storage.list_locations`: every location the registry knows,
/// built in ones first, then by creation.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LocationList {
    /// The locations.
    pub items: Vec<StorageLocationView>,
}

/// Reply of `settings.get`, and of `settings.update` once it ended.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DaemonSettingsView {
    /// Rises with every change; `settings.update` and `storage.set_location`
    /// take it back as `expectedRevision`.
    pub revision: Revision,
    /// The startup policy of an instance created without one.
    pub default_startup: StartupPolicyKind,
    /// The update channel.
    pub update_channel: UpdateChannel,
    /// Whether the person agreed to share usage statistics. Nothing is sent
    /// whatever the value.
    pub telemetry: bool,
    /// The version of the text the person agreed to.
    pub telemetry_copy_version: Count,
    /// The default locations.
    pub locations: DefaultLocations,
    /// What the instance logs keep, from the daemon's constants.
    pub log_retention: LogRetention,
    /// When a setting last changed.
    pub updated_at: Timestamp,
}

/// A partial change of the settings. At least one field must be set.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SettingsPatch {
    /// The startup policy of instances created from now on.
    #[serde(default)]
    pub default_startup: Option<StartupPolicyKind>,
    /// The update channel.
    #[serde(default)]
    pub update_channel: Option<UpdateChannel>,
    /// Whether the person agrees to share usage statistics.
    #[serde(default)]
    pub telemetry: Option<bool>,
}

impl Validate for SettingsPatch {
    /// A patch needs a field, and no field may hold a value this build does
    /// not know.
    fn validate(&self) -> Vec<FieldError> {
        let mut errors = Vec::new();
        if self.default_startup.is_none()
            && self.update_channel.is_none()
            && self.telemetry.is_none()
        {
            errors.push(FieldError::new("patch", FieldCode::Required));
        }
        if self.default_startup == Some(StartupPolicyKind::Unrecognized) {
            errors.push(FieldError::new(
                "patch.defaultStartup",
                FieldCode::UnknownValue,
            ));
        }
        if self.update_channel == Some(UpdateChannel::Unrecognized) {
            errors.push(FieldError::new(
                "patch.updateChannel",
                FieldCode::UnknownValue,
            ));
        }
        errors
    }
}

/// Params of `settings.update`.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSettingsParams {
    /// Refuse with `resource.revision_mismatch` unless the settings are at
    /// this revision.
    pub expected_revision: Revision,
    /// What to change.
    pub patch: SettingsPatch,
}

impl Validate for UpdateSettingsParams {
    fn validate(&self) -> Vec<FieldError> {
        self.patch.validate()
    }
}

/// Params of `storage.set_location`.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SetLocationParams {
    /// Which default to change.
    pub kind: LocationKind,
    /// The new default.
    pub target: LocationTarget,
    /// Refuse with `resource.revision_mismatch` unless the settings are at
    /// this revision.
    pub expected_revision: Revision,
}

impl Validate for SetLocationParams {
    /// The kind must be known, and a path must be 1 to
    /// [`LOCATION_PATH_MAX`] bytes without a NUL byte.
    fn validate(&self) -> Vec<FieldError> {
        let mut errors = Vec::new();
        if self.kind == LocationKind::Unrecognized {
            errors.push(FieldError::new("kind", FieldCode::UnknownValue));
        }
        if let LocationTarget::Path(path) = &self.target {
            if path.is_empty() {
                errors.push(FieldError::new("target", FieldCode::Required));
            } else if path.len() > LOCATION_PATH_MAX {
                errors.push(FieldError::new("target", FieldCode::TooLong));
            } else if path.contains('\0') {
                errors.push(FieldError::new("target", FieldCode::InvalidFormat));
            }
        }
        errors
    }
}

/// Content of the `settings.changed` event: the settings or a default
/// location changed, so a client reads them again.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SettingsChanged {
    /// The revision after the change.
    pub revision: Revision,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_patch_is_refused_and_a_patch_with_a_field_is_not() {
        assert_eq!(SettingsPatch::default().validate().len(), 1);
        let patch = SettingsPatch {
            telemetry: Some(true),
            ..SettingsPatch::default()
        };
        assert!(patch.validate().is_empty());
    }

    #[test]
    fn a_patch_with_an_unrecognized_value_is_refused() {
        let patch = SettingsPatch {
            default_startup: Some(StartupPolicyKind::Unrecognized),
            update_channel: Some(UpdateChannel::Unrecognized),
            ..SettingsPatch::default()
        };
        let fields: Vec<_> = patch
            .validate()
            .iter()
            .map(|error| error.path().to_owned())
            .collect();
        assert_eq!(fields, ["patch.defaultStartup", "patch.updateChannel"]);
    }

    #[test]
    fn a_location_path_must_be_text_of_a_sane_size() {
        let params = |target| SetLocationParams {
            kind: LocationKind::Data,
            target,
            expected_revision: Revision::new(1).unwrap(),
        };
        assert!(params(LocationTarget::BuiltIn).validate().is_empty());
        assert!(
            params(LocationTarget::Path("/Volumes/x".to_owned()))
                .validate()
                .is_empty()
        );
        for bad in [
            String::new(),
            "a".repeat(LOCATION_PATH_MAX + 1),
            "a\0b".to_owned(),
        ] {
            assert_eq!(params(LocationTarget::Path(bad)).validate().len(), 1);
        }
    }

    #[test]
    fn every_rule_has_a_code_and_a_sentence() {
        let rules = [
            LocationRule::NotAbsolute,
            LocationRule::Missing,
            LocationRule::NotDirectory,
            LocationRule::Symlink,
            LocationRule::NotOwned,
            LocationRule::TooOpen,
            LocationRule::ProtectedFolder,
            LocationRule::NotLocalVolume,
            LocationRule::NotInternalVolume,
            LocationRule::OverlapsRoot,
            LocationRule::ForeignMarker,
            LocationRule::NotWritable,
        ];
        for rule in rules {
            let json = serde_json::to_string(&rule).unwrap();
            assert_eq!(json, format!("\"{}\"", rule.as_code()));
            assert!(rule.sentence().ends_with('.'));
        }
    }
}
