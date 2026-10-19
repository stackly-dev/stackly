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
mod instance_dir;
mod inventory;
mod layout;
mod observation;
pub mod operation;
mod ownership;
pub mod provider;
#[cfg(all(test, unix))]
mod test_support;

pub use daemon::{DaemonInfo, daemon_info};
pub use error::{CoreError, ManifestError};
pub use instance_dir::{
    INSTANCE_MANIFEST_MAX_LEN, create_instance_layout, read_instance_manifest,
    write_instance_manifest,
};
pub use inventory::{InstanceLocation, Inventory, InventoryEntry, inventory_instances};
pub use layout::{
    PreparedLayout, ensure_instance_run_dir, ensure_private_dir, ensure_runtime_root,
    prepare_layout,
};
pub use observation::READINESS_MAX_AGE;
pub use ownership::{OwnedPath, check_data_root_overlap, check_owned_path, remove_owned_tree};
