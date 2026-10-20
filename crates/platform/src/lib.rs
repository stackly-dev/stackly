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
//!
//! The secret store port ([`SecretStore`]) carries values only inside
//! [`SecretValue`], which never prints itself and wipes its memory on drop
//! (spec 0011).
//!
//! The process supervision port ([`ProcessSupervision`]) spawns main
//! processes suspended until their identity is recorded, checks a
//! process's identity again before every signal, and has no way to kill a
//! process that ran, except a helper's group (spec 0010).

mod capability;
mod clock;
mod error;
mod fs;
mod identity;
mod info;
mod layout;
mod platform;
mod private_dir;
mod process;
mod secret;
mod tree;
mod unsupported;

pub use capability::{CapabilityStatus, PlatformCapabilities, PlatformCapability};
pub use clock::{Clock, SystemClock};
pub use error::PlatformError;
pub use fs::{
    AppendFile, DirEntryInfo, FileIdentity, FileInfo, FileKind, Filesystem, REMOVE_TREE_MAX_DEPTH,
    ReadSeekFile, RemoveReport, VolumeSpace, is_same_or_inside, remove_tree_names,
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
pub use process::{
    CapturedOutput, ChildRole, ExitListener, ExitNotice, ExitStatus, HELPER_CAPTURE_MAX,
    HelperHandle, HoldsFile, InstanceLock, LockOutcome, LockState, MainHandle, OutputStream,
    OutputTarget, ProcessError, ProcessFacts, ProcessSupervision, ProviderSignal, RequestPart,
    SignalTarget, SpawnRequest, SpoolReader, SpoolWatermark, SpoolWriter, StdinSource,
    SuspendedChild, UnsupportedProcessSupervision,
};
pub use secret::{
    Interaction, MAX_SECRET_LEN, SCRUB_ANYWHERE_LEN, SCRUB_MIN_LEN, SECRET_SERVICE, SecretBackend,
    SecretPresence, SecretStore, SecretStoreError, SecretValue, SecretValueError, StoreState,
    UnsupportedSecretStore, WriteMode,
};
pub use tree::{
    ENTRY_NAME_MAX_LEN, ENTRY_PATH_MAX_DEPTH, ENTRY_PATH_MAX_LEN, EntryPath, EntryPathError,
    FileMode, TreeFile, TreeWriter,
};
pub use unsupported::UnsupportedPlatform;
