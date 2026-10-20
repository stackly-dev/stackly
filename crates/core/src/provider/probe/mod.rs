//! Native probes (spec 0023, *Jenis probe native*): the protocol logic of
//! each native readiness probe, as a state machine without IO.
//!
//! The supervisor (spec 0010, `supervisor::probe::native`) owns the
//! connection and the deadline of one attempt; it feeds the bytes it reads
//! to a machine here and writes the bytes the machine returns. Nothing in
//! this module opens a socket, reads a clock, or blocks, so every outcome
//! can be tested and fuzzed from bytes alone.

use stackly_domain::ReasonCode;

pub mod nats;

/// Why one native probe attempt failed.
///
/// `detail` is raw English text. The runner of spec 0010 turns it into an
/// `ObservationDetail` the same way for every probe: it redacts the text
/// through the `Redactor`, cuts it to 256 characters, then builds the
/// detail. Every value the server sent is already shortened and stripped
/// of control characters here.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct NativeProbeFailure {
    /// The readiness reason, for example `identity_mismatch`.
    pub reason: ReasonCode,
    /// What went wrong, as raw text that names no secret.
    pub detail: String,
}

impl NativeProbeFailure {
    /// A failure with `reason` and `detail`.
    #[must_use]
    pub fn new(reason: ReasonCode, detail: impl Into<String>) -> Self {
        Self {
            reason,
            detail: detail.into(),
        }
    }
}
