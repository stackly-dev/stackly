//! The NATS provider through the real daemon (specs 0023 and 0039): the
//! managed NATS 2.15.1 runtime from the runtime cache of `cargo xtask`, the
//! `official/nats` package, and real `nats-server` processes, one test root
//! per test, with a data root whose name holds a space and Unicode. Every
//! test is native: `#[ignore = "native: ..."]`, run with
//! `cargo test -p stacklyd --test nats -- --ignored`. An artifact that is
//! not in the cache ends a test as `BLOCKED`, never as a download.
#![cfg(target_os = "macos")]

#[path = "nats/client.rs"]
mod client;
#[path = "common/mod.rs"]
mod common;
#[path = "nats/compatibility.rs"]
mod compatibility;
#[path = "nats/config.rs"]
mod config;
#[path = "supervisor/config_calls.rs"]
mod config_calls;
#[path = "nats/harness.rs"]
mod harness;
#[path = "nats/lifecycle.rs"]
mod lifecycle;
#[path = "nats/lifecycle_ops.rs"]
mod lifecycle_ops;
#[path = "nats/metrics.rs"]
mod metrics;
#[path = "supervisor/metrics_calls.rs"]
mod metrics_calls;
#[path = "supervisor/support.rs"]
mod support;
