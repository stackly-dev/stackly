//! Finds the `stackly-fake-server` binary, the dummy server behind the
//! `testing/fake` provider (spec 0009).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

/// Package and binary name of the dummy server.
const PACKAGE: &str = "stackly-fake-server";

/// Builds `stackly-fake-server` once per test process and returns the path
/// of the binary.
///
/// The first call runs `cargo build -p stackly-fake-server --locked` with
/// the `CARGO` that runs the tests, in the workspace target directory
/// (`CARGO_TARGET_DIR` when set, else `<workspace>/target`), so the build is
/// shared with the rest of the workspace. Later calls return the same path.
/// How the binary is installed as a runtime under a test root is up to the
/// tests of features 10 and 14.
///
/// # Panics
///
/// Panics with cargo's exit status when cargo cannot run, the build fails,
/// or the binary is not where cargo puts it.
#[must_use]
pub fn binary() -> PathBuf {
    static BINARY: OnceLock<PathBuf> = OnceLock::new();
    BINARY.get_or_init(build).clone()
}

fn build() -> PathBuf {
    let workspace = workspace_root();
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map_or_else(|| workspace.join("target"), PathBuf::from);
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let status = Command::new(cargo)
        .current_dir(&workspace)
        .args(["build", "--package", PACKAGE, "--bin", PACKAGE, "--locked"])
        .arg("--target-dir")
        .arg(&target)
        .status()
        .unwrap_or_else(|error| panic!("cannot run cargo to build {PACKAGE}: {error}"));
    assert!(
        status.success(),
        "cargo build of {PACKAGE} failed with {status}"
    );

    let binary = target.join("debug").join(PACKAGE);
    assert!(
        binary.is_file(),
        "cargo built {PACKAGE} but {} is missing",
        binary.display()
    );
    binary
}

/// The workspace root, two levels above this crate's manifest folder.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("stackly-testkit lives at crates/testkit, two levels under the workspace root")
        .to_path_buf()
}
