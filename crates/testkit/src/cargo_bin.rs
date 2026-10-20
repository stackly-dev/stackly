//! Builds a workspace binary for tests that run another crate's binary,
//! such as CLI tests that need `stacklyd` (spec 0006, *Testkit*).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock, PoisonError};

/// The target folder for these builds, so they never wait on the build
/// lock of the `cargo test` that runs them.
pub(crate) const TARGET_DIR: &str = "target/testkit-bin";

/// Builds `bin` of `package` once per test process and returns the path of
/// the binary.
///
/// The first call runs `$CARGO build --locked -p <package> --bin <bin>
/// --target-dir <workspace>/target/testkit-bin`; later calls return the
/// same path. Tests of a binary's own package use
/// `env!("CARGO_BIN_EXE_<bin>")` instead.
///
/// # Panics
///
/// Panics with cargo's exit status when cargo cannot run, the build fails,
/// or the binary is not where cargo puts it.
#[must_use]
pub fn cargo_bin(package: &str, bin: &str) -> PathBuf {
    static BUILT: OnceLock<Mutex<HashMap<(String, String), PathBuf>>> = OnceLock::new();
    let built = BUILT.get_or_init(Mutex::default);
    // Holding the lock while cargo runs keeps two tests from building the
    // same binary at once.
    let mut built = built.lock().unwrap_or_else(PoisonError::into_inner);
    built
        .entry((package.to_owned(), bin.to_owned()))
        .or_insert_with(|| build(package, bin))
        .clone()
}

fn build(package: &str, bin: &str) -> PathBuf {
    let workspace = workspace_root();
    let target = workspace.join(TARGET_DIR);
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let status = Command::new(cargo)
        .current_dir(&workspace)
        .args(["build", "--locked", "--package", package, "--bin", bin])
        .arg("--target-dir")
        .arg(&target)
        .status()
        .unwrap_or_else(|error| panic!("cannot run cargo to build {package} {bin}: {error}"));
    assert!(
        status.success(),
        "cargo build of {package} {bin} failed with {status}"
    );
    let binary = target.join("debug").join(bin);
    assert!(
        binary.is_file(),
        "cargo built {package} {bin} but {} is missing",
        binary.display()
    );
    binary
}

/// The workspace root, two levels above this crate's manifest folder.
pub(crate) fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/testkit sits two levels below the workspace root")
        .to_path_buf()
}
