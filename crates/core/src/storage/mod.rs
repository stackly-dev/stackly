//! The free space of the disk that holds Stackly data (spec 0022).

use std::io;
use std::path::Path;

use jiff::Timestamp;
use stackly_domain::CapacityLevel;
use stackly_domain::capacity;
use stackly_platform::{Filesystem, PlatformError};

/// One reading of the volume that holds the data root.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct CapacityReading {
    /// When the reading was taken.
    pub checked_at: Timestamp,
    /// The size of the volume, in bytes. Never 0.
    pub total_bytes: u64,
    /// The bytes an unprivileged user can still write, held to `total_bytes`.
    pub available_bytes: u64,
    /// The percent of the volume in use, rounded down.
    pub used_percent: u8,
    /// How worried the user should be.
    pub level: CapacityLevel,
}

/// Why a capacity reading failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum CapacityError {
    /// The volume could not be read.
    #[error("reading the volume failed: {kind}")]
    Io {
        /// The I/O error kind of the failure.
        kind: io::ErrorKind,
    },
    /// The volume reports a size of 0, so it has no usable level.
    #[error("the volume reports no size")]
    NoSize,
}

/// Reads the space left on the volume that holds `data_root` (the data root
/// of the `RootSet`; the runtime, logs, and cache roots are not measured),
/// held to the volume size, with the level the domain computes.
///
/// # Errors
///
/// Returns [`CapacityError::Io`] when the volume cannot be read and
/// [`CapacityError::NoSize`] when it reports a size of 0.
pub fn capacity(
    fs: &dyn Filesystem,
    data_root: &Path,
    now: Timestamp,
) -> Result<CapacityReading, CapacityError> {
    let space = fs
        .volume_space(data_root)
        .map_err(|err| CapacityError::Io {
            kind: kind_of(&err),
        })?;
    let available_bytes = space.available.min(space.total);
    let level = capacity::level(space.total, available_bytes).ok_or(CapacityError::NoSize)?;
    Ok(CapacityReading {
        checked_at: now,
        total_bytes: space.total,
        available_bytes,
        used_percent: capacity::used_percent(space.total, available_bytes),
        level,
    })
}

fn kind_of(error: &PlatformError) -> io::ErrorKind {
    match error {
        PlatformError::Io { kind, .. } => *kind,
        PlatformError::NotFound { .. } => io::ErrorKind::NotFound,
        PlatformError::PermissionDenied { .. } => io::ErrorKind::PermissionDenied,
        _ => io::ErrorKind::Other,
    }
}

#[cfg(test)]
mod tests {
    use stackly_testkit::{FakeFilesystem, FakeVolume, Fault, FsOp, TestRoot};

    use super::*;

    fn now() -> Timestamp {
        "2026-10-21T08:00:00Z".parse().expect("a valid timestamp")
    }

    fn read(total: u64, available: u64) -> Result<CapacityReading, CapacityError> {
        let root = TestRoot::new().expect("a test root can be created");
        let fs = FakeFilesystem::new(&root);
        fs.set_volume(root.path(), FakeVolume::new(total, available));
        capacity(&fs, root.path(), now())
    }

    #[test]
    fn a_reading_carries_the_sizes_the_percent_the_level_and_the_time() {
        let reading = read(100_000_000_000, 30_000_000_000).expect("the volume reads");

        assert_eq!(reading.checked_at, now());
        assert_eq!(reading.total_bytes, 100_000_000_000);
        assert_eq!(reading.available_bytes, 30_000_000_000);
        assert_eq!(reading.used_percent, 70);
        assert_eq!(reading.level, CapacityLevel::Ok);
    }

    #[test]
    fn the_level_changes_at_the_documented_available_bytes() {
        // 20 GB keeps the percent rules out of the way: 5 GB left is 75
        // percent used.
        let level = |available| read(20_000_000_000, available).unwrap().level;

        assert_eq!(level(1_999_999_999), CapacityLevel::Critical);
        assert_eq!(level(2_000_000_000), CapacityLevel::Low);
        assert_eq!(level(4_999_999_999), CapacityLevel::Low);
        assert_eq!(level(5_000_000_000), CapacityLevel::Ok);
    }

    #[test]
    fn the_level_changes_at_eighty_and_ninety_percent_used() {
        let total = 10_000_000_000_000;
        let level = |used: u64| read(total, total - total / 100 * used).unwrap().level;

        assert_eq!(level(79), CapacityLevel::Ok);
        assert_eq!(level(80), CapacityLevel::Filling);
        assert_eq!(level(89), CapacityLevel::Filling);
        assert_eq!(level(90), CapacityLevel::Low);
    }

    #[test]
    fn available_above_total_is_held_to_total() {
        let reading = read(10_000_000_000, 99_000_000_000).expect("the volume reads");

        assert_eq!(reading.available_bytes, 10_000_000_000);
        assert_eq!(reading.used_percent, 0);
    }

    #[test]
    fn a_volume_of_size_zero_is_no_size() {
        assert_eq!(read(0, 0), Err(CapacityError::NoSize));
    }

    #[test]
    fn a_failing_read_is_an_io_error_with_the_kind() {
        let root = TestRoot::new().expect("a test root can be created");
        let fs = FakeFilesystem::new(&root);
        fs.fail_next(FsOp::VolumeSpace, Fault::Io);
        fs.fail_next(FsOp::VolumeSpace, Fault::PermissionDenied);

        let other = capacity(&fs, root.path(), now());
        let denied = capacity(&fs, root.path(), now());
        let missing = capacity(&fs, &root.path().join("missing"), now());

        assert_eq!(
            other,
            Err(CapacityError::Io {
                kind: io::ErrorKind::Other
            })
        );
        assert_eq!(
            denied,
            Err(CapacityError::Io {
                kind: io::ErrorKind::PermissionDenied
            })
        );
        assert_eq!(
            missing,
            Err(CapacityError::Io {
                kind: io::ErrorKind::NotFound
            })
        );
    }
}
