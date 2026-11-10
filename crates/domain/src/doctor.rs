//! The doctor (spec 0034): the read only checks the daemon runs on demand,
//! the codes of what they find, how a check's severity follows from its
//! findings, and the limits of one report.
//!
//! The domain holds the closed vocabulary and the rules. The checks
//! themselves live in `stackly-core`, the wire form in `stackly-protocol`,
//! and the English text in the daemon.

use std::cmp::Ordering;
use std::time::Duration;

/// The most findings one check carries; the rest are counted in `omitted`.
pub const FINDINGS_MAX: usize = 100;

/// The most details one finding carries.
pub const DETAILS_MAX: usize = 16;

/// The longest detail key, in bytes.
pub const DETAIL_KEY_BYTES_MAX: usize = 32;

/// The longest detail value, in bytes.
pub const DETAIL_VALUE_BYTES_MAX: usize = 512;

/// The most excerpt lines one finding carries.
pub const EXCERPT_LINES_MAX: usize = 10;

/// The longest excerpt line, in characters.
pub const EXCERPT_LINE_CHARS_MAX: usize = 256;

/// The longest finding title, in characters.
pub const TITLE_CHARS_MAX: usize = 120;

/// The longest finding explanation, in characters.
pub const EXPLANATION_CHARS_MAX: usize = 600;

/// The longest check summary, in characters. A summary is never empty.
pub const SUMMARY_CHARS_MAX: usize = 200;

/// How long one check may run before it reports `check_unavailable` with
/// reason `timeout`.
pub const CHECK_TIME_LIMIT: Duration = Duration::from_secs(5);

/// How long one run may take in all; checks still running then report
/// `timeout`.
pub const RUN_TIME_LIMIT: Duration = Duration::from_secs(15);

/// How many runs may go on at once, bundle previews included.
pub const RUNS_AT_ONCE: usize = 2;

/// How long a run waits for a free slot before it starts anyway.
pub const RUN_SLOT_WAIT: Duration = Duration::from_secs(15);

/// A folder in `<data>/operations/` younger than this is never reported as
/// left over, so an operation that is being created is not counted.
pub const SCRATCH_MIN_AGE: Duration = Duration::from_secs(10 * 60);

/// The most entries the `permissions` check reads in one instance folder.
pub const PERMISSION_ENTRIES_MAX: usize = 5000;

/// The most `permissions` findings for one instance; the rest are counted
/// in `omitted`.
pub const PERMISSION_FINDINGS_PER_INSTANCE: usize = 10;

/// How deep the `permissions` check reads inside `data/`.
pub const PERMISSION_DEPTH_MAX: usize = 2;

/// How serious a finding, or a check, is.
///
/// The order is from the least to the most serious, so the heaviest of a
/// set is its maximum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DoctorSeverity {
    /// Nothing is wrong.
    Ok,
    /// Something may need your attention.
    Warning,
    /// Something is wrong and keeps Stackly from doing its work.
    Error,
}

impl DoctorSeverity {
    /// The wire value, such as `warning`.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }
}

/// The severity of a check: the heaviest of its findings, and
/// [`DoctorSeverity::Ok`] without findings.
#[must_use]
pub fn check_severity(findings: impl IntoIterator<Item = DoctorSeverity>) -> DoctorSeverity {
    findings.into_iter().max().unwrap_or(DoctorSeverity::Ok)
}

