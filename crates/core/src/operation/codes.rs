//! The catalog codes the operation engine stores in a journal's failure
//! error (spec 0007, "Kode error"). `stacklyd` looks each one up in the
//! catalog of `stackly-protocol`; a test there keeps this list and the
//! catalog in step.

use stackly_domain::ErrorCodeName;

/// The final error of an operation closed after the daemon stopped.
pub const INTERRUPTED: ErrorCodeName = ErrorCodeName::from_static("operation.interrupted");
/// The final error of an operation that shutdown stopped.
pub const DAEMON_STOPPING: ErrorCodeName = ErrorCodeName::from_static("operation.daemon_stopping");
/// The final error of an abandoned operation.
pub const ABANDONED: ErrorCodeName = ErrorCodeName::from_static("operation.abandoned");
/// The error recorded when compensation failed.
pub const COMPENSATION_FAILED: ErrorCodeName =
    ErrorCodeName::from_static("operation.compensation_failed");
/// A lock key is held by an operation in `needs_recovery`.
pub const RECOVERY_REQUIRED: ErrorCodeName =
    ErrorCodeName::from_static("operation.recovery_required");
/// More than 256 steps or 1024 created items.
pub const JOURNAL_FULL: ErrorCodeName = ErrorCodeName::from_static("operation.journal_full");
/// A journal payload over its limit.
pub const PAYLOAD_TOO_LARGE: ErrorCodeName =
    ErrorCodeName::from_static("operation.payload_too_large");
/// The resource changed since the request read it.
pub const REVISION_MISMATCH: ErrorCodeName =
    ErrorCodeName::from_static("resource.revision_mismatch");
/// The resource no longer exists.
pub const NOT_FOUND: ErrorCodeName = ErrorCodeName::from_static("resource.not_found");
/// A bug: a panic, a journal write that kept failing, or work that ended
/// with intended items.
pub const INTERNAL_UNEXPECTED: ErrorCodeName = ErrorCodeName::from_static("internal.unexpected");

/// Every code above, for the catalog test of `stackly-protocol`.
pub const ALL: &[ErrorCodeName] = &[
    INTERRUPTED,
    DAEMON_STOPPING,
    ABANDONED,
    COMPENSATION_FAILED,
    RECOVERY_REQUIRED,
    JOURNAL_FULL,
    PAYLOAD_TOO_LARGE,
    REVISION_MISMATCH,
    NOT_FOUND,
    INTERNAL_UNEXPECTED,
];
