//! The Traefik provider through the real daemon (spec 0029, milestone 4, and
//! spec 0041): the
//! managed Traefik 3.7.15 runtime from the runtime cache of `cargo xtask`,
//! the `official/traefik` package, and real `traefik` processes, one test
//! root per test. Every test is native: `#[ignore = "native: ..."]`, run with
//! `cargo test -p stacklyd --test traefik -- --ignored`. An artifact that is
//! not in the cache ends a test as `BLOCKED`, never as a download.
#![cfg(target_os = "macos")]

#[path = "traefik/cli.rs"]
mod cli;
#[path = "common/mod.rs"]
mod common;
#[path = "traefik/compatibility.rs"]
mod compatibility;
#[path = "traefik/config.rs"]
mod config;
#[path = "supervisor/config_calls.rs"]
mod config_calls;
#[path = "traefik/harness.rs"]
mod harness;
#[path = "traefik/readiness.rs"]
mod readiness;
#[path = "traefik/routing.rs"]
mod routing;
#[path = "supervisor/support.rs"]
mod support;
#[path = "traefik/surface.rs"]
mod surface;
#[path = "traefik/watch.rs"]
mod watch;
