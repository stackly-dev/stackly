//! Duplicating an instance (spec 0024): a way to fill the create form, not
//! an operation. [`gather_duplicate`] reads the registry, the loaded
//! catalog, and the pinned package of the source instance, and nothing
//! under the source's data, log, or run folders. [`build_duplicate`] then
//! builds the form and the create draft from those facts without IO. The
//! draft goes through the preflight and the `create_instance` operation of
//! spec 0020 like any other.

mod build;
mod error;
mod gather;
mod name;

pub use build::{DuplicateModel, build_duplicate};
pub use error::DuplicateError;
pub use gather::{DuplicateDeps, DuplicateFacts, gather_duplicate};
pub use name::duplicate_name;
