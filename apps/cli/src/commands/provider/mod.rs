//! `stackly provider list` and `inspect` (spec 0017, *CLI*, AC-9). They call
//! `catalog.list_providers` and `catalog.get_provider` and never read a
//! catalog file or a provider package: every status, reason, note, and
//! install verdict is the daemon's, written as it arrives.

mod inspect;
mod list;
pub mod render;

use clap::Subcommand;

use self::inspect::InspectArgs;
use self::list::ListArgs;
use super::execute;
use crate::context::Context;

/// The verbs of `stackly provider`.
#[derive(Debug, Subcommand)]
pub enum ProviderCommand {
    /// List the providers in the catalog with their state and versions
    #[command(
        after_help = "Examples:\n  stackly provider list\n  stackly provider list --category messaging --json"
    )]
    List(ListArgs),
    /// Show one provider: capabilities, the version matrix, and artifacts
    #[command(
        after_help = "Examples:\n  stackly provider inspect official/nats\n  stackly provider inspect official/nats --json"
    )]
    Inspect(InspectArgs),
}

impl ProviderCommand {
    /// Runs the verb and returns the exit code.
    pub async fn run(self, cx: &mut Context) -> u8 {
        match self {
            Self::List(args) => execute(args, cx).await,
            Self::Inspect(args) => execute(args, cx).await,
        }
    }
}
