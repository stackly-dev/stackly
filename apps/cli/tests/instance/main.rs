//! `stackly instance create`, `start`, `stop`, and `get` against a scripted
//! daemon (spec 0014, milestone 2): the flag to params mapping, the
//! `instance` rule after a wait or an `alreadyInState`, the goldens, and
//! the exit code rows. `list` and `inspect` (spec 0022, milestone 2) run
//! against the scripted daemon and against a real `stacklyd`.
#![cfg(target_os = "macos")]

#[path = "../common/mod.rs"]
mod common;

mod action;
mod create;
mod data;
mod duplicate;
mod duplicate_real;
mod exit_codes;
mod get;
mod inspect;
mod list;
mod parity;
#[path = "../operation/play.rs"]
mod play;
mod secrets;
mod stacklyd;
