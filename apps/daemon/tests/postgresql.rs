//! The PostgreSQL provider through the real daemon (spec 0019, milestone 2):
//! the managed PostgreSQL 18 runtime from the runtime cache of `cargo xtask`,
//! the `official/postgresql` package, and real `postgres` processes, one test
//! root per test. Every test is native: `#[ignore = "native: ..."]`, run with
//! `cargo test -p stacklyd --test postgresql -- --ignored`. An artifact that
//! is not in the cache ends a test as `BLOCKED`, never as a download.
#![cfg(target_os = "macos")]

#[path = "common/mod.rs"]
mod common;
#[path = "postgresql/harness.rs"]
mod harness;
#[path = "postgresql/install.rs"]
mod install;
#[path = "postgresql/lifecycle.rs"]
mod lifecycle;
#[path = "supervisor/support.rs"]
mod support;
