//! The runner of native probes (spec 0023, *Hasil probe dan loop
//! supervisor*): one attempt of a probe from the native registry.

use std::time::Duration;

use super::{PROTOCOL_ERROR, ProbeOutcome};
use crate::provider::NativeProbeSpec;
use crate::secret::Redactor;

/// Runs one attempt of `spec` within `attempt_timeout`.
// provisional: F23 langkah 5
pub(super) async fn run(
    _spec: &NativeProbeSpec,
    _attempt_timeout: Duration,
    redactor: &Redactor,
) -> ProbeOutcome {
    ProbeOutcome::failure(
        PROTOCOL_ERROR,
        "native probe driver is not built yet",
        redactor,
    )
}
