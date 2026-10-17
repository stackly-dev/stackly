//! Stackly application use cases.
//!
//! Each use case is a thin, sync function over the domain and the platform
//! ports: it takes a [`stackly_platform::Platform`] and runs the same way on
//! the macOS adapter and on the fakes in `stackly-testkit`. Async callers
//! reach it through `spawn_blocking`. Engines (runtime, instance,
//! supervisor, provider, environment, operation, diagnostics) start as
//! modules here and move to their own crate once their boundary is proven.

mod daemon;
mod error;
mod layout;
mod ownership;

pub use daemon::{DaemonInfo, daemon_info};
pub use error::CoreError;
pub use layout::{PreparedLayout, ensure_instance_run_dir, prepare_layout};
pub use ownership::{OwnedPath, check_owned_path};
