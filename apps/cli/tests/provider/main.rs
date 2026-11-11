//! `stackly provider list` and `inspect` (spec 0017, milestone 2, AC-9),
//! `validate` (spec 0036, AC-5), and `import`, `approve`, `revoke`, and
//! `remove` (spec 0037, AC-14):
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
mod trust;
mod trust_data;
mod trust_stacklyd;
mod validate;
