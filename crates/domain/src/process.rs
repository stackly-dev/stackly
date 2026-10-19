//! Process identity (spec 0010, *Identitas majemuk*): the facts that tie a
//! running process to an instance, the observation the supervisor records
//! at every spawn and adoption, and the evidence a classification gathered.
//!
//! None of this ever authorizes a signal on its own: the supervisor reads
//! the facts again right before every signal, and evidence that does not
//! prove the identity keeps the instance `unknown`.

use std::fmt;
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::str::FromStr;

use jiff::Timestamp;
use uuid::Uuid;

use crate::code::code_enum;
use crate::error::{ValueError, ValueKind};
use crate::id::InstanceId;
use crate::text::serde_as_text;
use crate::value::ObservationDetail;

/// The largest process ID, so a [`Pid`] always fits a signed 32 bit
/// `pid_t`.
const PID_MAX: u32 = i32::MAX.unsigned_abs();
/// Microseconds in one second, the exclusive bound of
/// [`ProcessStart::micros`].
const MICROS_PER_SECOND: u32 = 1_000_000;
/// Digits of the microsecond part in the text form of a [`ProcessStart`].
const MICRO_DIGITS: usize = 6;
/// Length of the hyphenated text form of a [`BootId`].
const BOOT_ID_LEN: usize = 36;

/// A process ID: 1 to 2147483647, so it always fits the OS's signed
/// `pid_t`. A session ID and a process group ID are the ID of their
/// leader, so they are a `Pid` too.
///
/// ```
/// use stackly_domain::Pid;
///
/// let pid: Pid = "4242".parse()?;
/// assert_eq!(pid.get(), 4242);
/// assert!(Pid::new(0).is_err());
/// # Ok::<(), stackly_domain::ValueError>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Pid(NonZeroU32);

impl Pid {
    /// Wraps a process ID.
    ///
    /// # Errors
    ///
    /// Returns a [`ValueError`] for 0 or a number above 2147483647.
    pub const fn new(pid: u32) -> Result<Self, ValueError> {
        match NonZeroU32::new(pid) {
            Some(pid) if pid.get() <= PID_MAX => Ok(Self(pid)),
            _ => Err(ValueError::new(ValueKind::Pid, PID_RULE)),
        }
    }

    /// Returns the number.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0.get()
    }

    /// Returns the number as a signed 32 bit `pid_t`, which it always fits.
    #[must_use]
    pub const fn as_i32(self) -> i32 {
        // Pid::new keeps the number at or below i32::MAX, so it never wraps.
        self.0.get().cast_signed()
    }
}

const PID_RULE: &str = "must be a decimal number from 1 to 2147483647";

impl fmt::Display for Pid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl FromStr for Pid {
    type Err = ValueError;

    /// Parses the decimal form [`Display`](fmt::Display) prints: digits
    /// only, without a sign or a leading zero.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let error = ValueError::new(ValueKind::Pid, PID_RULE);
        if !is_plain_decimal(s) {
            return Err(error);
        }
        s.parse::<u32>()
            .map_err(|_| error)
            .and_then(|pid| Self::new(pid).map_err(|_| error))
    }
}

/// When a process started, to the microsecond, as the OS reports it.
///
/// With the process ID it names one process for the life of the boot,
/// because the OS never gives a PID to two processes that started in the
/// same microsecond. Its text form, stored in the `start_identity` column,
/// is `<seconds>.<six digit microseconds>` since the Unix epoch.
///
/// ```
/// use stackly_domain::ProcessStart;
///
/// let start = ProcessStart::new(1_700_000_000, 42)?;
/// assert_eq!(start.to_string(), "1700000000.000042");
/// assert_eq!("1700000000.000042".parse::<ProcessStart>()?, start);
/// # Ok::<(), stackly_domain::ValueError>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ProcessStart {
    seconds: u64,
    micros: u32,
}

