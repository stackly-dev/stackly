use std::fmt;

/// The kind of value type a [`ValueError`] is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ValueKind {
    /// A [`ShortKey`](crate::ShortKey).
    ShortKey,
    /// An [`Os`](crate::Os).
    Os,
    /// An [`Arch`](crate::Arch).
    Arch,
    /// A [`Target`](crate::Target).
    Target,
}

impl ValueKind {
    /// Returns the name of this kind as it appears in error messages.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ShortKey => "short key",
            Self::Os => "os",
            Self::Arch => "arch",
            Self::Target => "target",
        }
    }
}

impl fmt::Display for ValueKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Error returned when a value type rejects its input.
///
/// Value types validate in their constructor, so a value that exists is
/// always valid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("invalid {kind}: {reason}")]
pub struct ValueError {
    kind: ValueKind,
    reason: &'static str,
}

impl ValueError {
    pub(crate) const fn new(kind: ValueKind, reason: &'static str) -> Self {
        Self { kind, reason }
    }

    /// Returns the kind of value that was rejected.
    #[must_use]
    pub const fn kind(&self) -> ValueKind {
        self.kind
    }

    /// Returns the rule the input broke, for example
    /// `must be 8 lowercase hex characters`.
    #[must_use]
    pub const fn reason(&self) -> &'static str {
        self.reason
    }
}
