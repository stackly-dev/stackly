//! Instance paths as the registry stores them: a logical root plus a path
//! relative to it, so a registry still makes sense when the roots move.

use std::fmt;
use std::str::FromStr;

use crate::error::{ValueError, ValueKind};
use crate::text::serde_as_text;

/// Longest [`RelativePath`], in bytes.
const RELATIVE_PATH_MAX: usize = 1024;

/// One of the owned roots a stored path is relative to.
///
/// The cache root holds nothing an instance owns, so it is not a logical
/// root.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum LogicalRoot {
    /// The data root.
    Data,
    /// The logs root.
    Logs,
    /// The short runtime root for sockets and PID files.
    Runtime,
}

impl LogicalRoot {
    /// Every logical root.
    pub const ALL: [Self; 3] = [Self::Data, Self::Logs, Self::Runtime];

    /// Returns the stable code, for example `data`.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::Data => "data",
            Self::Logs => "logs",
            Self::Runtime => "runtime",
        }
    }
}

impl fmt::Display for LogicalRoot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_code())
    }
}

impl FromStr for LogicalRoot {
    type Err = ValueError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|root| root.as_code() == s)
            .ok_or(ValueError::new(
                ValueKind::LogicalRoot,
                "must be data, logs, or runtime",
            ))
    }
}

/// What one stored path of an instance is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum InstancePathKind {
    /// The instance folder.
    Root,
    /// The software's data directory.
    Data,
    /// The rendered configuration.
    Config,
    /// Earlier configurations.
    ConfigHistory,
    /// Scratch space.
    Temp,
    /// The instance's log folder.
    Logs,
    /// The run folder for sockets and the PID file.
    Run,
}

impl InstancePathKind {
    /// Every kind, in the order an instance's entries are listed.
    pub const ALL: [Self; 7] = [
        Self::Root,
        Self::Data,
        Self::Config,
        Self::ConfigHistory,
        Self::Temp,
        Self::Logs,
        Self::Run,
    ];

    /// Returns the stable code, for example `config_history`.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::Root => "root",
            Self::Data => "data",
            Self::Config => "config",
            Self::ConfigHistory => "config_history",
            Self::Temp => "temp",
            Self::Logs => "logs",
            Self::Run => "run",
        }
    }
}

impl fmt::Display for InstancePathKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_code())
    }
}

impl FromStr for InstancePathKind {
    type Err = ValueError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|kind| kind.as_code() == s)
            .ok_or(ValueError::new(
                ValueKind::InstancePathKind,
                "must be root, data, config, config_history, temp, logs, or run",
            ))
    }
}

/// A path relative to a [`LogicalRoot`]: 1 to 1024 bytes of `/` separated
/// components, none of them empty, `.`, or `..`, with no leading or
/// trailing `/`.
///
/// A NUL byte or a backslash is rejected too, so no component can name a
/// parent folder or a separator on any OS. Joining one onto a root
/// therefore never leaves that root.
///
/// ```
/// use stackly_domain::RelativePath;
///
/// let path: RelativePath = "instances/01928c3e-6f1a-7b2c-8d4e-5f60718293a4/data".parse()?;
/// assert_eq!(path.components().count(), 3);
/// assert!("../etc".parse::<RelativePath>().is_err());
/// # Ok::<(), stackly_domain::ValueError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RelativePath(String);

impl RelativePath {
    /// Checks and wraps a relative path.
    ///
    /// # Errors
    ///
    /// Returns a [`ValueError`] of kind [`ValueKind::RelativePath`] when the
    /// text breaks a rule in the type's description.
    pub fn new(path: impl Into<String>) -> Result<Self, ValueError> {
        let path = path.into();
        let invalid = |reason| Err(ValueError::new(ValueKind::RelativePath, reason));
        if path.is_empty() || path.len() > RELATIVE_PATH_MAX {
            return invalid("must be 1 to 1024 bytes");
        }
        if path.contains(['\0', '\\']) {
            return invalid("must not contain a NUL byte or a backslash");
        }
        if path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
        {
            return invalid(
                "must be `/` separated components without a leading or trailing `/`, \
                 and no component may be empty, `.`, or `..`",
            );
        }
        Ok(Self(path))
    }