impl ProcessStart {
    /// Creates a start time from whole seconds since the Unix epoch and the
    /// microseconds past them.
    ///
    /// # Errors
    ///
    /// Returns a [`ValueError`] when `micros` is 1000000 or more.
    pub const fn new(seconds: u64, micros: u32) -> Result<Self, ValueError> {
        if micros >= MICROS_PER_SECOND {
            return Err(ValueError::new(ValueKind::ProcessStart, START_RULE));
        }
        Ok(Self { seconds, micros })
    }

    /// Returns the whole seconds since the Unix epoch.
    #[must_use]
    pub const fn seconds(self) -> u64 {
        self.seconds
    }

    /// Returns the microseconds past [`seconds`](Self::seconds).
    #[must_use]
    pub const fn micros(self) -> u32 {
        self.micros
    }

    /// Returns the start as a timestamp, for `process.startedAt`, or `None`
    /// when it lies outside the range a timestamp holds.
    #[must_use]
    pub fn to_timestamp(self) -> Option<Timestamp> {
        let seconds = i64::try_from(self.seconds).ok()?;
        let nanos = i32::try_from(self.micros * 1_000).ok()?;
        Timestamp::new(seconds, nanos).ok()
    }
}

const START_RULE: &str =
    "must be <seconds>.<six digit microseconds>, with seconds in decimal and no leading zero";

impl fmt::Display for ProcessStart {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{:06}", self.seconds, self.micros)
    }
}

impl FromStr for ProcessStart {
    type Err = ValueError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let error = ValueError::new(ValueKind::ProcessStart, START_RULE);
        let (seconds, micros) = s.split_once('.').ok_or(error)?;
        let seconds_valid = seconds == "0" || is_plain_decimal(seconds);
        let micros_valid =
            micros.len() == MICRO_DIGITS && micros.bytes().all(|b| b.is_ascii_digit());
        if !seconds_valid || !micros_valid {
            return Err(error);
        }
        let seconds = seconds.parse::<u64>().map_err(|_| error)?;
        let micros = micros.parse::<u32>().map_err(|_| error)?;
        Self::new(seconds, micros)
    }
}

serde_as_text!(ProcessStart => "a process start time");

/// The ID of the current boot of the machine, from
/// `kern.bootsessionuuid` on macOS. A process from another boot is gone,
/// whatever its PID is now.
///
/// It parses the hyphenated UUID form in either case and always prints
/// lowercase, the form the `boot_id` column stores.
///
/// ```
/// use stackly_domain::BootId;
///
/// let boot: BootId = "6F9619FF-8B86-D011-B42D-00C04FC964FF".parse()?;
/// assert_eq!(boot.to_string(), "6f9619ff-8b86-d011-b42d-00c04fc964ff");
/// # Ok::<(), stackly_domain::ValueError>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BootId(Uuid);

impl fmt::Display for BootId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0.hyphenated(), f)
    }
}

impl FromStr for BootId {
    type Err = ValueError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let error = ValueError::new(
            ValueKind::BootId,
            "must be a UUID in its 36 character hyphenated form",
        );
        if s.len() != BOOT_ID_LEN {
            return Err(error);
        }
        Uuid::try_parse(s).map(Self).map_err(|_| error)
    }
}

serde_as_text!(BootId => "a boot ID");

/// The recorded identity of one process, captured while a freshly spawned
/// child is still suspended, or when the supervisor adopts a running
/// process.
///
/// The user is not stored: the daemon of that user writes every record, so
/// a check compares the process's user with the user running now.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ProcessIdentity {
    /// The process ID.
    pub pid: Pid,
    /// When the process started.
    pub start: ProcessStart,
    /// The canonical path of its executable. Runtime folders are content
    /// addressed, so the path is the identity of the runtime.
    pub executable: PathBuf,
    /// The ID of its session. A main process leads its own session, so it
    /// equals the PID.
    pub session: Pid,
    /// The boot the process runs in.
    pub boot: BootId,
}

