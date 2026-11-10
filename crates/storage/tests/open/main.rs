//! Integration tests for opening the registry (spec 0005 milestone 1):
//! create, schema, migration with snapshots, recovery detection, and the
//! frozen fixtures. Every test runs in its own `TestRoot` with the fake
//! platform, so nothing touches the real Application Support folder.
#![cfg(unix)]

mod audit;
mod common;
mod create;
mod detect;
mod failure;
mod fixture;
mod journal;
mod locations;
mod migrate;
mod schema;
mod secret;
mod supervision;
