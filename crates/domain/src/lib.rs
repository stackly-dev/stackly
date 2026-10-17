//! The Stackly domain model: entities, invariants, and state machines.
//!
//! This crate is the center of the architecture. It imports no other Stackly
//! crate, no async runtime, and no OS API, so it builds for every target.
//! Values validate in their constructor, so a value that exists is always
//! valid, and value types travel through serde in their text form.

mod error;
mod id;
mod manifest;
mod provider;
mod rule;
mod target;
mod text;
mod value;

pub use error::{InvariantDetail, InvariantError, Machine, TransitionError, ValueError, ValueKind};
pub use id::{
    BackupId, EnvironmentId, InstanceId, InvalidId, LeaseId, OperationId, RuntimeId, ShortKey,
};
pub use manifest::{
    INSTANCE_MANIFEST_SCHEMA, InstallKind, InstanceManifest, ManifestData, ManifestRuntime,
    NewInstanceManifest, RetainedMarker,
};
pub use provider::ProviderRevisionKey;
pub use rule::Rule;
pub use target::{Arch, Os, Target};
pub use value::{
    DisplayName, ExactVersion, InstanceName, ProviderName, RevisionLabel, RuntimeLine,
    Sha256Digest, SoftwareName,
};
