//! The doctor (spec 0034): `doctor.run` runs the read only checks of the
//! daemon and answers a [`DoctorReport`] with one [`DoctorCheck`] per
//! check, each with its findings, their English text, and at most one safe
//! action. The client checks (`connection`, `protocol`, `background_agent`)
//! use the same types but never come from the daemon.
//!
//! A doctor run is not an operation (spec 0007): it writes nothing, holds no
//! instance lock, and signals no process.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use typeshare::typeshare;
use uuid::Uuid;

use crate::error::FieldError;
use crate::instance::IdentityEvidence;
use crate::json::{FieldCode, Validate};
use crate::storage::CapacityReport;
use crate::wire::{Count, Timestamp};

/// The most checks one `doctor.run` may name: every daemon check once.
pub const DOCTOR_CHECKS_MAX: usize = 13;
/// The most findings one check carries; the rest are counted in `omitted`.
pub const DOCTOR_FINDINGS_MAX: usize = 100;
/// The most details one finding carries.
pub const DOCTOR_DETAILS_MAX: usize = 16;
/// The longest detail key, in bytes.
pub const DOCTOR_DETAIL_KEY_MAX: usize = 32;
/// The longest detail value, in bytes.
pub const DOCTOR_DETAIL_VALUE_MAX: usize = 512;
/// The most excerpt lines one finding carries.
pub const DOCTOR_EXCERPT_LINES_MAX: usize = 10;
/// The longest excerpt line, in characters.
pub const DOCTOR_EXCERPT_LINE_MAX: usize = 256;
/// The longest finding title, in characters.
pub const DOCTOR_TITLE_MAX: usize = 120;
/// The longest finding explanation, in characters.
pub const DOCTOR_EXPLANATION_MAX: usize = 600;
/// The longest check summary, in characters.
pub const DOCTOR_SUMMARY_MAX: usize = 200;

/// How serious a finding or a check is. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DoctorSeverity {
    /// Something is wrong and keeps Stackly from doing its work.
    Error,
    /// Something may need your attention.
    Warning,
    /// Nothing is wrong.
    Ok,
    /// A severity this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// One check of the doctor. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DoctorCheckId {
    /// Whether the client reaches the daemon. A client check.
    Connection,
    /// Whether client and daemon speak the same protocol. A client check.
    Protocol,
    /// The state of the background agent. A client check.
    BackgroundAgent,
    /// The daemon's own subsystems.
    Daemon,
    /// The registry file.
    Registry,
    /// The roots and the instance folders.
    Permissions,
    /// The volume that holds the data root.
    DiskSpace,
    /// The managed runtimes.
    Runtimes,
    /// The pinned provider packages instances use.
    ProviderPackages,
    /// Ports of instances that are not running.
    Endpoints,
    /// Recorded processes and their identity.
    Processes,
    /// The data folder and runtime of each instance.
    InstanceFiles,
    /// Instances that failed or are not ready.
    InstanceFailures,
    /// Rendered config that differs from its revision.
    ConfigDrift,
    /// Secrets an instance needs and cannot find.
    Secrets,
    /// Operations that need recovery and scratch folders left behind.
    Operations,
    /// A check this build does not know. A client never sends it.
    #[serde(other)]
    Unrecognized,
}

impl DoctorCheckId {
    /// Whether the daemon runs this check; a `doctor.run` that names any
    /// other is refused.
    #[must_use]
    pub const fn runs_in_daemon(self) -> bool {
        !matches!(
            self,
            Self::Connection | Self::Protocol | Self::BackgroundAgent | Self::Unrecognized
        )
    }
}

