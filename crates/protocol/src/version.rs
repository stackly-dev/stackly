use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use typeshare::typeshare;

/// Version of the IPC protocol a peer speaks.
///
/// `major` changes when the wire breaks, `minor` when it only gains optional
/// additions. Peers with the same major always talk, whatever their minors.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolVersion {
    /// Bumped for every breaking change to the wire.
    pub major: u32,
    /// Bumped for every compatible addition.
    pub minor: u32,
}

impl ProtocolVersion {
    /// The protocol version this build speaks.
    ///
    /// The commit that changes the normalized contract digest bumps this in
    /// the same commit; `cargo xtask codegen` records each version's digest
    /// in `protocols/contract-versions.json` (spec 0006).
    pub const CURRENT: Self = Self {
        major: 1,
        minor: 10,
    };

    /// Whether a peer speaking `other` can talk to this one: the majors match.
    #[must_use]
    pub const fn is_compatible_with(self, other: Self) -> bool {
        self.major == other.major
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_version_is_one_point_ten() {
        assert_eq!(
            ProtocolVersion::CURRENT,
            ProtocolVersion {
                major: 1,
                minor: 10
            }
        );
    }

    #[test]
    fn any_minor_of_the_same_major_is_compatible_and_another_major_is_not() {
        let current = ProtocolVersion::CURRENT;
        assert!(current.is_compatible_with(ProtocolVersion {
            major: 1,
            minor: 99
        }));
        assert!(!current.is_compatible_with(ProtocolVersion { major: 2, minor: 0 }));
    }
}
