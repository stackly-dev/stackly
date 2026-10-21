//! `stacklyd`, the per user Stackly daemon.
//!
//! `main` only reads the arguments and the environment (`STACKLY_ROOT`,
//! `STACKLY_LOG`, and `STACKLY_CATALOG_DIR`), once, and hands
//! them to [`stacklyd::run`]. Exit codes: 0 after a normal stop or
//! `--print-roots`, 1 when the daemon cannot start, 2 for bad arguments
//! (clap), and 3 when another daemon holds the lock.

use std::process::ExitCode;

use clap::Parser;
use stackly_platform::ROOT_OVERRIDE_ENV;
use stacklyd::{CATALOG_DIR_ENV, LOG_FILTER_ENV, StartConfig};

/// The per user Stackly daemon.
#[derive(Debug, Parser)]
#[command(version = stackly_core::daemon_info().version, about)]
struct Args {
    /// Log the resolved roots and the socket path, then exit, without
    /// creating folders, taking the lock, or binding the socket.
    #[arg(long)]
    print_roots: bool,
}

/// The variable launchd sets to the job label.
const XPC_SERVICE_NAME_ENV: &str = "XPC_SERVICE_NAME";

/// The parent process ID; the daemon only runs on macOS, so any other
/// target reports 0 (not launchd).
fn parent_pid() -> u32 {
    #[cfg(target_os = "macos")]
    {
        stackly_platform_macos::parent_pid()
    }
    #[cfg(not(target_os = "macos"))]
    {
        0
    }
}

fn main() -> ExitCode {
    let args = Args::parse();
    stacklyd::run(
        StartConfig::new(
            std::env::var_os(ROOT_OVERRIDE_ENV),
            std::env::var_os(LOG_FILTER_ENV),
            args.print_roots,
        )
        .with_catalog_dir(std::env::var_os(CATALOG_DIR_ENV))
        .with_launch_environment(
            std::env::var_os(XPC_SERVICE_NAME_ENV),
            parent_pid(),
            std::env::current_exe().ok(),
        ),
    )
}
