//! The `create_instance` operation on fakes (spec 0020 AC-7, AC-8, AC-10 to
//! AC-12, AC-13): the supervisor rig of spec 0010 with the create work, its
//! compensators, and the real dummy server for nothing but its package.
#![cfg(unix)]

#[allow(dead_code)]
#[path = "../supervisor_start/common.rs"]
mod common;

mod cases;
mod harness;
