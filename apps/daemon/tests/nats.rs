//! The NATS provider through the real daemon (spec 0023, milestone 2): the
//! managed NATS 2.15.1 runtime from the runtime cache of `cargo xtask`, the
//! `official/nats` package, and real `nats-server` processes, one test root
//! per test, with a data root whose name holds a space and Unicode. Every
//! test is native: `#[ignore = "native: ..."]`, run with
//! `cargo test -p stacklyd --test nats -- --ignored`. An artifact that is
//! not in the cache ends a test as `BLOCKED`, never as a download.
#![cfg(target_os = "macos")]

#[path = "common/mod.rs"]
mod common;
#[path = "nats/compatibility.rs"]
mod compatibility;
#[path = "nats/harness.rs"]
mod harness;
#[path = "nats/lifecycle.rs"]
mod lifecycle;
#[path = "supervisor/support.rs"]
mod support;