/// What a finding is about. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DoctorFindingCode {
    /// `connection`: the client cannot reach the daemon; the `code` detail
    /// holds the `ipc.*` error.
    DaemonUnreachable,
    /// `protocol`: the daemon speaks another protocol major.
    ProtocolIncompatible,
    /// `background_agent`: the agent needs you; the `state` detail names
    /// the state (spec 0015).
    AgentState,
    /// `background_agent`: the daemon wrote a start failure record.
    DaemonStartFailed,
    /// `background_agent`: the daemon binary may have been replaced since
    /// launchd last started it.
    BinaryReplaced,
    /// `daemon`: one subsystem of the daemon is degraded.
    SubsystemDegraded,
    /// `registry`: the registry is open in recovery mode.
    RecoveryMode,
    /// `registry`: `PRAGMA quick_check` failed.
    QuickCheckFailed,
    /// `registry`: a row holds a code this build does not know.
    UnrecognizedValue,
    /// `permissions`: a root is not private to you.
    RootNotPrivate,
    /// `permissions`: a folder of an instance is not writable by you.
    PathNotWritable,
    /// `permissions`: an entry of an instance belongs to another user.
    PathForeignOwner,
    /// `disk_space`: the data volume is filling.
    VolumeFilling,
    /// `disk_space`: the data volume is low on space.
    VolumeLow,
    /// `disk_space`: the data volume is almost full.
    VolumeCritical,
    /// `runtimes`: a runtime is `missing`, `drifted`, or `failed`.
    RuntimeState,
    /// `runtimes`: an installed runtime misses a file.
    RuntimeFilesMissing,
    /// `provider_packages`: a pinned package does not load.
    PackageUnreadable,
    /// `endpoints`: a port of an instance that is not running is in use.
    PortInUse,
    /// `processes`: a recorded PID now belongs to another process.
    PidReused,
    /// `processes`: the data folder is in use by a process Stackly did not
    /// start.
    DataInUse,
    /// `processes`: the recorded process can't be proven to be the
    /// instance's.
    IdentityUnproven,
    /// `processes`: the recorded process is suspended.
    ProcessSuspended,
    /// `instance_files`: the data folder of an instance is gone.
    DataMissing,
    /// `instance_files`: the runtime of an instance is gone.
    RuntimeMissing,
    /// `instance_failures`: an instance failed.
    StartFailed,
    /// `instance_failures`: a running instance is not ready.
    NotReady,
    /// `config_drift`: a rendered file differs from its revision.
    ConfigDrift,
    /// `secrets`: a secret an instance needs is missing.
    SecretMissing,
    /// `operations`: an operation needs recovery.
    NeedsRecovery,
    /// `operations`: a scratch folder has no operation.
    ScratchLeftover,
    /// Any check: the check could not run; the `reason` detail says why
    /// (`timeout`, `recovery_mode`, `not_supported`, or `io`).
    CheckUnavailable,
    /// A code this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// The kind of a safe action. Open: a client hides a kind it does not
/// know.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DoctorActionKind {
    /// Open the instance.
    ShowInstance,
    /// Open the instance's configuration to review the drift.
    ReviewConfig,
    /// Open the endpoints of the instance with the suggested port filled
    /// in; nothing changes until you apply it.
    ChangePort,
    /// Open the operation.
    ShowOperation,
    /// Open the storage settings.
    OpenStorageSettings,
    /// An action this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// Who holds a port. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PortOwnerKind {
    /// A process of yours that is not a Stackly instance.
    Process,
    /// Another Stackly instance: the PID is its recorded process and the
    /// start time matches.
    StacklyInstance,
    /// No process of yours holds it; another user or the system may.
    Unknown,
    /// A kind this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// What a PID file of a remedy holds. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PidFileState {
    /// The file is not there.
    Missing,
    /// The file could not be read, or its first line is not a PID.
    Unreadable,
    /// No process has the PID.
    NotRunning,
    /// The PID is a process of the instance's runtime.
    RuntimeProcess,
    /// The PID is another process.
    OtherProcess,
    /// A state this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// Params of `doctor.run`.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DoctorParams {
    /// Limits the checks that are about instances to this one; the global
    /// checks still run. An ID that is not there is `resource.not_found`.
    #[serde(default)]
    pub instance_id: Option<Uuid>,
    /// The daemon checks to run, each at most once; empty means all of
    /// them. A client check or an unknown one is refused.
    #[serde(default)]
    pub checks: Vec<DoctorCheckId>,
}

impl Validate for DoctorParams {
    /// Every entry of `checks` is a daemon check, and none is there twice.
    fn validate(&self) -> Vec<FieldError> {
        let mut fields = Vec::new();
        if self.checks.len() > DOCTOR_CHECKS_MAX {
            fields.push(FieldError::new("checks", FieldCode::TooMany));
            return fields;
        }
        for (index, check) in self.checks.iter().enumerate() {
            if !check.runs_in_daemon() {
                fields.push(FieldError::new(
                    format!("checks[{index}]"),
                    FieldCode::UnknownValue,
                ));
            } else if self.checks[..index].contains(check) {
                fields.push(FieldError::new(
                    format!("checks[{index}]"),
                    FieldCode::InvalidValue,
                ));
            }
        }
        fields
    }
}

/// An instance a report or a finding is about.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DoctorInstanceRef {
    /// Its ID.
    pub id: Uuid,
    /// Its name.
    pub name: String,
}

/// Reply of `doctor.run`.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DoctorReport {
    /// The ID of this run (UUID version 7).
    pub run_id: Uuid,
    /// When the run started.
    pub started_at: Timestamp,
    /// When the run finished.
    pub finished_at: Timestamp,
    /// True when the registry is open in recovery mode.
    pub recovery: bool,
    /// The instance the run was limited to, if any.
    #[serde(default)]
    pub scope: Option<DoctorInstanceRef>,
    /// One entry per check, in the order of the check table.
    #[serde(default)]
    pub checks: Vec<DoctorCheck>,
    /// The findings with severity `error`, the omitted ones included.
    pub errors: Count,
    /// The findings with severity `warning`, the omitted ones included.
    pub warnings: Count,
}

