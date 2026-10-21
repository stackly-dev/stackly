//! The storage contract (spec 0022): the one method that reports how full
//! the disk is that holds the Stackly data root.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use typeshare::typeshare;

use crate::wire::{Count, Timestamp};

/// The space left on the volume that holds the data root, read now. The
/// daemon computes `level`; clients never apply thresholds themselves.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CapacityReport {
    /// When the daemon finished reading the volume.
    pub checked_at: Timestamp,
    /// The size of the volume, in bytes. Never 0.
    pub total_bytes: Count,
    /// The bytes an unprivileged user can still write, held to
    /// `totalBytes`.
    pub available_bytes: Count,
    /// The percent of the volume in use, 0 to 100, rounded down.
    pub used_percent: u8,
    /// How worried the user should be about the space left.
    pub level: CapacityLevel,
}

/// How full a volume is. Open: newer daemons may add values.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CapacityLevel {
    /// Plenty of space is left.
    Ok,
    /// At least 80 percent is used.
    Filling,
    /// At least 90 percent is used, or under 5 GB is left.
    Low,
    /// Under 2 GB is left.
    Critical,
    /// A level this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}
