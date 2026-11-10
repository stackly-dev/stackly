//! `stackly doctor` (spec 0034, *CLI*, AC-15, AC-18): the client checks
//! `connection`, `protocol`, and `background_agent` first, then
//! `doctor.run` when a daemon answered and speaks this protocol. The doctor
//! changes nothing: the daemon's checks are read only, the client checks
//! only read, and every action is written as the command you can run
//! yourself.
//!
//! Exit 0 without an Error finding (Warnings alone are 0), and
//! `cli.doctor_found_errors` (exit 7) with the result kept when there is
//! one. A daemon that is not reachable or not compatible ends with its own
//! error and exit instead, with `result.local` filled and `result.report`
//! null.

pub mod local;
mod render;

use clap::{Args, ValueEnum};
use serde::Serialize;
use stackly_protocol::doctor::{
    DoctorCheck, DoctorCheckId, DoctorParams, DoctorReport, DoctorSeverity,
};
use stackly_protocol::{Call, ProtocolVersion, Reply};

use self::local::Reach;
use crate::commands::RunCommand;
use crate::commands::instance::resolve::InstanceTarget;
use crate::context::Context;
use crate::failure::CliFailure;
use crate::output::CommandOutput;
use crate::output::envelope::warning;
use crate::progress::ProgressMode;
use crate::session::{CallSpec, RequestTimeout};

/// The progress line while the doctor runs.
pub const RUNNING: &str = "Running the doctor…";

/// `stackly doctor`.
#[derive(Debug, Args)]
pub struct DoctorArgs {
    /// Limit the checks about instances to this one, its name or its whole ID; the checks of this Mac still run
    #[arg(long, value_name = "INSTANCE")]
    pub instance: Option<InstanceTarget>,
    /// Run only this check of the daemon; repeat it for more. The connection, protocol, and background agent checks always run
    #[arg(long = "check", value_name = "CHECK", value_enum)]
    pub checks: Vec<CheckArg>,
}

/// The checks of the daemon `--check` takes, by their snake_case ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "snake_case")]
pub enum CheckArg {
    /// stacklyd's own subsystems
    Daemon,
    /// The registry file
    Registry,
    /// The roots and the instance folders
    Permissions,
    /// The volume that holds the data root
    DiskSpace,
    /// The managed runtimes
    Runtimes,
    /// The pinned provider packages instances use
    ProviderPackages,
    /// Ports of instances that are not running
    Endpoints,
    /// Recorded processes and their identity
    Processes,
    /// The data folder and runtime of each instance
    InstanceFiles,
    /// Instances that failed or are not ready
    InstanceFailures,
    /// Rendered config that differs from its revision
    ConfigDrift,
    /// Secrets an instance needs and cannot find
    Secrets,
    /// Operations that need recovery and scratch folders left behind
    Operations,
}

impl CheckArg {
    /// The check on the wire.
    #[must_use]
    pub const fn id(self) -> DoctorCheckId {
        match self {
            Self::Daemon => DoctorCheckId::Daemon,
            Self::Registry => DoctorCheckId::Registry,
            Self::Permissions => DoctorCheckId::Permissions,
            Self::DiskSpace => DoctorCheckId::DiskSpace,
            Self::Runtimes => DoctorCheckId::Runtimes,
            Self::ProviderPackages => DoctorCheckId::ProviderPackages,
            Self::Endpoints => DoctorCheckId::Endpoints,
            Self::Processes => DoctorCheckId::Processes,
            Self::InstanceFiles => DoctorCheckId::InstanceFiles,
            Self::InstanceFailures => DoctorCheckId::InstanceFailures,
            Self::ConfigDrift => DoctorCheckId::ConfigDrift,
            Self::Secrets => DoctorCheckId::Secrets,
            Self::Operations => DoctorCheckId::Operations,
        }
    }
}

/// The `stackly doctor` result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorResult {
    /// The client checks, `connection`, `protocol`, and `background_agent`.
    pub local: Vec<DoctorCheck>,
    /// The daemon's report; null when no daemon answered, it speaks another
    /// protocol, or `doctor.run` failed.
    pub report: Option<DoctorReport>,
}

impl DoctorResult {
    /// The findings with severity `severity`: those of the client checks
    /// plus the report's count, which includes the omitted ones.
    #[must_use]
    pub fn count(&self, severity: DoctorSeverity) -> u64 {
        let local = self
            .local
            .iter()
            .flat_map(|check| &check.findings)
            .filter(|finding| finding.severity == severity)
            .count() as u64;
        let report = self.report.as_ref().map_or(0, |report| match severity {
            DoctorSeverity::Error => report.errors.get(),
            DoctorSeverity::Warning => report.warnings.get(),
            _ => 0,
        });
        local.saturating_add(report)
    }
}