macro_rules! doctor_codes {
    (
        $(#[$meta:meta])*
        pub enum $name:ident {
            $($(#[$variant_meta:meta])* $variant:ident = $code:literal,)*
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum $name {
            $($(#[$variant_meta])* $variant,)*
        }

        impl $name {
            /// Every value, in declaration order.
            pub const ALL: &'static [Self] = &[$(Self::$variant),*];

            /// The wire value, in snake case.
            #[must_use]
            pub const fn code(self) -> &'static str {
                match self {
                    $(Self::$variant => $code,)*
                }
            }

            /// The value with this wire text, if there is one.
            #[must_use]
            pub fn from_code(code: &str) -> Option<Self> {
                Self::ALL.iter().copied().find(|value| value.code() == code)
            }
        }
    };
}

doctor_codes! {
    /// One check of the doctor, in the order of the *Daftar check* table of
    /// spec 0034. The first three run in the client, the rest in the daemon.
    pub enum DoctorCheckId {
        /// Whether the client reaches the daemon (client).
        Connection = "connection",
        /// Whether client and daemon speak the same protocol (client).
        Protocol = "protocol",
        /// The state of the background agent (client).
        BackgroundAgent = "background_agent",
        /// The daemon's own subsystems.
        Daemon = "daemon",
        /// The registry file: recovery mode, `quick_check`, unknown codes.
        Registry = "registry",
        /// The roots and the instance folders.
        Permissions = "permissions",
        /// The volume that holds the data root.
        DiskSpace = "disk_space",
        /// The managed runtimes.
        Runtimes = "runtimes",
        /// The pinned provider packages instances use.
        ProviderPackages = "provider_packages",
        /// Ports of instances that are not running.
        Endpoints = "endpoints",
        /// Recorded processes and their identity.
        Processes = "processes",
        /// The data folder and runtime of each instance.
        InstanceFiles = "instance_files",
        /// Instances that failed or are not ready.
        InstanceFailures = "instance_failures",
        /// Rendered config that differs from its revision.
        ConfigDrift = "config_drift",
        /// Secrets an instance needs and cannot find.
        Secrets = "secrets",
        /// Operations that need recovery and scratch folders left behind.
        Operations = "operations",
    }
}

/// The checks the daemon runs, in the order it reports them.
pub const DAEMON_CHECKS: [DoctorCheckId; 13] = [
    DoctorCheckId::Daemon,
    DoctorCheckId::Registry,
    DoctorCheckId::Permissions,
    DoctorCheckId::DiskSpace,
    DoctorCheckId::Runtimes,
    DoctorCheckId::ProviderPackages,
    DoctorCheckId::Endpoints,
    DoctorCheckId::Processes,
    DoctorCheckId::InstanceFiles,
    DoctorCheckId::InstanceFailures,
    DoctorCheckId::ConfigDrift,
    DoctorCheckId::Secrets,
    DoctorCheckId::Operations,
];

impl DoctorCheckId {
    /// Whether the daemon runs this check; the other three run in the
    /// client and a `doctor.run` that names one is refused.
    #[must_use]
    pub const fn runs_in_daemon(self) -> bool {
        !matches!(
            self,
            Self::Connection | Self::Protocol | Self::BackgroundAgent
        )
    }

    /// Whether this check reads the registry. In recovery mode such a check
    /// reports `check_unavailable` with reason `recovery_mode`, except
    /// `registry` itself and `permissions`, which then checks only the
    /// roots.
    #[must_use]
    pub const fn needs_registry(self) -> bool {
        !matches!(
            self,
            Self::Connection
                | Self::Protocol
                | Self::BackgroundAgent
                | Self::Daemon
                | Self::DiskSpace
        )
    }

    /// Whether this check runs at all in recovery mode, where it must not
    /// touch the registry.
    #[must_use]
    pub const fn runs_in_recovery(self) -> bool {
        matches!(
            self,
            Self::Daemon | Self::Registry | Self::Permissions | Self::DiskSpace
        )
    }

    /// The position of this check among [`DAEMON_CHECKS`], or `None` for a
    /// client check.
    #[must_use]
    pub fn daemon_index(self) -> Option<usize> {
        DAEMON_CHECKS.iter().position(|check| *check == self)
    }
}

doctor_codes! {
    /// What a finding is about, by check (spec 0034, *Daftar check*).
    pub enum DoctorFindingCode {
        /// `connection`: the client cannot reach the daemon; the `code`
        /// detail holds the `ipc.*` error.
        DaemonUnreachable = "daemon_unreachable",
        /// `protocol`: the daemon speaks another protocol major.
        ProtocolIncompatible = "protocol_incompatible",
        /// `background_agent`: the agent is in a state of spec 0015 that
        /// needs you; the `state` detail names it.
        AgentState = "agent_state",
        /// `background_agent`: the daemon wrote a start failure record.
        DaemonStartFailed = "daemon_start_failed",
        /// `background_agent`: the agent's daemon binary may have been
        /// replaced since launchd last started it.
        BinaryReplaced = "binary_replaced",
        /// `daemon`: one subsystem of the daemon is degraded.
        SubsystemDegraded = "subsystem_degraded",
        /// `registry`: the registry is open in recovery mode.
        RecoveryMode = "recovery_mode",
        /// `registry`: `PRAGMA quick_check` failed.
        QuickCheckFailed = "quick_check_failed",
        /// `registry`: a row holds a code this build does not know.
        UnrecognizedValue = "unrecognized_value",
        /// `permissions`: a root is not private to you.
        RootNotPrivate = "root_not_private",
        /// `permissions`: a folder of an instance is not writable by you.
        PathNotWritable = "path_not_writable",
        /// `permissions`: an entry of an instance belongs to another user.
        PathForeignOwner = "path_foreign_owner",
        /// `disk_space`: the data volume is filling.
        VolumeFilling = "volume_filling",
        /// `disk_space`: the data volume is low on space.
        VolumeLow = "volume_low",
        /// `disk_space`: the data volume is almost full.
        VolumeCritical = "volume_critical",
        /// `runtimes`: a runtime is `missing`, `drifted`, or `failed`.
        RuntimeState = "runtime_state",
        /// `runtimes`: an installed runtime misses a file.
        RuntimeFilesMissing = "runtime_files_missing",
        /// `provider_packages`: a pinned package does not load.
        PackageUnreadable = "package_unreadable",
        /// `endpoints`: a port of an instance that is not running is in use.
        PortInUse = "port_in_use",
        /// `processes`: a recorded PID now belongs to another process.
        PidReused = "pid_reused",
        /// `processes`: the data folder is in use by a process Stackly did
        /// not start.
        DataInUse = "data_in_use",
        /// `processes`: the recorded process can't be proven to be the
        /// instance's.
        IdentityUnproven = "identity_unproven",
        /// `processes`: the recorded process is suspended.
        ProcessSuspended = "process_suspended",
        /// `instance_files`: the data folder of an instance is gone.
        DataMissing = "data_missing",
        /// `instance_files`: the runtime of an instance is gone.
        RuntimeMissing = "runtime_missing",
        /// `instance_failures`: an instance failed.
        StartFailed = "start_failed",
        /// `instance_failures`: a running instance is not ready.
        NotReady = "not_ready",
        /// `config_drift`: a rendered file differs from its revision.
        ConfigDrift = "config_drift",
        /// `secrets`: a secret an instance needs is missing.
        SecretMissing = "secret_missing",
        /// `operations`: an operation needs recovery.
        NeedsRecovery = "needs_recovery",
        /// `operations`: a scratch folder has no operation.
        ScratchLeftover = "scratch_leftover",
        /// Any check: the check could not run; the `reason` detail says why.
        CheckUnavailable = "check_unavailable",
    }
}

doctor_codes! {
    /// Why a check reports `check_unavailable`.
    pub enum CheckUnavailableReason {
        /// The check ran past [`CHECK_TIME_LIMIT`] or [`RUN_TIME_LIMIT`].
        Timeout = "timeout",
        /// The registry is open in recovery mode and the check needs it.
        RecoveryMode = "recovery_mode",
        /// This Mac or this build cannot run the check.
        NotSupported = "not_supported",
        /// Reading what the check needs failed.
        Io = "io",
    }
}

/// The parts of a finding that order it within its check: severity (the
/// most serious first), then findings without an instance, then instance
/// name, then code, then subject (an endpoint, a path, or an operation ID).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FindingOrder<'a> {
    /// The severity of the finding.
    pub severity: DoctorSeverity,
    /// The name of the instance it is about, if any.
    pub instance: Option<&'a str>,
    /// Its code.
    pub code: DoctorFindingCode,
    /// What it is about within the instance, if anything.
    pub subject: Option<&'a str>,
}

impl Ord for FindingOrder<'_> {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .severity
            .cmp(&self.severity)
            .then_with(|| match (self.instance, other.instance) {
                (None, None) => Ordering::Equal,
                (None, Some(_)) => Ordering::Less,
                (Some(_), None) => Ordering::Greater,
                (Some(left), Some(right)) => left.cmp(right),
            })
            .then_with(|| self.code.code().cmp(other.code.code()))
            .then_with(|| self.subject.cmp(&other.subject))
    }
}

impl PartialOrd for FindingOrder<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// How many findings of each severity a set holds, the omitted ones
/// included.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SeverityTally {
    /// Findings with [`DoctorSeverity::Error`].
    pub errors: u64,
    /// Findings with [`DoctorSeverity::Warning`].
    pub warnings: u64,
}

impl SeverityTally {
    /// Counts one finding.
    pub fn add(&mut self, severity: DoctorSeverity) {
        match severity {
            DoctorSeverity::Error => self.errors = self.errors.saturating_add(1),
            DoctorSeverity::Warning => self.warnings = self.warnings.saturating_add(1),
            DoctorSeverity::Ok => {}
        }
    }

    /// The sum of two tallies.
    #[must_use]
    pub const fn plus(self, other: Self) -> Self {
        Self {
            errors: self.errors.saturating_add(other.errors),
            warnings: self.warnings.saturating_add(other.warnings),
        }
    }
}

/// The findings of one check after the limit: the first [`FINDINGS_MAX`]
/// in order, how many were left out, and the tally of all of them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CappedFindings<T> {
    /// The findings that are kept, in order.
    pub kept: Vec<T>,
    /// How many findings were left out, beyond the ones counted before.
    pub omitted: u64,
    /// The severity of the check: the heaviest finding, kept or not.
    pub severity: DoctorSeverity,
    /// The findings of each severity, kept and left out.
    pub tally: SeverityTally,
}

