//! `stacklyd`, the per user Stackly daemon.
//!
//! `main` only reads the arguments and the environment, once, and hands
//! them to [`stacklyd::run`]. Exit codes: 0 after a normal stop or
//! `--print-roots`, 1 when the daemon cannot start, 2 for bad arguments
//! (clap), and 3 when another daemon holds the lock.

use std::process::ExitCode;

use clap::Parser;
use stackly_platform::ROOT_OVERRIDE_ENV;
use stacklyd::{LOG_FILTER_ENV, StartConfig};

/// The per user Stackly daemon.
#[derive(Debug, Parser)]
#[command(version = stackly_core::daemon_info().version, about)]
struct Args {
    /// Log the resolved roots and the socket path, then exit, without
    /// creating folders, taking the lock, or binding the socket.
    #[arg(long)]
    print_roots: bool,
}

fn main() -> ExitCode {
    let args = Args::parse();
    stacklyd::run(StartConfig::new(
        std::env::var_os(ROOT_OVERRIDE_ENV),
        std::env::var_os(LOG_FILTER_ENV),
        args.print_roots,
    ))
}
