//! In process tests of the daemon server (spec 0006 milestone 4): the
//! server runs inside the test with a test root, its lock, and handlers a
//! test may wrap, while clients connect over the real Unix socket.
#![cfg(target_os = "macos")]

mod catalog;
mod common;
mod events;
mod handshake;
mod launch;
mod lifecycle;
mod limits;
mod secret;
mod secret_log;
mod shutdown;
