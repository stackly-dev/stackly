//! Builds a workspace example for tests that run it, such as the
//! `skeleton_seed` example of `stacklyd` (spec 0014).

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Mutex, OnceLock, PoisonError};

use crate::cargo_bin::{TARGET_DIR, workspace_root};

/// Builds example `name` of `package` once per test process and returns the
/// path of the binary.
///
/// The first call runs `$CARGO build --locked -p <package> --example <name>
/// --target-dir <workspace>/target/testkit-bin`, the same folder and lock
/// pattern as [`cargo_bin`](crate::cargo_bin); later calls return the same
/// path. Building an example turns on the dev dependencies of its package,
/// which is how `skeleton_seed` gets the `fixture` feature of
/// `stackly-core`.
///
/// # Panics
///
/// Panics with cargo's exit status when cargo cannot run, the build fails,
/// or the example is not where cargo puts it.
#[must_use]
pub fn cargo_example(package: &str, name: &str) -> PathBuf {
    static BUILT: OnceLock<Mutex<HashMap<(String, String), PathBuf>>> = OnceLock::new();
    let built = BUILT.get_or_init(Mutex::default);
    // Holding the lock while cargo runs keeps two tests from building the
    // same example at once.
    let mut built = built.lock().unwrap_or_else(PoisonError::into_inner);
    built
        .entry((package.to_owned(), name.to_owned()))
        .or_insert_with(|| build(package, name))
        .clone()
}

fn build(package: &str, name: &str) -> PathBuf {
    let workspace = workspace_root();
    let target = workspace.join(TARGET_DIR);
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let status = Command::new(cargo)
        .current_dir(&workspace)
        .args(["build", "--locked", "--package", package, "--example", name])
        .arg("--target-dir")
        .arg(&target)
        .status()
        .unwrap_or_else(|error| {
            panic!("cannot run cargo to build example {name} of {package}: {error}")
        });
    assert!(
        status.success(),
        "cargo build of example {name} of {package} failed with {status}"
    );
    let binary = target.join("debug").join("examples").join(name);
    assert!(
        binary.is_file(),
        "cargo built example {name} of {package} but {} is missing",
        binary.display()
    );
    binary
}
