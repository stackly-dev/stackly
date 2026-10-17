//! macOS adapters for the Stackly platform traits.
//!
//! [`MacosPlatform::new`] resolves the per user roots from the user database
//! and the private temporary directory (or from `STACKLY_ROOT`, which the
//! binary passes in) and assembles a [`stackly_platform::Platform`].
//!
//! This is the only crate where `unsafe` code may live. Every `unsafe` block
//! needs a `// SAFETY:` comment, and `cargo xtask check-deps` rejects this
//! opt out anywhere else.
#![allow(unsafe_code)]

mod dirs;
mod fs;
mod identity;
mod info;
mod platform;

pub use fs::MacosFilesystem;
pub use identity::MacosIdentity;
pub use info::MacosPlatformInfo;
pub use platform::MacosPlatform;
