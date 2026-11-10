//! The English text of the doctor (spec 0034): check titles, summary lines,
//! finding titles, and explanations, one source for the CLI and the GUI.
//! Every value that reaches a sentence was redacted, and every path in the
//! home folder is written `~`, before it gets here.

use stackly_core::doctor::{
    CheckReport, FindingFacts, IdentityReason, PathProblem, PortOwnerFacts, PortProblemKind,
    RootProblem, SummaryFacts,
};
use stackly_core::supervisor::BindOutcome;
use stackly_domain::CapacityLevel;
use stackly_domain::doctor::{CheckUnavailableReason, DoctorCheckId, DoctorFindingCode};
use stackly_protocol::daemon::{DaemonInfo, LaunchContextKind};

use super::map::Shown;

/// The title of a check.
pub(super) const fn check_title(check: DoctorCheckId) -> &'static str {
    match check {
        DoctorCheckId::Connection => "Connection",
        DoctorCheckId::Protocol => "Protocol",
        DoctorCheckId::BackgroundAgent => "Background agent",
        DoctorCheckId::Daemon => "stacklyd",
        DoctorCheckId::Registry => "Registry",
        DoctorCheckId::Permissions => "Permissions",
        DoctorCheckId::DiskSpace => "Disk space",
        DoctorCheckId::Runtimes => "Runtimes",
        DoctorCheckId::ProviderPackages => "Provider packages",
        DoctorCheckId::Endpoints => "Endpoints",
        DoctorCheckId::Processes => "Processes",
        DoctorCheckId::InstanceFiles => "Instance files",
        DoctorCheckId::InstanceFailures => "Start failures",
        DoctorCheckId::ConfigDrift => "Configuration revisions",
        DoctorCheckId::Secrets => "Secrets",
        DoctorCheckId::Operations => "Operations",
    }
}

/// The summary line of a check.
pub(super) fn summary(check: &CheckReport, info: &DaemonInfo) -> String {
    match &check.summary {
        SummaryFacts::Daemon if check.findings.is_empty() => {
            let launch = match info.launch.as_ref().map(|launch| launch.context) {
                Some(LaunchContextKind::Launchd) => "launchd",
                _ => "manual",
            };
            format!(
                "stacklyd {} · protocol {}.{} · {launch}",
                info.version, info.protocol.major, info.protocol.minor
            )
        }
        SummaryFacts::Registry {
            schema_version: Some(schema),
        } if check.findings.is_empty() => format!("quick_check passed · schema {schema}"),
        SummaryFacts::Registry {
            schema_version: None,
        } => "The registry is open in recovery mode.".to_owned(),
        SummaryFacts::Capacity(reading) => format!(
            "{}% used · {} free",
            reading.used_percent,
            bytes(reading.available_bytes)
        ),
        SummaryFacts::Unavailable(reason) => unavailable(*reason, None),
        SummaryFacts::Checked(checked) if check.findings.is_empty() => {
            ok_summary(check.id, *checked)
        }
        _ => tally(check),
    }
}

fn ok_summary(check: DoctorCheckId, checked: u64) -> String {
    match check {
        DoctorCheckId::Permissions => {
            "All Stackly folders are private and writable by you.".to_owned()
        }
        DoctorCheckId::Runtimes => format!(
            "{checked} {}. Every executable is in place.",
            plural(checked, "runtime", "runtimes")
        ),
        DoctorCheckId::ProviderPackages => "Every pinned package loads.".to_owned(),
        DoctorCheckId::Endpoints => format!(
            "No conflicts across {checked} configured {}.",
            plural(checked, "endpoint", "endpoints")
        ),
        DoctorCheckId::Processes => "Every recorded process matches its instance.".to_owned(),
        DoctorCheckId::InstanceFiles => {
            "Every instance has its data folder and runtime.".to_owned()
        }
        DoctorCheckId::InstanceFailures => "No instance has failed.".to_owned(),
        DoctorCheckId::ConfigDrift => "All effective configs match their revision.".to_owned(),
        DoctorCheckId::Secrets => "Every secret an instance needs is in the Keychain.".to_owned(),
        DoctorCheckId::Operations => "No operation needs recovery.".to_owned(),
        _ => "No issues.".to_owned(),
    }
}

