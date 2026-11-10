//! `stackly audit list` against a scripted daemon (spec 0035, AC-16): the
//! table and its golden from the shared `audit.list` fixture, the params
//! each flag sends, a name looked up and a whole UUID sent as it is, the
//! exit code of an unknown name and of refused flags, and the `--json`
//! envelope with the page. The same command against a real `stacklyd` is
//! in `tests/instance/audit_real.rs`.
#![cfg(target_os = "macos")]

#[path = "../common/mod.rs"]
mod common;

mod list;
