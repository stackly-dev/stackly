//! What a doctor run takes and what it reports.

use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::Arc;

use jiff::Timestamp;
use stackly_domain::doctor::{
    CheckUnavailableReason, DoctorCheckId, DoctorFindingCode, DoctorSeverity, SeverityTally,
};
use stackly_domain::{
    EndpointName, IdentityEvidence, Instance, InstanceId, InstanceName, NeedsSecretReason,
    Operation, OperationId, Pid, Port, ProcessIdentity, ProviderRevisionKey, RecoveryCause,
    RegistryError, Runtime, SecretRef, Transport, UnrecognizedValue,
};
use stackly_platform::{ListenerOwner, Platform, ProcessFacts, RootKind, UserId};
use uuid::Uuid;

use crate::PreparedLayout;
use crate::catalog::Catalog;
use crate::provider::RenderedRemedy;
use crate::storage::CapacityReading;
use crate::supervisor::{BindOutcome, InstanceStatus, Supervisor};
use stackly_domain::CapacityLevel;

/// The ports and the state a run reads. Everything is shared, so a check
/// that runs past its limit can finish on its own.
#[derive(Clone)]
#[non_exhaustive]
pub struct DoctorSources {
    /// The platform ports.
    pub platform: Arc<Platform>,
    /// The checked layout.
    pub layout: Arc<PreparedLayout>,
    /// Whether the registry can be read.
    pub mode: DoctorMode,
    /// What the daemon knows about itself.
    pub daemon: DaemonFacts,
    /// The loaded provider catalog, whose package revisions the remedies
    /// come from first (spec 0034, AC-10); `None` without one.
    pub catalog: Option<Arc<Catalog>>,
}

impl DoctorSources {
    /// Bundles the sources of a run, without a catalog.
    #[must_use]
    pub const fn new(
        platform: Arc<Platform>,
        layout: Arc<PreparedLayout>,
        mode: DoctorMode,
        daemon: DaemonFacts,
    ) -> Self {
        Self {
            platform,
            layout,
            mode,
            daemon,
            catalog: None,
        }
    }

    /// The same sources with the loaded catalog, if there is one.
    #[must_use]
    pub fn with_catalog(mut self, catalog: Option<Arc<Catalog>>) -> Self {
        self.catalog = catalog;
        self
    }
}

impl std::fmt::Debug for DoctorSources {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DoctorSources")
            .field("mode", &self.mode)
            .field("daemon", &self.daemon)
            .field("catalog", &self.catalog.is_some())
            .finish_non_exhaustive()
    }
}

/// Whether the checks can read the registry.
#[derive(Clone)]
#[non_exhaustive]
pub enum DoctorMode {
    /// The registry is open: the supervisor holds it, the port probe, and
    /// the operation engine.
    Ready {
        /// The supervisor of this daemon.
        supervisor: Supervisor,
        /// The schema version of the open registry.
        schema_version: u32,
    },
    /// The registry is open in recovery mode (spec 0005): no check reads
    /// or writes it.
    Recovery {
        /// The reason code of the recovery report.
        reason: String,
        /// Its message, already redacted.
        message: String,
    },
    /// The daemon is starting or lost its registry; the checks that need it
    /// report `check_unavailable`.
    Unavailable {
        /// Why, already redacted.
        message: String,
    },
}

impl std::fmt::Debug for DoctorMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Ready { schema_version, .. } => f
                .debug_struct("Ready")
                .field("schema_version", schema_version)
                .finish_non_exhaustive(),
            Self::Recovery { reason, message } => f
                .debug_struct("Recovery")
                .field("reason", reason)
                .field("message", message)
                .finish(),
            Self::Unavailable { message } => f
                .debug_struct("Unavailable")
                .field("message", message)
                .finish(),
        }
    }
}

/// What the daemon knows about its own health.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct DaemonFacts {
    /// The subsystems that are not healthy, in the daemon's order.
    pub degraded: Vec<DegradedSubsystem>,
}

