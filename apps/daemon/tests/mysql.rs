//! The MySQL provider through the real daemon (spec 0042, milestone 2): the
//! managed MySQL 8.4 runtime from the runtime cache of `cargo xtask`, the
//! `official/mysql` package, and real `mysqld` processes, one test root per
//! test. Every test is native: `#[ignore = "native: ..."]`, run with
//! `cargo test -p stacklyd --test mysql -- --ignored --test-threads 1`. An
//! artifact that is not in the cache ends a test as `BLOCKED`, never as a
//! download.
#![cfg(target_os = "macos")]

#[path = "mysql/blocked.rs"]
mod blocked;
#[path = "common/mod.rs"]
mod common;
#[path = "mysql/harness.rs"]
mod harness;
#[path = "supervisor/support.rs"]
mod support;
