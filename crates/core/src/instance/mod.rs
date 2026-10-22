//! Instance use cases that span more than one engine (spec 0020): creating
//! an instance, from the form and the preflight to the operation, the data
//! compatibility check start shares with the rebind check (spec 0025), the
//! connection strings of an instance (spec 0021), and
//! the fuzz harness of the paths it writes.

pub mod connection;
pub mod create;
pub mod data_compat;
pub mod duplicate;
#[cfg(unix)]
pub mod fuzzing;
pub mod rebind;
