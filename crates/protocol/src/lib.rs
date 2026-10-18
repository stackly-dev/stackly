//! The Stackly IPC contract (spec 0006): frames, methods, events, the error
//! envelope and catalog, limits, and the pure codecs that read them.
//!
//! These types are the single source for the generated Swift client and for
//! the JSON Schemas in `protocols/schemas/`. Run `cargo xtask codegen` after
//! any change, and keep the shared fixtures in `protocols/fixtures/` in step.
//! A change to the normalized contract digest bumps
//! [`ProtocolVersion::CURRENT`] in the same commit.
//!
//! The crate depends on no other Stackly crate and does no IO: `stacklyd`
//! maps domain types to these DTOs, and `stackly-ipc` moves the bytes.
//!
//! Wire rules every DTO follows: camelCase keys, snake_case unit enum
//! values, data enums tagged `type` and `content`, `#[serde(default)]` on
//! every `Option` and `Vec` field, never `deny_unknown_fields`, `u64` only
//! through [`Revision`], [`Sequence`], and [`Count`], time only through
//! [`Timestamp`], and no `BTreeMap`. Open unit enums end with `Unknown`
//! marked `#[serde(other)]`.

pub mod codes;
mod contract_digest;
pub mod daemon;
pub mod error;
pub mod events;
pub mod frame;
#[doc(hidden)]
pub mod fuzzing;
pub mod handshake;
pub mod json;
pub mod limits;
pub mod message;
mod version;
pub mod wire;

pub use contract_digest::CONTRACT_DIGEST;
pub use error::{ErrorBody, ErrorCategory, ErrorDetail, FieldError};
pub use limits::Limits;
pub use message::{Call, ErrorFrame, Reply, Request, Response, ServerFrame};
pub use version::ProtocolVersion;
pub use wire::{Count, ResourceKind, ResourceRef, Revision, Sequence, Timestamp};

/// Returns the JSON Schema of every top level contract type, keyed by file
/// stem.
///
/// `cargo xtask codegen` writes each one to
/// `protocols/schemas/<stem>.schema.json` and hashes them, with the error
/// catalog, into [`CONTRACT_DIGEST`].
#[must_use]
pub fn json_schemas() -> Vec<(&'static str, schemars::Schema)> {
    vec![
        ("protocol-version", schemars::schema_for!(ProtocolVersion)),
        ("request", schemars::schema_for!(Request)),
        ("server-frame", schemars::schema_for!(ServerFrame)),
    ]
}
