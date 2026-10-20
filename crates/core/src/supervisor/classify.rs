//! Whether a recorded process still belongs to its instance (spec 0010,
//! *Identitas majemuk*): the six identity checks and the classification
//! table, as pure functions over evidence the caller gathered.
//!
//! Nothing here reads the OS. Reconciliation, the `unknown` recheck, and
//! the start guard gather the evidence in the order the spec fixes, then
//! call [`identity_checks`] or [`classify`]. An error in any input, or an
//! input the outcome needs that was not gathered, gives
//! [`Classification::Unproven`], never [`Classification::Gone`], and
//! nothing on the unproven path ever leads to a signal.

use stackly_domain::{
    BootId, IdentityCheck, IdentityCheckName, IdentityEvidence, LockState, ProcessIdentity,
};
use stackly_platform::{HoldsFile, ProcessError, ProcessFacts, UserId};

use super::observation_detail;

/// The six checks that need no lock file, in gathering order.
const SIX_CHECKS: [IdentityCheckName; 6] = [
    IdentityCheckName::BootMatches,
    IdentityCheckName::PidExists,
    IdentityCheckName::StartMatches,
    IdentityCheckName::ExecutableMatches,
    IdentityCheckName::UserMatches,
    IdentityCheckName::SessionMatches,
];

/// Why a recorded process counts as gone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GoneReason {
    /// The instance has no recorded process.
    NoRecord,
    /// The record is from an earlier boot.
    BootChanged,
    /// No live process has the recorded PID.
    Exited,
    /// A live process has the PID, but its start time, executable, user, or
    /// session differs.
    PidReused,
}

impl GoneReason {
    /// Returns the stable snake_case code, for logs.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::NoRecord => "no_record",
            Self::BootChanged => "boot_changed",
            Self::Exited => "exited",
            Self::PidReused => "pid_reused",
        }
    }
}

/// The blocker an unproven classification puts on the instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnprovenBlocker {
    /// The instance lock is held by a process that is not proven to be the
    /// instance's (`dataInUse`).
    DataInUse,
    /// The identity could not be proven: the process matches but does not
    /// hold the lock, or gathering the evidence failed (`identityUnproven`).
    IdentityUnproven,
}

impl UnprovenBlocker {
    /// Returns the stable snake_case code, for logs.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::DataInUse => "data_in_use",
            Self::IdentityUnproven => "identity_unproven",
        }
    }
}

/// The result of [`classify`], with the evidence behind it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Classification {
    /// All seven checks passed: the recorded process is the instance's and
    /// holds its lock. It may be adopted, and resumed first when the
    /// evidence says it is suspended.
    Proven {
        /// The recorded identity.
        identity: ProcessIdentity,
        /// The evidence, with every check passed.
        evidence: IdentityEvidence,
    },
    /// The recorded process no longer runs, and nobody holds the lock.
    Gone {
        /// Why.
        reason: GoneReason,
        /// The checks that ran.
        evidence: IdentityEvidence,
    },
    /// Neither proven nor gone. The instance gets the blocker, and its
    /// process is never signalled, resumed, or discarded.
    Unproven {
        /// The blocker for the instance.
        blocker: UnprovenBlocker,
        /// The checks that ran, the lock state, and the error, if any.
        evidence: IdentityEvidence,
    },
}

impl Classification {
    /// Returns the evidence behind the result.
    #[must_use]
    pub const fn evidence(&self) -> &IdentityEvidence {
        match self {
            Self::Proven { evidence, .. }
            | Self::Gone { evidence, .. }
            | Self::Unproven { evidence, .. } => evidence,
        }
    }
}

