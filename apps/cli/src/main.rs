//! `stackly`, the Stackly command line client.
//!
//! The CLI only talks to `stacklyd` over IPC. It never runs provider
//! binaries, never starts the daemon, never creates folders, and never
//! writes the registry. It reads `STACKLY_ROOT` once, here, with the same
//! rules as the daemon.

use std::process::ExitCode;

use clap::{CommandFactory, Parser, Subcommand};
use stackly_platform::ROOT_OVERRIDE_ENV;

mod daemon_status;

/// The Stackly command line client.
#[derive(Debug, Parser)]
#[command(name = "stackly", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Look at the Stackly daemon.
    Daemon {
        #[command(subcommand)]
        command: DaemonCommand,
    },
}

#[derive(Debug, Subcommand)]
enum DaemonCommand {
    /// Show which daemon runs for you, its health, and where it keeps
    /// things.
    Status {
        /// Print one JSON object on stdout instead of text.
        #[arg(long)]
        json: bool,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let root_override = std::env::var_os(ROOT_OVERRIDE_ENV);
    match cli.command {
        None => match Cli::command().print_help() {
            Ok(()) => ExitCode::SUCCESS,
            Err(_) => ExitCode::FAILURE,
        },
        Some(Command::Daemon {
            command: DaemonCommand::Status { json },
        }) => daemon_status::run(root_override, json),
    }
}
