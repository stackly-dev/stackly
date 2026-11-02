//! Stop All (spec 0031, *Pilihan stop all* and *Operasi `stop_all_instances`*):
//! the reply of `instance.preview_stop_all`, the params of `instance.stop_all`,
//! and its reply once the operation ended.
//!
//! Both methods choose instances through one rule, so the preview shows what
//! the operation would stop. The preview is a read; the operation is the only
//! one that writes.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use typeshare::typeshare;
use uuid::Uuid;

use crate::instance::{InstanceAutostart, InstanceLifecycle};
use crate::json::Validate;
use crate::wire::Count;

/// Why an instance is wanted running, from its active lease. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DemandSourceKind {
    /// A person started it.
    Manual,
    /// It starts at login and this session holds the lease.
    Startup,
    /// An environment holds it.
    Environment,
    /// A source this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// One instance Stop All would stop.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StopAllItem {
    /// The instance.
    pub instance_id: Uuid,
    /// Its name.
    pub name: String,
    /// Its lifecycle now.
    pub lifecycle: InstanceLifecycle,
    /// The sources of its active leases.
    pub demand: Vec<DemandSourceKind>,
    /// Its startup policy and the lease of this session.
    pub autostart: InstanceAutostart,
}

/// An instance named without its state.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct InstanceRef {
    /// The instance.
    pub instance_id: Uuid,
    /// Its name.
    pub name: String,
}

/// An instance Stop All leaves alone, and why.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StopAllSkip {
    /// The instance.
    pub instance_id: Uuid,
    /// Its name.
    pub name: String,
    /// The error code that says why, such as `supervisor.identity_unproven`.
    pub code: String,
}

/// Reply of `instance.preview_stop_all`. Every list is ordered by name and
/// then ID.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StopAllPreview {
    /// The instances `instance.stop_all` would stop.
    pub items: Vec<StopAllItem>,
    /// The instances that are stopping already, on their own.
    pub already_stopping: Vec<InstanceRef>,
    /// The instances left alone.
    pub skipped: Vec<StopAllSkip>,
    /// How many of `items` start at login.
    pub at_login_count: Count,
}

/// Params of `instance.stop_all`. None yet; the struct keeps room for
/// fields without a new major.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StopAllParams {}

impl Validate for StopAllParams {}

/// One instance that did not stop, with the error code that says why.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StopAllEntry {
    /// The instance.
    pub instance_id: Uuid,
    /// The error code, such as `supervisor.stop_timeout`.
    pub code: String,
}

/// Reply of `instance.stop_all`, once the operation ended.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StopAllResult {
    /// The instances that stopped.
    pub stopped: Vec<Uuid>,
    /// The instances whose stop failed.
    pub failed: Vec<StopAllEntry>,
    /// The instances left alone, including the ones a cancel reached.
    pub skipped: Vec<StopAllEntry>,
}