    /// Returns the text form.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns the components, in order.
    pub fn components(&self) -> impl Iterator<Item = &str> {
        self.0.split('/')
    }

    /// Returns true when `self` equals `other` or lies inside it.
    #[must_use]
    pub fn is_same_or_inside(&self, other: &Self) -> bool {
        let mut mine = self.components();
        other
            .components()
            .all(|part| mine.next().is_some_and(|own| own == part))
    }
}

impl fmt::Display for RelativePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for RelativePath {
    type Err = ValueError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

serde_as_text!(RelativePath => "a relative path");

/// One stored path of an instance: what it is for, the root it is
/// relative to, and the path under that root.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct InstancePathEntry {
    kind: InstancePathKind,
    root: LogicalRoot,
    path: RelativePath,
}

impl InstancePathEntry {
    /// Bundles one stored path.
    #[must_use]
    pub const fn new(kind: InstancePathKind, root: LogicalRoot, path: RelativePath) -> Self {
        Self { kind, root, path }
    }

    /// Returns what the path is for.
    #[must_use]
    pub const fn kind(&self) -> InstancePathKind {
        self.kind
    }

    /// Returns the root the path is relative to.
    #[must_use]
    pub const fn root(&self) -> LogicalRoot {
        self.root
    }

    /// Returns the path under the root.
    #[must_use]
    pub const fn path(&self) -> &RelativePath {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_round_trip_for_every_root_and_kind() {
        for root in LogicalRoot::ALL {
            assert_eq!(root.as_code().parse::<LogicalRoot>().unwrap(), root);
        }
        for kind in InstancePathKind::ALL {
            assert_eq!(kind.to_string().parse::<InstancePathKind>().unwrap(), kind);
        }
        let err = "cache".parse::<LogicalRoot>().unwrap_err();
        assert_eq!(err.kind(), ValueKind::LogicalRoot);
        let err = "manifest".parse::<InstancePathKind>().unwrap_err();
        assert_eq!(err.kind(), ValueKind::InstancePathKind);
    }

    #[test]
    fn relative_paths_accept_plain_components() {
        for text in ["a", "instances/x/data", "i/718293a4", "a.b/c-d/e_f"] {
            assert_eq!(RelativePath::new(text).unwrap().as_str(), text);
        }
        let longest = "a".repeat(RELATIVE_PATH_MAX);
        RelativePath::new(longest).unwrap();
    }

    #[test]
    fn relative_paths_reject_escapes_empty_components_and_separators() {
        let too_long = "a".repeat(RELATIVE_PATH_MAX + 1);
        let cases = [
            "",
            "/abs",
            "trailing/",
            "a//b",
            ".",
            "..",
            "a/./b",
            "a/../b",
            "../a",
            "a/..",
            "a\\b",
            "a\0b",
            too_long.as_str(),
        ];
        for text in cases {
            let err = RelativePath::new(text).unwrap_err();
            assert_eq!(err.kind(), ValueKind::RelativePath, "{text:?}");
        }
    }

    #[test]
    fn same_or_inside_compares_whole_components() {
        let path = |text: &str| RelativePath::new(text).unwrap();

        assert!(path("instances/a").is_same_or_inside(&path("instances/a")));
        assert!(path("instances/a/data").is_same_or_inside(&path("instances/a")));
        assert!(!path("instances/a").is_same_or_inside(&path("instances/a/data")));
        assert!(!path("instances/ab").is_same_or_inside(&path("instances/a")));
    }

    #[test]
    fn relative_paths_travel_through_serde_as_text_and_validate() {
        let path = RelativePath::new("logs/x").unwrap();
        let json = serde_json::to_string(&path).unwrap();

        assert_eq!(json, "\"logs/x\"");
        assert_eq!(serde_json::from_str::<RelativePath>(&json).unwrap(), path);
        assert!(serde_json::from_str::<RelativePath>("\"../x\"").is_err());
    }
}
