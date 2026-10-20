//! The process supervisor through the real daemon server (spec 0010 build
//! steps 13 and 14): the instance methods over the socket, the fake runtime
//! and the fake provider package from `install_fake`, real processes of the
//! dummy server, and one test root per test. The daemon runs in process
//! unless a test needs to kill it, which uses the `stacklyd` binary.
#![cfg(target_os = "macos")]

mod create;
mod lifecycle;
mod support;