/// One check of a report.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DoctorCheck {
    /// Which check.
    pub id: DoctorCheckId,
    /// The heaviest severity of its findings; `ok` without findings.
    pub severity: DoctorSeverity,
    /// Its title, such as `Endpoints`.
    pub title: String,
    /// One line, 1 to 200 characters, such as `No conflicts across 4
    /// configured endpoints.`.
    pub summary: String,
    /// At most 100 findings, the most serious first.
    #[serde(default)]
    pub findings: Vec<DoctorFinding>,
    /// The findings left out past the limit.
    pub omitted: Count,
    /// The reading behind `disk_space`; absent for every other check.
    #[serde(default)]
    pub capacity: Option<CapacityReport>,
}

/// One finding of a check.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DoctorFinding {
    /// What it is about.
    pub code: DoctorFindingCode,
    /// How serious it is.
    pub severity: DoctorSeverity,
    /// Its title, at most 120 characters, such as `Port conflict ·
    /// mysql-legacy`.
    pub title: String,
    /// One to three sentences, at most 600 characters.
    pub explanation: String,
    /// What it is about within its instance: an endpoint, a path, or an
    /// operation ID.
    #[serde(default)]
    pub subject: Option<String>,
    /// The instance it is about.
    #[serde(default)]
    pub instance: Option<DoctorInstanceRef>,
    /// The operation it is about.
    #[serde(default)]
    pub operation_id: Option<Uuid>,
    /// At most 16 details, by key; never a secret value.
    #[serde(default)]
    pub details: Vec<DoctorDetail>,
    /// The identity evidence of a `processes` finding.
    #[serde(default)]
    pub evidence: Option<DoctorEvidence>,
    /// Who holds the port of a `port_in_use` finding.
    #[serde(default)]
    pub port_owner: Option<PortOwner>,
    /// At most 10 redacted lines of output, each at most 256 characters.
    #[serde(default)]
    pub excerpt: Vec<String>,
    /// The provider's guidance for this failure, if any.
    #[serde(default)]
    pub remedy: Option<DoctorRemedy>,
    /// The one safe action, if any.
    #[serde(default)]
    pub action: Option<DoctorAction>,
}

/// One detail of a finding.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DoctorDetail {
    /// The key, camelCase, at most 32 bytes, such as `port`.
    pub key: String,
    /// The value, at most 512 bytes.
    pub value: String,
}

/// What the passive identity checks found about a recorded process.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DoctorEvidence {
    /// The checks of spec 0010 without the lock probe; `lock` comes from a
    /// stored blocker, or is null.
    pub identity: IdentityEvidence,
    /// The process the registry recorded.
    #[serde(default)]
    pub recorded: Option<ProcessRef>,
    /// The process that holds the PID now, if one does.
    #[serde(default)]
    pub current: Option<ProcessRef>,
    /// When the evidence was gathered.
    pub observed_at: Timestamp,
}

/// A process, as the doctor shows it.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProcessRef {
    /// Its PID.
    pub pid: u32,
    /// When it started, if known.
    #[serde(default)]
    pub started_at: Option<Timestamp>,
    /// Its executable, with your home folder written `~`; null for a
    /// process of another user.
    #[serde(default)]
    pub executable: Option<String>,
    /// False for a process of another user; null when it was not looked
    /// at.
    #[serde(default)]
    pub same_user: Option<bool>,
}

/// Who holds a port.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PortOwner {
    /// Which kind of owner.
    pub kind: PortOwnerKind,
    /// The process, for `process` and `stackly_instance`.
    #[serde(default)]
    pub process: Option<ProcessRef>,
    /// The instance, for `stackly_instance`.
    #[serde(default)]
    pub instance: Option<DoctorInstanceRef>,
}

/// A provider's guidance for a failure (spec 0034, *Blok remedies*). Only
/// text, commands to copy, and paths; Stackly never runs it.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DoctorRemedy {
    /// The provider of the package the entry comes from.
    pub provider: String,
    /// The revision of that package.
    pub revision: String,
    /// The name of the entry, such as `stale_postmaster_pid`.
    pub name: String,
    /// Its title.
    pub title: String,
    /// The steps, in order.
    #[serde(default)]
    pub steps: Vec<RemedyStep>,
    /// What the PID file the entry names holds.
    #[serde(default)]
    pub pid_file: Option<PidFileEvidence>,
}

/// One step of a remedy.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RemedyStep {
    /// What to do.
    pub text: String,
    /// A command to copy and run yourself.
    #[serde(default)]
    pub command: Option<String>,
    /// A path to look at.
    #[serde(default)]
    pub path: Option<String>,
}

