//! The `instance` area (spec 0010, *Status dan event* and *Permukaan IPC*):
//! the DTOs of `instance.create`, `instance.start`, `instance.stop`,
//! `instance.get`, and `instance.list`, the three instance events, and
//! `daemon.reconciling`.
//!
//! `InstanceSummary` is the one snapshot every client shows: the registry
//! row plus the state the supervisor keeps in memory. Its status fields
//! (readiness, health, process, restart, blocker, `logsUnavailable`) follow
//! `statusSeq`, never the revision: a client applies a status update when
//! its `statusSeq` is greater than the one it holds for that instance.
//! `stacklyd` maps the domain and core types to these DTOs and never sends
//! a domain type.

pub mod command;
pub mod connection;
mod create;
mod detail;
mod duplicate;

pub use create::{
    CreateEndpoint, CreateForm, CreatePreflight, CreateProvider, CreateProviderSource,
    EndpointPurpose, FieldApply, FieldKind, FieldVisibility, FormField, FormItemField,
    GetCreateFormParams, PreflightCheck, PreflightCheckKind, PreflightCreateParams,
    PreflightStatus, ResolvedProvider, ScalarSetting, SecretPlan, SecretSource, SettingEntry,
    SettingItem, SettingValue,
};
pub use detail::{
    ConnectionAvailability, ConnectionString, ConnectionUnavailableReason, DataUsage,
    DataUsageReply, DependentOwnership, DetailDependent, DetailProvider, DetailRuntime,
    DetailSecret, GetDataUsageParams, GetInstanceDetailParams, InstanceDetail, StartupPolicyKind,
};
pub use duplicate::{DuplicateForm, DuplicateSecret, DuplicateSource, GetDuplicateFormParams};

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use typeshare::typeshare;
use uuid::Uuid;

use crate::catalog::provider_id_code;
use crate::error::FieldError;
use crate::json::{FieldCode, Validate};
use crate::operation::OperationAccepted;
use crate::secret::{NeedsSecretBlocker, SecretInput};
use crate::wire::{Count, Revision, Sequence, Timestamp};

/// The longest `instance.create` `name`, in bytes. The format is checked
/// by the daemon (VAL-001), so a bad name still gets a full preflight.
pub const INSTANCE_NAME_MAX: usize = 256;
/// The longest `displayName`, in bytes.
pub const INSTANCE_DISPLAY_NAME_MAX: usize = 512;
/// The longest `provider.revision`, in bytes.
pub const PROVIDER_REVISION_MAX: usize = 64;
/// The longest endpoint name or secret field key, in bytes.
pub const INSTANCE_KEY_MAX: usize = 64;
/// The most endpoint requests one `instance.create` carries.
pub const CREATE_ENDPOINTS_MAX: usize = 16;
/// The most secret fields one `instance.create` carries.
pub const CREATE_SECRETS_MAX: usize = 64;
/// The smallest page size `instance.list` accepts.
pub const INSTANCE_LIST_LIMIT_MIN: u32 = 1;
/// The largest page size `instance.list` accepts.
pub const INSTANCE_LIST_LIMIT_MAX: u32 = 200;
/// The page size of `instance.list` when `limit` is absent.
pub const INSTANCE_LIST_LIMIT_DEFAULT: u32 = 100;
/// The longest `before` cursor `instance.list` accepts, in bytes
/// (`<name>_<uuid>`).
pub const INSTANCE_CURSOR_MAX: usize = 128;
/// The most JSON bytes the summaries of one `instance.list` page add up to
/// (900 KiB), so the reply always fits in a control frame.
pub const INSTANCE_PAGE_BYTES_MAX: usize = 900 * 1024;

/// The lifecycle of an instance (PRD section 19.1). Closed: `unknown` is a
/// real state and the set is fixed, so a new value is a protocol major.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum InstanceLifecycle {
    /// Created; the data folder is not initialized yet.
    Uninitialized,
    /// The data folder is being initialized.
    Initializing,
    /// Not running.
    Stopped,
    /// The process is starting.
    Starting,
    /// The process runs.
    Running,
    /// The process is stopping.
    Stopping,
    /// Initialization, a start, or the process failed.
    Failed,
    /// The process crashed and is being restarted.
    Recovering,
    /// The identity of the process is lost; it is never signalled.
    Unknown,
    /// Removed with its data kept.
    Retained,
}

/// Whether the person wants an instance running. Closed.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum InstanceDesired {
    /// The person wants it running.
    Running,
    /// The person wants it stopped.
    Stopped,
}

