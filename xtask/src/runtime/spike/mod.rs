//! The runtime spike of spec 0003: proof that each provider executable runs
//! from its managed artifact as a child of a noninteractive process.
//!
//! `runtime spike --id <id> --result <file>` is the driver; it loads a
//! temporary LaunchAgent whose program is `runtime spike-worker`, which
//! extracts, audits, signs, starts, checks, and stops the provider and
//! writes `stackly.runtime-spike-result/v1`. `--cleanup` and
//! `--prune-cache` take the same lock as the driver.
// provisional: F9 (the provider manifest takes over the per provider steps)

mod archive;
mod audit;
mod cleanup;
mod driver;
mod launchd;
mod lock;
mod plan;
mod process;
mod providers;
mod quarantine;
mod result;
mod signing;
mod worker;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use anyhow::{Result, bail};

use super::cache::Cache;
use super::inventory::Inventory;
use lock::SpikeLock;

/// The fixed label of the worker agent; the lock makes it unique.
pub const LABEL: &str = "dev.stackly.xtask.runtime-spike";
/// The name prefix of every spike root in the temporary folder.
pub const ROOT_PREFIX: &str = "stackly-runtime-spike-";
/// The prefix of every private socket folder.
pub const SOCKET_PREFIX: &str = "/tmp/sk-";
/// The folder socket folders live in.
pub const SOCKET_PARENT: &str = "/tmp";
/// The file that marks a root as owned by this user; it holds the uid.
pub const OWNER_MARKER: &str = ".stackly-spike-owner";
/// The worker's own time limit.
pub const WORKER_TIMEOUT: Duration = Duration::from_secs(12 * 60);
/// The driver's limit, not counting the download.
pub const DRIVER_TIMEOUT: Duration = Duration::from_secs(15 * 60);
/// How long `--cleanup` and `--prune-cache` wait for the lock.
const LOCK_WAIT: Duration = Duration::from_secs(20 * 60);

/// `cargo xtask runtime spike ...`.
pub fn run(root: &Path, args: &[&str]) -> Result<ExitCode> {
    match args {
        ["--cleanup"] => cleanup_now(),
        ["--prune-cache"] => prune_cache(root),
        ["--id", id, "--result", result] | ["--result", result, "--id", id] => {
            driver::run(root, id, &PathBuf::from(result))
        }
        _ => bail!(
            "use `cargo xtask runtime spike --id <id> --result <file>`, `runtime spike --cleanup`, or `runtime spike --prune-cache`"
        ),
    }
}

/// `cargo xtask runtime spike-worker --plan <file>`.
pub fn run_worker(args: &[&str]) -> Result<ExitCode> {
    worker::run(args)
}

fn cleanup_now() -> Result<ExitCode> {
    if !cfg!(target_os = "macos") {
        bail!("runtime spike --cleanup runs on macOS only");
    }
    let cache = Cache::open()?;
    let _lock = SpikeLock::acquire_within(cache.dir(), LOCK_WAIT)?;
    let swept = cleanup::sweep(launchd::uid()?)?;
    println!(
        "spike cleanup: {} process(es) stopped, agent {}, {} folder(s) deleted",
        swept.processes,
        if swept.agent { "removed" } else { "not loaded" },
        swept.folders
    );
    Ok(ExitCode::SUCCESS)
}

fn prune_cache(root: &Path) -> Result<ExitCode> {
    if !cfg!(target_os = "macos") {
        bail!("runtime spike --prune-cache runs on macOS only");
    }
    let inventory = Inventory::load(root)?;
    let pinned: HashSet<String> = inventory
        .artifacts
        .iter()
        .filter_map(|artifact| artifact.sha256.clone())
        .collect();
    let cache = Cache::open()?;
    let _lock = SpikeLock::acquire_within(cache.dir(), LOCK_WAIT)?;
    let deleted = cleanup::prune_cache(cache.dir(), &pinned)?;
    for name in &deleted {
        println!("spike prune-cache: deleted {name}");
    }
    println!("spike prune-cache: {} file(s) deleted", deleted.len());
    Ok(ExitCode::SUCCESS)
}