/// `1 error`, `2 errors · 1 warning`, and the like, the omitted ones
/// included.
fn tally(check: &CheckReport) -> String {
    let mut parts = Vec::new();
    if check.tally.errors > 0 {
        parts.push(format!(
            "{} {}",
            check.tally.errors,
            plural(check.tally.errors, "error", "errors")
        ));
    }
    if check.tally.warnings > 0 {
        parts.push(format!(
            "{} {}",
            check.tally.warnings,
            plural(check.tally.warnings, "warning", "warnings")
        ));
    }
    if parts.is_empty() {
        "No issues.".to_owned()
    } else {
        parts.join(" · ")
    }
}

const fn plural<'a>(count: u64, one: &'a str, many: &'a str) -> &'a str {
    if count == 1 { one } else { many }
}

/// What a check that did not run says.
pub(super) fn unavailable(reason: CheckUnavailableReason, message: Option<&str>) -> String {
    match reason {
        CheckUnavailableReason::Timeout => "The check didn’t finish within 5 seconds.".to_owned(),
        CheckUnavailableReason::RecoveryMode => {
            "The check needs the registry, which is open in recovery mode.".to_owned()
        }
        CheckUnavailableReason::NotSupported => "This check can’t run on this Mac.".to_owned(),
        CheckUnavailableReason::Io => match message {
            Some(message) => format!("The check couldn’t read what it needs: {message}"),
            None => "The check couldn’t read what it needs.".to_owned(),
        },
    }
}

/// The title of a finding: the problem, then the instance (or what it is
/// about) after a `·`.
pub(super) fn finding_title(shown: &Shown<'_>) -> String {
    let problem = problem(shown);
    match (shown.instance, shown.label.as_deref()) {
        (Some(instance), _) => format!("{problem} · {instance}"),
        (None, Some(label)) => format!("{problem} · {label}"),
        (None, None) => problem,
    }
}

fn problem(shown: &Shown<'_>) -> String {
    let text = match shown.code {
        DoctorFindingCode::DaemonUnreachable => "stacklyd isn’t reachable",
        DoctorFindingCode::ProtocolIncompatible => "Protocol mismatch",
        DoctorFindingCode::AgentState => "Background agent needs you",
        DoctorFindingCode::DaemonStartFailed => "stacklyd failed to start",
        DoctorFindingCode::BinaryReplaced => "stacklyd may have been replaced",
        DoctorFindingCode::SubsystemDegraded => "Subsystem degraded",
        DoctorFindingCode::RecoveryMode => "Registry in recovery mode",
        DoctorFindingCode::QuickCheckFailed => "Registry integrity check failed",
        DoctorFindingCode::UnrecognizedValue => "Unknown value in the registry",
        DoctorFindingCode::RootNotPrivate => "Folder isn’t private",
        DoctorFindingCode::PathNotWritable => "Folder isn’t writable",
        DoctorFindingCode::PathForeignOwner => "Owned by another user",
        DoctorFindingCode::VolumeFilling => "Data volume is filling",
        DoctorFindingCode::VolumeLow => "Data volume is low on space",
        DoctorFindingCode::VolumeCritical => "Data volume is almost full",
        DoctorFindingCode::RuntimeState => match &shown.facts {
            FindingFacts::Runtime { runtime, .. } => {
                return format!("Runtime {}", runtime.state());
            }
            _ => "Runtime unavailable",
        },
        DoctorFindingCode::RuntimeFilesMissing => "Runtime files missing",
        DoctorFindingCode::PackageUnreadable => "Package doesn’t load",
        DoctorFindingCode::PortInUse => "Port conflict",
        DoctorFindingCode::PidReused => "PID reused",
        DoctorFindingCode::DataInUse => "Data folder in use",
        DoctorFindingCode::IdentityUnproven => "Process identity unproven",
        DoctorFindingCode::ProcessSuspended => "Process suspended",
        DoctorFindingCode::DataMissing => "Data missing",
        DoctorFindingCode::RuntimeMissing => "Runtime missing",
        DoctorFindingCode::StartFailed => "Start failed",
        DoctorFindingCode::NotReady => "Not ready",
        DoctorFindingCode::ConfigDrift => "Config changed outside Stackly",
        DoctorFindingCode::SecretMissing => "Secret missing",
        DoctorFindingCode::NeedsRecovery => "Operation needs recovery",
        DoctorFindingCode::ScratchLeftover => "Scratch folder left behind",
        DoctorFindingCode::CheckUnavailable => "Check unavailable",
    };
    text.to_owned()
}

