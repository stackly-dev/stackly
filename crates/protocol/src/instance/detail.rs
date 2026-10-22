//! The DTOs of `instance.get_detail` and `instance.get_data_usage` (spec
//! 0021, *Method IPC*): everything the instance detail page and `stackly
//! instance inspect` show beyond `InstanceSummary`, and the size of the data
//! folder, which is measured only when asked.
//!
//! No DTO here holds a secret value. A connection string travels as text
//! parts and secret references, and the client joins it with the values it
//! fetched through `secret.reveal` (`connection::compose`).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use typeshare::typeshare;
use uuid::Uuid;

use super::connection::ConnectionPart;
use crate::catalog::HostTarget;
use crate::error::FieldError;
use crate::json::{FieldCode, Validate};
use crate::runtime::InstallModeKind;
use crate::secret::SecretStatus;
use crate::wire::{Count, Revision, Timestamp};

/// Params of `instance.get_detail`.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GetInstanceDetailParams {
    /// The instance to describe.
    pub instance_id: Uuid,
}

impl Validate for GetInstanceDetailParams {}

/// When an instance starts on its own. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StartupPolicyKind {
    /// Only when the user starts it.
    Manual,
    /// When the user logs in.
    AtLogin,
    /// A policy this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// Whether an environment owns an instance it holds. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DependentOwnership {
    /// Only this environment uses the instance.
    Dedicated,
    /// Other environments may use it too.
    Shared,
    /// An ownership this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// The pinned provider revision of an instance.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DetailProvider {
    /// The provider name, such as `official/postgresql`.
    pub name: String,
    /// The pinned revision.
    pub revision: String,
    /// The SHA-256 of the pinned manifest.
    pub manifest_sha256: String,
    /// `metadata.displayName` of the pinned package; null when the package
    /// cannot be read. Never taken from the catalog.
    #[serde(default)]
    pub display_name: Option<String>,
    /// False when the pinned package cannot be read, or the package found
    /// is not the pinned one; then `displayName` is null and the detail has
    /// no connection. True otherwise, also for a package without a
    /// `connection` block.
    pub package_readable: bool,
}

/// The runtime an instance runs on.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DetailRuntime {
    /// The runtime ID.
    pub id: Uuid,
    /// The exact version.
    pub version: String,
    /// The OS and architecture it was built for.
    pub target: HostTarget,
    /// Whether Stackly installed it or it lives in a folder the user owns.
    pub install_mode: InstallModeKind,
    /// The SHA-256 of the installed artifact; null for an external runtime.
    #[serde(default)]
    pub artifact_sha256: Option<String>,
}

/// One secret an instance uses.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DetailSecret {
    /// The settings field.
    pub field: String,
    /// The title from the schema of the pinned package, else the field name.
    pub title: String,
    /// The reference `secret.reveal` takes.
    pub reference: String,
    /// What the store held when the daemon last looked; `unknown` before the
    /// first look.
    pub status: SecretStatus,
}

/// An environment that holds the instance.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DetailDependent {
    /// The environment.
    pub environment_id: Uuid,
    /// Its name.
    pub name: String,
    /// Whether the environment owns the instance.
    pub ownership: DependentOwnership,
    /// Whether the environment holds an active lease on the instance.
    pub lease_active: bool,
}

/// Whether a connection string could be built. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionAvailability {
    /// The string is ready except for its secrets.
    Available,
    /// The string cannot be built now; `reason` says why.
    Unavailable,
    /// A state this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// Why a connection string cannot be built. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionUnavailableReason {
    /// An endpoint the string names has no effective port yet.
    PortNotAssigned,
    /// The package of the instance cannot be used to render the string.
    PackageUnavailable,
    /// The instance was removed and kept.
    Retained,
    /// A reason this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// One connection string of an instance, in manifest order.
///
/// When `state` is `available`, `masked` holds the string with every secret
/// as `****`, `parts` is not empty, and `reason` is null. When it is
/// `unavailable`, `reason` is set, `masked` is null, and `parts` is empty.
/// [`ConnectionString::is_consistent`] checks that.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionString {
    /// The entry name from the manifest, such as `uri`.
    pub name: String,
    /// The label a client shows.
    pub title: String,
    /// Whether the string could be built.
    pub state: ConnectionAvailability,
    /// Set if and only if the string is unavailable.
    #[serde(default)]
    pub reason: Option<ConnectionUnavailableReason>,
    /// The string with each secret as four stars; set if and only if the
    /// string is available.
    #[serde(default)]
    pub masked: Option<String>,
    /// The string in order: encoded text and secret references, never a
    /// value. Empty when unavailable.
    pub parts: Vec<ConnectionPart>,
    /// True when the port may differ the next time the instance starts.
    pub port_may_change: bool,
}

