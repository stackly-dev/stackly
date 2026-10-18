//! Data compatibility preflight (spec 0009, "Preflight kompatibilitas
//! data", BR-009).

use std::path::PathBuf;

use stackly_domain::RuntimeLine;
use stackly_platform::{InstancePaths, PlatformError};

use super::validate::ValidatedPackage;

/// Largest data marker file the caller reads, in bytes.
pub const DATA_MARKER_MAX_LEN: u64 = 4096;

/// What the caller found when it read the data marker file.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum MarkerRead {
    /// The manifest declares no data marker.
    NotDeclared,
    /// The file does not exist.
    Missing,
    /// The file is not a regular file, is larger than
    /// [`DATA_MARKER_MAX_LEN`], is not UTF-8, or could not be read.
    Unreadable,
    /// The file's text.
    Contents(String),
}

impl MarkerRead {
    /// Classifies the result of `read_file_limited` on
    /// [`ValidatedPackage::data_marker_path`] with [`DATA_MARKER_MAX_LEN`].
    #[must_use]
    pub fn from_read(result: Result<Vec<u8>, PlatformError>) -> Self {
        match result {
            Ok(bytes) => String::from_utf8(bytes).map_or(Self::Unreadable, Self::Contents),
            Err(PlatformError::NotFound { .. }) => Self::Missing,
            Err(_) => Self::Unreadable,
        }
    }
}

/// Whether a runtime may run an instance's data.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PreflightOutcome {
    /// The data line and the runtime line fit.
    Compatible,
    /// The runtime line may not run data of that line (BR-009).
    Incompatible {
        /// The line the data belongs to.
        data: RuntimeLine,
        /// The runtime's line.
        runtime: RuntimeLine,
    },
    /// The marker does not match its pattern or names another line than
    /// the recorded one.
    MarkerMismatch {
        /// The line the registry or instance manifest records.
        recorded: Option<RuntimeLine>,
        /// The line the marker names, when it matched the pattern.
        marker: Option<RuntimeLine>,
    },
    /// The manifest declares a marker, but the file is missing.
    MarkerMissing,
    /// The marker file could not be read.
    MarkerUnreadable,
    /// Nothing tells which line the data belongs to.
    Unknown,
}

impl ValidatedPackage {
    /// Returns the data marker file in the instance's data folder, when
    /// the manifest declares `compatibility.dataMarker`.
    #[must_use]
    pub fn data_marker_path(&self, paths: &InstancePaths) -> Option<PathBuf> {
        self.manifest
            .data_marker
            .as_ref()
            .map(|marker| paths.data.join(&marker.file))
    }

    /// Decides whether a runtime of line `runtime` may run the instance's
    /// data.
    ///
    /// `recorded` comes from the registry, or from `instance-manifest.json`
    /// when the registry has no record. The marker text is trimmed of
    /// trailing whitespace and matched in full against the pattern, whose
    /// capture group is the line. The rules apply in order: an unreadable
    /// marker, a missing marker, a marker that does not match or disagrees
    /// with `recorded`, no line at all, and a data line that
    /// [`LineCompatibility`](stackly_domain::LineCompatibility) does not
    /// allow for `runtime`.
    #[must_use]
    pub fn preflight(
        &self,
        recorded: Option<RuntimeLine>,
        marker: MarkerRead,
        runtime: &RuntimeLine,
    ) -> PreflightOutcome {
        let mut marker_line = None;
        if let Some(declared) = &self.manifest.data_marker {
            let contents = match marker {
                MarkerRead::Unreadable => return PreflightOutcome::MarkerUnreadable,
                MarkerRead::Missing | MarkerRead::NotDeclared => {
                    return PreflightOutcome::MarkerMissing;
                }
                MarkerRead::Contents(contents) => contents,
            };
            let line = declared
                .pattern
                .regex()
                .captures(contents.trim_end())
                .and_then(|captures| captures.get(1))
                .and_then(|found| found.as_str().parse::<RuntimeLine>().ok());
            match (&recorded, line) {
                (_, None) => {
                    return PreflightOutcome::MarkerMismatch {
                        recorded,
                        marker: None,
                    };
                }
                (Some(recorded_line), Some(line)) if *recorded_line != line => {
                    return PreflightOutcome::MarkerMismatch {
                        recorded,
                        marker: Some(line),
                    };
                }
                (_, Some(line)) => marker_line = Some(line),
            }
        }
        let Some(data) = recorded.or(marker_line) else {
            return PreflightOutcome::Unknown;
        };
        if self.manifest.line_compatibility.allows(&data, runtime) {
            PreflightOutcome::Compatible
        } else {
            PreflightOutcome::Incompatible {
                data,
                runtime: runtime.clone(),
            }
        }
    }
}
