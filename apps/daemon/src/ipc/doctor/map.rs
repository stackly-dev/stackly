//! The doctor report on the wire: core findings to DTOs, with their text
//! from [`super::copy`], every free text redacted, every path in the home
//! folder written `~`, and every field held to its limit.

use std::collections::BTreeMap;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use stackly_core::doctor::{
    Action, CheckReport, DiskReading, Finding, FindingFacts, IdentityFacts, InstanceRef,
    PortOwnerFacts, PortProblemKind, Report,
};
use stackly_core::secret::Redactor;
use stackly_domain::doctor::{
    DETAIL_KEY_BYTES_MAX, DETAIL_VALUE_BYTES_MAX, DETAILS_MAX, DoctorCheckId as Check,
    DoctorFindingCode as Code, DoctorSeverity as Severity, EXCERPT_LINE_CHARS_MAX,
    EXCERPT_LINES_MAX, EXPLANATION_CHARS_MAX, SUMMARY_CHARS_MAX, TITLE_CHARS_MAX, clip_bytes,
    clip_chars,
};
use stackly_domain::{CapacityLevel, ProcessStart};
use stackly_platform::{ListenerOwner, ProcessFacts};
use stackly_protocol::Timestamp;
use stackly_protocol::daemon::DaemonInfo;
use stackly_protocol::doctor::{
    DoctorAction, DoctorActionKind, DoctorCheck, DoctorCheckId, DoctorDetail, DoctorEvidence,
    DoctorFinding, DoctorFindingCode, DoctorInstanceRef, DoctorReport, DoctorSeverity, PortOwner,
    PortOwnerKind, ProcessRef,
};
use stackly_protocol::storage::{CapacityLevel as WireLevel, CapacityReport};
use stackly_protocol::wire::Count;

use super::copy;
use crate::ipc::instance::dto;
use crate::state::DaemonCore;

/// What the text of one finding is built from.
pub(super) struct Shown<'a> {
    pub(super) code: Code,
    pub(super) facts: &'a FindingFacts,
    /// The instance name.
    pub(super) instance: Option<&'a str>,
    /// What the finding is about when no instance is: a runtime, a
    /// package, or a subsystem.
    pub(super) label: Option<String>,
    /// The path of the finding, with `~`.
    pub(super) path: Option<String>,
    /// The name of the process that holds a port or a PID now.
    pub(super) owner_name: Option<String>,
    /// Its executable, with `~`.
    pub(super) owner_path: Option<String>,
    /// The `lastFailure.code` of a `start_failed`.
    pub(super) failure_code: Option<String>,
    /// True when the process that holds a recorded PID now belongs to
    /// another user.
    pub(super) other_user: bool,
}

/// Maps a run to the reply of `doctor.run`.
pub(super) fn report(core: &DaemonCore, report: &Report) -> DoctorReport {
    let info = core.info();
    let redactor = core.redactor();
    DoctorReport {
        run_id: report.run_id,
        started_at: Timestamp::from_jiff(report.started_at),
        finished_at: Timestamp::from_jiff(report.finished_at),
        recovery: report.recovery,
        scope: report.scope.as_ref().map(instance_ref),
        checks: report
            .checks
            .iter()
            .map(|check| check_dto(core, redactor, &info, check))
            .collect(),
        errors: Count::saturating(report.tally.errors),
        warnings: Count::saturating(report.tally.warnings),
    }
}

fn check_dto(
    core: &DaemonCore,
    redactor: &Redactor,
    info: &DaemonInfo,
    check: &CheckReport,
) -> DoctorCheck {
    let summary = clip_chars(
        &redactor.redact(&copy::summary(check, info)),
        SUMMARY_CHARS_MAX,
    );
    DoctorCheck {
        id: check_id(check.id),
        severity: severity(check.severity),
        title: copy::check_title(check.id).to_owned(),
        summary: if summary.is_empty() {
            "No issues.".to_owned()
        } else {
            summary
        },
        findings: check
            .findings
            .iter()
            .map(|finding| finding_dto(core, redactor, finding))
            .collect(),
        omitted: Count::saturating(check.omitted),
        capacity: check.capacity.as_ref().map(capacity_dto),
    }
}