impl DaemonFacts {
    /// Facts with these degraded subsystems.
    #[must_use]
    pub const fn new(degraded: Vec<DegradedSubsystem>) -> Self {
        Self { degraded }
    }
}

/// One subsystem of the daemon that is degraded or unavailable.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct DegradedSubsystem {
    /// Its name, such as `log_sink`.
    pub name: String,
    /// Its status, such as `degraded`.
    pub status: String,
    /// What the daemon says about it, already redacted.
    pub detail: Option<String>,
}

impl DegradedSubsystem {
    /// One degraded subsystem.
    #[must_use]
    pub const fn new(name: String, status: String, detail: Option<String>) -> Self {
        Self {
            name,
            status,
            detail,
        }
    }
}

/// What a client asked for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct DoctorRequest {
    /// Limits the checks about instances to this one.
    pub instance: Option<InstanceId>,
    /// The daemon checks to run; empty means all of them.
    pub checks: Vec<DoctorCheckId>,
}

impl DoctorRequest {
    /// A request for `checks` (empty for all) about `instance` (or all).
    #[must_use]
    pub const fn new(instance: Option<InstanceId>, checks: Vec<DoctorCheckId>) -> Self {
        Self { instance, checks }
    }
}

/// The free space reading behind `disk_space`, or why there is none.
pub type CapacityOutcome = Result<DiskReading, CapacityFailure>;

/// One reading of the volume that holds the data root, as
/// `storage.get_capacity` answered it (spec 0022).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct DiskReading {
    /// When the reading was taken.
    pub checked_at: Timestamp,
    /// The size of the volume, in bytes.
    pub total_bytes: u64,
    /// The bytes an unprivileged user can still write.
    pub available_bytes: u64,
    /// The percent in use, rounded down.
    pub used_percent: u8,
    /// How worried the user should be.
    pub level: CapacityLevel,
}

impl DiskReading {
    /// A reading of `total_bytes` with `available_bytes` left, taken at
    /// `checked_at`; the percent and the level follow from them
    /// (`stackly_domain::capacity`). `None` when `total_bytes` is 0.
    #[must_use]
    pub fn new(checked_at: Timestamp, total_bytes: u64, available_bytes: u64) -> Option<Self> {
        let level = stackly_domain::capacity::level(total_bytes, available_bytes)?;
        Some(Self {
            checked_at,
            total_bytes,
            available_bytes: available_bytes.min(total_bytes),
            used_percent: stackly_domain::capacity::used_percent(total_bytes, available_bytes),
            level,
        })
    }
}

impl From<CapacityReading> for DiskReading {
    fn from(reading: CapacityReading) -> Self {
        Self {
            checked_at: reading.checked_at,
            total_bytes: reading.total_bytes,
            available_bytes: reading.available_bytes,
            used_percent: reading.used_percent,
            level: reading.level,
        }
    }
}

/// Why the free space could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct CapacityFailure {
    /// `timeout` when the reading ran past its limit, else `io`.
    pub reason: CheckUnavailableReason,
    /// What went wrong, already redacted.
    pub message: String,
}

impl CapacityFailure {
    /// A failure for `reason` with `message`.
    #[must_use]
    pub const fn new(reason: CheckUnavailableReason, message: String) -> Self {
        Self { reason, message }
    }
}

/// Why a run could not start.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum DoctorError {
    /// The instance the run was limited to is not there.
    #[error("instance {0} not found")]
    InstanceNotFound(InstanceId),
    /// Reading the instance the run was limited to failed.
    #[error("the registry could not be read: {0}")]
    Registry(RegistryError),
}

/// An instance, by ID and name.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct InstanceRef {
    /// Its ID.
    pub id: InstanceId,
    /// Its name.
    pub name: InstanceName,
}

impl InstanceRef {
    /// The reference of `instance`.
    #[must_use]
    pub fn of(instance: &Instance) -> Self {
        Self {
            id: instance.id(),
            name: instance.name().clone(),
        }
    }
}