/// Readiness of an instance as read now. Closed.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum InstanceReadinessState {
    /// Nothing is known, or the last observation is too old.
    Unknown,
    /// The process runs but does not accept work yet.
    NotReady,
    /// The instance accepts work.
    Ready,
}

/// Health of an instance as read now. Closed.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum InstanceHealthState {
    /// Nothing is known, or the last observation is too old.
    Unknown,
    /// Everything works.
    Healthy,
    /// It works with problems.
    Degraded,
    /// It does not work.
    Unhealthy,
}

/// How an instance came to be managed here. Open. Not the registry column
/// `instances.ownership` (where the data came from: `created`, later
/// `restored` and `adopted`, spec 0005), and not the `Ownership` of an
/// environment in the domain (`Dedicated` or `Shared`): this says whether
/// a person runs the instance by hand or an environment owns it. `stacklyd`
/// sends `manual` until feature 44 adds environment membership.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum InstanceOwnership {
    /// A person starts and stops it.
    #[default]
    Manual,
    /// A value this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// The transport of an endpoint. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EndpointTransport {
    /// TCP.
    Tcp,
    /// UDP.
    Udp,
    /// A Unix domain socket.
    Unix,
    /// A transport this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// How an IP endpoint asked for its port. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EndpointPortMode {
    /// Exactly the requested port.
    Fixed,
    /// Any free port, given when the process starts.
    Auto,
    /// A mode this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// The package an instance runs, pinned when it was created.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProviderPin {
    /// The provider ID, such as `official/nats`.
    pub name: String,
    /// The revision of the pinned package.
    pub revision: String,
    /// SHA-256 of the pinned package, in lowercase hex.
    pub manifest_sha256: String,
}

/// Readiness as read now (`effective` at the time of the snapshot).
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct InstanceReadiness {
    /// The state.
    pub state: InstanceReadinessState,
    /// A snake case code saying why, such as `probe_ok` or
    /// `daemon_restarted`.
    pub reason: String,
    /// Redacted text with more about the reason.
    #[serde(default)]
    pub detail: Option<String>,
    /// When the observation was made.
    pub observed_at: Timestamp,
}

/// Health as read now (`effective` at the time of the snapshot).
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct InstanceHealth {
    /// The state.
    pub state: InstanceHealthState,
    /// A snake case code saying why.
    pub reason: String,
    /// Redacted text with more about the reason.
    #[serde(default)]
    pub detail: Option<String>,
    /// When the observation was made.
    pub observed_at: Timestamp,
}

/// The process of an instance.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct InstanceProcess {
    /// The process ID.
    pub pid: u32,
    /// When the process started.
    pub started_at: Timestamp,
    /// True for a process the daemon found running after its own restart.
    pub adopted: bool,
    /// Whether a daemon restart can adopt the process; null until
    /// readiness succeeded once.
    #[serde(default)]
    pub adoptable: Option<bool>,
}

/// How the last start, init, or run failed.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct InstanceLastFailure {
    /// The failure code, such as `process_exited` or `crash_loop`.
    pub code: String,
    /// When it happened; null after a daemon restart.
    #[serde(default)]
    pub at: Option<Timestamp>,
    /// The exit code, if the process exited.
    #[serde(default)]
    pub exit_code: Option<i32>,
    /// The signal number, if a signal ended the process.
    #[serde(default)]
    pub signal: Option<i32>,
    /// At most ten redacted lines from the end of the process output,
    /// stderr first, each at most 256 characters.
    #[serde(default)]
    pub excerpt: Vec<String>,
}

/// The restart policy's state.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct InstanceRestart {
    /// Crashes in the sliding window.
    pub attempts: u32,
    /// The respawns the window allows.
    pub budget: u32,
    /// The window, in seconds.
    pub window_seconds: u32,
    /// When the next respawn happens, while one waits.
    #[serde(default)]
    pub next_attempt_at: Option<Timestamp>,
    /// True after the budget ran out.
    pub exhausted: bool,
}

/// The result of one identity check. Open. `not_checked` is spelled
/// `notChecked` on the wire.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum IdentityCheckResult {
    /// The check ran and passed.
    Passed,
    /// The check ran and failed.
    Failed,
    /// The check did not run, because an earlier one already decided the
    /// outcome or an input was missing.
    NotChecked,
    /// A result this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// The seven checks that prove a process belongs to an instance. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum IdentityCheckName {
    /// The boot ID now equals the recorded one.
    BootMatches,
    /// A live process has the recorded PID.
    PidExists,
    /// Its start time equals the recorded one.
    StartMatches,
    /// Its executable is the recorded canonical path.
    ExecutableMatches,
    /// It runs as the user running now.
    UserMatches,
    /// Its session ID equals the recorded one.
    SessionMatches,
    /// It holds the instance lock file open.
    HoldsLock,
    /// A check this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// Whether the instance lock is held, as a probe saw it. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum InstanceLockState {
    /// Nobody holds the lock.
    Free,
    /// A process holds the lock.
    Held,
    /// A state this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// The result of one identity check, by name.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct IdentityCheckEntry {
    /// Which check.
    pub name: IdentityCheckName,
    /// What it found.
    pub result: IdentityCheckResult,
}

