//! `stackly operation` against a scripted daemon (spec 0008, milestone 2):
//! list, inspect, cancel, resume, and abandon with their goldens and exit
//! codes, and the `--wait` matrix with timeouts, Ctrl+C, reconnects, and
//! progress.
#![cfg(target_os = "macos")]

#[path = "../common/mod.rs"]
mod common;

mod commands;
mod play;
mod wait;
