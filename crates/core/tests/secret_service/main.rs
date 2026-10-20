//! The secret service of spec 0011 build steps 4 and 4a on fakes: the
//! store worker and its limits, `put` with the rules of its secret field,
//! list, status, and the probes, reveal and delete with their audit rows,
//! `resolve_for_start` with the needs secret blocker, `learn_running`, the
//! operation hooks of feature 11, and how a create maps a failed `put`.
#![cfg(unix)]

mod access;
mod common;
mod create_failure;
mod effects;
mod engine;
mod printable_only;
mod put;
mod resolve;
mod status;
mod value_sourcing;
mod worker;
