use std::path::PathBuf;

use stackly_domain::{InvariantError, Rule, ShortKey};
use stackly_platform::PlatformError;

/// Error returned by a core use case.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum CoreError {
    /// An owned root or standard folder exists but is not a private folder
    /// of the current user. It is rejected, never repaired.
    #[error("{} is not a private Stackly folder: {reason}", path.display())]
    RootNotPrivate {
        /// The folder that was rejected.
        path: PathBuf,
        /// Why, for example `owned by another user`.
        reason: &'static str,
    },
    /// Two owned roots are the same folder or nest once resolved.
    #[error("owned roots {} and {} overlap", a.display(), b.display())]
    RootsOverlap {
        /// The first root, canonical.
        a: PathBuf,
        /// The second root, canonical.
        b: PathBuf,
    },
    /// The path does not lie inside any owned root.
    #[error("{} is outside the Stackly roots", path.display())]
    OutsideOwnedRoots {
        /// The path as given.
        path: PathBuf,
    },
    /// The path is an owned root itself, which no operation may target.
    #[error("{} is a Stackly root itself", path.display())]
    IsOwnedRoot {
        /// The path as given.
        path: PathBuf,
    },
    /// The path is relative or has a `.` or `..` component.
    #[error("{} must be absolute, without `.` or `..` components", path.display())]
    NotNormalized {
        /// The path as given.
        path: PathBuf,
    },
    /// The instance folder already exists under `instances/` or
    /// `retained/`, so creating it again would take over existing data
    /// (BR-008).
    #[error("instance folder {} already exists (BR-008)", path.display())]
    AlreadyExists {
        /// The existing folder.
        path: PathBuf,
    },
    /// A known instance already uses the new instance's short key, so their
    /// run folders would collide. The caller creates a new ID and retries.
    #[error("short key {key} is already used by a known instance")]
    ShortKeyTaken {
        /// The short key both instances share.
        key: ShortKey,
    },
    /// Creating an instance folder failed after some paths were created.
    /// Nothing was deleted: `created` lists exactly the paths this call
    /// made, in order, for compensation to act on (BR-023).
    #[error("instance layout stopped after creating {} paths: {source} (BR-023)", created.len())]
    Partial {
        /// Every path this call created, in the order it created them.
        created: Vec<PathBuf>,
        /// The failure that stopped it.
        source: PlatformError,
    },
    /// An instance manifest could not be encoded or decoded.
    #[error(transparent)]
    Manifest(#[from] ManifestError),
    /// The action would break a domain invariant.
    #[error(transparent)]
    Invariant(#[from] InvariantError),
    /// A platform port failed.
    #[error(transparent)]
    Platform(#[from] PlatformError),
}

impl CoreError {
    /// Returns the business rule this error enforces, or `None` when it is
    /// not a BR violation.
    #[must_use]
    pub const fn rule(&self) -> Option<Rule> {
        match self {
            Self::AlreadyExists { .. } => Some(Rule::Br008),
            Self::Partial { .. } => Some(Rule::Br023),
            Self::Invariant(err) => err.rule(),
            _ => None,
        }
    }
}

/// Error returned when an `instance-manifest.json` cannot be encoded or
/// decoded.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ManifestError {
    /// The manifest names a schema this version does not read. The rest of
    /// it was not decoded.
    #[error("unsupported instance manifest schema {schema:?}")]
    UnsupportedSchema {
        /// The `schema` value the manifest holds.
        schema: String,
    },
    /// The bytes are not a valid v1 manifest: not JSON, no string `schema`,
    /// or a field that fails its rule.
    #[error("malformed instance manifest: {reason}")]
    Malformed {
        /// What the decoder rejected.
        reason: String,
    },
    /// The encoded manifest is larger than the limit.
    #[error("instance manifest is {len} bytes, more than the {max} byte limit")]
    TooLarge {
        /// The encoded length in bytes.
        len: usize,
        /// The largest manifest allowed.
        max: u64,
    },
    /// A new instance folder needs a new manifest, one with no
    /// initialization time and no retained marker.
    #[error("instance manifest is not new: {reason}")]
    NotNew {
        /// The field that is already set.
        reason: &'static str,
    },
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use stackly_domain::{InstanceId, InvariantDetail};

    use super::*;

    #[test]
    fn create_errors_name_their_business_rule() {
        let partial = CoreError::Partial {
            created: vec![PathBuf::from("/d/instances/x")],
            source: PlatformError::NoSpace {
                path: PathBuf::from("/d/instances/x/config"),
            },
        };
        let exists = CoreError::AlreadyExists {
            path: PathBuf::from("/d/instances/x"),
        };
        let overlap = CoreError::from(InvariantError::new(InvariantDetail::DataRootOverlap {
            candidate: PathBuf::from("/a/b"),
            existing: PathBuf::from("/a"),
        }));
        let taken = CoreError::ShortKeyTaken {
            key: InstanceId::generate().short_key(),
        };

        assert_eq!(exists.rule(), Some(Rule::Br008));
        assert_eq!(partial.rule(), Some(Rule::Br023));
        assert_eq!(overlap.rule(), Some(Rule::Br002));
        assert_eq!(taken.rule(), None);
        assert!(exists.to_string().ends_with("(BR-008)"), "{exists}");
        assert!(partial.to_string().ends_with("(BR-023)"), "{partial}");
        assert!(overlap.to_string().ends_with("(BR-002)"), "{overlap}");
    }
}