/// One observation of an instance's process, recorded in the same batch
/// as `spawned` or `adopted` (spec 0005, *Tambahan fitur 10*).
///
/// It is never the only source of truth and never authorizes a kill: the
/// newest one only says which process to check.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ProcessObservation {
    /// The instance whose process this is.
    pub instance: InstanceId,
    /// The identity of the process.
    pub identity: ProcessIdentity,
    /// When the supervisor built the batch, from the platform clock.
    pub observed_at: Timestamp,
}

code_enum! {
    /// Whether a file lock is held, as a probe saw it. The probe never
    /// keeps the lock it tried.
    pub enum LockState: LockState {
        /// Nobody holds the lock.
        Free => "free",
        /// A process holds the lock.
        Held => "held",
    }
}

code_enum! {
    /// The result of one identity check. The wire spells `not_checked` as
    /// `notChecked`; it is never stored in the registry.
    pub enum IdentityCheck: IdentityCheck {
        /// The check ran and passed.
        Passed => "passed",
        /// The check ran and failed.
        Failed => "failed",
        /// The check did not run, because an earlier one already decided
        /// the outcome or an input was missing.
        NotChecked => "not_checked",
    }
}

impl IdentityCheck {
    /// Returns [`Passed`](Self::Passed) when `passed` is true and
    /// [`Failed`](Self::Failed) otherwise.
    #[must_use]
    pub const fn from_passed(passed: bool) -> Self {
        if passed { Self::Passed } else { Self::Failed }
    }
}

code_enum! {
    /// The seven checks that prove a process belongs to an instance, in the
    /// order evidence is gathered (spec 0010, *Identitas majemuk*).
    pub enum IdentityCheckName: IdentityCheckName {
        /// The boot ID now equals the recorded one.
        BootMatches => "boot_matches",
        /// A live process has the recorded PID.
        PidExists => "pid_exists",
        /// Its start time, seconds and microseconds, equals the recorded
        /// one.
        StartMatches => "start_matches",
        /// Its executable is the recorded canonical path.
        ExecutableMatches => "executable_matches",
        /// It runs as the user running now.
        UserMatches => "user_matches",
        /// Its session ID equals the recorded one.
        SessionMatches => "session_matches",
        /// It holds the instance lock file open.
        HoldsLock => "holds_lock",
    }
}

impl IdentityCheckName {
    const fn index(self) -> usize {
        match self {
            Self::BootMatches => 0,
            Self::PidExists => 1,
            Self::StartMatches => 2,
            Self::ExecutableMatches => 3,
            Self::UserMatches => 4,
            Self::SessionMatches => 5,
            Self::HoldsLock => 6,
        }
    }
}

/// What gathering evidence about an instance's process found: one
/// [`IdentityCheck`] for each of the seven checks, the PID, the state of
/// the instance lock, whether the process is suspended, and the error, if
/// one stopped the gathering.
///
/// It is the content of the `dataInUse` and `identityUnproven` blockers and
/// of their log line. A new value has every check
/// [`NotChecked`](IdentityCheck::NotChecked) and nothing else known.
///
/// ```
/// use stackly_domain::{IdentityCheck, IdentityCheckName, IdentityEvidence, LockState};
///
/// let evidence = IdentityEvidence::new()
///     .with_check(IdentityCheckName::BootMatches, IdentityCheck::Failed)
///     .with_lock(LockState::Held);
/// assert_eq!(evidence.check(IdentityCheckName::BootMatches), IdentityCheck::Failed);
/// assert_eq!(evidence.check(IdentityCheckName::PidExists), IdentityCheck::NotChecked);
/// assert_eq!(evidence.lock(), Some(LockState::Held));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct IdentityEvidence {
    checks: [IdentityCheck; IdentityCheckName::ALL.len()],
    pid: Option<Pid>,
    lock: Option<LockState>,
    stopped: bool,
    error: Option<ObservationDetail>,
}

