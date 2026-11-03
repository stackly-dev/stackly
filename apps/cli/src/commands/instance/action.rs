//! `stackly instance start` and `stop` (spec 0014, AC-6; by name, spec 0022,
//! AC-15), and `restart`, `reload`, and `stop --force` (spec 0026, AC-13).

use clap::Args;
use stackly_protocol::instance::{
    ForceStopInstanceParams, ReloadInstanceParams, RestartInstanceParams, StartInstanceParams,
    StopInstanceParams,
};
use stackly_protocol::{Call, Reply};

use super::resolve::{InstanceTarget, TargetArg};
use super::stop::StopAll;
use super::{InstanceAction, Verb, accepted, conclude};
use crate::commands::RunCommand;
use crate::confirm::{Approval, Confirm, ConfirmRequest, ConfirmRole, OnNoninteractive};
use crate::context::Context;
use crate::failure::CliFailure;
use crate::output::{CommandOutput, OutputMode};
use crate::wait::{Accepted, AsyncArgs};

/// `stackly instance start`.
#[derive(Debug, Args)]
pub struct StartArgs {
    /// The instance.
    #[command(flatten)]
    pub instance: TargetArg,
    /// `--wait`, `--timeout`, and `--idempotency-key`.
    #[command(flatten)]
    pub run: AsyncArgs,
}

/// `stackly instance stop`. It never overrides a lease: stopping an
/// instance that an environment holds waits for the environment feature.
/// With `--all` it stops every service the preview shows, after asking.
#[derive(Debug, Args)]
#[command(group(clap::ArgGroup::new("confirms").args(["force", "all"])))]
pub struct StopArgs {
    /// The instance name, or its whole ID
    #[arg(value_name = "INSTANCE", required_unless_present = "all")]
    pub target: Option<InstanceTarget>,
    /// `--wait`, `--timeout`, and `--idempotency-key`.
    #[command(flatten)]
    pub run: AsyncArgs,
    /// Send SIGKILL to the main process of an instance that is still
    /// stopping after a normal stop; asks first
    #[arg(long, conflicts_with = "all")]
    pub force: bool,
    /// Stop every running service after showing the list and asking; the
    /// command waits for the end
    #[arg(long, conflicts_with_all = ["target", "wait"])]
    pub all: bool,
    /// Confirm --force or --all without asking; needed noninteractive and with --json
    #[arg(short = 'y', long, requires = "confirms")]
    pub yes: bool,
}

impl StopArgs {
    /// The command of `--all`, or the args back when it was not given.
    ///
    /// # Errors
    ///
    /// Returns `self` unchanged when `--all` was not given.
    pub fn into_all(self) -> Result<StopAll, Self> {
        if self.all {
            Ok(StopAll {
                yes: self.yes,
                idempotency_key: self.run.idempotency_key,
            })
        } else {
            Err(self)
        }
    }
}

/// `stackly instance restart`: one daemon operation that stops the
/// instance with its provider's strategy and starts it again.
#[derive(Debug, Args)]
pub struct RestartArgs {
    /// The instance.
    #[command(flatten)]
    pub instance: TargetArg,
    /// `--wait`, `--timeout`, and `--idempotency-key`.
    #[command(flatten)]
    pub run: AsyncArgs,
}

/// `stackly instance reload`: the provider reads its configuration again
/// without a restart, where it can.
#[derive(Debug, Args)]
pub struct ReloadArgs {
    /// The instance.
    #[command(flatten)]
    pub instance: TargetArg,
    /// `--wait`, `--timeout`, and `--idempotency-key`.
    #[command(flatten)]
    pub run: AsyncArgs,
}

/// What a force stop risks, the same text the app shows (spec 0026,
/// *Copy*).
pub const FORCE_RISK: &str = "Force stop sends SIGKILL to the main process. Unsaved work is lost and the next start may run crash recovery. Child processes may keep running and hold the data until they exit. The last stop is marked forced, so no backup will call this data clean.";

pub(super) fn extract_start(reply: Reply) -> Option<Accepted> {
    match reply {
        Reply::InstanceStart(reply) => Some(accepted(&reply)),
        _ => None,
    }
}

fn extract_stop(reply: Reply) -> Option<Accepted> {
    match reply {
        Reply::InstanceStop(reply) => Some(accepted(&reply)),
        _ => None,
    }
}

fn extract_force(reply: Reply) -> Option<Accepted> {
    match reply {
        Reply::InstanceForceStop(reply) => Some(accepted(&reply)),
        _ => None,
    }
}

