//! The create evaluator over fakes (spec 0020, AC-2 to AC-7, AC-9, AC-15,
//! and the evaluation table of *Critical test scenarios*): `gather` reads a
//! `FakePlatform` and a `FakeRegistry`, and `evaluate` judges the draft.
#![cfg(unix)]

mod common;
mod endpoints;
mod form;
mod init_port;
mod listeners;
mod name_runtime_provider;
mod secrets_data;
mod settings;
