//! Integration tests for the SQLite registry (spec 0005 steps 10 to 12):
//! commits with their check order, the mapping between rows and
//! aggregates, the queries, and retention. Every test opens its own
//! registry in a `TestRoot` with the fake platform, and the shared
//! `registry_conformance` suite runs against it too (step 13).
#![cfg(unix)]

mod commit;
mod common;
mod conformance;
mod mapping;
mod prune;
mod query;
mod supervision;
