//! OS neutral platform ports for Stackly.
//!
//! Adapters such as `stackly-platform-macos` implement these traits, and the
//! use cases in `stackly-core` depend only on them, through one
//! [`Platform`] bundle. This crate also derives the per user directory
//! layout ([`RootSet`], [`DirectoryLayout`]) without any I/O. Nothing here
//! calls an OS API or reads the environment, so the crate builds for every
//! target.
//!
//! Every trait is sync, object safe, and `Send + Sync`; async callers reach
//! it through `spawn_blocking`.

mod capability;
mod clock;
mod error;
mod fs;
mod identity;
mod info;
mod layout;
mod platform;
mod private_dir;
mod unsupported;

pub use capability::{CapabilityStatus, PlatformCapabilities, PlatformCapability};
pub use clock::{Clock, SystemClock};
pub use error::PlatformError;
pub use fs::{
    DirEntryInfo, FileIdentity, FileInfo, FileKind, Filesystem, REMOVE_TREE_MAX_DEPTH,
    RemoveReport, is_same_or_inside, remove_tree_names,
};
pub use identity::{UserId, UserIdentity};
pub use info::{PlatformInfo, host_target};
pub use layout::{
    DirectoryLayout, DirectoryResolver, FixedRoots, INSTANCE_MANIFEST_FILE, InstancePaths,
    ROOT_OVERRIDE_ENV, RUNTIME_ROOT_MAX, RootKind, RootSet, SOCKET_NAME_MAX, SOCKET_PATH_MAX,
    default_runtime_root, instance_path_entries, validate_root_override,
};
pub use platform::Platform;
pub use private_dir::{PrivateDirViolation, check_private_dir};
pub use unsupported::UnsupportedPlatform;
