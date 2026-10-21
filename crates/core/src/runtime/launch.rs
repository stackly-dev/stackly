//! Checks a runtime executable before and right after it runs (spec 0018,
//! *Validasi binary*; spec 0010, K7): the quarantine attribute, and a main
//! process that macOS killed. Install validation and the start of an
//! instance both call them, so a binary the OS refuses is diagnosed the same
//! way in both.

use std::path::{Path, PathBuf};

use stackly_platform::{ExitStatus, Platform};

/// The signal number of `SIGKILL`. Only the name is kept out of this
/// crate's source: `cargo xtask check-deps` allows the token in one file of
/// the platform adapter.
const KILL_SIGNAL: i32 = 9;

/// Why an executable must not run, or did not run.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum LaunchRefusal {
    /// The executable carries `com.apple.quarantine`. Stackly never removes
    /// it, so the runtime is refused as it is.
    Quarantined {
        /// The executable.
        path: PathBuf,
        /// The name of the agent that set the attribute, if it says one.
        agent: Option<String>,
    },
    /// The OS killed a process of this daemon with `SIGKILL` that the
    /// daemon did not send, which is how macOS refuses a binary whose
    /// signature or origin it rejects.
    BinaryRejectedByOs {
        /// The signal that ended it.
        signal: i32,
    },
}

/// Refuses an executable that carries the quarantine attribute.
///
/// A quarantine attribute that cannot be read does not refuse the start: the
/// spawn reports what is wrong with the file.
///
/// # Errors
///
/// Returns [`LaunchRefusal::Quarantined`] when the executable has the
/// attribute.
pub fn check_launchable(platform: &Platform, executable: &Path) -> Result<(), LaunchRefusal> {
    match platform.code_signing().quarantine(executable) {
        Ok(Some(info)) => Err(LaunchRefusal::Quarantined {
            path: executable.to_path_buf(),
            agent: info.agent,
        }),
        Ok(None) => Ok(()),
        Err(error) => {
            tracing::warn!(%error, "the quarantine attribute of a runtime executable could not be read");
            Ok(())
        }
    }
}

/// Classifies the end of a main process that did not exit by itself in the
/// way the provider expects.
///
/// Only a child of this daemon with status `Some(Signaled(9))` that the
/// daemon did not terminate is classified. A status of `None` (a process the
/// daemon adopted, which is not its child) and a kill the daemon sent through
/// a discard or a terminate are never classified.
#[must_use]
pub fn classify_early_exit(
    status: Option<ExitStatus>,
    terminated_by_daemon: bool,
) -> Option<LaunchRefusal> {
    match status {
        Some(ExitStatus::Signaled(signal)) if signal == KILL_SIGNAL && !terminated_by_daemon => {
            Some(LaunchRefusal::BinaryRejectedByOs { signal })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_unrequested_sigkill_of_a_child_is_a_binary_the_os_rejected() {
        assert_eq!(
            classify_early_exit(Some(ExitStatus::Signaled(9)), false),
            Some(LaunchRefusal::BinaryRejectedByOs { signal: 9 })
        );
        for (status, by_daemon) in [
            (Some(ExitStatus::Signaled(9)), true),
            (None, false),
            (None, true),
            (Some(ExitStatus::Signaled(6)), false),
            (Some(ExitStatus::Signaled(15)), false),
            (Some(ExitStatus::Exited(0)), false),
            (Some(ExitStatus::Exited(137)), false),
        ] {
            assert_eq!(classify_early_exit(status, by_daemon), None, "{status:?}");
        }
    }
}
