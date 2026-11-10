//! `stackly diagnostics preview` and `export` (spec 0034, AC-14, AC-16).
//! Against a scripted daemon: the text and `--json` goldens of the shared
//! fixtures, what `export` sends, the confirmation with and without a
//! terminal, the destination made absolute, and the exit code of each
//! diagnostics error. Against a real `stacklyd`: the exported file is the
//! previewed zip, nothing is replaced without `--force`, and a secret the
//! daemon learned never reaches the CLI's output.
#![cfg(target_os = "macos")]

#[path = "../common/mod.rs"]
mod common;

mod real;
mod scripted;
