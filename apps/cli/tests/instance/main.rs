//! `stackly instance create`, `start`, `stop`, and `get` against a scripted
//! daemon (spec 0014, milestone 2): the flag to params mapping, the
//! `instance` rule after a wait or an `alreadyInState`, the goldens, and
//! the exit code rows.
#![cfg(target_os = "macos")]

#[path = "../common/mod.rs"]
mod common;

mod action;
mod create;
mod data;
mod exit_codes;
mod get;
#[path = "../operation/play.rs"]
mod play;
mod secrets;