/// One doctor run.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Report {
    /// The ID of the run (UUID version 7).
    pub run_id: Uuid,
    /// When it started.
    pub started_at: Timestamp,
    /// When it finished.
    pub finished_at: Timestamp,
    /// True when the registry is open in recovery mode.
    pub recovery: bool,
    /// The instance it was limited to.
    pub scope: Option<InstanceRef>,
    /// One entry per check, in the order of the check table.
    pub checks: Vec<CheckReport>,
    /// The findings of every check by severity, the omitted ones included.
    pub tally: SeverityTally,
}

/// One check of a run.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct CheckReport {
    /// Which check.
    pub id: DoctorCheckId,
    /// The heaviest severity of its findings, kept or omitted.
    pub severity: DoctorSeverity,
    /// At most [`FINDINGS_MAX`](stackly_domain::doctor::FINDINGS_MAX)
    /// findings, in order.
    pub findings: Vec<Finding>,
    /// The findings left out.
    pub omitted: u64,
    /// The findings by severity, kept and left out.
    pub tally: SeverityTally,
    /// What the summary line says.
    pub summary: SummaryFacts,
    /// The reading of `disk_space`.
    pub capacity: Option<DiskReading>,
}

/// What the summary line of a check is built from.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SummaryFacts {
    /// `daemon`: the daemon's version and launch, which the daemon knows.
    Daemon,
    /// `registry`: the schema version of the open registry.
    Registry {
        /// The schema version, absent in recovery mode.
        schema_version: Option<u32>,
    },
    /// `disk_space`: the reading.
    Capacity(DiskReading),
    /// How many things the check looked at: runtimes, packages, endpoints,
    /// instances, or operations.
    Checked(u64),
    /// The check did not run.
    Unavailable(CheckUnavailableReason),
}

/// One finding.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Finding {
    /// What it is about.
    pub code: DoctorFindingCode,
    /// How serious it is.
    pub severity: DoctorSeverity,
    /// The instance it is about.
    pub instance: Option<InstanceRef>,
    /// What it is about within the instance: an endpoint, a path, or an
    /// operation ID.
    pub subject: Option<String>,
    /// The facts behind it.
    pub facts: FindingFacts,
    /// The one safe action.
    pub action: Option<Action>,
    /// The provider remedy that matches a `start_failed` or `not_ready`.
    pub remedy: Option<Box<RemedyFacts>>,
}

impl Finding {
    /// A finding without an instance, a subject, or an action.
    #[must_use]
    pub const fn new(
        code: DoctorFindingCode,
        severity: DoctorSeverity,
        facts: FindingFacts,
    ) -> Self {
        Self {
            code,
            severity,
            instance: None,
            subject: None,
            facts,
            action: None,
            remedy: None,
        }
    }

    /// The same finding about `instance`.
    #[must_use]
    pub fn about(mut self, instance: InstanceRef) -> Self {
        self.instance = Some(instance);
        self
    }

    /// The same finding with `subject`.
    #[must_use]
    pub fn subject(mut self, subject: impl Into<String>) -> Self {
        self.subject = Some(subject.into());
        self
    }

    /// The same finding with `action`.
    #[must_use]
    pub fn action(mut self, action: Action) -> Self {
        self.action = Some(action);
        self
    }

    /// The same finding with `remedy`, if there is one.
    #[must_use]
    pub fn remedy(mut self, remedy: Option<RemedyFacts>) -> Self {
        self.remedy = remedy.map(Box::new);
        self
    }

    /// The finding of a check that could not run.
    #[must_use]
    pub const fn unavailable(reason: CheckUnavailableReason, message: Option<String>) -> Self {
        Self::new(
            DoctorFindingCode::CheckUnavailable,
            DoctorSeverity::Warning,
            FindingFacts::Unavailable { reason, message },
        )
    }
}

/// The provider remedy of a finding (spec 0034, AC-10): the first entry of
/// the package whose `when` matches, rendered for the instance.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct RemedyFacts {
    /// The package revision the entry comes from: the one of the catalog,
    /// else the one the instance pins.
    pub package: ProviderRevisionKey,
    /// The display name of the provider, such as `PostgreSQL`.
    pub display_name: String,
    /// The entry, rendered.
    pub remedy: RenderedRemedy,
    /// What the PID file of the entry holds, read passively.
    pub pid_file: Option<PidFileFacts>,
}

