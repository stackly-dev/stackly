use std::fmt;
use std::path::PathBuf;

use crate::rule::Rule;

/// The kind of value type a [`ValueError`] is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ValueKind {
    /// A [`ShortKey`](crate::ShortKey).
    ShortKey,
    /// A [`LogicalRoot`](crate::LogicalRoot).
    LogicalRoot,
    /// An [`InstancePathKind`](crate::InstancePathKind).
    InstancePathKind,
    /// A [`RelativePath`](crate::RelativePath).
    RelativePath,
    /// An [`Os`](crate::Os).
    Os,
    /// An [`Arch`](crate::Arch).
    Arch,
    /// A [`Target`](crate::Target).
    Target,
    /// An [`InstanceName`](crate::InstanceName).
    InstanceName,
    /// A [`DisplayName`](crate::DisplayName).
    DisplayName,
    /// A [`SoftwareName`](crate::SoftwareName).
    SoftwareName,
    /// A [`ProviderName`](crate::ProviderName).
    ProviderName,
    /// A [`RevisionLabel`](crate::RevisionLabel).
    RevisionLabel,
    /// An [`ExactVersion`](crate::ExactVersion).
    ExactVersion,
    /// A [`RuntimeLine`](crate::RuntimeLine).
    RuntimeLine,
    /// A [`Sha256Digest`](crate::Sha256Digest).
    Sha256Digest,
    /// An [`InstallKind`](crate::InstallKind).
    InstallKind,
    /// An [`InstanceManifest`](crate::InstanceManifest) as a whole.
    InstanceManifest,
}

impl ValueKind {
    /// Returns the name of this kind as it appears in error messages.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ShortKey => "short key",
            Self::LogicalRoot => "logical root",
            Self::InstancePathKind => "instance path kind",
            Self::RelativePath => "relative path",
            Self::Os => "os",
            Self::Arch => "arch",
            Self::Target => "target",
            Self::InstanceName => "instance name",
            Self::DisplayName => "display name",
            Self::SoftwareName => "software name",
            Self::ProviderName => "provider name",
            Self::RevisionLabel => "revision label",
            Self::ExactVersion => "exact version",
            Self::RuntimeLine => "runtime line",
            Self::Sha256Digest => "sha256 digest",
            Self::InstallKind => "install mode",
            Self::InstanceManifest => "instance manifest",
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

    /// Returns the business rule this error enforces. Value rules come from
    /// the PRD's validation IDs, not its business rules, so this is always
    /// `None`.
    #[must_use]
    pub const fn rule(&self) -> Option<Rule> {
        None
    }
}

/// One of the domain's state machines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Machine {
    /// The state of an installed runtime.
    Runtime,
    /// The lifecycle of an instance.
    InstanceLifecycle,
    /// The state of an operation.
    Operation,
}

impl Machine {
    /// Returns the stable snake_case code, for example `instance_lifecycle`.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::Runtime => "runtime",
            Self::InstanceLifecycle => "instance_lifecycle",
            Self::Operation => "operation",
        }
    }

    /// Returns the name used in error messages, for example
    /// `instance lifecycle`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Runtime => "runtime",
            Self::InstanceLifecycle => "instance lifecycle",
            Self::Operation => "operation",
        }
    }
}

impl fmt::Display for Machine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Error returned when a state machine has no transition for an event in
/// its current state. The state does not change.
///
/// ```
/// use stackly_domain::{Machine, TransitionError};
///
/// let err = TransitionError::new(Machine::InstanceLifecycle, "running", "start_begin");
/// assert_eq!(
///     err.to_string(),
///     "instance lifecycle cannot apply start_begin in state running",
/// );
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("{machine} cannot apply {event} in state {from}")]
pub struct TransitionError {
    machine: Machine,
    from: &'static str,
    event: &'static str,
}

impl TransitionError {
    /// Creates the error for `event` in state `from`, both given as their
    /// stable snake_case codes. The state machines in this crate are the
    /// callers; other crates read it.
    #[must_use]
    pub const fn new(machine: Machine, from: &'static str, event: &'static str) -> Self {
        Self {
            machine,
            from,
            event,
        }
    }

