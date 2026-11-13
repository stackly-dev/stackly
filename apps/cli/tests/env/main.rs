//! `stackly env validate`, `plan`, `import`, `list`, and `remove` (spec
//! 0044, milestone 2, AC-16, AC-17) against a scripted daemon that answers
//! with the fixtures of `protocols/fixtures/env/`, the same files the
//! Swift view model tests read, for the text and JSON goldens and the exit
//! codes.
#![cfg(target_os = "macos")]

#[path = "../common/mod.rs"]
mod common;

mod data;
mod manage;
mod plan;
mod validate;