fn finding_dto(core: &DaemonCore, redactor: &Redactor, finding: &Finding) -> DoctorFinding {
    let failure = failure_of(core, finding);
    let shown = shown(finding, failure.as_ref());
    let title = clip_chars(
        &redactor.redact(&copy::finding_title(&shown)),
        TITLE_CHARS_MAX,
    );
    let explanation = clip_chars(
        &redactor.redact(&copy::explanation(&shown)),
        EXPLANATION_CHARS_MAX,
    );
    let mut details = details(&shown, failure.as_ref());
    details.truncate(DETAILS_MAX);
    let excerpt = failure
        .as_ref()
        .map(|failure| failure.excerpt.as_slice())
        .unwrap_or_default()
        .iter()
        .take(EXCERPT_LINES_MAX)
        .map(|line| clip_chars(&redactor.redact(line), EXCERPT_LINE_CHARS_MAX))
        .chain(quick_check_lines(&finding.facts, redactor))
        .take(EXCERPT_LINES_MAX)
        .collect();
    DoctorFinding {
        code: finding_code(finding.code),
        severity: severity(finding.severity),
        title,
        explanation,
        subject: finding
            .subject
            .as_deref()
            .map(|subject| clip_bytes(&redactor.redact(&tilde(subject)), DETAIL_VALUE_BYTES_MAX)),
        instance: finding.instance.as_ref().map(instance_ref),
        operation_id: match &finding.facts {
            FindingFacts::NeedsRecovery { operation, .. } => Some(operation.id().as_uuid()),
            _ => None,
        },
        details: details
            .into_iter()
            .map(|(key, value)| DoctorDetail {
                key: clip_bytes(key, DETAIL_KEY_BYTES_MAX),
                value: clip_bytes(&redactor.redact(&value), DETAIL_VALUE_BYTES_MAX),
            })
            .collect(),
        evidence: match &finding.facts {
            FindingFacts::Identity(facts) => Some(evidence(facts)),
            _ => None,
        },
        port_owner: match &finding.facts {
            FindingFacts::Port { owner, .. } => Some(port_owner(owner)),
            _ => None,
        },
        excerpt,
        remedy: None,
        action: finding.action.as_ref().map(action),
    }
}

/// The messages of a failed `quick_check`, shown as the excerpt.
fn quick_check_lines<'a>(
    facts: &'a FindingFacts,
    redactor: &'a Redactor,
) -> impl Iterator<Item = String> + 'a {
    let messages: &[String] = match facts {
        FindingFacts::QuickCheck { messages } => messages,
        _ => &[],
    };
    messages
        .iter()
        .map(move |line| clip_chars(&redactor.redact(line), EXCERPT_LINE_CHARS_MAX))
}

/// The `lastFailure` of a `start_failed`, built by the same function as the
/// instance summary (spec 0033), so both show the same values.
fn failure_of(
    core: &DaemonCore,
    finding: &Finding,
) -> Option<stackly_protocol::instance::InstanceLastFailure> {
    let FindingFacts::Failure(snapshot) = &finding.facts else {
        return None;
    };
    let excerpt = snapshot
        .excerpt
        .iter()
        .map(|line| dto::excerpt_line(line))
        .collect();
    let id = snapshot.instance.id();
    dto::summary(
        &snapshot.instance,
        &snapshot.status,
        excerpt,
        &BTreeMap::new(),
        core.supervision().autostart().get(id),
    )
    .last_failure
}

