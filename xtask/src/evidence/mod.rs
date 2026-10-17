//! `cargo xtask evidence ...`: test evidence (spec 0002).
//!
//! `evidence run` is the only way a test run becomes evidence. `evidence
//! check` holds the static rules (manual records, the baseline file, and the
//! native test markers), `evidence new` starts a manual record, and
//! `evidence baseline` records this Mac as the benchmark baseline.

mod baseline;
mod check;
mod collect;
mod context;
mod new;
mod run;
mod rust_scan;
mod swift_scan;

use std::path::Path;
use std::process::ExitCode;

use anyhow::{Result, bail};

pub fn run(root: &Path, args: &[&str]) -> Result<ExitCode> {
    match args {
        ["run", rest @ ..] => run::run(rest),
        ["check", rest @ ..] => check::run(root, rest),
        ["new", rest @ ..] => new::run(root, rest),
        ["baseline", rest @ ..] => baseline::run(root, rest),
        _ => bail!(
            "unknown evidence task `{}`; use `run`, `check`, `new`, or `baseline` (see `cargo xtask help`)",
            args.join(" ")
        ),
    }
}
