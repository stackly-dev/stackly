//! `stackly instance start` and `stop` (spec 0014, AC-6; by name, spec 0022,
//! AC-15).

use clap::Args;
use stackly_protocol::instance::{StartInstanceParams, StopInstanceParams};
use stackly_protocol::{Call, Reply};

use super::resolve::TargetArg;
use super::{InstanceAction, Verb, accepted, conclude};
use crate::commands::RunCommand;
use crate::context::Context;
use crate::output::CommandOutput;
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
#[derive(Debug, Args)]
pub struct StopArgs {
    /// The instance.
    #[command(flatten)]
    pub instance: TargetArg,
    /// `--wait`, `--timeout`, and `--idempotency-key`.
    #[command(flatten)]
    pub run: AsyncArgs,
}

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
        let id = match self.instance.target.resolve(cx).await {
            Ok(id) => id,
            Err(failure) => return CommandOutput::failed(failure),
        };
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