/// Runs the six checks that need no lock file against the facts of the
/// recorded PID (spec 0010, `identity_checks`).
///
/// `facts` is what `inspect(recorded.pid)` answered: `None` when no live
/// process has the PID. When the boot differs the facts are ignored and
/// every later check stays [`IdentityCheck::NotChecked`]; when the PID
/// does not exist, the four fact checks stay `NotChecked`. The result
/// carries the PID, and whether the process is suspended when facts were
/// read. [`IdentityCheckName::HoldsLock`] is never run here.
#[must_use]
pub fn identity_checks(
    recorded: &ProcessIdentity,
    boot: BootId,
    facts: Option<&ProcessFacts>,
    uid: &UserId,
) -> IdentityEvidence {
    let evidence = IdentityEvidence::new().with_pid(recorded.pid);
    if boot != recorded.boot {
        return evidence.with_check(IdentityCheckName::BootMatches, IdentityCheck::Failed);
    }
    let evidence = evidence.with_check(IdentityCheckName::BootMatches, IdentityCheck::Passed);
    let Some(facts) = facts else {
        return evidence.with_check(IdentityCheckName::PidExists, IdentityCheck::Failed);
    };
    let check = IdentityCheck::from_passed;
    evidence
        .with_check(IdentityCheckName::PidExists, IdentityCheck::Passed)
        .with_check(
            IdentityCheckName::StartMatches,
            check(facts.start == recorded.start),
        )
        .with_check(
            IdentityCheckName::ExecutableMatches,
            check(facts.executable == recorded.executable),
        )
        .with_check(IdentityCheckName::UserMatches, check(facts.uid == *uid))
        .with_check(
            IdentityCheckName::SessionMatches,
            check(facts.session == recorded.session),
        )
        .with_stopped(facts.stopped)
}

/// Returns true when the six checks of [`identity_checks`] all passed.
#[must_use]
pub fn six_checks_passed(evidence: &IdentityEvidence) -> bool {
    SIX_CHECKS
        .iter()
        .all(|name| evidence.check(*name) == IdentityCheck::Passed)
}

/// Classifies a recorded process with the table of spec 0010
/// (*Identitas majemuk*).
///
/// The inputs are the answers of the platform calls, each with its error:
/// `boot` from `boot_id`, `facts` from `inspect` and `holds` from
/// `holds_file` (`None` when not gathered), `lock` from `probe_lock`, and
/// `uid` from `current_user`.
///
/// | Condition | Result |
/// | --- | --- |
/// | an input is an error | `Unproven`, `IdentityUnproven` |
/// | no record, or another boot, or the PID gone or reused; lock free | `Gone` |
/// | the same, lock held | `Unproven`, `DataInUse` |
/// | the six checks passed and `holds` is `Yes` | `Proven` |
/// | the six checks passed and `holds` is `No` or `Unreadable` | `Unproven`, `IdentityUnproven` |
/// | an input the outcome needs was not gathered | `Unproven`, `IdentityUnproven` |
#[must_use]
pub fn classify(
    recorded: Option<&ProcessIdentity>,
    boot: &Result<BootId, ProcessError>,
    facts: Option<&Result<Option<ProcessFacts>, ProcessError>>,
    uid: &UserId,
    lock: &Result<LockState, ProcessError>,
    holds: Option<&Result<HoldsFile, ProcessError>>,
) -> Classification {
    let base = recorded.map_or_else(IdentityEvidence::new, |identity| {
        IdentityEvidence::new().with_pid(identity.pid)
    });
    let base = match lock {
        Ok(state) => base.with_lock(*state),
        Err(_) => base,
    };
    let errors = [
        boot.as_ref().err(),
        lock.as_ref().err(),
        facts.and_then(|facts| facts.as_ref().err()),
        holds.and_then(|holds| holds.as_ref().err()),
    ];
    if let Some(error) = errors.into_iter().flatten().next() {
        return unproven_by_error(base, error);
    }
    // The errors are ruled out above.
    let (Ok(boot), Ok(lock)) = (boot, lock) else {
        return unproven(base);
    };
    let Some(recorded) = recorded else {
        return gone_or_held(GoneReason::NoRecord, base, *lock);
    };
    if *boot != recorded.boot {
        let evidence = identity_checks(recorded, *boot, None, uid).with_lock(*lock);
        return gone_or_held(GoneReason::BootChanged, evidence, *lock);
    }
    let Some(Ok(facts)) = facts else {
        let evidence = IdentityEvidence::new()
            .with_pid(recorded.pid)
            .with_check(IdentityCheckName::BootMatches, IdentityCheck::Passed);
        return unproven(evidence.with_lock(*lock));
    };
    let evidence = identity_checks(recorded, *boot, facts.as_ref(), uid).with_lock(*lock);
    if facts.is_none() {
        return gone_or_held(GoneReason::Exited, evidence, *lock);
    }
    if !six_checks_passed(&evidence) {
        return gone_or_held(GoneReason::PidReused, evidence, *lock);
    }
    match holds {
        Some(Ok(HoldsFile::Yes)) => Classification::Proven {
            identity: recorded.clone(),
            evidence: evidence.with_check(IdentityCheckName::HoldsLock, IdentityCheck::Passed),
        },
        Some(Ok(HoldsFile::No | HoldsFile::Unreadable)) => {
            unproven(evidence.with_check(IdentityCheckName::HoldsLock, IdentityCheck::Failed))
        }
        // Not gathered (an error is ruled out above): never proven.
        Some(Err(_)) | None => unproven(evidence),
    }
}