/// What gathering evidence found about a process, in the order the checks
/// run. Checks that did not run are `notChecked`.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct IdentityEvidence {
    /// One entry per check.
    #[serde(default)]
    pub checks: Vec<IdentityCheckEntry>,
    /// The PID of the recorded observation.
    #[serde(default)]
    pub pid: Option<u32>,
    /// The state of the instance lock.
    #[serde(default)]
    pub lock: Option<InstanceLockState>,
    /// True when the process is still suspended: its daemon died after
    /// spawning it and before resuming it.
    pub stopped: bool,
    /// Redacted text of the error that stopped the gathering.
    #[serde(default)]
    pub error: Option<String>,
}

/// The content of the `dataInUse` and `identityUnproven` blockers.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct IdentityBlocker {
    /// What the checks found.
    pub evidence: IdentityEvidence,
    /// When the evidence was gathered.
    pub observed_at: Timestamp,
}

/// Why an instance cannot start or be trusted, one value with the priority
/// `identityUnproven`, then `dataInUse`, then `needsSecret`. A blocker this
/// build does not know makes the snapshot fail to decode, so a new blocker
/// is a protocol major.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", content = "content", rename_all = "camelCase")]
pub enum InstanceBlocker {
    /// A process matches the recorded identity but does not hold the
    /// instance lock, or gathering the evidence failed.
    IdentityUnproven(IdentityBlocker),
    /// A process Stackly cannot prove is the instance's own holds the
    /// instance lock.
    DataInUse(IdentityBlocker),
    /// A secret the instance needs cannot be read (spec 0011).
    NeedsSecret(NeedsSecretBlocker),
}

/// One endpoint of an instance.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EndpointStatus {
    /// The endpoint name from the manifest.
    pub name: String,
    /// The transport.
    pub transport: EndpointTransport,
    /// The host of an IP endpoint.
    #[serde(default)]
    pub host: Option<String>,
    /// The effective port: the bound one if there is one, else the `Fixed`
    /// port; null for an `Auto` endpoint without a bound port.
    #[serde(default)]
    pub port: Option<u16>,
    /// How the port was asked for.
    pub port_mode: EndpointPortMode,
    /// True only after readiness succeeded for the current process.
    pub bound: bool,
    /// The socket path of a `unix` endpoint, null while its port is not
    /// known.
    #[serde(default)]
    pub socket_path: Option<String>,
}

/// One instance, as `instance.get`, `instance.list`, and
/// `instance.state_changed` show it.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct InstanceSummary {
    /// The instance ID, a lowercase UUID.
    pub id: Uuid,
    /// The unique instance name.
    pub name: String,
    /// The name to show, if the person gave one.
    #[serde(default)]
    pub display_name: Option<String>,
    /// The revision of the registry row.
    pub revision: Revision,
    /// The software, such as `postgresql`.
    pub software: String,
    /// The pinned package.
    pub provider: ProviderPin,
    /// The runtime the instance runs on.
    pub runtime_id: Uuid,
    /// The exact version of that runtime.
    pub runtime_version: String,
    /// The lifecycle.
    pub lifecycle: InstanceLifecycle,
    /// What the person wants.
    pub desired: InstanceDesired,
    /// Readiness.
    pub readiness: InstanceReadiness,
    /// Health.
    pub health: InstanceHealth,
    /// Counts the changes of the status fields per instance and per daemon
    /// run, from 1; it starts again from 1 after a daemon restart.
    pub status_seq: Sequence,
    /// The process, if one runs.
    #[serde(default)]
    pub process: Option<InstanceProcess>,
    /// The last failure, if there was one.
    #[serde(default)]
    pub last_failure: Option<InstanceLastFailure>,
    /// The restart policy.
    pub restart: InstanceRestart,
    /// Why the instance is blocked, if it is.
    #[serde(default)]
    pub blocker: Option<InstanceBlocker>,
    /// Every endpoint.
    #[serde(default)]
    pub endpoints: Vec<EndpointStatus>,
    /// PIDs of group members left after the last stop; reported, never
    /// signalled.
    #[serde(default)]
    pub leftover_processes: Vec<u32>,
    /// True when the output of the running process cannot be read.
    pub logs_unavailable: bool,
    /// Who runs the instance. Absent in older daemons; read as `manual`.
    #[serde(default)]
    pub ownership: InstanceOwnership,
    /// Whether the data folder was initialized (`data_initialized_at` is
    /// set). Absent in older daemons, which means true; read it through
    /// [`InstanceSummary::is_initialized`]. It is an `Option` so the Swift
    /// type is optional too: typeshare reads only a plain
    /// `#[serde(default)]` as optional, and this default is true.
    #[serde(default)]
    pub initialized: Option<bool>,
}

