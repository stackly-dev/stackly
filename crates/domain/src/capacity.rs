//! How full the disk that holds Stackly data is: the level a free space
//! reading maps to (spec 0022). Only the daemon computes the level, so the
//! thresholds live here and nowhere else.

/// Fewer bytes than this left is [`CapacityLevel::Critical`] (2 GB).
pub const CAPACITY_CRITICAL_AVAILABLE: u64 = 2_000_000_000;

/// Fewer bytes than this left is [`CapacityLevel::Low`] (5 GB).
pub const CAPACITY_LOW_AVAILABLE: u64 = 5_000_000_000;

/// This percent used or more is [`CapacityLevel::Low`].
pub const CAPACITY_LOW_USED_PERCENT: u8 = 90;

/// This percent used or more is [`CapacityLevel::Filling`].
pub const CAPACITY_FILLING_USED_PERCENT: u8 = 80;

/// How worried the user should be about the free space of a volume, from
/// the least to the most.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CapacityLevel {
    /// Plenty of space is left.
    Ok,
    /// At least 80 percent is used.
    Filling,
    /// At least 90 percent is used, or under 5 GB is left.
    Low,
    /// Under 2 GB is left.
    Critical,
}

/// The percent of the volume that is in use, rounded down, with
/// `available` held to `total`. A `total` of 0 gives 0.
#[must_use]
pub fn used_percent(total: u64, available: u64) -> u8 {
    if total == 0 {
        return 0;
    }
    let available = available.min(total);
    let used = u128::from(total - available) * 100 / u128::from(total);
    // `used` is at most 100 because `available` is held to `total`.
    u8::try_from(used).unwrap_or(100)
}

/// The level of a volume of `total` bytes with `available` bytes left, or
/// `None` when `total` is 0, which says nothing about the volume.
///
/// `available` is held to `total` first. The first rule that matches wins:
/// `Critical` under [`CAPACITY_CRITICAL_AVAILABLE`] left, `Low` at
/// [`CAPACITY_LOW_USED_PERCENT`] used or under [`CAPACITY_LOW_AVAILABLE`]
/// left, `Filling` at [`CAPACITY_FILLING_USED_PERCENT`] used, else `Ok`.
#[must_use]
pub fn level(total: u64, available: u64) -> Option<CapacityLevel> {
    if total == 0 {
        return None;
    }
    let available = available.min(total);
    let used = used_percent(total, available);
    Some(if available < CAPACITY_CRITICAL_AVAILABLE {
        CapacityLevel::Critical
    } else if used >= CAPACITY_LOW_USED_PERCENT || available < CAPACITY_LOW_AVAILABLE {
        CapacityLevel::Low
    } else if used >= CAPACITY_FILLING_USED_PERCENT {
        CapacityLevel::Filling
    } else {
        CapacityLevel::Ok
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const TB: u64 = 1_000_000_000_000;

    #[test]
    fn used_percent_rounds_down_and_holds_available_to_total() {
        assert_eq!(used_percent(100, 100), 0);
        assert_eq!(used_percent(100, 0), 100);
        assert_eq!(used_percent(1000, 201), 79);
        assert_eq!(used_percent(1000, 200), 80);
        assert_eq!(used_percent(100, 500), 0);
        assert_eq!(used_percent(0, 0), 0);
        assert_eq!(used_percent(u64::MAX, 0), 100);
    }

    #[test]
    fn a_volume_of_size_zero_has_no_level() {
        assert_eq!(level(0, 0), None);
        assert_eq!(level(0, 5), None);
    }

    #[test]
    fn critical_starts_below_two_gigabytes_available() {
        assert_eq!(level(TB, 1_999_999_999), Some(CapacityLevel::Critical));
        assert_eq!(level(TB, 2_000_000_000), Some(CapacityLevel::Low));
        assert_eq!(level(1, 0), Some(CapacityLevel::Critical));
    }

    #[test]
    fn low_covers_under_five_gigabytes_available() {
        // A volume of 20 GB keeps the percent rules out of the way: 5 GB left
        // is 75 percent used.
        assert_eq!(
            level(20_000_000_000, 4_999_999_999),
            Some(CapacityLevel::Low)
        );
        assert_eq!(
            level(20_000_000_000, 5_000_000_000),
            Some(CapacityLevel::Ok)
        );
    }

    #[test]
    fn low_starts_at_ninety_percent_used_and_filling_at_eighty() {
        let total = 1000 * 5_000_000_000;
        let at = |percent: u64| total - total / 100 * percent;

        assert_eq!(level(total, at(79)), Some(CapacityLevel::Ok));
        assert_eq!(level(total, at(80)), Some(CapacityLevel::Filling));
        assert_eq!(level(total, at(89)), Some(CapacityLevel::Filling));
        assert_eq!(level(total, at(90)), Some(CapacityLevel::Low));
        assert_eq!(level(total, at(99)), Some(CapacityLevel::Low));
    }

    #[test]
    fn critical_beats_the_percent_rules_and_available_above_total_counts_as_all_free() {
        assert_eq!(level(1000, 0), Some(CapacityLevel::Critical));
        assert_eq!(level(10 * TB, 10 * TB + 7), Some(CapacityLevel::Ok));
        assert_eq!(level(1000, u64::MAX), Some(CapacityLevel::Critical));
    }
}
