//! The process supervisor through the real daemon server (spec 0010 build
//! step 14): the instance methods over the socket, the fake runtime and the
//! fake provider package from `install_fake`, real processes of the dummy
//! server, and one test root per test. The daemon runs in process unless a
//! test needs to kill it, which uses the `stacklyd` binary.
#![cfg(target_os = "macos")]

#[path = "../common/mod.rs"]
mod common;

mod actions;
mod agent_off;
mod budget;
mod config;
mod crash_loop;
mod create;
mod create_flow;
mod create_form;
mod create_form_pinned;
mod data_compat;
mod data_usage;
mod detail;
mod durable_logs;
mod lifecycle;
mod list_performance;
mod login;
mod logs;
mod metrics;
mod native_keychain;
mod offline;
mod open_with;
mod ports;
mod process;
mod restart;
mod same_runtime;
mod session_end;
mod settings;
mod slow_logs;
mod stop_all;
mod stop_restart_reload;
mod storage_locations;
mod support;
