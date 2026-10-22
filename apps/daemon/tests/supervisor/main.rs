//! The process supervisor through the real daemon server (spec 0010 build
//! step 14): the instance methods over the socket, the fake runtime and the
//! fake provider package from `install_fake`, real processes of the dummy
//! server, and one test root per test. The daemon runs in process unless a
//! test needs to kill it, which uses the `stacklyd` binary.
#![cfg(target_os = "macos")]

#[path = "../common/mod.rs"]
mod common;

mod actions;
mod budget;
mod create;
mod data_compat;
mod lifecycle;
mod logs;
mod process;
mod restart;
mod support;