/// What the PID file of a remedy holds (spec 0034, AC-10).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PidFileFacts {
    /// The file.
    pub path: PathBuf,
    /// The PID on its first line, if it is one.
    pub pid: Option<Pid>,
    /// What the PID is.
    pub state: PidFileState,
    /// The process with that PID, when it is alive and its facts could be
    /// read.
    pub process: Option<ProcessFacts>,
    /// True when that process belongs to another user, whose executable is
    /// never shown.
    pub other_user: bool,
}

/// What the PID of a PID file is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PidFileState {
    /// The file is not there.
    Missing,
    /// The file could not be read, its folder is outside the instance's
    /// data and config folders, or its first line is not a PID.
    Unreadable,
    /// No process has the PID.
    NotRunning,
    /// The PID is a process of one of the instance's runtime executables.
    RuntimeProcess,
    /// The PID is another process.
    OtherProcess,
}

/// A place to go, or a flow that asks before it changes anything.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Action {
    /// Open the instance.
    ShowInstance(InstanceId),
    /// Open the configuration of the instance.
    ReviewConfig(InstanceId),
    /// Open the endpoints of the instance with `port` filled in.
    ChangePort {
        /// The instance.
        instance: InstanceId,
        /// The endpoint.
        endpoint: EndpointName,
        /// The suggested port.
        port: Port,
    },
    /// Open the operation.
    ShowOperation(OperationId),
    /// Open the storage settings.
    OpenStorageSettings,
}

/// Why a root is not private.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum RootProblem {
    /// The root is not there.
    Missing,
    /// The root is not a folder.
    NotDirectory,
    /// The root belongs to another user.
    ForeignOwner,
    /// Group or other may write to it.
    GroupOrOtherWritable,
}

/// What is wrong with a path of an instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PathProblem {
    /// One of the instance folders is not a folder.
    NotDirectory,
    /// A folder lacks the owner's write or search bit.
    NotWritable,
    /// The entry belongs to another user.
    ForeignOwner,
}

/// Why a port is refused (spec 0029, AC-14).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PortProblemKind {
    /// Another endpoint of the same instance has the port.
    Sibling,
    /// Another instance has the port in its configuration.
    Instance(InstanceRef),
    /// The trial bind was not free.
    Bind(BindOutcome),
}

/// Who holds a port, as far as this user can see.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PortOwnerFacts {
    /// A process of this user that is no Stackly instance.
    Process(ListenerOwner),
    /// The recorded process of another instance, start time included.
    StacklyInstance {
        /// The process.
        owner: ListenerOwner,
        /// The instance.
        instance: InstanceRef,
    },
    /// No process of this user holds it.
    Unknown,
}

/// Why a `processes` finding was made (spec 0034, *Hasil check processes*).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum IdentityReason {
    /// A stored `dataInUse` or `identityUnproven` blocker.
    Blocker,
    /// The record is from before this Mac last restarted.
    BootChanged,
    /// The recorded process is gone.
    ProcessGone,
    /// The PID now belongs to another process.
    PidReused,
    /// The process matches but does not hold the instance lock.
    LockNotHeld,
    /// The process is suspended.
    Suspended,
}

/// The facts behind a `processes` finding.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct IdentityFacts {
    /// Which row of the table matched.
    pub reason: IdentityReason,
    /// The passive checks, with the lock state of the stored blocker.
    pub evidence: IdentityEvidence,
    /// The process the registry recorded.
    pub recorded: Option<ProcessIdentity>,
    /// The process that holds the PID now, when it could be read.
    pub current: Option<ProcessFacts>,
    /// True when a process of another user holds the PID now.
    pub current_other_user: bool,
    /// The user that runs the daemon, to tell another user's process.
    pub uid: Option<UserId>,
    /// When the evidence was gathered: the blocker's time, or now.
    pub observed_at: Timestamp,
}

