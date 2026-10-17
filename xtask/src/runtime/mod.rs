//! `cargo xtask runtime ...`: the runtime inventory of spec 0003.
//!
//! `check` enforces the inventory and evidence rules, `pin` turns real
//! downloads into the `urls`, `sha256`, `size`, and `min_os` of an entry,
//! and `verify` downloads every available artifact again to prove the
//! pinned values still hold. The spike, `record`, and `watch` arrive with
//! the next build steps of spec 0003.

mod cache;
mod check;
mod checksum;
mod inventory;
mod macho;
mod pin;
mod verify;

use std::path::Path;
use std::process::ExitCode;

use anyhow::{Result, bail};

pub fn run(root: &Path, args: &[&str]) -> Result<ExitCode> {
    match args {
        ["check"] => check::run(root),
        ["pin", rest @ ..] => pin::run(rest).map(|()| ExitCode::SUCCESS),
        ["verify", rest @ ..] => verify::run(root, rest),
        _ => bail!(
            "unknown runtime task `{}`; use `cargo xtask runtime check`, `runtime pin`, or `runtime verify`",
            args.join(" ")
        ),
    }
}
