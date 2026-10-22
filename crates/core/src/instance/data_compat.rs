//! The one place that decides whether data of a recorded line may run on a
//! runtime (spec 0025, *Kecocokan data*, AC-8, BR-009): it reads the data
//! marker of an instance and asks the pinned package's preflight (spec
//! 0009). Start (`verify`) and the rebind check both call it, so they can
//! never disagree about the same data.

use stackly_domain::RuntimeLine;
use stackly_platform::{Filesystem, InstancePaths};

use crate::provider::{DATA_MARKER_MAX_LEN, MarkerRead, PreflightOutcome, ValidatedPackage};

/// The outcome of a preflight that did not say `Compatible`, as
/// `supervisor.data_incompatible` carries it on the wire.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum DataOutcome {
    /// The runtime line may not run data of that line.
    Incompatible,
    /// The marker does not match its pattern or names another line than the
    /// recorded one.
    MarkerMismatch,
    /// The package declares a marker, but the file is missing.
    MarkerMissing,
    /// The marker file could not be read.
    MarkerUnreadable,
    /// Nothing tells which line the data belongs to. It cannot come out of
    /// [`check_data_compatibility`], which always has a recorded line.
    Unknown,
}

impl DataOutcome {
    /// Returns the code of the outcome, the `outcome` detail of
    /// `supervisor.data_incompatible`.
    #[must_use]
    pub const fn as_code(&self) -> &'static str {
        match self {
            Self::Incompatible => "incompatible",
            Self::MarkerMismatch => "marker_mismatch",
            Self::MarkerMissing => "marker_missing",
            Self::MarkerUnreadable => "marker_unreadable",
            Self::Unknown => "unknown",
        }
    }
}

/// What `preflight` said when it did not answer `Compatible`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct DataIncompatible {
    /// Why the data does not fit.
    pub outcome: DataOutcome,
    /// The data line, only for [`DataOutcome::Incompatible`].
    pub data: Option<RuntimeLine>,
    /// The line of the runtime that was asked.
    pub runtime: RuntimeLine,
}

/// Reads the data marker of `paths` (when the package declares one) and
/// asks `package.preflight` whether data of line `recorded` may run on
/// `runtime`.
///
/// # Errors
///
/// Returns [`DataIncompatible`] with the outcome, the data line when it is
/// known, and the runtime line, when the preflight answers anything but
/// `Compatible`.
pub fn check_data_compatibility(
    fs: &dyn Filesystem,
    package: &ValidatedPackage,
    paths: &InstancePaths,
    recorded: &RuntimeLine,
    runtime: &RuntimeLine,
) -> Result<(), DataIncompatible> {
    let marker = package
        .data_marker_path(paths)
        .map_or(MarkerRead::NotDeclared, |path| {
            MarkerRead::from_read(fs.read_file_limited(&path, DATA_MARKER_MAX_LEN))
        });
    let incompatible = |outcome, data| DataIncompatible {
        outcome,
        data,
        runtime: runtime.clone(),
    };
    match package.preflight(Some(recorded.clone()), marker, runtime) {
        PreflightOutcome::Compatible => Ok(()),
        PreflightOutcome::Incompatible { data, .. } => {
            Err(incompatible(DataOutcome::Incompatible, Some(data)))
        }
        PreflightOutcome::MarkerMismatch { .. } => {
            Err(incompatible(DataOutcome::MarkerMismatch, None))
        }
        PreflightOutcome::MarkerMissing => Err(incompatible(DataOutcome::MarkerMissing, None)),
        PreflightOutcome::MarkerUnreadable => {
            Err(incompatible(DataOutcome::MarkerUnreadable, None))
        }
        // `Unknown` and any outcome a later spec adds report as unknown.
        _ => Err(incompatible(DataOutcome::Unknown, None)),
    }
}
