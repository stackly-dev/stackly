//! The diagnostic bundle (spec 0034): `diagnostics.preview_bundle` builds a
//! zip in the daemon's cache and answers what it holds, and
//! `diagnostics.export_bundle` copies that exact zip to a path the client
//! chose. Neither is an operation (spec 0007), neither is audited
//! (spec 0035), and the bundle is never sent over the network.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use typeshare::typeshare;
use uuid::Uuid;

use crate::error::FieldError;
use crate::json::{FieldCode, Validate};
use crate::wire::{Count, Timestamp};

/// The longest export destination, in bytes.
pub const DIAGNOSTICS_DESTINATION_MAX: usize = 1024;
/// The longest file name of an export destination, in bytes.
pub const DIAGNOSTICS_FILE_NAME_MAX: usize = 200;
/// The most entries one preview lists.
pub const DIAGNOSTICS_ENTRIES_MAX: usize = 64;

/// What an entry of the bundle is. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BundleEntryKind {
    /// `README.txt`: what the bundle holds and leaves out.
    Readme,
    /// `doctor.json`: the doctor report of the preview.
    Doctor,
    /// `system.json`: versions, macOS, launch context, capacity, roots.
    System,
    /// `instances.json`: every instance, secrets only as references.
    Instances,
    /// `runtimes.json`: every runtime.
    Runtimes,
    /// `operations.json`: the newest operations and those that need
    /// recovery.
    Operations,
    /// `logs/stacklyd.log`: the tail of the daemon log.
    DaemonLog,
    /// `logs/instances/<name>.log`: the tail of one instance log.
    InstanceLog,
    /// A file of `agent/`: the launch record or the start failure record.
    Agent,
    /// A kind this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// One entry of a preview.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BundleEntry {
    /// Its path in the zip, such as `logs/stacklyd.log`.
    pub path: String,
    /// What it is.
    pub kind: BundleEntryKind,
    /// One sentence about it.
    pub description: String,
    /// Its bytes before compression.
    pub bytes: Count,
    /// Its lines, for a log.
    #[serde(default)]
    pub lines: Option<Count>,
    /// True when it holds less than its source had.
    pub truncated: bool,
}

/// Reply of `diagnostics.preview_bundle`: what the staged zip holds. The
/// same bytes are what `diagnostics.export_bundle` writes.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BundlePreview {
    /// The preview, for `diagnostics.export_bundle`.
    pub preview_id: Uuid,
    /// When it was built.
    pub created_at: Timestamp,
    /// When it can no longer be exported, 15 minutes later.
    pub expires_at: Timestamp,
    /// `stackly-diagnostics-<YYYYMMDD>T<HHMMSS>Z.zip`, from `createdAt` in
    /// UTC.
    pub suggested_file_name: String,
    /// Its entries, in zip order.
    #[serde(default)]
    pub entries: Vec<BundleEntry>,
    /// The bytes of all entries before compression.
    pub total_bytes: Count,
    /// The size of the zip.
    pub zip_bytes: Count,
    /// True when the registry is in recovery mode, so the entries that need
    /// it are left out.
    pub recovery: bool,
}

/// Params of `diagnostics.export_bundle`.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExportBundleParams {
    /// The preview to copy.
    pub preview_id: Uuid,
    /// The absolute path to write, at most 1024 bytes, without a `.` or
    /// `..` component; its file name is at most 200 bytes and its parent
    /// folder must exist. The daemon checks the rest and answers
    /// `diagnostics.destination_invalid` with its reason.
    pub destination: String,
    /// Whether a file or symlink already at `destination` is replaced; a
    /// client sends true only after the user confirmed the replacement.
    #[serde(default)]
    pub replace: bool,
}

impl Validate for ExportBundleParams {
    /// `destination` is not empty and at most
    /// [`DIAGNOSTICS_DESTINATION_MAX`] bytes.
    fn validate(&self) -> Vec<FieldError> {
        if self.destination.is_empty() {
            vec![FieldError::new("destination", FieldCode::Required)]
        } else if self.destination.len() > DIAGNOSTICS_DESTINATION_MAX {
            vec![FieldError::new("destination", FieldCode::TooLong)]
        } else {
            Vec::new()
        }
    }
}

/// Reply of `diagnostics.export_bundle`.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExportedBundle {
    /// The file written, with its parent folder made canonical.
    pub path: String,
    /// Its size.
    pub bytes: Count,
    /// Its sha256, lowercase hex, the same as the staged preview's.
    pub sha256: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(destination: &str) -> ExportBundleParams {
        ExportBundleParams {
            preview_id: Uuid::nil(),
            destination: destination.to_owned(),
            replace: false,
        }
    }

    #[test]
    fn an_export_needs_a_destination_of_at_most_1024_bytes() {
        assert!(params("/Users/me/Downloads/b.zip").validate().is_empty());
        assert_eq!(
            params("").validate(),
            [FieldError::new("destination", FieldCode::Required)]
        );
        let long = format!("/{}", "d".repeat(DIAGNOSTICS_DESTINATION_MAX));
        assert_eq!(
            params(&long).validate(),
            [FieldError::new("destination", FieldCode::TooLong)]
        );
        // The daemon refuses a relative path with its own reason.
        assert!(params("relative.zip").validate().is_empty());
    }

    #[test]
    fn replace_defaults_to_false() {
        let decoded: ExportBundleParams = serde_json::from_str(
            r#"{"previewId":"0192f000-0000-7000-8000-000000000001","destination":"/a/b.zip"}"#,
        )
        .unwrap();
        assert!(!decoded.replace);
    }

    #[test]
    fn an_entry_kind_this_build_does_not_know_reads_unrecognized() {
        assert_eq!(
            serde_json::from_str::<BundleEntryKind>("\"core_dump\"").unwrap(),
            BundleEntryKind::Unrecognized
        );
        assert_eq!(
            serde_json::to_string(&BundleEntryKind::InstanceLog).unwrap(),
            "\"instance_log\""
        );
    }
}
