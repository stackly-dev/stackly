//! `stackly daemon` (spec 0008 and spec 0015): `status`, and the verbs that
//! install the background agent and start, stop, or restart the daemon.
//!
//! Each verb's logic is a function of [`Deps`] (the agent, a
//! [`DaemonProbe`], and Ctrl+C), so tests run it against
//! `FakeBackgroundAgent`, a scripted daemon, and a paused clock. The verbs
//! below only build the live `Deps` and call them.

pub mod agent;
pub mod install;
pub mod restart;
pub mod start;
pub mod status;
pub mod stop;
pub mod wait;

use clap::{Args, Subcommand};

use self::agent::AgentEnv;
use self::install::{InstallResult, install};
use self::restart::{RestartResult, restart};
use self::start::{StartResult, start};
use self::status::StatusArgs;
use self::stop::{StopResult, stop};
use self::wait::{DaemonProbe, Interrupt, LiveProbe};
use super::{RunCommand, execute};
use crate::context::Context;
use crate::output::CommandOutput;

/// The verbs of `stackly daemon`.
#[derive(Debug, Subcommand)]
pub enum DaemonCommand {
    /// Show which daemon runs for you, its health, versions, and where it
    /// keeps things, and the state of the background agent.
    #[command(after_help = "Examples:\n  stackly daemon status\n  stackly daemon status --json")]
    Status(StatusArgs),
    /// Register the background agent that starts the daemon when you log in
    #[command(after_help = "Examples:\n  stackly daemon install\n  stackly daemon install --json")]
    Install(InstallArgs),
    /// Start the daemon, unless it already runs
    #[command(after_help = "Examples:\n  stackly daemon start\n  stackly daemon start --json")]
    Start(StartArgs),
    /// Stop the daemon; instances keep running
    #[command(after_help = "Examples:\n  stackly daemon stop\n  stackly daemon stop --json")]
    Stop(StopArgs),
    /// Stop the daemon and wait for a new one; instances keep running
    #[command(after_help = "Examples:\n  stackly daemon restart\n  stackly daemon restart --json")]
    Restart(RestartArgs),
}

impl DaemonCommand {
    /// Runs the verb and returns the exit code.
    pub async fn run(self, cx: &mut Context) -> u8 {
        match self {
            Self::Status(args) => execute(args, cx).await,
            Self::Install(args) => execute(args, cx).await,
            Self::Start(args) => execute(args, cx).await,
            Self::Stop(args) => execute(args, cx).await,
            Self::Restart(args) => execute(args, cx).await,
        }
    }
}

/// `stackly daemon install` takes no arguments of its own.
#[derive(Debug, Args)]
pub struct InstallArgs {}

/// `stackly daemon start` takes no arguments of its own.
#[derive(Debug, Args)]
pub struct StartArgs {}

/// `stackly daemon stop` takes no arguments of its own.
#[derive(Debug, Args)]
pub struct StopArgs {}

/// `stackly daemon restart` takes no arguments of its own.
#[derive(Debug, Args)]
pub struct RestartArgs {}

/// What the logic of the verbs runs against.
pub struct Deps<'a> {
    /// The background agent, the launch record, and the clock.
    pub agent: &'a AgentEnv,
    /// The daemon.
    pub probe: &'a dyn DaemonProbe,
    /// Ctrl+C.
    pub interrupt: &'a mut dyn Interrupt,
}

/// An [`Interrupt`] that never fires, for a process that cannot catch
/// SIGINT.
struct NeverInterrupted;

impl Interrupt for NeverInterrupted {
    fn pending(&mut self) -> bool {
        false
    }

    fn wait(&mut self) -> wait::BoxFuture<'_, ()> {
        Box::pin(std::future::pending())
    }
}

/// SIGINT caught from now on, or an interrupt that never fires when the
/// signal cannot be caught.
fn live_interrupt() -> Box<dyn Interrupt> {
    match wait::SigInt::install() {
        Ok(signal) => Box::new(signal),
        Err(_) => Box::new(NeverInterrupted),
    }
}

impl RunCommand for InstallArgs {
    type Output = InstallResult;

    async fn run(self, cx: &mut Context) -> CommandOutput<InstallResult> {
        let agent = cx.agent.clone();
        let probe = LiveProbe::new(cx.connector().clone());
        let mut interrupt = live_interrupt();
        let mut deps = Deps {
            agent: &agent,
            probe: &probe,
            interrupt: &mut *interrupt,
        };
        install(&mut deps).await
    }
}

impl RunCommand for StartArgs {
    type Output = StartResult;

    async fn run(self, cx: &mut Context) -> CommandOutput<StartResult> {
        let agent = cx.agent.clone();
        let probe = LiveProbe::new(cx.connector().clone());
        let mut interrupt = live_interrupt();
        let mut deps = Deps {
            agent: &agent,
            probe: &probe,
            interrupt: &mut *interrupt,
        };
        start(&mut deps).await
    }
}

impl RunCommand for StopArgs {
    type Output = StopResult;

    async fn run(self, cx: &mut Context) -> CommandOutput<StopResult> {
        let agent = cx.agent.clone();
        let probe = LiveProbe::new(cx.connector().clone());
        let mut interrupt = live_interrupt();
        let mut deps = Deps {
            agent: &agent,
            probe: &probe,
            interrupt: &mut *interrupt,
        };
        stop(&mut deps).await
    }
}

impl RunCommand for RestartArgs {
    type Output = RestartResult;

    async fn run(self, cx: &mut Context) -> CommandOutput<RestartResult> {
        let agent = cx.agent.clone();
        let probe = LiveProbe::new(cx.connector().clone());
        let mut interrupt = live_interrupt();
        let mut deps = Deps {
            agent: &agent,
            probe: &probe,
            interrupt: &mut *interrupt,
        };
        restart(&mut deps).await
    }
}

#[cfg(test)]
mod testing;
