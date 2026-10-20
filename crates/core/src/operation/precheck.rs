//! The feature's own check in the begin gate (spec 0007, "Idempotency",
//! check 3a): after the idempotency key lookup and before anything that
//! depends on the resource, a feature may refuse the request, answer it
//! without an operation, or let it go on.

use std::fmt;
use std::sync::Arc;

use stackly_domain::{DetailKey, ErrorCodeName, FailureItem, Rule, StageName};

use super::work::{FinalBatch, OperationError};

/// The stage of the `succeeded` operation that records a no operation
/// answer which wrote something. A work never sets it.
pub const NO_OP_STAGE: StageName = StageName::from_static("no_op");

/// A feature's check of a request, run once in the begin gate on a
/// blocking thread, so it may read the registry.
pub trait Precheck: Send + 'static {
    /// Decides what happens to the request.
    fn check(self: Box<Self>) -> PrecheckOutcome;
}

impl<F> Precheck for F
where
    F: FnOnce() -> PrecheckOutcome + Send + 'static,
{
    fn check(self: Box<Self>) -> PrecheckOutcome {
        (*self)()
    }
}

/// What a [`Precheck`] decided.
pub enum PrecheckOutcome {
    /// Refuse the request with the feature's error; nothing is written.
    Reject(Box<dyn OperationError>),
    /// Answer without running any work.
    NoOp(NoOpReply),
    /// Go on with the rest of the begin checks.
    Proceed,
}

impl fmt::Debug for PrecheckOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Reject(error) => write!(f, "Reject({}: {error})", error.code()),
            Self::NoOp(reply) => f.debug_tuple("NoOp").field(reply).finish(),
            Self::Proceed => f.write_str("Proceed"),
        }
    }
}

/// A no operation answer.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct NoOpReply {
    /// What the answer still writes, for example a changed desired state.
    /// The engine commits it with a `succeeded` operation that has no
    /// steps, so the idempotency key is bound and a replay gets the same
    /// answer. `None` writes nothing, creates no operation, and binds no
    /// key.
    pub batch: Option<FinalBatch>,
}

impl NoOpReply {
    /// An answer that writes nothing.
    #[must_use]
    pub const fn nothing() -> Self {
        Self { batch: None }
    }

    /// An answer that still commits `batch`.
    #[must_use]
    pub const fn writing(batch: FinalBatch) -> Self {
        Self { batch: Some(batch) }
    }
}

type RejectionParts = (
    ErrorCodeName,
    String,
    bool,
    Vec<(DetailKey, String)>,
    Vec<FailureItem>,
    Option<Rule>,
);

/// The error a [`Precheck`] refused a request with, shared so
/// [`BeginError`](super::BeginError) stays cheap to clone. Two rejections
/// are equal when their code, message, retryability, details, items, and
/// rule are.
#[derive(Clone)]
pub struct Rejection(Arc<dyn OperationError>);

impl Rejection {
    /// Wraps the feature's error.
    #[must_use]
    pub fn new(error: Box<dyn OperationError>) -> Self {
        Self(Arc::from(error))
    }

    /// Returns the feature's error.
    #[must_use]
    pub fn error(&self) -> &dyn OperationError {
        self.0.as_ref()
    }

    /// Returns its catalog code.
    #[must_use]
    pub fn code(&self) -> ErrorCodeName {
        self.0.code()
    }

    fn parts(&self) -> RejectionParts {
        (
            self.0.code(),
            self.0.to_string(),
            self.0.retryable(),
            self.0.details(),
            self.0.items(),
            self.0.rule(),
        )
    }
}

impl PartialEq for Rejection {
    fn eq(&self, other: &Self) -> bool {
        self.parts() == other.parts()
    }
}

impl Eq for Rejection {}

impl fmt::Debug for Rejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Rejection({}: {})", self.0.code(), self.0)
    }
}

impl fmt::Display for Rejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}
