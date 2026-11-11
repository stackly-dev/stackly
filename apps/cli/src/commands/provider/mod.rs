//! `stackly provider list` and `inspect` (spec 0017, *CLI*, AC-9). They call
//! `catalog.list_providers` and `catalog.get_provider` and never read a
//! catalog file or a provider package: every status, reason, note, and
//! install verdict is the daemon's, written as it arrives. `validate`
//! (spec 0036) asks the daemon to check a package folder.

mod inspect;
mod list;
pub mod render;
mod validate;

use clap::Subcommand;

use self::inspect::InspectArgs;
use self::list::ListArgs;
use self::validate::ValidateArgs;
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
    /// Check a provider package folder without running anything from it
    #[command(
        after_help = "Exits 0 when the package is publishable, 2 with cli.provider_invalid otherwise.\n\nExamples:\n  stackly provider validate providers/mytool\n  stackly provider validate ./mytool --json"
    )]
    Validate(ValidateArgs),
}

impl ProviderCommand {
    /// Runs the verb and returns the exit code.
    pub async fn run(self, cx: &mut Context) -> u8 {
        match self {
            Self::List(args) => execute(args, cx).await,
            Self::Inspect(args) => execute(args, cx).await,
            Self::Validate(args) => execute(args, cx).await,
        }
    }
}
