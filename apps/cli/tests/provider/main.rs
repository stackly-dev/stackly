//! `stackly provider list` and `inspect` (spec 0017, milestone 2, AC-9):
//! against a scripted daemon that answers with the fixtures of
//! `protocols/fixtures/catalog/` for the text and JSON goldens, and against
//! a real `stacklyd` that serves a built catalog folder.
#![cfg(target_os = "macos")]

#[path = "../common/mod.rs"]
mod common;

mod data;
mod inspect;
mod list;
mod stacklyd;
