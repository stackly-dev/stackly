//! The `stacklyd` library: everything the daemon does, so `main.rs` stays
//! thin and tests can drive the parts in process (spec 0006, *Daemon*).
//!
//! [`run`] is the whole daemon. [`lock`] keeps one daemon per data root,
//! [`log_sink`] writes every log line without ever blocking the daemon,
//! [`server`] binds the socket and serves [`connection`]s through the
//! [`dispatch`] handlers, whose operation methods live in [`ipc`],
//! [`operations`] holds the operation engine, [`secrets`] the secret service, [`supervisor`] the process
//! supervisor, [`events`] is the event bus, [`launch`] the launch record of the
//! background agent,
//! [`state`] holds the health, [`catalog`] the provider catalog, [`storage`]
//! the disk capacity method, and
//! [`shutdown`] the stop order's pieces. No other crate depends on this
//! library.

#[cfg(unix)]
pub mod catalog;
#[cfg(unix)]
pub mod connection;
#[cfg(unix)]
pub mod dispatch;
#[cfg(unix)]
pub mod events;
#[cfg(unix)]
pub mod ipc;
#[cfg(unix)]
pub mod launch;
#[cfg(unix)]
pub mod lock;
#[cfg(unix)]
pub mod log_sink;
#[cfg(unix)]
pub mod map;
#[cfg(unix)]
pub mod operations;
#[cfg(unix)]
pub mod secrets;
#[cfg(unix)]
pub mod server;
#[cfg(unix)]
pub mod shutdown;
pub mod startup;
#[cfg(unix)]
pub mod state;
#[cfg(unix)]
pub mod storage;
#[cfg(unix)]
pub mod supervisor;

pub use startup::{
    CATALOG_DIR_ENV, EXIT_LOCK_HELD, EXIT_RESTART, EXIT_START_FAILED, LOG_FILTER_ENV, StartConfig,
    parse_catalog_dir, parse_log_filter, run,
};
