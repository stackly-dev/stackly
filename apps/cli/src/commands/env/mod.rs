//! `stackly env` (spec 0044, *CLI*, AC-16, AC-17; spec 0045 AC-16,
//! AC-17): `validate` and `plan` ask the daemon's read only `env.validate`
//! and `env.plan`, the methods the Environments screen uses, so findings
//! and blockers are the same for the same file. `import` and `remove`
//! start the daemon's `import_environment` and `remove_environment`
//! operations, `list` reads `env.list`, `lock` asks `env.lock` to write
//! `stackly.lock`, and `export` prints the text of `env.export`. `up`
//! starts the daemon's `environment_up` with `env.up` and `status` reads
//! `env.get` (spec 0046 AC-17, AC-18). The daemon reads the manifest; the
//! CLI only makes its path absolute.

mod export;
mod import;
mod list;
mod lock;
mod plan;
mod remove;
mod render;
mod services;
mod status;
mod target;
mod up;
mod validate;

use clap::Subcommand;

use self::export::ExportArgs;
use self::import::ImportArgs;
use self::list::ListArgs;
use self::lock::LockArgs;
use self::plan::PlanArgs;
use self::remove::RemoveArgs;
use self::status::StatusArgs;
use self::up::UpArgs;
use self::validate::ValidateArgs;
use super::execute;
use crate::context::Context;

/// The manifest `--file` names when it is left out.
const DEFAULT_FILE: &str = "./stackly.yaml";

/// The verbs of `stackly env`.
#[derive(Debug, Subcommand)]
pub enum EnvCommand {
    /// Check a stackly.yaml without changing anything
    #[command(
        after_help = "Exits 0 when the manifest is valid, 2 with cli.env_invalid otherwise.\n\nExamples:\n  stackly env validate\n  stackly env validate --file ../erp/stackly.yaml --json"
    )]
    Validate(ValidateArgs),
    /// Show what Up would install, create, start, or keep; changes nothing
    #[command(
        after_help = "Exits 0 when the plan can be applied, 2 with cli.env_invalid for a manifest that is not valid, 3 with cli.env_plan_blocked when something blocks it, and 5 with cli.env_locked_unsupported when --locked reaches a daemon that does not know it.\n\nExamples:\n  stackly env plan\n  stackly env plan --file ../erp/stackly.yaml\n  stackly env plan --locked\n  stackly env plan ~/Code/erp\n  stackly env plan erp-development --json"
    )]
    Plan(PlanArgs),
    /// Install, create, and start every service in dependency order
    #[command(
        after_help = "Asks nothing. Without --wait it prints the operation it started. With --wait it writes one line per service change to stderr, then the services with the ports they really use, and exits 0 only when every service is ready. Exits 2 with cli.env_invalid for a manifest that is not valid, 3 with cli.env_plan_blocked when something blocks the plan, 5 with cli.env_locked_unsupported when --locked reaches a daemon that does not know it, 6 with secret.needs_secret for a reference without a value, 7 with env.up_incomplete when a service is not ready, and 8 when --timeout runs out (the Up keeps running).\n\nExamples:\n  stackly env up --wait\n  stackly env up ~/Code/erp --wait\n  stackly env up erp-development --locked --wait --timeout 10m\n  stackly env up --json"
    )]
    Up(UpArgs),
    /// Show every service of an environment with its status and ports
    #[command(
        after_help = "Exits 0 whatever the status is, 2 with cli.env_invalid for a manifest that is not valid, and 10 with cli.environment_not_found for an environment that is not registered.\n\nExamples:\n  stackly env status\n  stackly env status ~/Code/erp\n  stackly env status erp-development --json"
    )]
    Status(StatusArgs),
    /// Write stackly.lock with the exact versions and artifacts to use
    #[command(
        after_help = "Keeps the versions already locked unless --update is given. Exits 0 when the lock is written or up to date, 2 with cli.env_invalid for a manifest that is not valid, 3 with cli.env_lock_blocked when something blocks it, and 10 for a --service the manifest does not declare.\n\nExamples:\n  stackly env lock\n  stackly env lock --update\n  stackly env lock --update --service postgres\n  stackly env lock erp-development --json"
    )]
    Lock(LockArgs),
    /// Print the host and port of every service as environment variables
    #[command(
        after_help = "Prints no secret, path, or PID. Exits 0, also when every service is skipped, and 10 for an environment that is not registered.\n\nExamples:\n  stackly env export > .env\n  eval \"$(stackly env export --format shell)\"\n  stackly env export erp-development --json"
    )]
    Export(ExportArgs),
    /// Register a stackly.yaml as an environment; nothing is installed or started
    #[command(
        after_help = "Examples:\n  stackly env import\n  stackly env import --file ../erp/stackly.yaml\n  stackly env import --file ./moved/stackly.yaml --relink"
    )]
    Import(ImportArgs),
    /// List the registered environments with their status
    #[command(after_help = "Examples:\n  stackly env list\n  stackly env list --json")]
    List(ListArgs),
    /// Remove a registered environment; its instances and data are kept
    #[command(
        after_help = "Asks first; --yes is needed when stackly runs noninteractive or with --json.\n\nExamples:\n  stackly env remove erp-development\n  stackly env remove erp-development --yes --json"
    )]
    Remove(RemoveArgs),
}

impl EnvCommand {
    /// Runs the verb and returns the exit code.
    pub async fn run(self, cx: &mut Context) -> u8 {
        match self {
            Self::Validate(args) => execute(args, cx).await,
            Self::Plan(args) => execute(args, cx).await,
            Self::Up(args) => execute(args, cx).await,
            Self::Status(args) => execute(args, cx).await,
            Self::Lock(args) => execute(args, cx).await,
            Self::Export(args) => execute(args, cx).await,
            Self::Import(args) => execute(args, cx).await,
            Self::List(args) => execute(args, cx).await,
            Self::Remove(args) => execute(args, cx).await,
        }
    }
}