fn shown<'a>(
    finding: &'a Finding,
    failure: Option<&stackly_protocol::instance::InstanceLastFailure>,
) -> Shown<'a> {
    let mut shown = Shown {
        code: finding.code,
        facts: &finding.facts,
        instance: finding
            .instance
            .as_ref()
            .map(|instance| instance.name.as_str()),
        label: None,
        path: None,
        owner_name: None,
        owner_path: None,
        failure_code: failure.map(|failure| failure.code.clone()),
        other_user: false,
    };
    match &finding.facts {
        FindingFacts::Subsystem(subsystem) => shown.label = Some(subsystem.name.clone()),
        FindingFacts::Root { root, path, .. } => {
            shown.label = Some(format!("{root} root"));
            shown.path = Some(tilde_path(path));
        }
        FindingFacts::Runtime {
            runtime, folder, ..
        } => {
            shown.label = Some(format!("{} {}", runtime.software(), runtime.version()));
            shown.path = Some(tilde_path(folder));
        }
        FindingFacts::Package { key, .. } => {
            shown.label = Some(format!("{} {}", key.provider(), key.revision()));
        }
        FindingFacts::Port { owner, .. } => {
            let process = match owner {
                PortOwnerFacts::Process(process)
                | PortOwnerFacts::StacklyInstance { owner: process, .. } => Some(process),
                _ => None,
            };
            if let Some(executable) = process.and_then(|process| process.executable.as_ref()) {
                shown.owner_name = executable
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned());
                shown.owner_path = Some(tilde_path(executable));
            }
        }
        FindingFacts::Identity(facts) => {
            shown.other_user = same_user(facts) == Some(false);
            if let Some(current) = facts.current.as_ref().filter(|_| !shown.other_user) {
                shown.owner_path = Some(tilde_path(&current.executable));
            }
        }
        _ => {}
    }
    shown
}