impl InstanceSummary {
    /// Whether the data folder was initialized; an absent field reads as
    /// true.
    #[must_use]
    pub fn is_initialized(&self) -> bool {
        self.initialized.unwrap_or(true)
    }
}

/// A JSON object of provider settings, as `instance.create` carries it.
///
/// It decodes only from a JSON object; the daemon checks the keys and the
/// values against the settings schema of the pinned package. Secret values
/// never travel here: a secret field is named in `secrets`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SettingsDocument(serde_json::Map<String, serde_json::Value>);

impl SettingsDocument {
    /// Wraps a JSON object.
    #[must_use]
    pub const fn new(map: serde_json::Map<String, serde_json::Value>) -> Self {
        Self(map)
    }

    /// The JSON object.
    #[must_use]
    pub const fn as_map(&self) -> &serde_json::Map<String, serde_json::Value> {
        &self.0
    }

    /// Takes the JSON object.
    #[must_use]
    pub fn into_map(self) -> serde_json::Map<String, serde_json::Value> {
        self.0
    }
}

impl JsonSchema for SettingsDocument {
    fn schema_name() -> Cow<'static, str> {
        "SettingsDocument".into()
    }

    fn inline_schema() -> bool {
        true
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "object",
            "additionalProperties": true,
        })
    }
}

/// The provider an instance is created from.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSelector {
    /// The provider ID, such as `official/nats`.
    pub name: String,
    /// The revision to pin. Absent means the revision of the loaded
    /// catalog. The first daemon builds refuse an absent revision with
    /// `request.invalid_params` at `provider.revision`.
    #[serde(default)]
    pub revision: Option<String>,
}

/// How an IP endpoint asks for its port. Tagged `type` and `content`:
/// `{ "type": "fixed", "content": 5432 }` or `{ "type": "auto" }`.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", content = "content", rename_all = "camelCase")]
pub enum PortRequest {
    /// Exactly this port, never a substitute.
    Fixed(u16),
    /// Any free port, picked when the process starts.
    Auto,
}

/// The port request of one manifest endpoint.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EndpointRequest {
    /// The endpoint name from the manifest.
    pub name: String,
    /// The port it asks for.
    pub port: PortRequest,
}

/// Where the value of one secret field comes from.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SecretFieldInput {
    /// The settings field the secret fills.
    pub field: String,
    /// Where its value comes from.
    pub input: SecretInput,
}

/// Params of `instance.create`.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateInstanceParams {
    /// The unique instance name.
    pub name: String,
    /// The name to show.
    #[serde(default)]
    pub display_name: Option<String>,
    /// The provider to create it from.
    pub provider: ProviderSelector,
    /// The installed runtime to run it on.
    pub runtime_id: Uuid,
    /// One request per manifest endpoint that asks for a port.
    #[serde(default)]
    pub endpoints: Vec<EndpointRequest>,
    /// The settings, without secret values.
    pub settings: SettingsDocument,
    /// The secret fields and where their values come from.
    #[serde(default)]
    pub secrets: Vec<SecretFieldInput>,
}

/// The field code of a text that must be 1 to `max` bytes, or `None`.
fn length_code(text: &str, max: usize) -> Option<FieldCode> {
    if text.is_empty() {
        Some(FieldCode::TooShort)
    } else if text.len() > max {
        Some(FieldCode::TooLong)
    } else {
        None
    }
}

