//! The Valkey provider through the real daemon (spec 0040, milestone 2): the
//! managed Valkey 9.1.2 runtime from the runtime cache of `cargo xtask`, the
//! `official/valkey` package, and real `valkey-server` processes, one test
//! root per test. Every test is native: `#[ignore = "native: ..."]`, run with
//! `cargo test -p stacklyd --test valkey -- --ignored`. An artifact that is
//! not in the cache ends a test as `BLOCKED`, never as a download.
#![cfg(target_os = "macos")]

#[path = "valkey/auth.rs"]
mod auth;
#[path = "common/mod.rs"]
mod common;
#[path = "supervisor/config_calls.rs"]
mod config_calls;
#[path = "valkey/doctor.rs"]
mod doctor;
#[path = "valkey/format.rs"]
mod format;
#[path = "valkey/harness.rs"]
mod harness;
#[path = "valkey/lifecycle.rs"]
mod lifecycle;
#[path = "valkey/owned.rs"]
mod owned;
#[path = "valkey/persistence.rs"]
mod persistence;
#[path = "valkey/readiness.rs"]
mod readiness;
#[path = "valkey/stop.rs"]
mod stop;
#[path = "supervisor/support.rs"]
mod support;
#[path = "valkey/tools.rs"]
mod tools;
