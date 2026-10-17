//! The Stackly domain model: entities, invariants, and state machines.
//!
//! This crate is the center of the architecture. It imports no other Stackly
//! crate, no async runtime, and no OS API, so it builds for every target.
//! Values validate in their constructor, so a value that exists is always
//! valid.

mod error;
mod id;
mod target;

pub use error::{ValueError, ValueKind};
pub use id::{EnvironmentId, InstanceId, InvalidId, ShortKey};
pub use target::{Arch, Os, Target};