impl IdentityEvidence {
    /// Creates evidence where no check ran and nothing is known.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            checks: [IdentityCheck::NotChecked; IdentityCheckName::ALL.len()],
            pid: None,
            lock: None,
            stopped: false,
            error: None,
        }
    }

    /// Returns a copy with the result of one check.
    #[must_use]
    pub const fn with_check(mut self, name: IdentityCheckName, result: IdentityCheck) -> Self {
        self.checks[name.index()] = result;
        self
    }

    /// Returns a copy with the PID of the recorded observation.
    #[must_use]
    pub const fn with_pid(mut self, pid: Pid) -> Self {
        self.pid = Some(pid);
        self
    }

    /// Returns a copy with the state of the instance lock.
    #[must_use]
    pub const fn with_lock(mut self, lock: LockState) -> Self {
        self.lock = Some(lock);
        self
    }

    /// Returns a copy that records whether the process is suspended (its
    /// daemon died after spawning it and before resuming it).
    #[must_use]
    pub const fn with_stopped(mut self, stopped: bool) -> Self {
        self.stopped = stopped;
        self
    }

    /// Returns a copy with the error that stopped the gathering. Any error
    /// leaves the identity unproven, never gone.
    #[must_use]
    pub fn with_error(mut self, error: ObservationDetail) -> Self {
        self.error = Some(error);
        self
    }

    /// Returns the result of one check.
    #[must_use]
    pub const fn check(&self, name: IdentityCheckName) -> IdentityCheck {
        self.checks[name.index()]
    }

    /// Returns every check with its result, in [`IdentityCheckName::ALL`]
    /// order.
    pub fn checks(&self) -> impl Iterator<Item = (IdentityCheckName, IdentityCheck)> + '_ {
        IdentityCheckName::ALL
            .iter()
            .map(|name| (*name, self.check(*name)))
    }

    /// Returns the PID of the recorded observation, if there was one.
    #[must_use]
    pub const fn pid(&self) -> Option<Pid> {
        self.pid
    }

    /// Returns the state of the instance lock, if the probe ran and
    /// answered.
    #[must_use]
    pub const fn lock(&self) -> Option<LockState> {
        self.lock
    }

    /// Returns true when the process was found suspended. False when its
    /// facts were not read.
    #[must_use]
    pub const fn stopped(&self) -> bool {
        self.stopped
    }

    /// Returns the error that stopped the gathering, if any.
    #[must_use]
    pub const fn error(&self) -> Option<&ObservationDetail> {
        self.error.as_ref()
    }
}

impl Default for IdentityEvidence {
    fn default() -> Self {
        Self::new()
    }
}

