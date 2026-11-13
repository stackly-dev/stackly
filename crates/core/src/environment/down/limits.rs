//! The constants of Down (spec 0047, *Precheck Down* and *Urutan*).

/// The most stops one Down runs at once (AC-6).
pub const DOWN_STOP_CONCURRENCY: usize = 4;

/// The most IDs `env.down` takes in `expectedStopInstanceIds`.
pub const DOWN_EXPECTED_IDS_MAX: usize = 256;

/// The most items `env.down_incomplete` carries.
pub const DOWN_ITEMS_MAX: usize = 32;
