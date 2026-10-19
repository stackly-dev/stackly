//! The secret service of spec 0011 build step 4 on fakes: the store worker
//! and its limits, `put`, list, status, and the probes, reveal and delete
//! with their audit rows, `resolve_for_start` with the needs secret
//! blocker, `learn_running`, and the operation hooks of feature 11.
#![cfg(unix)]

mod access;
mod common;
mod engine;
mod put;
mod resolve;
mod status;
mod worker;
