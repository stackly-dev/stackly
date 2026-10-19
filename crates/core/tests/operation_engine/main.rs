//! The operation engine of spec 0007 end to end on the fakes:
//! `FakeRegistry`, `FakePlatform` in a `TestRoot`, and the scripted works
//! of `stackly_core::operation::testing`.
#![cfg(unix)]

mod cancel;
mod close_interrupted;
mod common;
mod compensation;
mod failures;
mod flow;
mod restart;