/// What the PID file of a remedy holds, read passively.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PidFileEvidence {
    /// The file, with your home folder written `~`.
    pub path: String,
    /// The PID on its first line, if it is one.
    #[serde(default)]
    pub pid: Option<u32>,
    /// What the PID is.
    pub state: PidFileState,
    /// The process with that PID, if one is alive.
    #[serde(default)]
    pub process: Option<ProcessRef>,
}

/// The one safe action of a finding: a place to go or a flow that already
/// asks before it changes anything. A flat struct with an open `kind`, so
/// a new action is a minor version and an older client hides it.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DoctorAction {
    /// What to do.
    pub kind: DoctorActionKind,
    /// The instance, for `show_instance`, `review_config`, and
    /// `change_port`.
    #[serde(default)]
    pub instance_id: Option<Uuid>,
    /// The operation, for `show_operation`.
    #[serde(default)]
    pub operation_id: Option<Uuid>,
    /// The endpoint, for `change_port`.
    #[serde(default)]
    pub endpoint: Option<String>,
    /// The suggested port, for `change_port`.
    #[serde(default)]
    pub port: Option<u16>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(checks: &[DoctorCheckId]) -> DoctorParams {
        DoctorParams {
            instance_id: None,
            checks: checks.to_vec(),
        }
    }

    #[test]
    fn any_set_of_distinct_daemon_checks_is_valid() {
        assert!(params(&[]).validate().is_empty());
        assert!(
            params(&[DoctorCheckId::Endpoints, DoctorCheckId::Registry])
                .validate()
                .is_empty()
        );
        let all = [
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
        assert_eq!(all.len(), DOCTOR_CHECKS_MAX);
        assert!(params(&all).validate().is_empty());
    }

    #[test]
    fn a_client_check_an_unknown_check_or_a_repeat_is_refused_by_index() {
        let refused = params(&[
            DoctorCheckId::Registry,
            DoctorCheckId::Connection,
            DoctorCheckId::Protocol,
            DoctorCheckId::BackgroundAgent,
            DoctorCheckId::Unrecognized,
            DoctorCheckId::Registry,
        ])
        .validate();
        let seen: Vec<_> = refused
            .iter()
            .map(|field| (field.path().to_owned(), field.code().to_owned()))
            .collect();
        assert_eq!(
            seen,
            [
                (
                    "checks[1]".to_owned(),
                    FieldCode::UnknownValue.as_str().to_owned()
                ),
                (
                    "checks[2]".to_owned(),
                    FieldCode::UnknownValue.as_str().to_owned()
                ),
                (
                    "checks[3]".to_owned(),
                    FieldCode::UnknownValue.as_str().to_owned()
                ),
                (
                    "checks[4]".to_owned(),
                    FieldCode::UnknownValue.as_str().to_owned()
                ),
                (
                    "checks[5]".to_owned(),
                    FieldCode::InvalidValue.as_str().to_owned()
                ),
            ]
        );
        let too_many = params(&[DoctorCheckId::Daemon; DOCTOR_CHECKS_MAX + 1]).validate();
        assert_eq!(too_many.len(), 1);
        assert_eq!(too_many[0].path(), "checks");
        assert_eq!(too_many[0].code(), FieldCode::TooMany.as_str());
    }

    #[test]
    fn unknown_wire_values_read_as_unrecognized() {
        assert_eq!(
            serde_json::from_str::<DoctorCheckId>("\"telemetry\"").unwrap(),
            DoctorCheckId::Unrecognized
        );
        assert_eq!(
            serde_json::from_str::<DoctorFindingCode>("\"cosmic_ray\"").unwrap(),
            DoctorFindingCode::Unrecognized
        );
        assert_eq!(
            serde_json::from_str::<DoctorSeverity>("\"fatal\"").unwrap(),
            DoctorSeverity::Unrecognized
        );
        assert_eq!(
            serde_json::from_str::<DoctorActionKind>("\"reinstall\"").unwrap(),
            DoctorActionKind::Unrecognized
        );
        assert_eq!(
            serde_json::from_str::<PortOwnerKind>("\"kernel\"").unwrap(),
            PortOwnerKind::Unrecognized
        );
        assert_eq!(
            serde_json::from_str::<PidFileState>("\"zombie\"").unwrap(),
            PidFileState::Unrecognized
        );
        assert_eq!(
            serde_json::to_string(&DoctorCheckId::BackgroundAgent).unwrap(),
            "\"background_agent\""
        );
    }

    #[test]
    fn params_without_fields_ask_for_every_check_everywhere() {
        let params: DoctorParams = serde_json::from_str("{}").unwrap();
        assert_eq!(params, DoctorParams::default());
    }
}
