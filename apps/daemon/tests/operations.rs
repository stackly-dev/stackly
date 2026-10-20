//! Integration tests of the operation engine inside stacklyd (spec 0007
//! build step 15): a daemon in process on a real `SqliteRegistry` in a
//! test root, real IPC clients over its Unix socket, and the test's own
//! method that starts an operation through `start_operation`, since no
//! contract method starts one before feature 10.
#![cfg(target_os = "macos")]

#[path = "operations/flow.rs"]
mod flow;
#[path = "operations/perf.rs"]
mod perf;
#[path = "operations/recovery.rs"]
mod recovery;
#[path = "operations/secret_intent.rs"]
mod secret_intent;
#[path = "operations/secrets.rs"]
mod secrets;
#[path = "operations/support.rs"]
mod support;
