//! Gathering the evidence about a recorded process (spec 0010, *Identitas
//! majemuk*): one function, in a fixed order, shared by reconciliation, the
//! `unknown` recheck, and every other place that asks whether a recorded
//! process still belongs to its instance.
//!
//! The order is the last observation (given), `boot_id`, `probe_lock`, then
//! `inspect` only when there is a record and the boot is the same, then
//! `holds_file` only when the six checks that need no lock file passed. So
//! the facts of a process from another boot, and the file descriptors of a
//! process that is already not ours, are never read, and an error reading
//! them cannot turn `Gone` into `Unproven`.

use std::path::Path;

use stackly_domain::{IdentityEvidence, ProcessIdentity};
use stackly_platform::{FileIdentity, HoldsFile, Platform, ProcessError, ProcessFacts};

use super::classify::{
    Classification, UnprovenBlocker, classify, identity_checks, six_checks_passed,
};
use super::observation_detail;

/// What [`gather_evidence`] found.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Gathered {
    /// The classification of the recorded process.
    pub(crate) classification: Classification,
    /// The facts of the live process, when `inspect` ran and found one.
    pub(crate) facts: Option<ProcessFacts>,
    /// The identity of the lock file, when `holds_file` ran.
    pub(crate) lock_file: Option<FileIdentity>,
}

/// Gathers the evidence about `recorded` for the instance whose lock file is
/// `lock_path`, and classifies it. `recorded` is the last observation of
/// the instance, `None` when it has none.
///
/// Every error of the platform ends in [`Classification::Unproven`], never
/// [`Classification::Gone`].
pub(crate) fn gather_evidence(
    platform: &Platform,
    lock_path: &Path,
    recorded: Option<&ProcessIdentity>,
) -> Gathered {
    let uid = match platform.identity().current_user() {
        Ok(uid) => uid,
        Err(error) => {
            return Gathered {
                classification: unproven_by(recorded, &error.to_string()),
                facts: None,
                lock_file: None,
            };
        }
    };
    let processes = platform.processes();
    let boot = processes.boot_id();
    let lock = processes.probe_lock(lock_path);
    let facts = match (recorded, &boot) {
        (Some(recorded), Ok(boot)) if *boot == recorded.boot => {
            Some(processes.inspect(recorded.pid))
        }
        _ => None,
    };
    let mut lock_file = None;
    let holds = match (recorded, &boot, &facts) {
        (Some(recorded), Ok(boot), Some(Ok(found)))
            if six_checks_passed(&identity_checks(recorded, *boot, found.as_ref(), &uid)) =>
        {
            Some(holds_lock(platform, lock_path, recorded, &mut lock_file))
        }
        _ => None,
    };
    let classification = classify(recorded, &boot, facts.as_ref(), &uid, &lock, holds.as_ref());
    Gathered {
        classification,
        facts: facts.and_then(Result::ok).flatten(),
        lock_file,
    }
}

/// Asks whether the recorded process holds the lock file. A lock file that is
/// not there cannot be held by anyone, so the answer is `No`.
fn holds_lock(
    platform: &Platform,
    lock_path: &Path,
    recorded: &ProcessIdentity,
    lock_file: &mut Option<FileIdentity>,
) -> Result<HoldsFile, ProcessError> {
    let info = platform
        .fs()
        .symlink_metadata(lock_path)
        .map_err(|error| ProcessError::Io {
            op: "stat_lock",
            path: Some(lock_path.to_path_buf()),
            kind: std::io::ErrorKind::Other,
            message: error.to_string(),
        })?;
    let Some(info) = info else {
        return Ok(HoldsFile::No);
    };
    *lock_file = Some(info.identity);
    platform.processes().holds_file(recorded.pid, info.identity)
}

pub(crate) fn unproven_by(recorded: Option<&ProcessIdentity>, error: &str) -> Classification {
    let base = recorded.map_or_else(IdentityEvidence::new, |identity| {
        IdentityEvidence::new().with_pid(identity.pid)
    });
    let evidence = match observation_detail(error) {
        Some(detail) => base.with_error(detail),
        None => base,
    };
    Classification::Unproven {
        blocker: UnprovenBlocker::IdentityUnproven,
        evidence,
    }
}