fn extract_restart(reply: Reply) -> Option<Accepted> {
    match reply {
        Reply::InstanceRestart(reply) => Some(accepted(&reply)),
        _ => None,
    }
}

fn extract_reload(reply: Reply) -> Option<Accepted> {
    match reply {
        Reply::InstanceReload(reply) => Some(accepted(&reply)),
        _ => None,
    }
}

/// Confirms a force stop of `target` (AC-13): `--yes` passes; with `--json`
/// the noninteractive path holds even at a terminal, so `--yes` is needed;
/// otherwise the destructive question, answered no by default.
fn confirm_force(cx: &mut Context, target: &str, yes: bool) -> Result<(), CliFailure> {
    let approval = Approval {
        given: yes,
        flag: "--yes",
    };
    if !yes && cx.output.mode == OutputMode::Json {
        return Err(CliFailure::confirmation_required(approval.flag));
    }
    let question = format!("force stop {target}?");
    let request = ConfirmRequest {
        role: ConfirmRole::Destructive,
        question: &question,
        consequence: FORCE_RISK,
    };
    Confirm::ask(cx, request, approval, OnNoninteractive::Fail).map(|_| ())
}

impl RunCommand for StartArgs {
    type Output = InstanceAction;

    async fn run(self, cx: &mut Context) -> CommandOutput<InstanceAction> {
        let id = match self.instance.target.resolve(cx).await {
            Ok(id) => id,
            Err(failure) => return CommandOutput::failed(failure),
        };
        let call = Call::InstanceStart(StartInstanceParams {
            instance_id: id,
            expected_revision: None,
        });
        match cx.submit(call, &self.run, extract_start).await {
            Ok(submission) => conclude(cx, submission, Verb::Start, Some(id)).await,
            Err(failure) => CommandOutput::failed(failure),
        }
    }
}

impl RunCommand for StopArgs {
    type Output = InstanceAction;

    async fn run(self, cx: &mut Context) -> CommandOutput<InstanceAction> {
        let Some(named) = self.target.as_ref() else {
            return CommandOutput::failed(CliFailure::usage(
                "missing_argument",
                "give an instance, or --all",
            ));
        };
        if self.force {
            // Asked before anything reaches the daemon, the name included.
            let target = crate::output::escape::escape_display(&named.to_string()).into_owned();
            if let Err(failure) = confirm_force(cx, &target, self.yes) {
                return CommandOutput::failed(failure);
            }
        }
        let id = match named.resolve(cx).await {
            Ok(id) => id,
            Err(failure) => return CommandOutput::failed(failure),
        };
        if self.force {
            let call = Call::InstanceForceStop(ForceStopInstanceParams {
                instance_id: id,
                expected_revision: None,
            });
            return match cx.submit(call, &self.run, extract_force).await {
                Ok(submission) => conclude(cx, submission, Verb::ForceStop, Some(id)).await,
                Err(failure) => CommandOutput::failed(failure),
            };
        }
        let call = Call::InstanceStop(StopInstanceParams {
            instance_id: id,
            expected_revision: None,
            override_leases: false,
        });
        match cx.submit(call, &self.run, extract_stop).await {
            Ok(submission) => conclude(cx, submission, Verb::Stop, Some(id)).await,
            Err(failure) => CommandOutput::failed(failure),
        }
    }
}

impl RunCommand for RestartArgs {
    type Output = InstanceAction;

    async fn run(self, cx: &mut Context) -> CommandOutput<InstanceAction> {
        let id = match self.instance.target.resolve(cx).await {
            Ok(id) => id,
            Err(failure) => return CommandOutput::failed(failure),
        };
        let call = Call::InstanceRestart(RestartInstanceParams {
            instance_id: id,
            expected_revision: None,
        });
        match cx.submit(call, &self.run, extract_restart).await {
            Ok(submission) => conclude(cx, submission, Verb::Restart, Some(id)).await,
            Err(failure) => CommandOutput::failed(failure),
        }
    }
}

impl RunCommand for ReloadArgs {
    type Output = InstanceAction;

    async fn run(self, cx: &mut Context) -> CommandOutput<InstanceAction> {
        let id = match self.instance.target.resolve(cx).await {
            Ok(id) => id,
            Err(failure) => return CommandOutput::failed(failure),
        };
        let call = Call::InstanceReload(ReloadInstanceParams {
            instance_id: id,
            expected_revision: None,
        });
        match cx.submit(call, &self.run, extract_reload).await {
            Ok(submission) => conclude(cx, submission, Verb::Reload, Some(id)).await,
            Err(failure) => CommandOutput::failed(failure),
        }
    }
}
