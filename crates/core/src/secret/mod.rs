//! Secret values in the daemon (spec 0011): generating them, hiding them
//! from every text that leaves the daemon, and handing them to a provider
//! process.
//!
//! A value lives only in a [`SecretValue`](stackly_platform::SecretValue),
//! and only two modules here read it: [`Redactor`] matches known values in
//! text, and [`delivery`] hands values to provider processes and files.
//! `cargo xtask check-deps` rejects a call to `expose` anywhere else.

pub mod delivery;
mod generate;
mod redact;
mod resolved;

pub use generate::{GENERATED_LEN, GenerateError, generate_password};
pub use redact::{MASK, Redactor};
pub use resolved::ResolvedSecrets;