fn gone_or_held(reason: GoneReason, evidence: IdentityEvidence, lock: LockState) -> Classification {
    match lock {
        LockState::Free => Classification::Gone { reason, evidence },
        LockState::Held => Classification::Unproven {
            blocker: UnprovenBlocker::DataInUse,
            evidence,
        },
    }
}

const fn unproven(evidence: IdentityEvidence) -> Classification {
    Classification::Unproven {
        blocker: UnprovenBlocker::IdentityUnproven,
        evidence,
    }
}

fn unproven_by_error(evidence: IdentityEvidence, error: &ProcessError) -> Classification {
    let evidence = match observation_detail(&error.to_string()) {
        Some(detail) => evidence.with_error(detail),
        None => evidence,
    };
    unproven(evidence)
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::path::PathBuf;

    use stackly_domain::{Pid, ProcessStart};

    use super::*;

    /// Changes one fact of a matching process.
    type FactChange = fn(&mut ProcessFacts);

    const ME: UserId = UserId::Unix(501);
    const BOOT: &str = "6f2a2c66-0d6b-4d62-9b0e-6a1f7c7d2a10";
    const OTHER_BOOT: &str = "0b1c2d3e-4f50-4617-8829-3a4b5c6d7e8f";

    fn boot() -> BootId {
        BOOT.parse().unwrap()
    }

    fn recorded() -> ProcessIdentity {
        ProcessIdentity {
            pid: Pid::new(4242).unwrap(),
            start: ProcessStart::new(1_760_000_000, 123_456).unwrap(),
            executable: PathBuf::from("/runtimes/fake/bin/stackly-fake-server"),
            session: Pid::new(4242).unwrap(),
            boot: boot(),
        }
    }

    fn matching_facts() -> ProcessFacts {
        let recorded = recorded();
        ProcessFacts {
            pid: recorded.pid,
            parent: Pid::new(1).ok(),
            group: recorded.pid,
            session: recorded.session,
            uid: ME,
            start: recorded.start,
            executable: recorded.executable,
            stopped: false,
        }
    }

    fn io_error(op: &'static str) -> ProcessError {
        ProcessError::io(op, None, &io::Error::other("injected"))
    }

    /// Classifies with every input answered as given.
    fn run(
        recorded: Option<&ProcessIdentity>,
        facts: Option<Result<Option<ProcessFacts>, ProcessError>>,
        lock: LockState,
        holds: Option<Result<HoldsFile, ProcessError>>,
    ) -> Classification {
        classify(
            recorded,
            &Ok(boot()),
            facts.as_ref(),
            &ME,
            &Ok(lock),
            holds.as_ref(),
        )
    }

    fn assert_gone(result: &Classification, expected: GoneReason) {
        assert!(
            matches!(result, Classification::Gone { reason, .. } if *reason == expected),
            "expected gone {expected:?}, got {result:?}"
        );
    }

    fn assert_unproven(result: &Classification, expected: UnprovenBlocker) {
        assert!(
            matches!(result, Classification::Unproven { blocker, .. } if *blocker == expected),
            "expected unproven {expected:?}, got {result:?}"
        );
    }

    #[test]
    fn no_record_with_a_free_lock_is_gone() {
        let result = run(None, None, LockState::Free, None);

        assert_gone(&result, GoneReason::NoRecord);
        assert_eq!(result.evidence().lock(), Some(LockState::Free));
        assert_eq!(result.evidence().pid(), None);
    }

    #[test]
    fn no_record_with_a_held_lock_is_data_in_use() {
        let result = run(None, None, LockState::Held, None);

        assert_unproven(&result, UnprovenBlocker::DataInUse);
        assert_eq!(result.evidence().lock(), Some(LockState::Held));
    }

    #[test]
    fn another_boot_with_a_free_lock_is_gone_and_checks_nothing_else() {
        let recorded = recorded();
        let result = classify(
            Some(&recorded),
            &Ok(OTHER_BOOT.parse().unwrap()),
            None,
            &ME,
            &Ok(LockState::Free),
            None,
        );

        assert_gone(&result, GoneReason::BootChanged);
        let evidence = result.evidence();
        assert_eq!(
            evidence.check(IdentityCheckName::BootMatches),
            IdentityCheck::Failed
        );
        for name in &IdentityCheckName::ALL[1..] {
            assert_eq!(evidence.check(*name), IdentityCheck::NotChecked, "{name}");
        }
        assert_eq!(evidence.pid(), Some(recorded.pid));
    }

    #[test]
    fn another_boot_with_a_held_lock_is_data_in_use() {
        let recorded = recorded();
        let result = classify(
            Some(&recorded),
            &Ok(OTHER_BOOT.parse().unwrap()),
            None,
            &ME,
            &Ok(LockState::Held),
            None,
        );

        assert_unproven(&result, UnprovenBlocker::DataInUse);
    }

    #[test]
    fn a_missing_pid_with_a_free_lock_is_gone_exited() {
        let recorded = recorded();
        let result = run(Some(&recorded), Some(Ok(None)), LockState::Free, None);

        assert_gone(&result, GoneReason::Exited);
        let evidence = result.evidence();
        assert_eq!(
            evidence.check(IdentityCheckName::PidExists),
            IdentityCheck::Failed
        );
        assert_eq!(
            evidence.check(IdentityCheckName::StartMatches),
            IdentityCheck::NotChecked
        );
    }

    #[test]
    fn a_missing_pid_with_a_held_lock_is_data_in_use() {
        let recorded = recorded();
        let result = run(Some(&recorded), Some(Ok(None)), LockState::Held, None);

        assert_unproven(&result, UnprovenBlocker::DataInUse);
    }

    #[test]
    fn each_differing_fact_with_a_free_lock_is_gone_pid_reused() {
        let recorded = recorded();
        let changes: [(IdentityCheckName, FactChange); 4] = [
            (IdentityCheckName::StartMatches, |facts| {
                facts.start = ProcessStart::new(1_760_000_000, 123_457).unwrap();
            }),
            (IdentityCheckName::ExecutableMatches, |facts| {
                facts.executable = PathBuf::from("/usr/bin/sleep");
            }),
            (IdentityCheckName::UserMatches, |facts| {
                facts.uid = UserId::Unix(0);
            }),
            (IdentityCheckName::SessionMatches, |facts| {
                facts.session = Pid::new(1).unwrap();
            }),
        ];

        for (failed, change) in changes {
            let mut facts = matching_facts();
            change(&mut facts);
            let free = run(
                Some(&recorded),
                Some(Ok(Some(facts.clone()))),
                LockState::Free,
                None,
            );
            let held = run(
                Some(&recorded),
                Some(Ok(Some(facts))),
                LockState::Held,
                None,
            );

            assert_gone(&free, GoneReason::PidReused);
            assert_unproven(&held, UnprovenBlocker::DataInUse);
            for result in [&free, &held] {
                let evidence = result.evidence();
                assert_eq!(evidence.check(failed), IdentityCheck::Failed, "{failed}");
                assert_eq!(
                    evidence.check(IdentityCheckName::HoldsLock),
                    IdentityCheck::NotChecked,
                    "{failed}"
                );
            }
        }
    }

    #[test]
    fn every_check_passing_with_the_lock_held_by_the_process_is_proven() {
        let recorded = recorded();
        let result = run(
            Some(&recorded),
            Some(Ok(Some(matching_facts()))),
            LockState::Held,
            Some(Ok(HoldsFile::Yes)),
        );

        let Classification::Proven { identity, evidence } = result else {
            panic!("expected proven, got {result:?}");
        };
        assert_eq!(identity, recorded);
        for (name, check) in evidence.checks() {
            assert_eq!(check, IdentityCheck::Passed, "{name}");
        }
        assert!(!evidence.stopped());
    }

    #[test]
    fn a_proven_suspended_process_says_so_in_its_evidence() {
        let recorded = recorded();
        let mut facts = matching_facts();
        facts.stopped = true;

        let result = run(
            Some(&recorded),
            Some(Ok(Some(facts))),
            LockState::Held,
            Some(Ok(HoldsFile::Yes)),
        );

        assert!(matches!(result, Classification::Proven { .. }));
        assert!(result.evidence().stopped());
    }

    #[test]
    fn a_matching_process_that_does_not_hold_the_lock_is_identity_unproven() {
        let recorded = recorded();
        for answer in [HoldsFile::No, HoldsFile::Unreadable] {
            for lock in [LockState::Free, LockState::Held] {
                let result = run(
                    Some(&recorded),
                    Some(Ok(Some(matching_facts()))),
                    lock,
                    Some(Ok(answer)),
                );

                assert_unproven(&result, UnprovenBlocker::IdentityUnproven);
                assert_eq!(
                    result.evidence().check(IdentityCheckName::HoldsLock),
                    IdentityCheck::Failed
                );
            }
        }
    }

    #[test]
    fn a_matching_process_without_gathered_holds_is_never_proven() {
        let recorded = recorded();
        let result = run(
            Some(&recorded),
            Some(Ok(Some(matching_facts()))),
            LockState::Held,
            None,
        );

        assert_unproven(&result, UnprovenBlocker::IdentityUnproven);
        assert_eq!(
            result.evidence().check(IdentityCheckName::HoldsLock),
            IdentityCheck::NotChecked
        );
    }

    #[test]
    fn facts_not_gathered_in_the_same_boot_are_unproven_never_gone() {
        let recorded = recorded();
        let result = run(Some(&recorded), None, LockState::Free, None);

        assert_unproven(&result, UnprovenBlocker::IdentityUnproven);
    }

    #[test]
    fn an_error_in_any_input_is_unproven_never_gone() {
        let recorded = recorded();
        let free = Ok(LockState::Free);
        let cases: [(&str, Classification); 5] = [
            (
                "boot_id",
                classify(
                    Some(&recorded),
                    &Err(io_error("boot_id")),
                    None,
                    &ME,
                    &free,
                    None,
                ),
            ),
            (
                "probe_lock",
                classify(
                    Some(&recorded),
                    &Ok(boot()),
                    Some(&Ok(None)),
                    &ME,
                    &Err(io_error("probe_lock")),
                    None,
                ),
            ),
            (
                "inspect",
                classify(
                    Some(&recorded),
                    &Ok(boot()),
                    Some(&Err(io_error("inspect"))),
                    &ME,
                    &free,
                    None,
                ),
            ),
            (
                "holds_file",
                classify(
                    Some(&recorded),
                    &Ok(boot()),
                    Some(&Ok(Some(matching_facts()))),
                    &ME,
                    &Ok(LockState::Held),
                    Some(&Err(io_error("holds_file"))),
                ),
            ),
            (
                "probe_lock",
                classify(
                    None,
                    &Ok(boot()),
                    None,
                    &ME,
                    &Err(io_error("probe_lock")),
                    None,
                ),
            ),
        ];

        for (op, result) in cases {
            assert_unproven(&result, UnprovenBlocker::IdentityUnproven);
            let error = result
                .evidence()
                .error()
                .expect("the error is in the evidence");
            assert!(error.as_str().starts_with(op), "{op}: {error}");
        }
    }

    #[test]
    fn a_permission_error_from_inspect_is_unproven_not_gone() {
        let recorded = recorded();
        let denied = ProcessError::PermissionDenied { pid: recorded.pid };

        let result = classify(
            Some(&recorded),
            &Ok(boot()),
            Some(&Err(denied)),
            &ME,
            &Ok(LockState::Free),
            None,
        );

        assert_unproven(&result, UnprovenBlocker::IdentityUnproven);
    }

    #[test]
    fn identity_checks_ignore_the_facts_of_another_boot() {
        let recorded = recorded();

        let evidence = identity_checks(
            &recorded,
            OTHER_BOOT.parse().unwrap(),
            Some(&matching_facts()),
            &ME,
        );

        assert_eq!(
            evidence.check(IdentityCheckName::BootMatches),
            IdentityCheck::Failed
        );
        assert_eq!(
            evidence.check(IdentityCheckName::PidExists),
            IdentityCheck::NotChecked
        );
        assert!(!six_checks_passed(&evidence));
    }

    #[test]
    fn identity_checks_pass_all_six_for_matching_facts() {
        let evidence = identity_checks(&recorded(), boot(), Some(&matching_facts()), &ME);

        assert!(six_checks_passed(&evidence));
        assert_eq!(
            evidence.check(IdentityCheckName::HoldsLock),
            IdentityCheck::NotChecked
        );
    }

    #[test]
    fn gone_reasons_and_blockers_have_stable_codes() {
        let gone: Vec<_> = [
            GoneReason::NoRecord,
            GoneReason::BootChanged,
            GoneReason::Exited,
            GoneReason::PidReused,
        ]
        .map(GoneReason::as_code)
        .into();
        let blockers = [
            UnprovenBlocker::DataInUse,
            UnprovenBlocker::IdentityUnproven,
        ]
        .map(UnprovenBlocker::as_code);

        assert_eq!(gone, ["no_record", "boot_changed", "exited", "pid_reused"]);
        assert_eq!(blockers, ["data_in_use", "identity_unproven"]);
    }
}
