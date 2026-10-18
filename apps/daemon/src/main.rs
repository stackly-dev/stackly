//! `stacklyd`, the per user Stackly daemon.
//!
//! The daemon wires every dependency explicitly here. It is the only place
//! that reads `STACKLY_ROOT`, once at start. For now it resolves its roots,
//! prepares the layout, opens the registry, prunes its history, logs the
//! outcome, and exits; the IPC server comes with the daemon build
//! (feature 6).

use std::process::ExitCode;

use clap::Parser;
use stackly_core::DaemonInfo;

/// Exit code when the registry needs recovery.
// provisional: F6 keeps the daemon up in recovery mode instead.
#[cfg(target_os = "macos")]
const EXIT_RECOVERY: u8 = 3;

/// The per user Stackly daemon.
#[derive(Debug, Parser)]
#[command(version = stackly_core::daemon_info().version, about)]
struct Args {
    /// Log the resolved roots and exit, without preparing the layout or
    /// opening the registry. Only the home directory test uses it, so no
    /// test ever writes to the real Application Support folder.
    // provisional: F6 (spec 0006 replaces it with --print-roots)
    #[arg(long, hide = true)]
    roots_only: bool,
}

fn main() -> ExitCode {
    let args = Args::parse();
    tracing_subscriber::fmt()
        .json()
        .with_writer(std::io::stderr)
        .init();
    run(stackly_core::daemon_info(), &args)
}

#[cfg(target_os = "macos")]
fn run(info: DaemonInfo, args: &Args) -> ExitCode {
    use stackly_core::prepare_layout;
    use stackly_domain::{Registry, RetentionPolicy};
    use stackly_platform::ROOT_OVERRIDE_ENV;
    use stackly_platform_macos::MacosPlatform;
    use stackly_protocol::ProtocolVersion;
    use stackly_storage::{RegistryOpen, open_registry};

    let root_override = std::env::var_os(ROOT_OVERRIDE_ENV);
    let has_override = root_override.is_some();
    let resolved = MacosPlatform::new(root_override).and_then(|platform| {
        platform
            .directories()
            .roots()
            .map(|roots| (platform, roots))
    });
    let (platform, roots) = match resolved {
        Ok(resolved) => resolved,
        Err(err) => {
            tracing::error!(
                error = %err,
                root_override = has_override,
                "cannot resolve the Stackly directories",
            );
            return ExitCode::FAILURE;
        }
    };
    tracing::info!(
        version = info.version,
        protocol_major = ProtocolVersion::CURRENT.major,
        protocol_minor = ProtocolVersion::CURRENT.minor,
        data_root = %roots.data().display(),
        logs_root = %roots.logs().display(),
        cache_root = %roots.cache().display(),
        runtime_root = %roots.runtime().display(),
        root_override = has_override,
        "stacklyd resolved its directories",
    );
    if args.roots_only {
        return ExitCode::SUCCESS;
    }

    let prepared = match prepare_layout(&platform) {
        Ok(prepared) => prepared,
        Err(err) => {
            tracing::error!(error = %err, "cannot prepare the Stackly directories");
            return ExitCode::FAILURE;
        }
    };
    match open_registry(&platform, prepared.layout(), info.version) {
        Ok(RegistryOpen::Ready { registry, summary }) => {
            // provisional: F6 also prunes every 24 hours while it serves.
            let pruned = match registry.prune(&RetentionPolicy::DEFAULT, platform.clock().now()) {
                Ok(pruned) => pruned,
                Err(err) => {
                    tracing::error!(error = %err, "cannot prune the registry history");
                    return ExitCode::FAILURE;
                }
            };
            tracing::info!(
                registry_state = "ready",
                pruned_rows = pruned.total(),
                schema_version = summary.schema_version,
                created = summary.created,
                migrated_from = summary.migrated_from,
                snapshot = summary
                    .snapshot
                    .as_ref()
                    .map(|path| path.display().to_string()),
                sqlite_version = summary.sqlite_version,
                "registry ready; the IPC server is not built yet, exiting",
            );
            // Joins the registry thread, which checkpoints and closes the file.
            drop(registry);
            ExitCode::SUCCESS
        }
        Ok(RegistryOpen::Recovery(report)) => {
            tracing::error!(
                registry_state = "recovery",
                reason = report.reason.code(),
                report = %report.path.display(),
                "the registry needs recovery; it was left untouched",
            );
            ExitCode::from(EXIT_RECOVERY)
        }
        Err(err) => {
            tracing::error!(error = %err, "cannot open the registry");
            ExitCode::FAILURE
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn run(info: DaemonInfo, _args: &Args) -> ExitCode {
    tracing::error!(
        version = info.version,
        "stacklyd only runs on macOS for now"
    );
    ExitCode::FAILURE
}
