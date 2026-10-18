//! The `stacklyd` library: everything the daemon does, so `main.rs` stays
//! thin and tests can drive the parts in process (spec 0006, *Daemon*).
//!
//! [`run`] is the whole daemon. [`lock`] keeps one daemon per data root,
//! [`log_sink`] writes every log line without ever blocking the daemon,
//! [`server`] binds the socket and serves [`connection`]s through the
//! [`dispatch`] handlers, [`events`] is the event bus, [`state`] holds the
//! health, and [`shutdown`] the stop order's pieces. No other crate
//! depends on this library.

#[cfg(unix)]
pub mod connection;
#[cfg(unix)]
pub mod dispatch;
#[cfg(unix)]
pub mod events;
#[cfg(unix)]
pub mod lock;
#[cfg(unix)]
pub mod log_sink;
#[cfg(unix)]
pub mod map;
#[cfg(unix)]
pub mod server;
#[cfg(unix)]
pub mod shutdown;
pub mod startup;
#[cfg(unix)]
pub mod state;

pub use startup::{
    EXIT_LOCK_HELD, EXIT_START_FAILED, LOG_FILTER_ENV, StartConfig, parse_log_filter, run,
};