impl Validate for CreateInstanceParams {
    /// Sizes and counts only; the format of the name, the ports, and the
    /// settings is checked by the daemon so a preflight answers every row.
    /// Each `secrets[i].input` runs [`SecretInput::field_errors`], so an
    /// empty value, a NUL, or a value above the limit is
    /// `secrets[<i>].input.content`.
    fn validate(&self) -> Vec<FieldError> {
        let mut fields = Vec::new();
        if let Some(code) = length_code(&self.name, INSTANCE_NAME_MAX) {
            fields.push(FieldError::new("name", code));
        }
        if let Some(display_name) = &self.display_name
            && let Some(code) = length_code(display_name, INSTANCE_DISPLAY_NAME_MAX)
        {
            fields.push(FieldError::new("displayName", code));
        }
        if let Some(code) = provider_id_code(&self.provider.name) {
            fields.push(FieldError::new("provider.name", code));
        }
        if let Some(revision) = &self.provider.revision
            && let Some(code) = length_code(revision, PROVIDER_REVISION_MAX)
        {
            fields.push(FieldError::new("provider.revision", code));
        }
        if settings_has_unsafe_integer(self.settings.as_map()) {
            fields.push(FieldError::new("settings", FieldCode::OutOfRange));
        }
        if self.endpoints.len() > CREATE_ENDPOINTS_MAX {
            fields.push(FieldError::new("endpoints", FieldCode::TooMany));
        }
        for (index, endpoint) in self.endpoints.iter().enumerate().take(CREATE_ENDPOINTS_MAX) {
            if let Some(code) = length_code(&endpoint.name, INSTANCE_KEY_MAX) {
                fields.push(FieldError::new(format!("endpoints[{index}].name"), code));
            }
        }
        if self.secrets.len() > CREATE_SECRETS_MAX {
            fields.push(FieldError::new("secrets", FieldCode::TooMany));
        }
        for (index, secret) in self.secrets.iter().enumerate().take(CREATE_SECRETS_MAX) {
            if let Some(code) = length_code(&secret.field, INSTANCE_KEY_MAX) {
                fields.push(FieldError::new(format!("secrets[{index}].field"), code));
            }
            fields.extend(
                secret
                    .input
                    .field_errors(&format!("secrets[{index}].input")),
            );
        }
        fields
    }
}

/// Whether any integer in `settings` is beyond `MAX_SAFE_INTEGER` in size,
/// the same rule every wire integer follows, so a client that reads numbers
/// as `Double` never loses a digit. The size of the object is bounded by
/// the control frame and its depth by `MAX_JSON_DEPTH`, which the request
/// decoder checks before this runs.
fn settings_has_unsafe_integer(map: &serde_json::Map<String, serde_json::Value>) -> bool {
    use crate::limits::MAX_SAFE_INTEGER;
    let mut pending: Vec<&serde_json::Value> = map.values().collect();
    while let Some(value) = pending.pop() {
        match value {
            serde_json::Value::Number(number) => {
                let unsafe_u64 = number.as_u64().is_some_and(|n| n > MAX_SAFE_INTEGER);
                let unsafe_i64 = number
                    .as_i64()
                    .is_some_and(|n| n.unsigned_abs() > MAX_SAFE_INTEGER);
                if unsafe_u64 || unsafe_i64 {
                    return true;
                }
            }
            serde_json::Value::Array(items) => pending.extend(items),
            serde_json::Value::Object(members) => pending.extend(members.values()),
            _ => {}
        }
    }
    false
}

/// Params of `instance.start`.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StartInstanceParams {
    /// The instance to start.
    pub instance_id: Uuid,
    /// Refuse with `resource.revision_mismatch` unless the instance is at
    /// this revision.
    #[serde(default)]
    pub expected_revision: Option<Revision>,
}

impl Validate for StartInstanceParams {}

/// Params of `instance.stop`.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StopInstanceParams {
    /// The instance to stop.
    pub instance_id: Uuid,
    /// Refuse with `resource.revision_mismatch` unless the instance is at
    /// this revision.
    #[serde(default)]
    pub expected_revision: Option<Revision>,
    /// Stop even when an active environment lease holds the instance.
    pub override_leases: bool,
}

impl Validate for StopInstanceParams {}

/// The reply of `instance.start` and `instance.stop`.
///
/// An action that changes something starts an operation and returns it in
/// `accepted`. An instance already in the asked state answers
/// `alreadyInState` with no operation and no bound idempotency key.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct InstanceActionAccepted {
    /// The operation, as created or as it stands now; null when nothing
    /// ran.
    #[serde(default)]
    pub accepted: Option<OperationAccepted>,
    /// True when the instance was already in the asked state.
    pub already_in_state: bool,
}

/// Params of `instance.get`.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GetInstanceParams {
    /// The instance to read, retained ones included.
    pub instance_id: Uuid,
}

impl Validate for GetInstanceParams {}

/// Params of `instance.list`.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListInstancesParams {
    /// Most instances on the page, 1 to 200; absent means 100.
    #[serde(default)]
    pub limit: Option<u32>,
    /// The `next` cursor of the previous page, sent back unchanged.
    #[serde(default)]
    pub before: Option<String>,
}

