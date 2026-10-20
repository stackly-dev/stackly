//! The version of an operating system, such as macOS `27.0`.

use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::str::FromStr;

use crate::error::{ValueError, ValueKind};
use crate::text::serde_as_text;

/// Most bytes in the text form of an [`OsVersion`].
const TEXT_MAX: usize = 16;

/// The version of an operating system, or the lowest one an artifact needs:
/// 1 to 3 dot separated numeric components, such as `27`, `13.0`, or
/// `26.0.1`, at most 16 bytes, without leading zeros.
///
/// Versions compare numerically per component, and a missing component
/// counts as 0, so `13` equals `13.0` and `13.0` is less than `27.0`. The
/// text form keeps the components as written.
///
/// ```
/// use stackly_domain::OsVersion;
///
/// let artifact: OsVersion = "13.0".parse()?;
/// let app: OsVersion = "27.0".parse()?;
/// assert!(artifact < app);
/// assert_eq!("13".parse::<OsVersion>()?, artifact);
/// assert_eq!(artifact.to_string(), "13.0");
/// # Ok::<(), stackly_domain::ValueError>(())
/// ```
#[derive(Debug, Clone, Copy)]
pub struct OsVersion {
    parts: [u32; 3],
    len: u8,
}

impl OsVersion {
    /// Creates a `major.minor` version, such as `27.0`.
    #[must_use]
    pub const fn major_minor(major: u32, minor: u32) -> Self {
        Self {
            parts: [major, minor, 0],
            len: 2,
        }
    }

    /// Returns the major component.
    #[must_use]
    pub const fn major(&self) -> u32 {
        self.parts[0]
    }

    /// Returns the components as written, 1 to 3 of them.
    #[must_use]
    pub fn components(&self) -> &[u32] {
        &self.parts[..usize::from(self.len)]
    }
}

impl PartialEq for OsVersion {
    fn eq(&self, other: &Self) -> bool {
        self.parts == other.parts
    }
}

impl Eq for OsVersion {}

impl Hash for OsVersion {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.parts.hash(state);
    }
}

impl PartialOrd for OsVersion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for OsVersion {
    fn cmp(&self, other: &Self) -> Ordering {
        self.parts.cmp(&other.parts)
    }
}

impl fmt::Display for OsVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, part) in self.components().iter().enumerate() {
            if index > 0 {
                f.write_str(".")?;
            }
            write!(f, "{part}")?;
        }
        Ok(())
    }
}

impl FromStr for OsVersion {
    type Err = ValueError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let invalid = |reason| ValueError::new(ValueKind::OsVersion, reason);
        if s.is_empty() || s.len() > TEXT_MAX {
            return Err(invalid("must be 1 to 16 bytes"));
        }
        let mut parts = [0_u32; 3];
        let mut len = 0_u8;
        for component in s.split('.') {
            let slot = parts
                .get_mut(usize::from(len))
                .ok_or_else(|| invalid("must have 1 to 3 components, such as 27 or 13.0"))?;
            if component.is_empty() || !component.bytes().all(|b| b.is_ascii_digit()) {
                return Err(invalid("components must be numbers separated by dots"));
            }
            if component.len() > 1 && component.starts_with('0') {
                return Err(invalid("components must not have leading zeros"));
            }
            *slot = component
                .parse()
                .map_err(|_| invalid("components must fit in 32 bits"))?;
            len += 1;
        }
        Ok(Self { parts, len })
    }
}

serde_as_text!(OsVersion => "an os version such as 27.0");

#[cfg(test)]
mod tests {
    use super::*;

    fn version(text: &str) -> OsVersion {
        text.parse().unwrap()
    }

    #[test]
    fn versions_compare_numerically_per_component() {
        assert!(version("13.0") < version("27.0"));
        assert!(version("9.10") > version("9.9"));
        assert!(version("26.0.1") > version("26.0"));
        assert!(version("12") < version("12.0.1"));
    }

    #[test]
    fn a_missing_component_counts_as_zero_but_the_text_keeps_what_was_written() {
        assert_eq!(version("13"), version("13.0"));
        assert_eq!(version("13"), version("13.0.0"));
        assert_eq!(version("13").to_string(), "13");
        assert_eq!(version("13.0.0").to_string(), "13.0.0");
        assert_eq!(version("26.0.1").components(), &[26, 0, 1]);
    }

    #[test]
    fn equal_versions_hash_the_same() {
        use std::collections::HashSet;

        let set: HashSet<OsVersion> = [version("13"), version("13.0")].into_iter().collect();

        assert_eq!(set.len(), 1);
    }

    #[test]
    fn malformed_text_is_rejected() {
        for text in [
            "",
            ".",
            "13.",
            ".13",
            "13..0",
            "1.2.3.4",
            "v13",
            "13.a",
            "-1",
            "013",
            "13.04",
            "4294967296",
            "1.2.33333333333333",
        ] {
            assert!(text.parse::<OsVersion>().is_err(), "{text} was accepted");
        }
    }

    #[test]
    fn zero_components_are_accepted() {
        assert_eq!(version("0").components(), &[0]);
        assert_eq!(version("10.0").to_string(), "10.0");
    }

    #[test]
    fn major_minor_builds_a_two_component_version() {
        let app = OsVersion::major_minor(27, 0);

        assert_eq!(app.to_string(), "27.0");
        assert_eq!(app.major(), 27);
        assert_eq!(app, version("27"));
    }

    #[test]
    fn os_versions_serialize_as_their_text_form() {
        let json = serde_json::to_string(&version("26.0.1")).unwrap();

        assert_eq!(json, "\"26.0.1\"");
        assert_eq!(
            serde_json::from_str::<OsVersion>(&json).unwrap(),
            version("26.0.1")
        );
        assert!(serde_json::from_str::<OsVersion>("\"13.04\"").is_err());
    }
}