/// The explanation of a finding, one to three sentences.
pub(super) fn explanation(shown: &Shown<'_>) -> String {
    let instance = shown.instance.unwrap_or("The instance");
    match &shown.facts {
        FindingFacts::Unavailable { reason, message } => unavailable(*reason, message.as_deref()),
        FindingFacts::Subsystem(subsystem) => match &subsystem.detail {
            Some(detail) => format!(
                "The {} subsystem of stacklyd is {}: {detail}",
                subsystem.name, subsystem.status
            ),
            None => format!(
                "The {} subsystem of stacklyd is {}.",
                subsystem.name, subsystem.status
            ),
        },
        FindingFacts::Recovery { message, .. } => format!(
            "{message} Stackly reads and writes nothing in the registry until it is restored."
        ),
        FindingFacts::QuickCheck { .. } => "SQLite’s quick_check found problems in the registry file. Copy the Stackly data folder somewhere safe before you change anything.".to_owned(),
        FindingFacts::Unrecognized(value) => format!(
            "Row {} of {} holds “{}” in {}, which this build of Stackly doesn’t know. A newer Stackly may have written it.",
            value.row_id, value.table, value.value, value.column
        ),
        FindingFacts::Root { problem, mode, .. } => {
            let path = shown.path.as_deref().unwrap_or("The folder");
            match problem {
                RootProblem::Missing => format!("{path} is missing."),
                RootProblem::NotDirectory => format!("{path} isn’t a folder."),
                RootProblem::ForeignOwner => format!("{path} belongs to another user."),
                RootProblem::GroupOrOtherWritable => format!(
                    "{path} has mode {}, so other users can write to it. Stackly expects it to be private to you.",
                    octal(*mode)
                ),
                _ => format!("{path} isn’t private to you."),
            }
        }
        FindingFacts::Path {
            path,
            problem,
            mode,
        } => match problem {
            PathProblem::ForeignOwner => format!(
                "{path} belongs to another user, so {instance} may fail to read or write it."
            ),
            PathProblem::NotDirectory => format!("{path} isn’t a folder."),
            PathProblem::NotWritable => format!(
                "{path} has mode {}, so you can’t write to it. {instance} may fail to start or to save data.",
                octal(*mode)
            ),
            _ => format!("{path} can’t be used by {instance}."),
        },
        FindingFacts::Capacity(reading) => {
            let next = match reading.level {
                CapacityLevel::Critical => "Instances may fail to write. Free some space now.",
                CapacityLevel::Low => "Free some space soon.",
                _ => "Keep an eye on it.",
            };
            format!(
                "The volume that holds your Stackly data is {}% full with {} free. {next}",
                reading.used_percent,
                bytes(reading.available_bytes)
            )
        }
        FindingFacts::Runtime {
            runtime,
            used_by,
            missing,
            ..
        } => {
            let users = names(used_by.iter().map(|name| name.as_str()));
            let what = if shown.code == DoctorFindingCode::RuntimeState {
                format!("The registry marks this runtime {}.", runtime.state())
            } else if missing.iter().any(String::is_empty) {
                format!(
                    "The runtime folder {} is gone.",
                    shown.path.as_deref().unwrap_or("of this runtime")
                )
            } else {
                format!(
                    "{} {} missing or can’t run.",
                    names(missing.iter().map(String::as_str)).unwrap_or_default(),
                    if missing.len() == 1 { "is" } else { "are" }
                )
            };
            match users {
                Some(users) => format!("{what} Used by {users}."),
                None => format!("{what} No instance uses it."),
            }
        }
        FindingFacts::Package {
            key,
            reason,
            used_by,
        } => format!(
            "The pinned package {} {} doesn’t load: {reason}. Pinned by {}.",
            key.provider(),
            key.revision(),
            names(used_by.iter().map(|name| name.as_str())).unwrap_or_default()
        ),
        FindingFacts::Port {
            address,
            port,
            problem,
            suggestion,
            owner,
            ..
        } => {
            let at = super::map::socket_text(*address, port.get());
            let what = match problem {
                PortProblemKind::Sibling => {
                    format!("Another endpoint of {instance} also uses port {}.", port.get())
                }
                PortProblemKind::Instance(other) => {
                    format!("Instance {} is set to port {} too.", other.name, port.get())
                }
                PortProblemKind::Bind(BindOutcome::Denied) => {
                    format!("macOS refused to bind {at}.")
                }
                PortProblemKind::Bind(BindOutcome::Unavailable) => {
                    format!("{at} isn’t available on this Mac.")
                }
                _ => match owner {
                    PortOwnerFacts::Process(process) => format!(
                        "{at} is held by {} (PID {}, {}). Not owned by Stackly.",
                        shown.owner_name.as_deref().unwrap_or("a process"),
                        process.pid.get(),
                        shown.owner_path.as_deref().unwrap_or("unknown executable")
                    ),
                    PortOwnerFacts::StacklyInstance { owner, instance } => format!(
                        "{at} is held by instance {} (PID {}).",
                        instance.name,
                        owner.pid.get()
                    ),
                    _ => format!(
                        "{at} is in use. No process of yours holds it; another user or the system may."
                    ),
                },
            };
            match suggestion {
                Some(free) => format!("{what} Port {} is free.", free.get()),
                None => what,
            }
        }
        FindingFacts::Identity(facts) => {
            let pid = facts
                .recorded
                .as_ref()
                .map_or_else(|| "The recorded PID".to_owned(), |recorded| {
                    format!("PID {}", recorded.pid.get())
                });
            match facts.reason {
                IdentityReason::PidReused if shown.other_user => {
                    format!("{pid} now belongs to a process of another user. Stackly leaves it alone.")
                }
                IdentityReason::PidReused => format!(
                    "{pid} now belongs to {}, not the process Stackly recorded. Stackly leaves both alone.",
                    shown.owner_path.as_deref().unwrap_or("another process")
                ),
                IdentityReason::BootChanged => {
                    "Recorded before this Mac last restarted.".to_owned()
                }
                IdentityReason::ProcessGone => "The recorded process is gone.".to_owned(),
                IdentityReason::LockNotHeld => {
                    "The process matches but doesn’t hold the instance lock.".to_owned()
                }
                IdentityReason::Suspended => format!(
                    "{pid} is suspended: the daemon that started it stopped before it resumed it."
                ),
                IdentityReason::Blocker if shown.code == DoctorFindingCode::DataInUse => {
                    "Another process holds the lock on the data folder. Stackly won’t start the instance until it stops.".to_owned()
                }
                _ => "Stackly can’t prove the recorded process belongs to this instance, so it never signals it.".to_owned(),
            }
        }
        FindingFacts::Missing if shown.code == DoctorFindingCode::DataMissing => format!(
            "The data folder of {instance} is gone or empty. Stackly won’t start it on missing data."
        ),
        FindingFacts::Missing => {
            format!("The runtime executable {instance} runs is gone.")
        }
        FindingFacts::Failure(_) => match &shown.failure_code {
            Some(code) => format!("{instance} failed: {code}."),
            None => "The last failure isn’t known since stacklyd restarted.".to_owned(),
        },
        FindingFacts::NotReady { reason, detail } => match detail {
            Some(detail) => format!("{instance} runs but isn’t ready ({reason}): {detail}"),
            None => format!("{instance} runs but isn’t ready ({reason})."),
        },
        FindingFacts::Drift { files } => format!(
            "{} in config/ {} from the last render: {}. Review before the next change.",
            files.len(),
            if files.len() == 1 { "file differs" } else { "files differ" },
            names(files.iter().map(String::as_str)).unwrap_or_default()
        ),
        FindingFacts::Secret {
            reference,
            reason,
            field,
        } => match field {
            Some(field) => format!(
                "The secret {reference} for {field} isn’t available ({reason})."
            ),
            None => format!("The secret {reference} isn’t available ({reason})."),
        },
        FindingFacts::NeedsRecovery {
            operation, cause, ..
        } => {
            let cause = cause.map_or_else(String::new, |cause| format!(" ({cause})"));
            format!(
                "The {} operation stopped{cause} and needs you to resume or abandon it.",
                operation.kind()
            )
        }
        FindingFacts::Scratch { name } => format!(
            "operations/{name} has no operation. Stackly leaves it in place."
        ),
        _ => "Stackly found a problem here.".to_owned(),
    }
}

