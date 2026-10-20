//! The extraction limits (spec 0018, *Ekstraksi aman*, table *Batas*).
//!
//! Path, component, and depth limits are those of
//! [`stackly_platform::EntryPath`]: 1024 bytes, 255 bytes, and 32 levels.

/// `max_entries` when the artifact has no `entryCount`.
pub const DEFAULT_MAX_ENTRIES: u64 = 50_000;

/// Without `unpackedSize`, files may hold up to this many times the
/// artifact's size, capped at [`FALLBACK_MAX_TOTAL_FILE_BYTES`].
pub const FALLBACK_SIZE_RATIO: u64 = 32;

/// The cap of the fallback `max_total_file_bytes`: 16 GiB.
pub const FALLBACK_MAX_TOTAL_FILE_BYTES: u64 = 16 << 30;

/// The free space kept on top of the unpacked size before extracting:
/// 64 MiB.
pub const SPACE_RESERVE: u64 = 64 << 20;

/// Without `unpackedSize`, the space check asks for this many times the
/// artifact's size (the largest ratio measured is 4.13, ClickHouse).
pub const FALLBACK_SPACE_RATIO: u64 = 5;

/// The longest symlink target, in bytes.
pub const SYMLINK_TARGET_MAX_LEN: usize = 1024;

/// The most symlinks one resolution follows.
pub const SYMLINK_CHAIN_MAX: usize = 8;

/// The longest PAX extended header or GNU long name of one tar member, in
/// bytes.
pub const EXTENSION_HEADER_MAX_LEN: u64 = 64 << 10;

/// The buffer file contents are copied through.
pub const COPY_BUFFER_LEN: usize = 64 << 10;

/// The most zero bytes a tar stream may hold after its end marker: far
/// above the 10 KiB record padding tar writers add.
pub const TAR_PADDING_MAX_LEN: u64 = 1 << 20;

/// The longest member name an error carries, in characters.
pub const ENTRY_TEXT_MAX_CHARS: usize = 256;

/// The size and entry limits of one extraction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct ExtractLimits {
    /// The most bytes all regular files may hold together.
    pub max_total_file_bytes: u64,
    /// The most entries the tree may hold, skipped AppleDouble members
    /// counted (see [`ExtractReport::entry_count`](super::ExtractReport::entry_count)).
    pub max_entries: u64,
}

impl ExtractLimits {
    /// Limits of exactly these values.
    #[must_use]
    pub const fn new(max_total_file_bytes: u64, max_entries: u64) -> Self {
        Self {
            max_total_file_bytes,
            max_entries,
        }
    }

    /// The limits of an artifact of `size` bytes: its `unpackedSize` and
    /// `entryCount` when the catalog has them, else
    /// `min(32 × size, 16 GiB)` and 50 000.
    #[must_use]
    pub fn for_artifact(size: u64, unpacked_size: Option<u64>, entry_count: Option<u64>) -> Self {
        let fallback_bytes = size
            .saturating_mul(FALLBACK_SIZE_RATIO)
            .min(FALLBACK_MAX_TOTAL_FILE_BYTES);
        Self::new(
            unpacked_size.unwrap_or(fallback_bytes),
            entry_count.unwrap_or(DEFAULT_MAX_ENTRIES),
        )
    }
}

/// The free space the data root needs before extracting an artifact of
/// `size` bytes: `unpackedSize + 64 MiB`, or `5 × size + 64 MiB` without
/// it.
#[must_use]
pub fn required_space(size: u64, unpacked_size: Option<u64>) -> u64 {
    unpacked_size
        .unwrap_or_else(|| size.saturating_mul(FALLBACK_SPACE_RATIO))
        .saturating_add(SPACE_RESERVE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_values_win_over_the_fallbacks() {
        assert_eq!(
            ExtractLimits::for_artifact(1_000, Some(4_000), Some(12)),
            ExtractLimits::new(4_000, 12)
        );
        assert_eq!(required_space(1_000, Some(4_000)), 4_000 + SPACE_RESERVE);
    }

    #[test]
    fn the_fallbacks_scale_with_the_artifact_size_and_stop_at_16_gib() {
        assert_eq!(
            ExtractLimits::for_artifact(1_000, None, None),
            ExtractLimits::new(32_000, DEFAULT_MAX_ENTRIES)
        );
        assert_eq!(
            ExtractLimits::for_artifact(1 << 40, None, None).max_total_file_bytes,
            16 << 30
        );
        assert_eq!(required_space(1_000, None), 5_000 + SPACE_RESERVE);
        assert_eq!(required_space(u64::MAX, None), u64::MAX);
    }
}
