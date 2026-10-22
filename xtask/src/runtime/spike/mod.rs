//! The runtime spike of spec 0003: proof that each provider executable runs
//! from its managed artifact as a child of a noninteractive process.
//!
//! `runtime spike --id <id> --result <file> [--no-quarantine]` is the
//! driver; it loads a temporary LaunchAgent whose program is
//! `runtime spike-worker`, which extracts, audits, signs, starts, checks,
//! and stops the provider and writes `stackly.runtime-spike-result/v1`.
//! `--no-quarantine` is for the local run before a maintainer upload
//! (AC-19); native tests never pass it. `--cleanup` and `--prune-cache`
//! take the same lock as the driver.
// provisional: F36 (the feature 36 harness builds the spike steps from the provider packages)

mod archive;
mod audit;
mod cleanup;
mod closure;
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

use anyhow::{Context, Result, bail};

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

/// How to call `runtime spike`.
const USAGE: &str = "use `cargo xtask runtime spike --id <id> --result <file> [--no-quarantine]`, `runtime spike --cleanup`, or `runtime spike --prune-cache`";

/// `cargo xtask runtime spike ...`.
pub fn run(root: &Path, args: &[&str]) -> Result<ExitCode> {
    match args {
        ["--cleanup"] => cleanup_now(),
        ["--prune-cache"] => prune_cache(root),
        _ => {
            let driver = DriverArgs::parse(args)?;
            driver::run(root, &driver.id, &driver.result, driver.no_quarantine)
        }
    }
}

/// The driver's arguments, in any order.
#[derive(Debug, PartialEq, Eq)]
struct DriverArgs {
    /// The inventory ID.
    id: String,
    /// Where the spike result goes.
    result: PathBuf,
    /// Skip the quarantine probe: a local run before an upload (AC-9).
    no_quarantine: bool,
}

impl DriverArgs {
    fn parse(args: &[&str]) -> Result<Self> {
        let mut id = None;
        let mut result = None;
        let mut no_quarantine = false;
        let mut rest = args.iter();
        while let Some(arg) = rest.next() {
            let repeated = match *arg {
                "--id" => id
                    .replace(*rest.next().context("--id needs an artifact ID")?)
                    .is_some(),
                "--result" => result
                    .replace(PathBuf::from(
                        rest.next().context("--result needs a file path")?,
                    ))
                    .is_some(),
                "--no-quarantine" => std::mem::replace(&mut no_quarantine, true),
                other => bail!("unexpected argument `{other}`; {USAGE}"),
            };
            if repeated {
                bail!("{arg} is given more than once; {USAGE}");
            }
        }
        match (id, result) {
            (Some(id), Some(result)) => Ok(Self {
                id: id.to_owned(),
                result,
                no_quarantine,
            }),
            _ => bail!("{USAGE}"),
        }
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
    let uid = launchd::uid()?;
    let swept = cleanup::sweep(uid)?;
    let tests = cleanup::sweep_test_processes(uid)?;
    println!(
        "spike cleanup: {} process(es) stopped, agent {}, {} folder(s) deleted, {tests} native test process(es) stopped",
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_driver_takes_id_and_result_in_any_order_and_no_quarantine_anywhere() {
        let expected = |no_quarantine| DriverArgs {
            id: "valkey-9.1.2-r1-macos-aarch64".to_owned(),
            result: PathBuf::from("out/result.json"),
            no_quarantine,
        };
        let id = "valkey-9.1.2-r1-macos-aarch64";
        assert_eq!(
            DriverArgs::parse(&["--id", id, "--result", "out/result.json"]).unwrap(),
            expected(false)
        );
        assert_eq!(
            DriverArgs::parse(&["--result", "out/result.json", "--id", id]).unwrap(),
            expected(false)
        );
        assert_eq!(
            DriverArgs::parse(&["--no-quarantine", "--id", id, "--result", "out/result.json"])
                .unwrap(),
            expected(true)
        );
        assert_eq!(
            DriverArgs::parse(&["--id", id, "--result", "out/result.json", "--no-quarantine"])
                .unwrap(),
            expected(true)
        );
    }

    #[test]
    fn the_driver_refuses_missing_repeated_and_unknown_arguments() {
        for args in [
            &["--id", "x"][..],
            &["--result", "r.json"],
            &["--id", "x", "--result"],
            &["--id", "x", "--id", "y", "--result", "r.json"],
            &[
                "--id",
                "x",
                "--result",
                "r.json",
                "--no-quarantine",
                "--no-quarantine",
            ],
            &["--id", "x", "--result", "r.json", "--quarantine"],
        ] {
            assert!(DriverArgs::parse(args).is_err(), "{args:?}");
        }
    }
}
