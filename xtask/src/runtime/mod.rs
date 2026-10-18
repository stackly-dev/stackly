//! `cargo xtask runtime ...`: the runtime inventory of spec 0003.
//!
//! `check` enforces the inventory and evidence rules, `pin` turns real
//! downloads (or, with `--file`, a maintainer build on this Mac) into the
//! `format`, `urls`, `sha256`, `size`, and `min_os` of an entry, `verify`
//! downloads every available artifact again to prove
//! the pinned values still hold, `spike` runs one artifact under a
//! LaunchAgent, and `record` keeps the native evidence of a clean run in
//! `evidence/runtime/`.

mod cache;
mod check;
mod checksum;
pub(crate) mod inventory;
mod macho;
mod pin;
mod ports;
mod record;
mod spike;
#[cfg(test)]
mod test_zip;
mod verify;

use std::path::Path;
use std::process::ExitCode;

use anyhow::{Result, bail};

pub fn run(root: &Path, args: &[&str]) -> Result<ExitCode> {
    match args {
        ["check"] => check::run(root),
        ["pin", rest @ ..] => pin::run(rest).map(|()| ExitCode::SUCCESS),
        ["verify", rest @ ..] => verify::run(root, rest),
        ["spike", rest @ ..] => spike::run(root, rest),
        ["spike-worker", rest @ ..] => spike::run_worker(rest),
        ["record", report] => record::run(root, Path::new(report)),
        _ => bail!(
            "unknown runtime task `{}`; use `cargo xtask runtime check`, `pin`, `verify`, `spike`, or `record`",
            args.join(" ")
        ),
    }
}
