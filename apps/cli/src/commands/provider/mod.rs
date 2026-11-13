//! `stackly provider list` and `inspect` (spec 0017, *CLI*, AC-9). They call
//! `catalog.list_providers` and `catalog.get_provider` and never read a
//! catalog file or a provider package: every status, reason, note, and
//! install verdict is the daemon's, written as it arrives. `validate`
//! (spec 0036) asks the daemon to check a package folder. `import`,
//! `approve`, `revoke`, and `remove` (spec 0037) call the same methods as
//! the Catalog screen; the daemon reads the folder.

mod approve;
mod copy;
mod import;
mod inspect;
mod list;
pub(crate) mod local;
mod outcome;
mod remove;
pub mod render;
mod revoke;
mod validate;

use clap::Subcommand;

use self::approve::ApproveArgs;
use self::import::ImportArgs;
use self::inspect::InspectArgs;
use self::list::ListArgs;
use self::remove::RemoveArgs;
use self::revoke::RevokeArgs;
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
    /// Import a provider package folder into the catalog; nothing runs until you approve it
    #[command(
        after_help = "The folder is read by stacklyd and copied; it is never changed.\n\nExamples:\n  stackly provider import ./acme-demo --dry-run\n  stackly provider import ./acme-demo\n  stackly provider import ./acme-demo --approve --yes --json"
    )]
    Import(ImportArgs),
    /// Approve an imported provider so it can install runtimes and create instances
    #[command(
        after_help = "Asks first; --yes is needed when stackly runs noninteractive or with --json.\n\nExamples:\n  stackly provider approve acme/demo\n  stackly provider approve acme/demo --yes --json"
    )]
    Approve(ApproveArgs),
    /// Revoke the approval of an imported provider; existing instances keep running
    #[command(
        after_help = "Examples:\n  stackly provider revoke acme/demo\n  stackly provider revoke acme/demo --yes"
    )]
    Revoke(RevokeArgs),
    /// Remove an imported provider from the catalog; instances and data stay
    #[command(
        after_help = "Examples:\n  stackly provider remove acme/demo\n  stackly provider remove acme/demo --yes"
    )]
    Remove(RemoveArgs),
}

impl ProviderCommand {
    /// Runs the verb and returns the exit code.
    pub async fn run(self, cx: &mut Context) -> u8 {
        match self {
            Self::List(args) => execute(args, cx).await,
            Self::Inspect(args) => execute(args, cx).await,
            Self::Validate(args) => execute(args, cx).await,
            Self::Import(args) => execute(args, cx).await,
            Self::Approve(args) => execute(args, cx).await,
            Self::Revoke(args) => execute(args, cx).await,
            Self::Remove(args) => execute(args, cx).await,
        }
    }
}
