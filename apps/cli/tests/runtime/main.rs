//! `stackly runtime install`, `list`, and `inspect` against a scripted
//! daemon (spec 0018, milestone 3, AC-16): the flag to params mapping, the
//! result after a wait or an `alreadyInstalled`, the goldens, and the exit
//! code rows.
#![cfg(target_os = "macos")]

#[path = "../common/mod.rs"]
mod common;

mod data;
mod inspect;
mod install;
mod list;
#[path = "../operation/play.rs"]
mod play;
