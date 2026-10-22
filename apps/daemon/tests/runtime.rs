//! The `runtime` methods of stacklyd (spec 0018 build step 11): a daemon in
//! process on a real registry in a test root, a download served by the fake
//! fetcher, fake code signing and a fake process for the version check, and
//! real IPC clients over its Unix socket.
#![cfg(target_os = "macos")]

#[path = "runtime/install.rs"]
mod install;
#[path = "runtime/instances_untouched.rs"]
mod instances_untouched;
#[path = "runtime/read.rs"]
mod read;
#[path = "runtime/resolve.rs"]
mod resolve;
#[path = "runtime/support.rs"]
mod support;
