//! The `stacklyd` library: everything the daemon does, so `main.rs` stays
//! thin and tests can drive the parts in process (spec 0006, *Daemon*).
//!
//! [`run`] is the whole daemon. [`lock`] keeps one daemon per data root,
//! and [`log_sink`] writes every log line without ever blocking the
//! daemon. No other crate depends on this library.

#[cfg(unix)]
pub mod lock;
#[cfg(unix)]
pub mod log_sink;
pub mod startup;

pub use startup::{
    EXIT_LOCK_HELD, EXIT_START_FAILED, LOG_FILTER_ENV, StartConfig, parse_log_filter, run,
};