impl ListInstancesParams {
    /// The page size: `limit`, or [`INSTANCE_LIST_LIMIT_DEFAULT`].
    #[must_use]
    pub fn limit_or_default(&self) -> u32 {
        self.limit.unwrap_or(INSTANCE_LIST_LIMIT_DEFAULT)
    }
}

impl Validate for ListInstancesParams {
    fn validate(&self) -> Vec<FieldError> {
        let mut fields = Vec::new();
        if self.limit.is_some_and(|limit| {
            !(INSTANCE_LIST_LIMIT_MIN..=INSTANCE_LIST_LIMIT_MAX).contains(&limit)
        }) {
            fields.push(FieldError::new("limit", FieldCode::OutOfRange));
        }
        if let Some(before) = &self.before {
            if before.len() > INSTANCE_CURSOR_MAX {
                fields.push(FieldError::new("before", FieldCode::TooLong));
            } else if !is_instance_cursor(before) {
                fields.push(FieldError::new("before", FieldCode::InvalidFormat));
            }
        }
        fields
    }
}

/// Whether `text` is `<name>_<uuid>`, the opaque cursor of `instance.list`.
fn is_instance_cursor(text: &str) -> bool {
    text.rsplit_once('_')
        .is_some_and(|(name, id)| !name.is_empty() && id.parse::<Uuid>().is_ok())
}

/// Reply of `instance.list`: ordered by name, then ID, without retained
/// instances.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct InstancePage {
    /// The instances on this page.
    #[serde(default)]
    pub items: Vec<InstanceSummary>,
    /// The cursor of the next page, to send back as `before`; null on the
    /// last page. A page stops before its JSON passes
    /// [`INSTANCE_PAGE_BYTES_MAX`].
    #[serde(default)]
    pub next: Option<String>,
}

/// Content of `instance.state_changed`: the instance after a batch that
/// raised its revision (create, lifecycle, desired state, an `Auto` port).
/// The event's resource is the instance and its revision the instance's.
/// A retained instance is announced with lifecycle `retained`.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct InstanceStateChanged {
    /// The instance now.
    pub instance: InstanceSummary,
}

/// Content of `instance.health_changed`: the status fields of an instance
/// after the effective readiness, health, blocker, restart status,
/// process, or log availability changed. A client applies it when
/// `statusSeq` is greater than the one it holds for the instance, whatever
/// the event's revision says.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct InstanceHealthChanged {
    /// The instance.
    pub instance_id: Uuid,
    /// The status counter after this change.
    pub status_seq: Sequence,
    /// Readiness.
    pub readiness: InstanceReadiness,
    /// Health.
    pub health: InstanceHealth,
    /// Why the instance is blocked, if it is.
    #[serde(default)]
    pub blocker: Option<InstanceBlocker>,
    /// The restart policy.
    pub restart: InstanceRestart,
    /// The process, if one runs.
    #[serde(default)]
    pub process: Option<InstanceProcess>,
    /// True when the output of the running process cannot be read.
    pub logs_unavailable: bool,
}

/// Content of `instance.removed`: the instance row was deleted. The event's
/// revision is the last revision plus one, and a client drops the instance
/// from its list. No daemon sends it before feature 20.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct InstanceRemoved {
    /// The instance that is gone.
    pub instance_id: Uuid,
}

/// The phase of the reconciliation. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReconcilePhase {
    /// Reconciliation began.
    Started,
    /// Reconciliation ended.
    Finished,
    /// A phase this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// Content of `daemon.reconciling`, a daemon event published at the start
