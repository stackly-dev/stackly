//! The `stackly` output contract against a scripted daemon (spec 0008):
//! the exit code matrix, the `--json` envelope, text for people, `daemon
//! status` states, request resilience, broken pipes and failed writes, and
//! `-v`.
#![cfg(target_os = "macos")]

#[path = "../common/mod.rs"]
mod common;

mod exit_codes;
mod json;
mod noninteractive;
mod resilience;
mod robust;
mod status;
mod text;
mod verbose;
