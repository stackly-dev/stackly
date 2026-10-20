//! The writer of a helper's stdin pipe (spec 0010, K8): the only place of
//! the process adapter that reads a secret value, so it is the
//! `stackly-platform-macos` `process::stdin` entry of the `expose`
//! allowlist (spec 0011, *Penjaga di kode*).

use std::fs::File;
use std::io::Write;
use std::os::fd::{AsRawFd, OwnedFd};
use std::thread;

use stackly_platform::{ProcessError, SecretValue};

use super::spawn::last_error;
use super::sys;

/// Writes `value` to the write end of a helper's stdin pipe on a thread of
/// its own, then closes the pipe, so the helper reads the secret and then
/// end of file.
///
/// The thread may start before the helper exists: the pipe buffers what it
/// can, and the write waits for the helper to read the rest. A helper that
/// exits without reading ends the write with `EPIPE` (never `SIGPIPE`),
/// which is ignored, and a helper the supervisor terminates on its timeout
/// does the same, so the thread never outlives the helper's step.
///
/// # Errors
///
/// Returns [`ProcessError::Io`] when `SIGPIPE` cannot be turned off for
/// the pipe or the thread cannot start; the value is wiped either way.
pub(super) fn feed(pipe: OwnedFd, value: SecretValue) -> Result<(), ProcessError> {
    // SAFETY: fcntl only changes a flag of a descriptor this function owns.
    if unsafe { libc::fcntl(pipe.as_raw_fd(), sys::F_SETNOSIGPIPE, 1) } == -1 {
        return Err(last_error("fcntl F_SETNOSIGPIPE"));
    }
    thread::Builder::new()
        .name("stackly-proc-stdin".to_owned())
        .spawn(move || {
            let mut pipe = File::from(pipe);
            // A helper that stops reading is the helper's failure, which
            // its exit status reports; the bytes are not retried.
            let _ = pipe.write_all(value.expose().as_bytes());
        })
        .map(drop)
        .map_err(|error| ProcessError::io("spawn the stdin writer", None, &error))
}
