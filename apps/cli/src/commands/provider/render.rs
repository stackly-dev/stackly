//! The words of the provider text: wire values with their colors, and the
//! decimal sizes the Catalog screen writes the same way (spec 0017, *CLI*).

use stackly_protocol::catalog::{CatalogEntryState, SupportStatus};

use crate::output::style::Tone;

/// The color of a catalog entry state: a build that cannot run here is
/// plain, a blocked one asks for a maintainer, an invalid one broke.
#[must_use]
pub const fn state_tone(state: CatalogEntryState) -> Tone {
    match state {
        CatalogEntryState::Available => Tone::Positive,
        CatalogEntryState::Blocked | CatalogEntryState::Unrecognized => Tone::Caution,
        CatalogEntryState::Invalid => Tone::Critical,
        CatalogEntryState::Unsupported | CatalogEntryState::Planned => Tone::Neutral,
    }
}

/// The color of a version's status on one OS.
#[must_use]
pub const fn support_tone(status: SupportStatus) -> Tone {
    match status {
        SupportStatus::Supported => Tone::Positive,
        SupportStatus::Experimental | SupportStatus::Blocked | SupportStatus::Unrecognized => {
            Tone::Caution
        }
        SupportStatus::Unverified | SupportStatus::Planned | SupportStatus::Unsupported => {
            Tone::Neutral
        }
    }
}

/// `bytes` in decimal SI units the way `ByteCountFormatter` with the `.file`
/// style writes them, so the Catalog screen shows the same text: `512
/// bytes`, `12 kB`, `6.8 MB`, `1.23 GB`.
///
/// Kilobytes are whole numbers, megabytes have at most one decimal, and
/// larger units at most two; a size that rounds up to 1000 of its unit moves
/// to the next one.
#[must_use]
pub fn size_text(bytes: u64) -> String {
    match bytes {
        1 => return "1 byte".to_owned(),
        0..=999 => return format!("{bytes} bytes"),
        _ => {}
    }
    let kilobytes = bytes.saturating_add(500) / 1_000;
    if kilobytes < 1_000 {
        return format!("{kilobytes} kB");
    }
    let mut unit = 1_000_000_u128;
    for (name, decimals) in [("MB", 1_u32), ("GB", 2), ("TB", 2)] {
        let scale = 10_u128.pow(decimals);
        let scaled = (u128::from(bytes) * scale + unit / 2) / unit;
        if scaled < 1_000 * scale {
            return with_unit(scaled, scale, name);
        }
        unit *= 1_000;
    }
    with_unit((u128::from(bytes) * 100 + unit / 2) / unit, 100, "PB")
}

/// `scaled / scale` of `unit`, without trailing zeros.
fn with_unit(scaled: u128, scale: u128, unit: &str) -> String {
    let (whole, fraction) = (scaled / scale, scaled % scale);
    if fraction == 0 {
        return format!("{whole} {unit}");
    }
    let digits = scale.ilog10() as usize;
    let fraction = format!("{fraction:0digits$}");
    format!("{whole}.{} {unit}", fraction.trim_end_matches('0'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_are_decimal_with_one_decimal_from_megabytes_up() {
        assert_eq!(size_text(6_785_953), "6.8 MB");
        assert_eq!(size_text(48_100_000), "48.1 MB");
        assert_eq!(size_text(1_200_000_000), "1.2 GB");
        assert_eq!(size_text(1_000_000), "1 MB");
        assert_eq!(size_text(15_000_000), "15 MB");
        assert_eq!(size_text(1_234_000_000), "1.23 GB");
        assert_eq!(size_text(123_400_000_000), "123.4 GB");
        assert_eq!(size_text(2_500_000_000_000), "2.5 TB");
    }

    #[test]
    fn small_sizes_are_bytes_then_whole_kilobytes() {
        assert_eq!(size_text(0), "0 bytes");
        assert_eq!(size_text(1), "1 byte");
        assert_eq!(size_text(500), "500 bytes");
        assert_eq!(size_text(1_500), "2 kB");
        assert_eq!(size_text(12_345), "12 kB");
    }

    #[test]
    fn a_size_that_rounds_up_to_a_thousand_moves_to_the_next_unit() {
        assert_eq!(size_text(999_500), "1 MB");
        assert_eq!(size_text(999_999_999), "1 GB");
        assert_eq!(size_text(9_949_999), "9.9 MB");
        assert_eq!(size_text(99_950_000), "100 MB");
    }

    #[test]
    fn the_largest_size_still_gets_a_unit() {
        assert!(size_text(u64::MAX).ends_with("PB"));
    }
}