impl ConnectionString {
    /// Whether `state`, `reason`, `masked`, and `parts` agree with each
    /// other as the type documents. `Unrecognized` states are consistent
    /// with anything, because a newer daemon defines them.
    #[must_use]
    pub fn is_consistent(&self) -> bool {
        match self.state {
            ConnectionAvailability::Available => {
                self.masked.is_some() && !self.parts.is_empty() && self.reason.is_none()
            }
            ConnectionAvailability::Unavailable => {
                self.masked.is_none() && self.parts.is_empty() && self.reason.is_some()
            }
            ConnectionAvailability::Unrecognized => true,
        }
    }
}

/// Reply of `instance.get_detail`.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct InstanceDetail {
    /// The instance.
    pub instance_id: Uuid,
    /// The revision of the instance this detail was built from. A client
    /// drops a detail older than the one it holds.
    pub revision: Revision,
    /// The pinned provider revision.
    pub provider: DetailProvider,
    /// The runtime.
    pub runtime: DetailRuntime,
    /// The absolute path of the data folder.
    pub data_path: String,
    /// When the instance starts on its own.
    pub startup: StartupPolicyKind,
    /// One row per secret the instance uses, ordered by field.
    pub secrets: Vec<DetailSecret>,
    /// The environments that hold the instance, ordered by name and then ID.
    pub dependents: Vec<DetailDependent>,
    /// The connection strings, in manifest order; none when the pinned
    /// package cannot be read or declares none.
    pub connections: Vec<ConnectionString>,
}

/// Params of `instance.get_data_usage`.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GetDataUsageParams {
    /// The instance whose data folder is measured.
    pub instance_id: Uuid,
    /// Measure again even when a fresh result is cached.
    #[serde(default)]
    pub refresh: bool,
    /// Never start a scan: answer from the cache, or with a null `usage`.
    #[serde(default)]
    pub cached_only: bool,
}

impl Validate for GetDataUsageParams {
    /// `refresh` and `cachedOnly` contradict each other, so both true is
    /// `cachedOnly` with `invalid_value`.
    fn validate(&self) -> Vec<FieldError> {
        if self.refresh && self.cached_only {
            vec![FieldError::new("cachedOnly", FieldCode::InvalidValue)]
        } else {
            Vec::new()
        }
    }
}

/// The size of a data folder.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DataUsage {
    /// The allocated bytes (blocks times 512) of every entry counted.
    pub bytes: Count,
    /// False when the scan stopped at a limit, so `bytes` is a lower bound.
    pub complete: bool,
    /// When the scan finished.
    pub scanned_at: Timestamp,
}

/// Reply of `instance.get_data_usage`.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DataUsageReply {
    /// The measured size; null when `cachedOnly` found no fresh result.
    #[serde(default)]
    pub usage: Option<DataUsage>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(refresh: bool, cached_only: bool) -> GetDataUsageParams {
        GetDataUsageParams {
            instance_id: Uuid::nil(),
            refresh,
            cached_only,
        }
    }

    #[test]
    fn refresh_and_cached_only_together_break_cached_only() {
        let fields = params(true, true).validate();

        assert_eq!(fields.len(), 1);
        assert_eq!(
            (fields[0].path(), fields[0].code()),
            ("cachedOnly", "invalid_value")
        );
        assert!(params(true, false).validate().is_empty());
        assert!(params(false, true).validate().is_empty());
        assert!(params(false, false).validate().is_empty());
    }

    #[test]
    fn flags_default_to_false_when_absent() {
        let read: GetDataUsageParams =
            serde_json::from_str(r#"{"instanceId":"00000000-0000-0000-0000-000000000000"}"#)
                .expect("reads");

        assert!(!read.refresh && !read.cached_only);
    }

    #[test]
    fn a_connection_string_is_consistent_only_when_its_fields_agree() {
        let available = ConnectionString {
            name: "uri".to_owned(),
            title: "Connection string".to_owned(),
            state: ConnectionAvailability::Available,
            reason: None,
            masked: Some("http://x@y".to_owned()),
            parts: vec![ConnectionPart::text("http://x@y")],
            port_may_change: false,
        };
        let unavailable = ConnectionString {
            state: ConnectionAvailability::Unavailable,
            reason: Some(ConnectionUnavailableReason::PortNotAssigned),
            masked: None,
            parts: Vec::new(),
            ..available.clone()
        };
        let mixed = ConnectionString {
            reason: Some(ConnectionUnavailableReason::Retained),
            ..available.clone()
        };
        let empty = ConnectionString {
            parts: Vec::new(),
            ..available.clone()
        };

        assert!(available.is_consistent());
        assert!(unavailable.is_consistent());
        assert!(!mixed.is_consistent());
        assert!(!empty.is_consistent());
    }
}
