//! Secret values in the daemon (spec 0011): storing, reading, and deleting
//! them through one service and one store thread, generating them, hiding
//! them from every text that leaves the daemon, and handing them to a
//! provider process.
//!
//! A value lives only in a [`SecretValue`](stackly_platform::SecretValue),
//! and only two modules here read it: the redact module, where
//! [`Redactor`] matches known values in text and three helpers copy a
//! value for the [`SecretWorker`], count its characters, and look for a
//! control character for the rules of a secret field, and [`delivery`],
//! which hands values to provider processes and files. `cargo xtask
//! check-deps` rejects a call to `expose` anywhere else.

mod compensator;
pub mod delivery;
pub mod error;
mod generate;
mod redact;
mod resolved;
mod rules;
mod service;
mod worker;

pub use compensator::{SECRET_REF_COMPENSATOR, SecretRefCompensator};
pub use error::{
    ConflictReason, NeedsSecret, SecretError, classify_create_failure, codes, registry_code,
    store_code,
};
pub use generate::{GENERATED_LEN, GenerateError, generate_password};
pub use redact::{MASK, Redactor};
pub use resolved::ResolvedSecrets;
pub use rules::{EffectKind, InvalidValueReason, SecretEffect, SecretFieldRule, SecretFieldRules};
pub use service::{
    ChangeStatus, FieldContext, LearnReport, ProbeReport, PutOutcome, PutRequest, PutTarget,
    PutWarning, RevealPurpose, SecretCaller, SecretChange, SecretDeps, SecretEvents, SecretInput,
    SecretListing, SecretService, StoreStatus,
};
pub use worker::{SecretWorker, WorkerConfig};