/// Orders `findings` by [`FindingOrder`] and keeps the first
/// [`FINDINGS_MAX`]. `already_omitted` holds findings a check left out
/// before (for example past [`PERMISSION_FINDINGS_PER_INSTANCE`]) with
/// their severity, so the count and the tally include them.
pub fn cap_findings<T>(
    mut findings: Vec<T>,
    already_omitted: &[DoctorSeverity],
    order: impl Fn(&T) -> FindingOrder<'_>,
    severity: impl Fn(&T) -> DoctorSeverity,
) -> CappedFindings<T> {
    findings.sort_by(|left, right| order(left).cmp(&order(right)));
    let mut tally = SeverityTally::default();
    for finding in &findings {
        tally.add(severity(finding));
    }
    for omitted in already_omitted {
        tally.add(*omitted);
    }
    let heaviest = check_severity(
        findings
            .iter()
            .map(&severity)
            .chain(already_omitted.iter().copied()),
    );
    let dropped = findings.len().saturating_sub(FINDINGS_MAX);
    findings.truncate(FINDINGS_MAX);
    CappedFindings {
        kept: findings,
        omitted: u64::try_from(dropped.saturating_add(already_omitted.len())).unwrap_or(u64::MAX),
        severity: heaviest,
        tally,
    }
}

