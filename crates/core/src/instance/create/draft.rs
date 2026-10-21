//! The create draft and the constants of the create use case (spec 0020,
//! *Evaluator create* and *Konstanta*).

use stackly_domain::{InstanceId, RuntimeId};

pub use crate::secret::SecretValueShape;

/// The smallest free space `instances/` must have, in bytes ("256 MB").
pub const CREATE_MIN_FREE_BYTES: u64 = 256_000_000;
/// Most candidates the port suggestion tries.
pub const PORT_SUGGESTION_ATTEMPTS: usize = 64;
/// The lowest port the suggestion offers.
pub const PORT_SUGGESTION_MIN: u32 = 1024;
/// Most tries of the `register` step on a registry conflict (spec 0005).
pub const REGISTER_RETRY_LIMIT: usize = 5;
/// Largest suffix of a suggested name (`-2` to `-99`).
pub const NAME_SUGGESTION_MAX_SUFFIX: u32 = 99;
/// The instance ID dry run plans use.
pub const PREFLIGHT_PLACEHOLDER_ID: &str = "00000000-0000-7000-8000-000000000000";

/// The ID dry run plans use in place of the ID of an instance that does
/// not exist yet.
///
/// # Panics
///
/// Never: [`PREFLIGHT_PLACEHOLDER_ID`] is a valid UUID version 7.
#[must_use]
pub fn placeholder_instance_id() -> InstanceId {
    PREFLIGHT_PLACEHOLDER_ID
        .parse()
        .expect("the placeholder instance ID is a valid UUID version 7")
}

/// The provider a draft asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderChoice {
    /// The provider ID, `namespace/name`.
    pub name: String,
    /// The revision, or `None` for the one the loaded catalog holds.
    pub revision: Option<String>,
}

/// What a draft asks for one endpoint's port.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PortRequestDraft {
    /// Any free port, picked when the instance starts.
    Auto,
    /// This port; the wire carries a `u16`, so 0 is the one value
    /// outside 1 to 65535.
    Fixed(u16),
}

/// Where a secret field's value comes from, without the value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecretSourceKind {
    /// The daemon generates it.
    Generate,
    /// The client sends it with `instance.create`.
    Value,
    /// A copy of the value behind this registered reference.
    CopyFrom(String),
    /// The new instance uses this registered reference.
    Bind(String),
}

/// Everything a client asks to create, minus any secret value. Preflight and
/// create build the same draft, so they judge it the same way.
#[derive(Debug, Clone, PartialEq)]
pub struct CreateDraft {
    /// The instance name, as typed.
    pub name: String,
    /// The display name, as typed.
    pub display_name: Option<String>,
    /// The provider.
    pub provider: ProviderChoice,
    /// The runtime, exact.
    pub runtime_id: RuntimeId,
    /// A port request per endpoint name, in request order.
    pub endpoints: Vec<(String, PortRequestDraft)>,
    /// The settings document, without secret fields.
    pub settings: serde_json::Value,
    /// The source of each secret field the client names; a secret field it
    /// does not name is generated.
    pub secrets: Vec<(String, SecretSourceKind)>,
    /// The shape of each `value` secret, only on create.
    pub secret_values: Vec<(String, SecretValueShape)>,
    /// Whether the client starts the instance after create; only the
    /// equivalent command shows it.
    pub start: bool,
}
