//! The ClickHouse provider through the real daemon (spec 0043, milestone 2):
//! the managed ClickHouse 26.8.30.8 runtime from the runtime cache of
//! `cargo xtask`, the `official/clickhouse` package, and real
//! `clickhouse server` processes, one test root per test, with a data root
//! whose name holds a space and Unicode. Every test is native:
//! `#[ignore = "native: ..."]`, run with
//! `cargo test -p stacklyd --test clickhouse -- --ignored --test-threads=1`
//! (each test root installs the 892 MB runtime). An artifact that is not in
//! the cache ends a test as `BLOCKED`, never as a download.
#![cfg(target_os = "macos")]

#[path = "clickhouse/binary.rs"]
mod binary;
#[path = "common/mod.rs"]
mod common;
#[path = "clickhouse/compatibility.rs"]
mod compatibility;
#[path = "supervisor/config_calls.rs"]
mod config_calls;
#[path = "clickhouse/failure.rs"]
mod failure;
#[path = "clickhouse/harness.rs"]
mod harness;
#[path = "clickhouse/isolation.rs"]
mod isolation;
#[path = "clickhouse/lifecycle.rs"]
mod lifecycle;
#[path = "clickhouse/reload.rs"]
mod reload;
#[path = "clickhouse/security.rs"]
mod security;
#[path = "supervisor/support.rs"]
mod support;