/// `text` cut to at most `max` characters, never inside a character.
#[must_use]
pub fn clip_chars(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((end, _)) => text[..end].to_owned(),
        None => text.to_owned(),
    }
}

/// `text` cut to at most `max` bytes at a character boundary.
#[must_use]
pub fn clip_bytes(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_owned();
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_check_takes_the_heaviest_severity_of_its_findings() {
        assert_eq!(check_severity([]), DoctorSeverity::Ok);
        assert_eq!(
            check_severity([DoctorSeverity::Warning, DoctorSeverity::Ok]),
            DoctorSeverity::Warning
        );
        assert_eq!(
            check_severity([
                DoctorSeverity::Warning,
                DoctorSeverity::Error,
                DoctorSeverity::Warning
            ]),
            DoctorSeverity::Error
        );
    }

    #[test]
    fn the_daemon_checks_are_the_table_order_without_the_client_checks() {
        let from_table: Vec<_> = DoctorCheckId::ALL
            .iter()
            .copied()
            .filter(|check| check.runs_in_daemon())
            .collect();
        assert_eq!(from_table, DAEMON_CHECKS);
        assert_eq!(DoctorCheckId::Daemon.daemon_index(), Some(0));
        assert_eq!(DoctorCheckId::Operations.daemon_index(), Some(12));
        assert_eq!(DoctorCheckId::Connection.daemon_index(), None);
        for client in [
            DoctorCheckId::Connection,
            DoctorCheckId::Protocol,
            DoctorCheckId::BackgroundAgent,
        ] {
            assert!(!client.runs_in_daemon());
            assert!(!client.needs_registry());
        }
    }

    #[test]
    fn only_daemon_registry_permissions_and_disk_space_run_in_recovery() {
        let runs: Vec<_> = DAEMON_CHECKS
            .iter()
            .copied()
            .filter(|check| check.runs_in_recovery())
            .collect();
        assert_eq!(
            runs,
            [
                DoctorCheckId::Daemon,
                DoctorCheckId::Registry,
                DoctorCheckId::Permissions,
                DoctorCheckId::DiskSpace
            ]
        );
        assert!(!DoctorCheckId::Daemon.needs_registry());
        assert!(!DoctorCheckId::DiskSpace.needs_registry());
        assert!(DoctorCheckId::Registry.needs_registry());
    }

    #[test]
    fn every_code_round_trips_through_its_wire_text_and_is_snake_case() {
        for check in DoctorCheckId::ALL {
            assert_eq!(DoctorCheckId::from_code(check.code()), Some(*check));
        }
        for code in DoctorFindingCode::ALL {
            assert_eq!(DoctorFindingCode::from_code(code.code()), Some(*code));
            assert!(
                code.code()
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte == b'_'),
                "{}",
                code.code()
            );
        }
        for reason in CheckUnavailableReason::ALL {
            assert_eq!(
                CheckUnavailableReason::from_code(reason.code()),
                Some(*reason)
            );
        }
        assert_eq!(DoctorCheckId::from_code("unrecognized"), None);
        assert_eq!(DoctorFindingCode::from_code("port-in-use"), None);
    }

    fn order(
        severity: DoctorSeverity,
        instance: Option<&'static str>,
        code: DoctorFindingCode,
        subject: Option<&'static str>,
    ) -> FindingOrder<'static> {
        FindingOrder {
            severity,
            instance,
            code,
            subject,
        }
    }

    #[test]
    fn findings_order_by_severity_then_global_first_then_instance_code_and_subject() {
        let mut findings = [
            order(
                DoctorSeverity::Warning,
                None,
                DoctorFindingCode::ScratchLeftover,
                Some("operations/b"),
            ),
            order(
                DoctorSeverity::Error,
                Some("web"),
                DoctorFindingCode::PortInUse,
                Some("http"),
            ),
            order(
                DoctorSeverity::Error,
                Some("api"),
                DoctorFindingCode::StartFailed,
                None,
            ),
            order(
                DoctorSeverity::Error,
                Some("api"),
                DoctorFindingCode::PortInUse,
                Some("grpc"),
            ),
            order(
                DoctorSeverity::Error,
                Some("api"),
                DoctorFindingCode::PortInUse,
                Some("client"),
            ),
            order(
                DoctorSeverity::Error,
                None,
                DoctorFindingCode::NeedsRecovery,
                Some("0199"),
            ),
            order(
                DoctorSeverity::Warning,
                None,
                DoctorFindingCode::ScratchLeftover,
                Some("operations/a"),
            ),
        ];
        findings.sort();
        let seen: Vec<_> = findings
            .iter()
            .map(|finding| (finding.instance, finding.code.code(), finding.subject))
            .collect();
        assert_eq!(
            seen,
            [
                (None, "needs_recovery", Some("0199")),
                (Some("api"), "port_in_use", Some("client")),
                (Some("api"), "port_in_use", Some("grpc")),
                (Some("api"), "start_failed", None),
                (Some("web"), "port_in_use", Some("http")),
                (None, "scratch_leftover", Some("operations/a")),
                (None, "scratch_leftover", Some("operations/b")),
            ]
        );
    }

    #[test]
    fn capping_keeps_the_first_hundred_and_counts_every_finding_in_the_tally() {
        let mut findings: Vec<(DoctorSeverity, String)> = (0..120)
            .map(|index| (DoctorSeverity::Warning, format!("w{index:03}")))
            .collect();
        findings.push((DoctorSeverity::Error, "e".to_owned()));
        let capped = cap_findings(
            findings,
            &[DoctorSeverity::Warning, DoctorSeverity::Error],
            |(severity, subject)| FindingOrder {
                severity: *severity,
                instance: Some("api"),
                code: DoctorFindingCode::PathNotWritable,
                subject: Some(subject),
            },
            |(severity, _)| *severity,
        );
        assert_eq!(capped.kept.len(), FINDINGS_MAX);
        assert_eq!(capped.kept[0].1, "e");
        assert_eq!(capped.kept[1].1, "w000");
        assert_eq!(capped.omitted, 21 + 2);
        assert_eq!(capped.severity, DoctorSeverity::Error);
        assert_eq!(
            capped.tally,
            SeverityTally {
                errors: 2,
                warnings: 121
            }
        );
    }

    #[test]
    fn a_check_without_findings_is_ok_with_nothing_omitted() {
        let capped = cap_findings(
            Vec::<DoctorSeverity>::new(),
            &[],
            |severity| FindingOrder {
                severity: *severity,
                instance: None,
                code: DoctorFindingCode::DataMissing,
                subject: None,
            },
            |severity| *severity,
        );
        assert!(capped.kept.is_empty());
        assert_eq!(capped.omitted, 0);
        assert_eq!(capped.severity, DoctorSeverity::Ok);
        assert_eq!(capped.tally, SeverityTally::default());
    }

    #[test]
    fn clipping_never_cuts_inside_a_character() {
        assert_eq!(clip_chars("héllo", 2), "hé");
        assert_eq!(clip_chars("hi", 10), "hi");
        assert_eq!(clip_bytes("héllo", 2), "h");
        assert_eq!(clip_bytes("héllo", 3), "hé");
        assert_eq!(clip_bytes("hi", 10), "hi");
    }

    #[test]
    fn tallies_add_up_and_ignore_ok() {
        let mut tally = SeverityTally::default();
        tally.add(DoctorSeverity::Ok);
        tally.add(DoctorSeverity::Error);
        tally.add(DoctorSeverity::Warning);
        tally.add(DoctorSeverity::Warning);
        assert_eq!(
            tally.plus(SeverityTally {
                errors: 1,
                warnings: 0
            }),
            SeverityTally {
                errors: 2,
                warnings: 2
            }
        );
    }
}