/// and at the end of the reconciliation.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DaemonReconciling {
    /// Started or finished.
    pub phase: ReconcilePhase,
    /// Instances looked at.
    pub instances: Count,
    /// Processes adopted.
    pub adopted: Count,
    /// Instances left `unknown`.
    pub unknown: Count,
    /// Instances that restart.
    pub restarting: Count,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secret::SecretText;

    const ID: &str = "0192f3a0-7c1e-7b2a-9d4e-5f6a7b8c9d04";

    fn params() -> CreateInstanceParams {
        CreateInstanceParams {
            name: "orders-db".to_owned(),
            display_name: None,
            provider: ProviderSelector {
                name: "official/postgresql".to_owned(),
                revision: Some("2026.10.1".to_owned()),
            },
            runtime_id: ID.parse().unwrap(),
            endpoints: vec![EndpointRequest {
                name: "postgres".to_owned(),
                port: PortRequest::Fixed(5432),
            }],
            settings: SettingsDocument::default(),
            secrets: vec![SecretFieldInput {
                field: "password".to_owned(),
                input: SecretInput::Generate,
            }],
        }
    }

    fn paths(params: &CreateInstanceParams) -> Vec<(String, String)> {
        params
            .validate()
            .iter()
            .map(|field| (field.path().to_owned(), field.code().to_owned()))
            .collect()
    }

    fn pair(path: &str, code: &str) -> (String, String) {
        (path.to_owned(), code.to_owned())
    }

    #[test]
    fn a_complete_create_request_has_no_field_errors() {
        assert_eq!(paths(&params()), Vec::new());
    }

    #[test]
    fn create_checks_sizes_and_leaves_formats_to_the_daemon() {
        let mut bad = params();
        bad.name = String::new();
        bad.display_name = Some("x".repeat(INSTANCE_DISPLAY_NAME_MAX + 1));
        bad.provider.name = "Official".to_owned();
        bad.provider.revision = Some(String::new());
        assert_eq!(
            paths(&bad),
            [
                pair("name", "too_short"),
                pair("displayName", "too_long"),
                pair("provider.name", "invalid_format"),
                pair("provider.revision", "too_short"),
            ]
        );

        let mut odd = params();
        odd.name = "Not A Name!".to_owned();
        odd.endpoints[0].port = PortRequest::Fixed(0);
        assert_eq!(paths(&odd), Vec::new());
    }

    #[test]
    fn create_accepts_an_absent_provider_revision() {
        let mut absent = params();
        absent.provider.revision = None;
        assert_eq!(paths(&absent), Vec::new());
    }

    #[test]
    fn create_counts_endpoints_and_secrets_and_checks_their_keys() {
        let mut many = params();
        many.endpoints = (0..=CREATE_ENDPOINTS_MAX)
            .map(|_| EndpointRequest {
                name: String::new(),
                port: PortRequest::Auto,
            })
            .collect();
        many.secrets = Vec::new();
        let fields = paths(&many);
        assert_eq!(fields[0], pair("endpoints", "too_many"));
        assert_eq!(fields.len(), 1 + CREATE_ENDPOINTS_MAX);
        assert_eq!(fields[1], pair("endpoints[0].name", "too_short"));
    }

    #[test]
    fn create_runs_the_secret_input_rules_per_entry() {
        let mut bad = params();
        bad.secrets = vec![
            SecretFieldInput {
                field: "password".to_owned(),
                input: SecretInput::Generate,
            },
            SecretFieldInput {
                field: "token".to_owned(),
                input: SecretInput::Value(SecretText::new(String::new())),
            },
            SecretFieldInput {
                field: "key".to_owned(),
                input: SecretInput::Value(SecretText::new("a\0b".to_owned())),
            },
            SecretFieldInput {
                field: "other".to_owned(),
                input: SecretInput::CopyFrom("not a reference".to_owned()),
            },
        ];
        let fields = paths(&bad);
        assert_eq!(
            fields
                .iter()
                .map(|(path, _)| path.as_str())
                .collect::<Vec<_>>(),
            [
                "secrets[1].input.content",
                "secrets[2].input.content",
                "secrets[3].input.content"
            ]
        );
    }

    #[test]
    fn create_decodes_the_documented_wire_shape() {
        let json = serde_json::json!({
            "name": "orders-db",
            "provider": { "name": "official/postgresql" },
            "runtimeId": ID,
            "endpoints": [
                { "name": "postgres", "port": { "type": "fixed", "content": 5432 } },
                { "name": "admin", "port": { "type": "auto" } }
            ],
            "settings": { "maxConnections": 50 },
            "secrets": [{ "field": "password", "input": { "type": "generate" } }]
        });
        let decoded: CreateInstanceParams = serde_json::from_value(json).unwrap();
        assert_eq!(decoded.provider.revision, None);
        assert_eq!(decoded.endpoints[0].port, PortRequest::Fixed(5432));
        assert_eq!(decoded.endpoints[1].port, PortRequest::Auto);
        assert_eq!(decoded.settings.as_map().len(), 1);
    }

    #[test]
    fn create_refuses_an_integer_in_settings_above_the_safe_range_at_any_depth() {
        let mut big = params();
        big.settings =
            serde_json::from_str(r#"{"a":{"b":[1,9007199254740991,-9007199254740991]}}"#).unwrap();
        assert_eq!(paths(&big), Vec::new());
        for text in [
            r#"{"a":{"b":[1,9007199254740992]}}"#,
            r#"{"a":-9007199254740993}"#,
            r#"{"a":18446744073709551615}"#,
        ] {
            big.settings = serde_json::from_str(text).unwrap();
            assert_eq!(paths(&big), [pair("settings", "out_of_range")], "{text}");
        }
        big.settings = serde_json::from_str(r#"{"a":1.5e300}"#).unwrap();
        assert_eq!(paths(&big), Vec::new());
    }

    #[test]
    fn settings_decode_only_from_a_json_object() {
        assert!(serde_json::from_str::<SettingsDocument>("[1]").is_err());
        assert!(serde_json::from_str::<SettingsDocument>("\"text\"").is_err());
        assert!(serde_json::from_str::<SettingsDocument>("{}").is_ok());
    }

    #[test]
    fn list_params_accept_limits_from_1_to_200_and_a_name_uuid_cursor() {
        for limit in [None, Some(1), Some(100), Some(INSTANCE_LIST_LIMIT_MAX)] {
            let params = ListInstancesParams {
                limit,
                before: None,
            };
            assert_eq!(params.validate(), Vec::new(), "{limit:?}");
        }
        for limit in [0, 201] {
            let params = ListInstancesParams {
                limit: Some(limit),
                before: None,
            };
            assert_eq!(params.validate().len(), 1, "{limit}");
        }
        let cursor = ListInstancesParams {
            limit: None,
            before: Some(format!("orders-db_{ID}")),
        };
        assert_eq!(cursor.validate(), Vec::new());
        for bad in ["orders-db", "_", "orders-db_not-a-uuid"] {
            let params = ListInstancesParams {
                limit: None,
                before: Some(bad.to_owned()),
            };
            assert_eq!(params.validate().len(), 1, "{bad}");
        }
        let long = ListInstancesParams {
            limit: None,
            before: Some("a".repeat(INSTANCE_CURSOR_MAX + 1)),
        };
        assert_eq!(long.validate()[0].code(), "too_long");
        assert_eq!(ListInstancesParams::default().limit_or_default(), 100);
    }

    #[test]
    fn the_status_fields_ownership_and_initialized_read_as_defaults_when_absent() {
        let json = serde_json::json!({
            "id": ID,
            "name": "orders-db",
            "revision": 3,
            "software": "postgresql",
            "provider": { "name": "official/postgresql", "revision": "r1", "manifestSha256": "0".repeat(64) },
            "runtimeId": ID,
            "runtimeVersion": "17.2",
            "lifecycle": "stopped",
            "desired": "stopped",
            "readiness": { "state": "unknown", "reason": "not_running", "observedAt": "2026-10-20T01:00:00.000Z" },
            "health": { "state": "unknown", "reason": "not_running", "observedAt": "2026-10-20T01:00:00.000Z" },
            "statusSeq": 1,
            "restart": { "attempts": 0, "budget": 5, "windowSeconds": 60, "exhausted": false },
            "logsUnavailable": false
        });
        let summary: InstanceSummary = serde_json::from_value(json).unwrap();
        assert_eq!(summary.ownership, InstanceOwnership::Manual);
        assert!(summary.is_initialized());
        assert_eq!(summary.initialized, None);
        assert!(summary.endpoints.is_empty());
        assert_eq!(summary.blocker, None);
    }

    #[test]
    fn an_unknown_ownership_reads_as_unrecognized() {
        let value: InstanceOwnership = serde_json::from_str("\"environment\"").unwrap();
        assert_eq!(value, InstanceOwnership::Unrecognized);
    }

    #[test]
    fn a_not_checked_result_is_spelled_in_camel_case() {
        let text = serde_json::to_string(&IdentityCheckResult::NotChecked).unwrap();
        assert_eq!(text, "\"notChecked\"");
    }

    #[test]
    fn blockers_are_tagged_by_their_camel_case_name() {
        let blocker = InstanceBlocker::DataInUse(IdentityBlocker {
            evidence: IdentityEvidence {
                checks: vec![IdentityCheckEntry {
                    name: IdentityCheckName::HoldsLock,
                    result: IdentityCheckResult::NotChecked,
                }],
                pid: Some(4242),
                lock: Some(InstanceLockState::Held),
                stopped: false,
                error: None,
            },
            observed_at: "2026-10-20T01:00:00Z".parse().unwrap(),
        });
        let value = serde_json::to_value(&blocker).unwrap();
        assert_eq!(value["type"], "dataInUse");
        assert_eq!(
            value["content"]["evidence"]["checks"][0]["name"],
            "holds_lock"
        );
        assert_eq!(
            value["content"]["evidence"]["checks"][0]["result"],
            "notChecked"
        );
    }
}
