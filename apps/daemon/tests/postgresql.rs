//! The PostgreSQL provider through the real daemon (spec 0019, milestone 2):
//! the managed PostgreSQL 18 runtime from the runtime cache of `cargo xtask`,
//! the `official/postgresql` package, and real `postgres` processes, one test
//! root per test. Every test is native: `#[ignore = "native: ..."]`, run with
//! `cargo test -p stacklyd --test postgresql -- --ignored`. An artifact that
//! is not in the cache ends a test as `BLOCKED`, never as a download.
#![cfg(target_os = "macos")]

#[path = "postgresql/adoption.rs"]
mod adoption;
#[path = "common/mod.rs"]
mod common;
#[path = "postgresql/compat.rs"]
mod compat;
#[path = "postgresql/config.rs"]
mod config;
#[path = "supervisor/config_calls.rs"]
mod config_calls;
#[path = "postgresql/create.rs"]
mod create;
#[path = "postgresql/create_cli.rs"]
mod create_cli;
#[path = "postgresql/create_ports.rs"]
mod create_ports;
#[path = "postgresql/endpoints.rs"]
mod endpoints;
#[path = "postgresql/failure.rs"]
mod failure;
#[path = "postgresql/format.rs"]
mod format;
#[path = "postgresql/harness.rs"]
mod harness;
#[path = "postgresql/install.rs"]
mod install;
#[path = "postgresql/lifecycle.rs"]
mod lifecycle;
#[path = "postgresql/lifecycle_ops.rs"]
mod lifecycle_ops;
#[path = "postgresql/logs.rs"]
mod logs;
#[path = "postgresql/metrics.rs"]
mod metrics;
#[path = "supervisor/metrics_calls.rs"]
mod metrics_calls;
#[path = "postgresql/mismatch.rs"]
mod mismatch;
#[path = "postgresql/phases.rs"]
mod phases;
#[path = "postgresql/readiness.rs"]
mod readiness;
#[path = "postgresql/reconcile.rs"]
mod reconcile;
#[path = "postgresql/reload.rs"]
mod reload;
#[path = "postgresql/role_failure.rs"]
mod role_failure;
#[path = "postgresql/same_runtime.rs"]
mod same_runtime;
#[path = "postgresql/security.rs"]
mod security;
#[path = "postgresql/sentinel.rs"]
mod sentinel;
#[path = "postgresql/side_by_side.rs"]
mod side_by_side;
#[path = "postgresql/stop.rs"]
mod stop;
#[path = "supervisor/support.rs"]
mod support;
#[path = "postgresql/tools.rs"]
mod tools;