    /// Returns the machine that refused the event.
    #[must_use]
    pub const fn machine(&self) -> Machine {
        self.machine
    }

    /// Returns the code of the state the machine was in.
    #[must_use]
    pub const fn from(&self) -> &'static str {
        self.from
    }

    /// Returns the code of the refused event.
    #[must_use]
    pub const fn event(&self) -> &'static str {
        self.event
    }

    /// Returns the business rule this error enforces. A missing transition
    /// is not a business rule, so this is always `None`.
    #[must_use]
    pub const fn rule(&self) -> Option<Rule> {
        None
    }
}

/// What an [`InvariantError`] rejected.
///
/// Each detail maps to at most one business rule through
/// [`InvariantDetail::rule`]. Details that are not a BR violation map to
/// `None`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum InvariantDetail {
    /// Two data roots are the same folder, or one is inside the other
    /// (BR-002).
    DataRootOverlap {
        /// The data root that was checked.
        candidate: PathBuf,
        /// The data root it overlaps.
        existing: PathBuf,
    },
}

impl InvariantDetail {
    /// Returns the business rule this detail breaks, or `None` when it is
    /// not a BR violation.
    #[must_use]
    pub const fn rule(&self) -> Option<Rule> {
        match self {
            Self::DataRootOverlap { .. } => Some(Rule::Br002),
        }
    }
}

impl fmt::Display for InvariantDetail {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DataRootOverlap {
                candidate,
                existing,
            } => write!(
                f,
                "data root {} overlaps the data root {}",
                candidate.display(),
                existing.display()
            ),
        }
    }
}

/// Error returned when an action would break a domain invariant.
///
/// The message ends with the rule ID when there is one.
///
/// ```
/// use stackly_domain::{InvariantDetail, InvariantError, Rule};
///
/// let err = InvariantError::new(InvariantDetail::DataRootOverlap {
///     candidate: "/data/a/b".into(),
///     existing: "/data/a".into(),
/// });
/// assert_eq!(err.rule(), Some(Rule::Br002));
/// assert_eq!(
///     err.to_string(),
///     "data root /data/a/b overlaps the data root /data/a (BR-002)",
/// );
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvariantError {
    detail: InvariantDetail,
}

impl InvariantError {
    /// Creates the error for `detail`. Its rule follows from the detail.
    #[must_use]
    pub const fn new(detail: InvariantDetail) -> Self {
        Self { detail }
    }

    /// Returns the business rule that was broken, or `None` when the
    /// invariant is not a BR.
    #[must_use]
    pub const fn rule(&self) -> Option<Rule> {
        self.detail.rule()
    }

    /// Returns what was rejected.
    #[must_use]
    pub const fn detail(&self) -> &InvariantDetail {
        &self.detail
    }
}

impl From<InvariantDetail> for InvariantError {
    fn from(detail: InvariantDetail) -> Self {
        Self::new(detail)
    }
}

impl fmt::Display for InvariantError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.rule() {
            Some(rule) => write!(f, "{} ({rule})", self.detail),
            None => write!(f, "{}", self.detail),
        }
    }
}

impl std::error::Error for InvariantError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transition_error_names_machine_state_and_event_and_has_no_rule() {
        let err = TransitionError::new(Machine::Operation, "succeeded", "start");

        assert_eq!(
            err.to_string(),
            "operation cannot apply start in state succeeded"
        );
        assert_eq!(err.machine().as_code(), "operation");
        assert_eq!((err.from(), err.event()), ("succeeded", "start"));
        assert_eq!(err.rule(), None);
    }

    #[test]
    fn invariant_error_takes_its_rule_from_the_detail() {
        let detail = InvariantDetail::DataRootOverlap {
            candidate: PathBuf::from("a"),
            existing: PathBuf::from("a"),
        };
        let err = InvariantError::from(detail.clone());

        assert_eq!(err.rule(), Some(Rule::Br002));
        assert_eq!(err.detail(), &detail);
    }

    #[test]
    fn value_errors_carry_no_business_rule() {
        let err = ValueError::new(ValueKind::InstanceName, "must not be empty");

        assert_eq!(err.to_string(), "invalid instance name: must not be empty");
        assert_eq!(err.rule(), None);
    }
}
