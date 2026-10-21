//! Instance use cases that span more than one engine (spec 0020): creating
//! an instance, from the form and the preflight to the operation, and the
//! fuzz harness of the paths it writes.

pub mod create;
pub mod duplicate;
#[cfg(unix)]
pub mod fuzzing;
