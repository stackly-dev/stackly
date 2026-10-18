//! Integration tests for the `stackly-ipc` transport (spec 0006 milestone
//! 2): the private listener and its peer check, framing limits and timers,
//! the client's endpoint checks, and the verified client against a small
//! fake daemon. Every socket lives in the run folder of its own `TestRoot`.
#![cfg(unix)]

mod client;
mod common;
mod endpoint;
mod framing;
mod listener;
