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
//!
//! [`observe_passive`] runs the same steps without `probe_lock` for the
//! doctor (spec 0034, *Bukti identitas pasif*), so a diagnosis never takes
//! the instance lock, not even for a moment; [`gather_evidence`] is that
//! path with the probe in its place.

use std::path::Path;

use stackly_domain::{
    BootId, IdentityCheck, IdentityCheckName, IdentityEvidence, LockState, ProcessIdentity,
};
use stackly_platform::{
    FileIdentity, HoldsFile, Platform, ProcessError, ProcessFacts, ProcessSupervision, UserId,
};

use super::classify::{
    Classification, UnprovenBlocker, classify, identity_checks, six_checks_passed,
};
use super::observation_detail;

/// What the platform answered, in the order [`observe`] asked. `L` is the
/// answer of the lock probe, or `()` when it did not run.
struct Observed<L> {
    uid: UserId,
    boot: Result<BootId, ProcessError>,
    lock: L,
    facts: Option<Result<Option<ProcessFacts>, ProcessError>>,
    holds: Option<Result<HoldsFile, ProcessError>>,
    lock_file: Option<FileIdentity>,
}

/// Asks the platform about `recorded` in the fixed order of this module:
/// `current_user`, `boot_id`, then `probe` (the lock probe, or nothing),
/// then `inspect` and `holds_file` when they apply. An error of
/// `current_user` ends it, as nothing after it can be judged.
fn observe<L>(
    platform: &Platform,
    lock_path: &Path,
    recorded: Option<&ProcessIdentity>,
    probe: impl FnOnce(&dyn ProcessSupervision) -> L,
) -> Result<Observed<L>, String> {
    let uid = platform
        .identity()
        .current_user()
        .map_err(|error| error.to_string())?;
    let processes = platform.processes();
    let boot = processes.boot_id();
    let lock = probe(processes);
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
    Ok(Observed {
        uid,
        boot,
        lock,
        facts,
        holds,
        lock_file,
    })
}

/// What [`observe_passive`] found: the seven checks of spec 0010 with the
/// lock state left empty, and the facts of the process that holds the
/// recorded PID now.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PassiveEvidence {
    /// The checks that ran, with `lock` always `None`.
    pub(crate) evidence: IdentityEvidence,
    /// The facts of the live process with the recorded PID, when `inspect`
    /// ran in the same boot and found one.
    pub(crate) facts: Option<ProcessFacts>,
    /// True when `inspect` was refused: a live process of another user has
    /// the PID.
    pub(crate) other_user: bool,
}

/// Runs the identity checks of spec 0010 on `recorded` without probing the
/// lock of `lock_path` (spec 0034, *Bukti identitas pasif*): `boot_id`,
/// then `inspect` and `holds_file` (which reads the file descriptors of the
/// process, not the lock) when they apply. It never calls `probe_lock` or
/// `lock_instance`, so it never holds the instance lock.
pub(crate) fn observe_passive(
    platform: &Platform,
    lock_path: &Path,
    recorded: Option<&ProcessIdentity>,
) -> PassiveEvidence {
    let base = recorded.map_or_else(IdentityEvidence::new, |identity| {
        IdentityEvidence::new().with_pid(identity.pid)
    });
    let observed = match observe(platform, lock_path, recorded, |_| ()) {
        Ok(observed) => observed,
        Err(error) => {
            return PassiveEvidence {
                evidence: with_error(base, &error),
                facts: None,
                other_user: false,
            };
        }
    };
    let Some(recorded) = recorded else {
        return PassiveEvidence {
            evidence: base,
            facts: None,
            other_user: false,
        };
    };
    let boot = match observed.boot {
        Ok(boot) => boot,
        Err(error) => {
            return PassiveEvidence {
                evidence: with_error(base, &error.to_string()),
                facts: None,
                other_user: false,
            };
        }
    };
    let found = match observed.facts {
        None => None,
        Some(Ok(found)) => Some(found),
        Some(Err(error)) => {
            let other_user = matches!(error, ProcessError::PermissionDenied { .. });
            let evidence = base
                .with_check(IdentityCheckName::BootMatches, IdentityCheck::Passed)
                .with_check(
                    IdentityCheckName::PidExists,
                    if other_user {
                        IdentityCheck::Passed
                    } else {
                        IdentityCheck::NotChecked
                    },
                );
            return PassiveEvidence {
                evidence: with_error(evidence, &error.to_string()),
                facts: None,
                other_user,
            };
        }
    };
    let facts = found.flatten();
    let evidence = identity_checks(recorded, boot, facts.as_ref(), &observed.uid);
    let evidence = match observed.holds {
        Some(Ok(HoldsFile::Yes)) => {
            evidence.with_check(IdentityCheckName::HoldsLock, IdentityCheck::Passed)
        }
        Some(Ok(HoldsFile::No | HoldsFile::Unreadable)) => {
            evidence.with_check(IdentityCheckName::HoldsLock, IdentityCheck::Failed)
        }
        Some(Err(error)) => with_error(evidence, &error.to_string()),
        None => evidence,
    };
    PassiveEvidence {
        evidence,
        facts,
        other_user: false,
    }
}

fn with_error(evidence: IdentityEvidence, error: &str) -> IdentityEvidence {
    match observation_detail(error) {
        Some(detail) => evidence.with_error(detail),
        None => evidence,
    }
}

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
    let observed = match observe(platform, lock_path, recorded, |processes| {
        processes.probe_lock(lock_path)
    }) {
        Ok(observed) => observed,
        Err(error) => {
            return Gathered {
                classification: unproven_by(recorded, &error),
                facts: None,
                lock_file: None,
            };
        }
    };
    let lock: &Result<LockState, ProcessError> = &observed.lock;
    let classification = classify(
        recorded,
        &observed.boot,
        observed.facts.as_ref(),
        &observed.uid,
        lock,
        observed.holds.as_ref(),
    );
    Gathered {
        classification,
        facts: observed.facts.and_then(Result::ok).flatten(),
        lock_file: observed.lock_file,
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