/// The details of a finding, by key; never a secret value.
fn details(
    shown: &Shown<'_>,
    failure: Option<&stackly_protocol::instance::InstanceLastFailure>,
) -> Vec<(&'static str, String)> {
    let mut details: Vec<(&'static str, String)> = Vec::new();
    match shown.facts {
        FindingFacts::Unavailable { reason, .. } => {
            details.push(("reason", reason.code().to_owned()))
        }
        FindingFacts::Subsystem(subsystem) => {
            details.push(("subsystem", subsystem.name.clone()));
            details.push(("status", subsystem.status.clone()));
        }
        FindingFacts::Recovery { reason, .. } => details.push(("reason", reason.clone())),
        FindingFacts::QuickCheck { messages } => {
            details.push(("messages", messages.len().to_string()));
        }
        FindingFacts::Unrecognized(value) => {
            details.push(("table", value.table.to_owned()));
            details.push(("rowId", value.row_id.to_string()));
            details.push(("column", value.column.to_owned()));
            details.push(("value", value.value.clone()));
        }
        FindingFacts::Root {
            root,
            problem,
            mode,
            ..
        } => {
            details.push(("root", root.to_string()));
            if let Some(path) = &shown.path {
                details.push(("path", path.clone()));
            }
            details.push(("problem", format!("{problem:?}").to_lowercase()));
            if let Some(mode) = mode {
                details.push(("mode", format!("{:04o}", mode & 0o7777)));
            }
        }
        FindingFacts::Path { path, mode, .. } => {
            details.push(("path", path.clone()));
            if let Some(mode) = mode {
                details.push(("mode", format!("{:04o}", mode & 0o7777)));
            }
        }
        FindingFacts::Capacity(reading) => {
            details.push(("usedPercent", reading.used_percent.to_string()));
            details.push(("availableBytes", reading.available_bytes.to_string()));
            details.push(("totalBytes", reading.total_bytes.to_string()));
        }
        FindingFacts::Runtime {
            runtime,
            used_by,
            missing,
            ..
        } => {
            details.push(("runtimeId", runtime.id().to_string()));
            details.push(("software", runtime.software().to_string()));
            details.push(("version", runtime.version().to_string()));
            details.push(("state", runtime.state().to_string()));
            if let Some(path) = &shown.path {
                details.push(("folder", path.clone()));
            }
            if !used_by.is_empty() {
                details.push((
                    "usedBy",
                    used_by
                        .iter()
                        .map(|name| name.as_str())
                        .collect::<Vec<_>>()
                        .join(", "),
                ));
            }
            let files: Vec<&str> = missing
                .iter()
                .map(String::as_str)
                .filter(|file| !file.is_empty())
                .collect();
            if !files.is_empty() {
                details.push(("missing", files.join(", ")));
            }
        }
        FindingFacts::Package {
            key,
            reason,
            used_by,
        } => {
            details.push(("provider", key.provider().to_string()));
            details.push(("revision", key.revision().to_string()));
            details.push(("reason", reason.clone()));
            details.push((
                "usedBy",
                used_by
                    .iter()
                    .map(|name| name.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
            ));
        }
        FindingFacts::Port {
            endpoint,
            transport,
            address,
            port,
            problem,
            suggestion,
            ..
        } => {
            details.push(("endpoint", endpoint.to_string()));
            details.push(("address", address.to_string()));
            details.push(("port", port.get().to_string()));
            details.push(("transport", transport.to_string()));
            details.push((
                "problem",
                match problem {
                    PortProblemKind::Sibling => "sibling".to_owned(),
                    PortProblemKind::Instance(_) => "instance".to_owned(),
                    PortProblemKind::Bind(outcome) => outcome.as_code().to_owned(),
                    _ => "unrecognized".to_owned(),
                },
            ));
            if let PortProblemKind::Instance(other) = problem {
                details.push(("conflictsWith", other.name.to_string()));
            }
            if let Some(suggested) = suggestion {
                details.push(("suggestedPort", suggested.get().to_string()));
            }
        }
        FindingFacts::Identity(facts) => {
            if let Some(recorded) = &facts.recorded {
                details.push(("pid", recorded.pid.get().to_string()));
            }
        }
        FindingFacts::Failure(_) => {
            if let Some(failure) = failure {
                details.push(("failureCode", failure.code.clone()));
                if let Some(at) = failure.at {
                    details.push(("at", at.to_string()));
                }
                if let Some(code) = failure.exit_code {
                    details.push(("exitCode", code.to_string()));
                }
                if let Some(signal) = failure.signal {
                    details.push(("signal", signal.to_string()));
                }
            }
        }
        FindingFacts::NotReady { reason, detail } => {
            details.push(("reason", reason.clone()));
            if let Some(detail) = detail {
                details.push(("detail", detail.clone()));
            }
        }
        FindingFacts::Drift { files } => details.push(("files", files.join(", "))),
        FindingFacts::Secret {
            reference,
            reason,
            field,
        } => {
            if let Some(field) = field {
                details.push(("field", field.clone()));
            }
            details.push(("reference", reference.to_string()));
            details.push(("reason", reason.to_string()));
        }
        FindingFacts::NeedsRecovery {
            operation,
            cause,
            error_code,
            path,
        } => {
            details.push(("operationId", operation.id().to_string()));
            details.push(("kind", operation.kind().to_string()));
            if let Some(hint) = operation.recovery_hint() {
                details.push(("recoveryHint", hint.to_string()));
            }
            if let Some(cause) = cause {
                details.push(("recoveryCause", cause.to_string()));
            }
            if let Some(code) = error_code {
                details.push(("errorCode", code.clone()));
            }
            if let Some(path) = path {
                details.push(("path", tilde(path)));
            }
        }
        FindingFacts::Scratch { name } => details.push(("path", format!("operations/{name}"))),
        _ => {}
    }
    details
}

/// The evidence of a `processes` finding: the passive checks, the recorded
/// process, and the one that holds the PID now. A process of another user
/// shows no executable.
fn evidence(facts: &IdentityFacts) -> DoctorEvidence {
    let same = same_user(facts);
    DoctorEvidence {
        identity: dto::evidence_dto(&facts.evidence),
        recorded: facts.recorded.as_ref().map(|recorded| ProcessRef {
            pid: recorded.pid.get(),
            started_at: started_at(recorded.start),
            executable: Some(tilde_path(&recorded.executable)),
            same_user: Some(true),
        }),
        current: if facts.current_other_user {
            facts.recorded.as_ref().map(|recorded| ProcessRef {
                pid: recorded.pid.get(),
                started_at: None,
                executable: None,
                same_user: Some(false),
            })
        } else {
            facts
                .current
                .as_ref()
                .map(|current| current_ref(current, same))
        },
        observed_at: Timestamp::from_jiff(facts.observed_at),
    }
}

fn same_user(facts: &IdentityFacts) -> Option<bool> {
    if facts.current_other_user {
        return Some(false);
    }
    match (&facts.current, &facts.uid) {
        (Some(current), Some(uid)) => Some(current.uid == *uid),
        _ => None,
    }
}

fn current_ref(current: &ProcessFacts, same: Option<bool>) -> ProcessRef {
    ProcessRef {
        pid: current.pid.get(),
        started_at: started_at(current.start),
        executable: (same != Some(false)).then(|| tilde_path(&current.executable)),
        same_user: same,
    }
}

fn port_owner(owner: &PortOwnerFacts) -> PortOwner {
    match owner {
        PortOwnerFacts::Process(process) => PortOwner {
            kind: PortOwnerKind::Process,
            process: Some(listener_ref(process)),
            instance: None,
        },
        PortOwnerFacts::StacklyInstance { owner, instance } => PortOwner {
            kind: PortOwnerKind::StacklyInstance,
            process: Some(listener_ref(owner)),
            instance: Some(instance_ref(instance)),
        },
        _ => PortOwner {
            kind: PortOwnerKind::Unknown,
            process: None,
            instance: None,
        },
    }
}

fn listener_ref(owner: &ListenerOwner) -> ProcessRef {
    ProcessRef {
        pid: owner.pid.get(),
        started_at: owner.start.and_then(started_at),
        executable: owner.executable.as_deref().map(tilde_path),
        same_user: Some(true),
    }
}

fn started_at(start: ProcessStart) -> Option<Timestamp> {
    start.to_timestamp().map(Timestamp::from_jiff)
}

fn action(action: &Action) -> DoctorAction {
    let empty = DoctorAction {
        kind: DoctorActionKind::Unrecognized,
        instance_id: None,
        operation_id: None,
        endpoint: None,
        port: None,
    };
    match action {
        Action::ShowInstance(id) => DoctorAction {
            kind: DoctorActionKind::ShowInstance,
            instance_id: Some(id.as_uuid()),
            ..empty
        },
        Action::ReviewConfig(id) => DoctorAction {
            kind: DoctorActionKind::ReviewConfig,
            instance_id: Some(id.as_uuid()),
            ..empty
        },
        Action::ChangePort {
            instance,
            endpoint,
            port,
        } => DoctorAction {
            kind: DoctorActionKind::ChangePort,
            instance_id: Some(instance.as_uuid()),
            endpoint: Some(endpoint.to_string()),
            port: Some(port.get()),
            ..empty
        },
        Action::ShowOperation(id) => DoctorAction {
            kind: DoctorActionKind::ShowOperation,
            operation_id: Some(id.as_uuid()),
            ..empty
        },
        Action::OpenStorageSettings => DoctorAction {
            kind: DoctorActionKind::OpenStorageSettings,
            ..empty
        },
        _ => empty,
    }
}

fn instance_ref(instance: &InstanceRef) -> DoctorInstanceRef {
    DoctorInstanceRef {
        id: instance.id.as_uuid(),
        name: instance.name.to_string(),
    }
}

/// The reading of `disk_space` in the shape of `storage.get_capacity`.
fn capacity_dto(reading: &DiskReading) -> CapacityReport {
    CapacityReport {
        checked_at: Timestamp::from_jiff(reading.checked_at),
        total_bytes: Count::saturating(reading.total_bytes),
        available_bytes: Count::saturating(reading.available_bytes),
        used_percent: reading.used_percent,
        level: match reading.level {
            CapacityLevel::Ok => WireLevel::Ok,
            CapacityLevel::Filling => WireLevel::Filling,
            CapacityLevel::Low => WireLevel::Low,
            CapacityLevel::Critical => WireLevel::Critical,
            _ => WireLevel::Unrecognized,
        },
    }
}

/// `127.0.0.1:3307` or `[::1]:3307`.
pub(super) fn socket_text(address: IpAddr, port: u16) -> String {
    match address {
        IpAddr::V4(v4) => format!("{v4}:{port}"),
        IpAddr::V6(v6) => format!("[{v6}]:{port}"),
    }
}

/// The home folder, canonical, read once.
// provisional: F34 (spec 0034 step 9 reads HOME once at daemon start for
// the bundle; the doctor then takes it from there)
fn home() -> Option<&'static Path> {
    static HOME: OnceLock<Option<PathBuf>> = OnceLock::new();
    HOME.get_or_init(|| {
        let home = PathBuf::from(std::env::var_os("HOME")?);
        Some(std::fs::canonicalize(&home).unwrap_or(home))
    })
    .as_deref()
    .filter(|home| home.is_absolute() && home != &Path::new("/"))
}

/// `path` with the home folder written `~`.
pub(super) fn tilde_path(path: &Path) -> String {
    if let Some(home) = home()
        && let Ok(rest) = path.strip_prefix(home)
    {
        return if rest.as_os_str().is_empty() {
            "~".to_owned()
        } else {
            format!("~/{}", rest.display())
        };
    }
    path.display().to_string()
}

/// `text` with every occurrence of the home folder written `~`.
fn tilde(text: &str) -> String {
    match home().and_then(Path::to_str) {
        Some(home) => text.replace(home, "~"),
        None => text.to_owned(),
    }
}

/// The wire ID of a check.
pub(super) const fn check_id(check: Check) -> DoctorCheckId {
    match check {
        Check::Connection => DoctorCheckId::Connection,
        Check::Protocol => DoctorCheckId::Protocol,
        Check::BackgroundAgent => DoctorCheckId::BackgroundAgent,
        Check::Daemon => DoctorCheckId::Daemon,
        Check::Registry => DoctorCheckId::Registry,
        Check::Permissions => DoctorCheckId::Permissions,
        Check::DiskSpace => DoctorCheckId::DiskSpace,
        Check::Runtimes => DoctorCheckId::Runtimes,
        Check::ProviderPackages => DoctorCheckId::ProviderPackages,
        Check::Endpoints => DoctorCheckId::Endpoints,
        Check::Processes => DoctorCheckId::Processes,
        Check::InstanceFiles => DoctorCheckId::InstanceFiles,
        Check::InstanceFailures => DoctorCheckId::InstanceFailures,
        Check::ConfigDrift => DoctorCheckId::ConfigDrift,
        Check::Secrets => DoctorCheckId::Secrets,
        Check::Operations => DoctorCheckId::Operations,
    }
}

/// The domain ID of a wire check, or `None` for `unrecognized`.
pub(super) const fn domain_check(check: DoctorCheckId) -> Option<Check> {
    Some(match check {
        DoctorCheckId::Connection => Check::Connection,
        DoctorCheckId::Protocol => Check::Protocol,
        DoctorCheckId::BackgroundAgent => Check::BackgroundAgent,
        DoctorCheckId::Daemon => Check::Daemon,
        DoctorCheckId::Registry => Check::Registry,
        DoctorCheckId::Permissions => Check::Permissions,
        DoctorCheckId::DiskSpace => Check::DiskSpace,
        DoctorCheckId::Runtimes => Check::Runtimes,
        DoctorCheckId::ProviderPackages => Check::ProviderPackages,
        DoctorCheckId::Endpoints => Check::Endpoints,
        DoctorCheckId::Processes => Check::Processes,
        DoctorCheckId::InstanceFiles => Check::InstanceFiles,
        DoctorCheckId::InstanceFailures => Check::InstanceFailures,
        DoctorCheckId::ConfigDrift => Check::ConfigDrift,
        DoctorCheckId::Secrets => Check::Secrets,
        DoctorCheckId::Operations => Check::Operations,
        DoctorCheckId::Unrecognized => return None,
    })
}

const fn severity(severity: Severity) -> DoctorSeverity {
    match severity {
        Severity::Error => DoctorSeverity::Error,
        Severity::Warning => DoctorSeverity::Warning,
        Severity::Ok => DoctorSeverity::Ok,
    }
}

const fn finding_code(code: Code) -> DoctorFindingCode {
    match code {
        Code::DaemonUnreachable => DoctorFindingCode::DaemonUnreachable,
        Code::ProtocolIncompatible => DoctorFindingCode::ProtocolIncompatible,
        Code::AgentState => DoctorFindingCode::AgentState,
        Code::DaemonStartFailed => DoctorFindingCode::DaemonStartFailed,
        Code::BinaryReplaced => DoctorFindingCode::BinaryReplaced,
        Code::SubsystemDegraded => DoctorFindingCode::SubsystemDegraded,
        Code::RecoveryMode => DoctorFindingCode::RecoveryMode,
        Code::QuickCheckFailed => DoctorFindingCode::QuickCheckFailed,
        Code::UnrecognizedValue => DoctorFindingCode::UnrecognizedValue,
        Code::RootNotPrivate => DoctorFindingCode::RootNotPrivate,
        Code::PathNotWritable => DoctorFindingCode::PathNotWritable,
        Code::PathForeignOwner => DoctorFindingCode::PathForeignOwner,
        Code::VolumeFilling => DoctorFindingCode::VolumeFilling,
        Code::VolumeLow => DoctorFindingCode::VolumeLow,
        Code::VolumeCritical => DoctorFindingCode::VolumeCritical,
        Code::RuntimeState => DoctorFindingCode::RuntimeState,
        Code::RuntimeFilesMissing => DoctorFindingCode::RuntimeFilesMissing,
        Code::PackageUnreadable => DoctorFindingCode::PackageUnreadable,
        Code::PortInUse => DoctorFindingCode::PortInUse,
        Code::PidReused => DoctorFindingCode::PidReused,
        Code::DataInUse => DoctorFindingCode::DataInUse,
        Code::IdentityUnproven => DoctorFindingCode::IdentityUnproven,
        Code::ProcessSuspended => DoctorFindingCode::ProcessSuspended,
        Code::DataMissing => DoctorFindingCode::DataMissing,
        Code::RuntimeMissing => DoctorFindingCode::RuntimeMissing,
        Code::StartFailed => DoctorFindingCode::StartFailed,
        Code::NotReady => DoctorFindingCode::NotReady,
        Code::ConfigDrift => DoctorFindingCode::ConfigDrift,
        Code::SecretMissing => DoctorFindingCode::SecretMissing,
        Code::NeedsRecovery => DoctorFindingCode::NeedsRecovery,
        Code::ScratchLeftover => DoctorFindingCode::ScratchLeftover,
        Code::CheckUnavailable => DoctorFindingCode::CheckUnavailable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_daemon_check_maps_both_ways() {
        for check in stackly_domain::doctor::DoctorCheckId::ALL {
            assert_eq!(domain_check(check_id(*check)), Some(*check));
            let wire = serde_json::to_value(check_id(*check)).unwrap();
            assert_eq!(wire.as_str(), Some(check.code()));
        }
        assert_eq!(domain_check(DoctorCheckId::Unrecognized), None);
    }

    #[test]
    fn every_finding_code_keeps_its_wire_text() {
        for code in Code::ALL {
            let wire = serde_json::to_value(finding_code(*code)).unwrap();
            assert_eq!(wire.as_str(), Some(code.code()));
        }
        for level in [Severity::Error, Severity::Warning, Severity::Ok] {
            let wire = serde_json::to_value(severity(level)).unwrap();
            assert_eq!(wire.as_str(), Some(level.code()));
        }
    }

    #[test]
    fn sockets_read_like_urls() {
        assert_eq!(
            socket_text("127.0.0.1".parse().unwrap(), 3307),
            "127.0.0.1:3307"
        );
        assert_eq!(socket_text("::1".parse().unwrap(), 80), "[::1]:80");
    }
}
