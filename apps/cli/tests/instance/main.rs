//! `stackly instance create`, `start`, `stop`, and `get` against a scripted
//! daemon (spec 0014, milestone 2): the flag to params mapping, the
//! `instance` rule after a wait or an `alreadyInState`, the goldens, and
//! the exit code rows. `list` and `inspect` (spec 0022, milestone 2) run
//! against the scripted daemon and against a real `stacklyd`.
#![cfg(target_os = "macos")]

#[path = "../common/mod.rs"]
mod common;

mod action;
mod action_real;
mod audit_real;
mod autostart;
mod config;
mod config_parity;
mod connection;
mod create;
mod create_real;
mod data;
mod duplicate;
mod duplicate_real;
mod exit_codes;
mod failure;
mod get;
mod inspect;
mod list;
mod logs_real;
mod metrics;
mod open;
mod override_leases;
mod parity;
#[path = "../operation/play.rs"]
mod play;
mod secrets;
mod stacklyd;
mod stop_all;
