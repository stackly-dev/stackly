//! The MySQL provider through the real daemon (spec 0042, milestone 2): the
//! managed MySQL 8.4 runtime from the runtime cache of `cargo xtask`, the
//! `official/mysql` package, and real `mysqld` processes, one test root per
//! test. Every test is native: `#[ignore = "native: ..."]`, run with
//! `cargo test -p stacklyd --test mysql -- --ignored --test-threads 1`. An
//! artifact that is not in the cache ends a test as `BLOCKED`, never as a
//! download.
#![cfg(target_os = "macos")]

#[path = "mysql/adoption.rs"]
mod adoption;
#[path = "mysql/auth.rs"]
mod auth;
#[path = "mysql/blocked.rs"]
mod blocked;
#[path = "common/mod.rs"]
mod common;
#[path = "supervisor/config_calls.rs"]
mod config_calls;
#[path = "mysql/doctor.rs"]
mod doctor;
#[path = "mysql/endpoints.rs"]
mod endpoints;
#[path = "mysql/format.rs"]
mod format;
#[path = "mysql/harness.rs"]
mod harness;
#[path = "mysql/init.rs"]
mod init;
#[path = "mysql/lifecycle.rs"]
mod lifecycle;
#[path = "mysql/open_with.rs"]
mod open_with;
#[path = "mysql/paths.rs"]
mod paths;
#[path = "mysql/readiness.rs"]
mod readiness;
#[path = "mysql/stop.rs"]
mod stop;
#[path = "supervisor/support.rs"]
mod support;
#[path = "mysql/tools.rs"]
mod tools;