/// True for 1 or more ASCII digits that do not start with a zero.
fn is_plain_decimal(s: &str) -> bool {
    !s.is_empty() && !s.starts_with('0') && s.bytes().all(|b| b.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pid_is_one_to_the_largest_signed_32_bit_number() {
        assert_eq!(Pid::new(1).unwrap().get(), 1);
        assert_eq!(Pid::new(PID_MAX).unwrap().as_i32(), i32::MAX);
        for invalid in [0, PID_MAX + 1, u32::MAX] {
            assert_eq!(Pid::new(invalid).unwrap_err().kind(), ValueKind::Pid);
        }
    }

    #[test]
    fn a_pid_parses_only_its_plain_decimal_form() {
        assert_eq!("4242".parse::<Pid>().unwrap().to_string(), "4242");
        for invalid in ["", "0", "042", "+42", "-1", "4 2", "2147483648", "0x10"] {
            assert!(invalid.parse::<Pid>().is_err(), "{invalid:?}");
        }
    }

    #[test]
    fn a_process_start_prints_six_microsecond_digits_and_parses_back() {
        for (seconds, micros, text) in [
            (0, 0, "0.000000"),
            (1_700_000_000, 42, "1700000000.000042"),
            (u64::MAX, 999_999, "18446744073709551615.999999"),
        ] {
            let start = ProcessStart::new(seconds, micros).unwrap();
            assert_eq!(start.to_string(), text);
            assert_eq!(text.parse::<ProcessStart>().unwrap(), start);
        }
    }

    #[test]
    fn a_process_start_refuses_any_other_text_and_a_full_second_of_micros() {
        assert_eq!(
            ProcessStart::new(1, MICROS_PER_SECOND).unwrap_err().kind(),
            ValueKind::ProcessStart
        );
        for invalid in [
            "",
            "1700000000",
            "1700000000.42",
            "1700000000.0000420",
            "01.000000",
            "+1.000000",
            "1.00000a",
            "1.-00001",
            ".000000",
            "18446744073709551616.000000",
        ] {
            assert!(invalid.parse::<ProcessStart>().is_err(), "{invalid:?}");
        }
    }

    #[test]
    fn a_process_start_converts_to_a_timestamp_when_it_fits() {
        let start = ProcessStart::new(1_700_000_000, 250_000).unwrap();
        assert_eq!(
            start.to_timestamp().unwrap().to_string(),
            "2023-11-14T22:13:20.25Z"
        );
        assert_eq!(ProcessStart::new(u64::MAX, 0).unwrap().to_timestamp(), None);
    }

    #[test]
    fn a_boot_id_parses_either_case_and_prints_lowercase() {
        let lower = "6f9619ff-8b86-d011-b42d-00c04fc964ff";
        let boot: BootId = lower.to_uppercase().parse().unwrap();

        assert_eq!(boot.to_string(), lower);
        assert_eq!(lower.parse::<BootId>().unwrap(), boot);
        for invalid in [
            "",
            "6f9619ff8b86d011b42d00c04fc964ff",
            "{6f9619ff-8b86-d011-b42d-00c04fc964ff}",
            "6f9619ff-8b86-d011-b42d-00c04fc964fg",
        ] {
            assert_eq!(
                invalid.parse::<BootId>().unwrap_err().kind(),
                ValueKind::BootId
            );
        }
    }

    #[test]
    fn process_start_and_boot_id_travel_through_serde_as_text() {
        let start: ProcessStart = "1700000000.000001".parse().unwrap();
        let json = serde_json::to_string(&start).unwrap();

        assert_eq!(json, "\"1700000000.000001\"");
        assert_eq!(serde_json::from_str::<ProcessStart>(&json).unwrap(), start);
        assert!(serde_json::from_str::<BootId>("\"not-a-boot\"").is_err());
    }

    #[test]
    fn new_evidence_checks_nothing_and_knows_nothing() {
        let evidence = IdentityEvidence::default();

        assert!(
            evidence
                .checks()
                .all(|(_, result)| result == IdentityCheck::NotChecked)
        );
        assert_eq!(evidence.checks().count(), 7);
        assert_eq!(evidence.pid(), None);
        assert_eq!(evidence.lock(), None);
        assert!(!evidence.stopped());
        assert_eq!(evidence.error(), None);
    }

    #[test]
    fn evidence_keeps_each_check_apart_in_gathering_order() {
        let pid = Pid::new(4242).unwrap();
        let error: ObservationDetail = "proc_pidinfo failed".parse().unwrap();
        let evidence = IdentityEvidence::new()
            .with_check(IdentityCheckName::BootMatches, IdentityCheck::Passed)
            .with_check(
                IdentityCheckName::PidExists,
                IdentityCheck::from_passed(true),
            )
            .with_check(
                IdentityCheckName::StartMatches,
                IdentityCheck::from_passed(false),
            )
            .with_pid(pid)
            .with_lock(LockState::Free)
            .with_stopped(true)
            .with_error(error.clone());

        let checks: Vec<_> = evidence
            .checks()
            .map(|(name, result)| (name.as_code(), result.as_code()))
            .collect();
        assert_eq!(
            checks,
            [
                ("boot_matches", "passed"),
                ("pid_exists", "passed"),
                ("start_matches", "failed"),
                ("executable_matches", "not_checked"),
                ("user_matches", "not_checked"),
                ("session_matches", "not_checked"),
                ("holds_lock", "not_checked"),
            ]
        );
        assert_eq!(evidence.pid(), Some(pid));
        assert_eq!(evidence.lock(), Some(LockState::Free));
        assert!(evidence.stopped());
        assert_eq!(evidence.error(), Some(&error));
    }
}