/// What `start_failed` shows: the instance and its status as the
/// supervisor holds them now, the source of `lastFailure` (spec 0033).
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct FailureSnapshot {
    /// The instance.
    pub instance: Instance,
    /// Its status.
    pub status: InstanceStatus,
    /// The redacted output of its last start.
    pub excerpt: Vec<String>,
}

/// The facts behind a finding, by code.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum FindingFacts {
    /// `check_unavailable`.
    Unavailable {
        /// Why.
        reason: CheckUnavailableReason,
        /// The redacted error, if any.
        message: Option<String>,
    },
    /// `subsystem_degraded`.
    Subsystem(DegradedSubsystem),
    /// `recovery_mode`.
    Recovery {
        /// The reason code.
        reason: String,
        /// The message.
        message: String,
    },
    /// `quick_check_failed`: at most 10 messages.
    QuickCheck {
        /// The messages of `PRAGMA quick_check`.
        messages: Vec<String>,
    },
    /// `unrecognized_value`.
    Unrecognized(UnrecognizedValue),
    /// `root_not_private`.
    Root {
        /// Which root.
        root: RootKind,
        /// Its path.
        path: PathBuf,
        /// What is wrong.
        problem: RootProblem,
        /// Its mode bits, when read.
        mode: Option<u32>,
    },
    /// `path_not_writable` and `path_foreign_owner`.
    Path {
        /// The path relative to the instance folder, such as `data/base`.
        path: String,
        /// What is wrong.
        problem: PathProblem,
        /// Its mode bits, when read.
        mode: Option<u32>,
    },
    /// `volume_filling`, `volume_low`, and `volume_critical`.
    Capacity(DiskReading),
    /// `runtime_state` and `runtime_files_missing`.
    Runtime {
        /// The runtime.
        runtime: Box<Runtime>,
        /// Its install folder.
        folder: PathBuf,
        /// The instances that use it, by name.
        used_by: Vec<InstanceName>,
        /// For `runtime_files_missing`: the folder (empty) or the
        /// executables, relative to it, that are not in place.
        missing: Vec<String>,
    },
    /// `package_unreadable`.
    Package {
        /// The pinned package.
        key: ProviderRevisionKey,
        /// Why it does not load, redacted.
        reason: String,
        /// The instances that pin it, by name.
        used_by: Vec<InstanceName>,
    },
    /// `port_in_use`.
    Port {
        /// The endpoint.
        endpoint: EndpointName,
        /// TCP or UDP.
        transport: Transport,
        /// The address it binds.
        address: IpAddr,
        /// The port.
        port: Port,
        /// Why it is refused.
        problem: PortProblemKind,
        /// The first free port after it.
        suggestion: Option<Port>,
        /// Who holds it.
        owner: PortOwnerFacts,
    },
    /// The `processes` findings.
    Identity(Box<IdentityFacts>),
    /// `data_missing` and `runtime_missing`.
    Missing,
    /// `start_failed`.
    Failure(Box<FailureSnapshot>),
    /// `not_ready`.
    NotReady {
        /// The readiness reason code.
        reason: String,
        /// Its detail, redacted.
        detail: Option<String>,
    },
    /// `config_drift`.
    Drift {
        /// The changed files, relative to `config/`.
        files: Vec<String>,
    },
    /// `secret_missing`.
    Secret {
        /// The reference.
        reference: SecretRef,
        /// Why it is missing.
        reason: NeedsSecretReason,
        /// The field of the instance that uses it, when known.
        field: Option<String>,
    },
    /// `needs_recovery`.
    NeedsRecovery {
        /// The operation.
        operation: Box<Operation>,
        /// What stopped it.
        cause: Option<RecoveryCause>,
        /// The code of the error behind it.
        error_code: Option<String>,
        /// The path of a `runtime.install_conflict`.
        path: Option<String>,
    },
    /// `scratch_leftover`.
    Scratch {
        /// The folder name in `<data>/operations/`.
        name: String,
    },
}
