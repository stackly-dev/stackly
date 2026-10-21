//! macOS adapters for the Stackly platform traits.
//!
//! [`MacosPlatform::new`] resolves the per user roots from the user database
//! and the private temporary directory (or from `STACKLY_ROOT`, which the
//! binary passes in) and assembles a [`stackly_platform::Platform`].
//!
//! [`MacosProcessSupervision`] starts, watches, and signals the processes
//! of instances (spec 0010); [`MacosPlatform`] installs it with
//! `Platform::with_processes`.
//!
//! [`MacosFetcher`] downloads runtime artifacts over HTTPS with the macOS
//! trust store (spec 0018); [`MacosPlatform`] installs it with
//! `Platform::with_fetcher`.
//!
//! [`MacosSecretStore`] keeps secret values in the login Keychain, or in a
//! file keychain of its own for a root override and tests (spec 0011); the
//! daemon installs it with `Platform::with_secrets`.
//!
//! This is the only crate where `unsafe` code may live. Every `unsafe` block
//! needs a `// SAFETY:` comment, and `cargo xtask check-deps` rejects this
//! opt out anywhere else.
#![allow(unsafe_code)]

mod dirs;
mod fetch;
mod fs;
mod identity;
mod info;
mod platform;
mod process;
mod secret;

pub use fetch::{CONNECT_TIMEOUT, IDLE_TIMEOUT, MacosFetcher};
pub use fs::MacosFilesystem;
pub use identity::MacosIdentity;
pub use info::MacosPlatformInfo;
pub use platform::MacosPlatform;
#[doc(hidden)]
pub use process::KinfoFacts;
pub use process::MacosProcessSupervision;
pub use secret::{KeychainSetupError, KeychainTarget, MacosSecretStore, map_status};
