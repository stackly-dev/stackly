//! `stackly doctor` (spec 0034, AC-11, AC-15, AC-18). Against a scripted
//! daemon: the text and `--json` goldens of the shared `doctor.run`
//! fixture, the exit codes of errors, warnings, a daemon that is not there,
//! one of another protocol major, and one without `doctor.run`, the params
//! `--instance` and `--check` send, and the escaping of daemon text.
//! Against a real `stacklyd`: a port held by the test, and the record of a
//! failed daemon start.
#![cfg(target_os = "macos")]

#[path = "../common/mod.rs"]
mod common;

mod real;
mod scripted;