impl RunCommand for DoctorArgs {
    type Output = DoctorResult;

    async fn run(self, cx: &mut Context) -> CommandOutput<DoctorResult> {
        show_progress(cx);
        let agent = cx.agent.clone();
        let (_, assessment) = agent.status().await;
        let handshake = cx.handshake().await;
        let reach = match &handshake {
            Ok(accepted) => Reach::Accepted(accepted),
            Err(failure) => Reach::Failed(&failure.body),
        };
        let gathered = local::gather(&agent, &assessment, reach);
        let socket = cx.connector().socket.clone();
        let local = local::checks(reach, &socket, &gathered.facts());
        let mut result = DoctorResult {
            local,
            report: None,
        };
        let accepted = match handshake {
            Ok(accepted) => accepted,
            Err(failure) => return CommandOutput::partial(result, failure),
        };
        if accepted.contract_drift {
            cx.warn(
                warning::CONTRACT_DRIFT,
                format!(
                    "the daemon has another contract digest of protocol {}.{}; update stackly or stacklyd so both match",
                    ProtocolVersion::CURRENT.major,
                    ProtocolVersion::CURRENT.minor
                ),
            );
        }
        let instance_id = match &self.instance {
            Some(target) => match target.resolve(cx).await {
                Ok(id) => Some(id),
                Err(failure) => return CommandOutput::partial(result, failure),
            },
            None => None,
        };
        let params = DoctorParams {
            instance_id,
            checks: self.check_ids(),
        };
        // A run may wait 15 seconds for a slot and then run 15 seconds
        // more (spec 0034 AC-1), past the 10 seconds of a control call.
        let spec = CallSpec {
            timeout: RequestTimeout::Interactive,
            idempotency_key: None,
        };
        match cx.call(Call::DoctorRun(params), spec).await {
            Ok(Reply::DoctorRun(report)) => result.report = Some(*report),
            Ok(_) => return CommandOutput::partial(result, CliFailure::unexpected_reply(None)),
            Err(failure) => return CommandOutput::partial(result, failure),
        }
        let errors = result.count(DoctorSeverity::Error);
        if errors == 0 {
            return CommandOutput::ok(result);
        }
        let warnings = result.count(DoctorSeverity::Warning);
        CommandOutput::partial(result, CliFailure::doctor_found_errors(errors, warnings))
    }
}

impl DoctorArgs {
    /// The daemon checks `--check` named, each once, in the order given.
    fn check_ids(&self) -> Vec<DoctorCheckId> {
        let mut ids = Vec::with_capacity(self.checks.len());
        for id in self.checks.iter().map(|check| check.id()) {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
        ids
    }
}

/// Writes the progress line on stderr: redrawn in place on a terminal, one
/// plain line elsewhere, nothing with `--quiet`. Writing the result clears
/// a live line.
fn show_progress(cx: &mut Context) {
    match cx.progress {
        ProgressMode::Live => cx.output.stderr.live(RUNNING),
        ProgressMode::Lines => cx.output.stderr.line(RUNNING),
        ProgressMode::Off => {}
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    #[derive(Debug, Parser)]
    struct Wrapper {
        #[command(flatten)]
        args: DoctorArgs,
    }

    #[test]
    fn check_takes_every_daemon_check_by_its_snake_case_id_and_no_client_check() {
        for arg in CheckArg::value_variants() {
            let name = arg
                .to_possible_value()
                .expect("every check has a name")
                .get_name()
                .to_owned();
            let wire = serde_json::to_value(arg.id()).unwrap();
            assert_eq!(wire, serde_json::Value::String(name.clone()), "{arg:?}");
            assert!(arg.id().runs_in_daemon(), "{name}");
        }
        assert_eq!(
            CheckArg::value_variants().len(),
            stackly_protocol::doctor::DOCTOR_CHECKS_MAX
        );
        for client in [
            "connection",
            "protocol",
            "background_agent",
            "unrecognized",
            "disk-space",
        ] {
            assert!(
                Wrapper::try_parse_from(["doctor", "--check", client]).is_err(),
                "{client}"
            );
        }
    }

    #[test]
    fn a_check_named_twice_is_sent_once_in_the_order_given() {
        let wrapper = Wrapper::try_parse_from([
            "doctor",
            "--check",
            "endpoints",
            "--check",
            "disk_space",
            "--check",
            "endpoints",
        ])
        .unwrap();

        assert_eq!(
            wrapper.args.check_ids(),
            [DoctorCheckId::Endpoints, DoctorCheckId::DiskSpace]
        );
    }
}
