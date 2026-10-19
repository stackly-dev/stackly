//! `stackly`, the Stackly command line client (spec 0008).
//!
//! The CLI only talks to `stacklyd` over IPC. It never runs provider
//! binaries or any other process, never starts the daemon, never creates
//! folders, and never writes files or the registry. It reads
//! `STACKLY_ROOT` and `STACKLY_NO_INPUT` once at start.
//!
//! Only `--json` and the exit codes are a contract: exit codes come from the
//! error category alone, and `--json` writes one envelope line on stdout.
//! Progress always goes to stderr.

use std::process::ExitCode;

mod app;
mod args;
mod commands;
mod confirm;
mod context;
mod env;
mod exit;
mod failure;
mod output;
mod panic;
mod progress;
mod session;
mod wait;

fn main() -> ExitCode {
    panic::install_hook();
    let code = std::panic::catch_unwind(app::run).unwrap_or_else(|_| panic::report());
    ExitCode::from(code)
}