/// `0755` style mode bits, or `unknown`.
fn octal(mode: Option<u32>) -> String {
    mode.map_or_else(
        || "unknown".to_owned(),
        |mode| format!("{:04o}", mode & 0o7777),
    )
}

/// `a, b, and c`, or `None` without names.
fn names<'a>(names: impl Iterator<Item = &'a str>) -> Option<String> {
    let names: Vec<&str> = names.collect();
    match names.as_slice() {
        [] => None,
        [one] => Some((*one).to_owned()),
        [first, second] => Some(format!("{first} and {second}")),
        [rest @ .., last] => Some(format!("{}, and {last}", rest.join(", "))),
    }
}

/// A byte count in decimal units, as Finder shows it.
pub(super) fn bytes(count: u64) -> String {
    const UNITS: [(u64, &str); 4] = [
        (1_000_000_000_000, "TB"),
        (1_000_000_000, "GB"),
        (1_000_000, "MB"),
        (1_000, "KB"),
    ];
    for (size, unit) in UNITS {
        if count >= size {
            let whole = count / size;
            let tenth = (count % size) / (size / 10);
            return if whole >= 10 || tenth == 0 {
                format!("{whole} {unit}")
            } else {
                format!("{whole}.{tenth} {unit}")
            };
        }
    }
    format!("{count} bytes")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_read_like_finder() {
        assert_eq!(bytes(0), "0 bytes");
        assert_eq!(bytes(999), "999 bytes");
        assert_eq!(bytes(41_000_000_000), "41 GB");
        assert_eq!(bytes(3_400_000_000), "3.4 GB");
        assert_eq!(bytes(2_000_000_000_000), "2 TB");
    }

    #[test]
    fn names_join_with_and() {
        assert_eq!(names([].into_iter()), None);
        assert_eq!(names(["a"].into_iter()).as_deref(), Some("a"));
        assert_eq!(names(["a", "b"].into_iter()).as_deref(), Some("a and b"));
        assert_eq!(
            names(["a", "b", "c"].into_iter()).as_deref(),
            Some("a, b, and c")
        );
    }

    #[test]
    fn every_daemon_check_has_its_own_title() {
        let titles: std::collections::BTreeSet<_> = stackly_domain::doctor::DAEMON_CHECKS
            .iter()
            .map(|check| check_title(*check))
            .collect();
        assert_eq!(titles.len(), stackly_domain::doctor::DAEMON_CHECKS.len());
    }
}
