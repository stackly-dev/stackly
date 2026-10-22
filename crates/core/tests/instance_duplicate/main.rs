//! Duplicating an instance in the core (spec 0024 AC-1 to AC-5 and the core
//! part of AC-7): `gather_duplicate` reads a `FakePlatform` and a
//! `FakeRegistry`, and `build_duplicate` builds the form and the draft.
#![cfg(unix)]

#[path = "../create_evaluate/common.rs"]
#[allow(dead_code)]
mod common;
mod draft;
mod gather;
mod support;
