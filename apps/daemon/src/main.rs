//! `stacklyd`, the per user Stackly daemon.
//!
//! The daemon wires every dependency explicitly here. It is the only place
//! that reads `STACKLY_ROOT`, once at start. For now it resolves its roots,
//! logs where it would run, and exits; the IPC server comes with the daemon
//! build.

use std::process::ExitCode;

use clap::Parser;
use stackly_core::DaemonInfo;

/// The per user Stackly daemon.
#[derive(Debug, Parser)]
#[command(version = stackly_core::daemon_info().version, about)]
struct Args {}

fn main() -> ExitCode {
    let _args = Args::parse();
    tracing_subscriber::fmt()
        .json()
        .with_writer(std::io::stderr)
        .init();
    run(stackly_core::daemon_info())
}

#[cfg(target_os = "macos")]
fn run(info: DaemonInfo) -> ExitCode {
    use stackly_platform::ROOT_OVERRIDE_ENV;
    use stackly_platform_macos::MacosPlatform;
    use stackly_protocol::ProtocolVersion;

    let root_override = std::env::var_os(ROOT_OVERRIDE_ENV);
    let has_override = root_override.is_some();
    let roots =
        MacosPlatform::new(root_override).and_then(|platform| platform.directories().roots());
    match roots {
        Ok(roots) => {
            tracing::info!(
                version = info.version,
                protocol_major = ProtocolVersion::CURRENT.major,
                protocol_minor = ProtocolVersion::CURRENT.minor,
                data_root = %roots.data().display(),
                logs_root = %roots.logs().display(),
                cache_root = %roots.cache().display(),
                runtime_root = %roots.runtime().display(),
                root_override = has_override,
                "stacklyd started; the IPC server is not built yet, exiting",
            );
            ExitCode::SUCCESS
        }
        Err(err) => {
            tracing::error!(
                error = %err,
                root_override = has_override,
                "cannot resolve the Stackly directories",
            );
            ExitCode::FAILURE
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn run(info: DaemonInfo) -> ExitCode {
    tracing::error!(
        version = info.version,
        "stacklyd only runs on macOS for now"
    );
    ExitCode::FAILURE
}
