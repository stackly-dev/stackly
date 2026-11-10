//! Frames and the method table: [`Request`] from clients, [`ServerFrame`]
//! from the daemon, and the [`Call`] and [`Reply`] enums with one variant
//! per method.
//!
//! The frozen parts never change across majors: the `Request` envelope
//! (`requestId`, `idempotencyKey`, `call.type`, `call.content`) and the
//! `error` variant of `ServerFrame`.
//!
//! To add a method: add a variant to [`Call`] and to [`Reply`] with the same
//! name and the same `#[serde(rename)]`, then add one line to the
//! `methods!` table below, saying whether the daemon answers it while the
//! registry is in recovery mode (spec 0006 AC-18). A variant missing from the table does not
//! compile, and the tests check that each table name is the variant's serde
//! tag. typeshare cannot see items a macro expands to, so the two enums stay
//! written by hand.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
use typeshare::typeshare;

use crate::audit::{AuditPage, ListAuditParams};
use crate::catalog::{GetProviderParams, ListProvidersReply, ProviderDetail};
use crate::daemon::{Capabilities, DaemonHealth, DaemonInfo, ShutdownAccepted, ShutdownParams};
use crate::doctor::{DoctorParams, DoctorReport};
use crate::error::{ErrorBody, FieldError};
use crate::events::{Event, SubscribeParams, Subscribed, UnsubscribeParams};
use crate::handshake::{HandshakeAccepted, HandshakeParams};
use crate::instance::config::{
    ApplyConfigParams, ChangeEndpointsParams, ChangeEndpointsReply, ConfigApplyAccepted,
    ConfigValidation, GetConfigParams, InstanceConfigView, KeepRevisionParams,
    RenameInstanceParams, ValidateConfigParams,
};
use crate::instance::{
    CreateForm, CreateInstanceParams, CreatePreflight, DataUsageReply, DuplicateForm,
    ForceStopInstanceParams, GetCreateFormParams, GetDataUsageParams, GetDuplicateFormParams,
    GetInstanceDetailParams, GetInstanceParams, GetMetricsParams, InstanceActionAccepted,
    InstanceDetail, InstancePage, InstanceSummary, ListInstancesParams, MetricsReply,
    PreflightCreateParams, PrepareTerminalParams, PrepareTerminalReply, ReloadInstanceParams,
    RestartInstanceParams, SetStartupParams, StartInstanceParams, StopInstanceParams,
};
use crate::json;
use crate::logs::{
    LogFrame, LogPage, LogsFollowParams, LogsFollowing, LogsReadParams, LogsUnfollowParams,
};
use crate::operation::{
    AbandonOperationParams, AbandonReply, CancelOperationParams, CancelReply, GetOperationParams,
    ListOperationsParams, OperationDetail, OperationPage, OperationStats, OperationSummary,
    ResumeOperationParams,
};
use crate::restart::RestartPreview;
use crate::runtime::{
    GetRuntimeParams, InstallRuntimeParams, InstallRuntimeReply, ListRuntimesParams,
    ResolvedArtifact, RuntimeDetail, RuntimeListPage,
};
use crate::secret::{
    DeleteSecretParams, DeleteSecretReply, ListSecretsParams, ListSecretsReply, RevealSecretParams,
    RevealSecretReply, SecretStatusParams, SecretStatusReply, SetSecretParams, SetSecretReply,
};
use crate::settings::{
    DaemonSettingsView, LocationList, SetLocationParams, StorageLocationView, UpdateSettingsParams,
};
use crate::stop_all::{StopAllParams, StopAllPreview, StopAllResult};
use crate::storage::CapacityReport;

/// The only frame a client sends.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    /// 1 to 64 bytes of `[A-Za-z0-9._:-]`, chosen by the client and unique
    /// on the connection while the request runs. `stackly-ipc` uses a
    /// lowercase UUID v7.
    pub request_id: String,
    /// 1 to 128 bytes of printable ASCII without spaces. Spec 0007 decides
    /// what it means.
    #[serde(default)]
    pub idempotency_key: Option<String>,
    /// The method and its params.
    pub call: Call,
}

/// One variant per method, tagged by the method name. Methods without
/// params are unit variants, written without `content`.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", content = "content")]
pub enum Call {
    /// The first request on every connection.
    #[serde(rename = "daemon.handshake")]
    DaemonHandshake(HandshakeParams),
    /// Who the daemon is and where it keeps things.
    #[serde(rename = "daemon.get_info")]
    DaemonGetInfo,
    /// The daemon's health and counters.
    #[serde(rename = "daemon.get_health")]
    DaemonGetHealth,
    /// Platform capabilities, methods, and event kinds the daemon knows.
    #[serde(rename = "daemon.get_capabilities")]
    DaemonGetCapabilities,
    /// Stops the daemon, or stops it so launchd starts it again.
    #[serde(rename = "daemon.shutdown")]
    DaemonShutdown(ShutdownParams),
    /// The plan a restart asked for now would write (spec 0030).
    #[serde(rename = "daemon.restart_preview")]
    DaemonRestartPreview,
    /// Starts an event subscription.
    #[serde(rename = "events.subscribe")]
    EventsSubscribe(SubscribeParams),
    /// Ends an event subscription.
    #[serde(rename = "events.unsubscribe")]
    EventsUnsubscribe(UnsubscribeParams),
    /// One operation with its steps and created items.
    #[serde(rename = "operation.get")]
    OperationGet(GetOperationParams),
    /// One page of operations, newest first.
    #[serde(rename = "operation.list")]
    OperationList(ListOperationsParams),
    /// Asks an operation to stop.
    #[serde(rename = "operation.cancel")]
    OperationCancel(CancelOperationParams),
    /// Queues an operation that needs recovery again.
    #[serde(rename = "operation.resume")]
    OperationResume(ResumeOperationParams),
    /// Cleans up what it can of an operation that needs recovery, then
    /// closes it.
    #[serde(rename = "operation.abandon")]
    OperationAbandon(AbandonOperationParams),
    /// The engine's counters and limits.
    #[serde(rename = "operation.get_stats")]
    OperationGetStats,
    /// One page of audit rows, newest first (spec 0035).
    #[serde(rename = "audit.list")]
    AuditList(ListAuditParams),
    /// Every catalog entry with the facts about the catalog.
    #[serde(rename = "catalog.list_providers")]
    CatalogListProviders,
    /// One catalog entry with its versions and artifacts.
    #[serde(rename = "catalog.get_provider")]
    CatalogGetProvider(GetProviderParams),
    /// The secret references, from the cached status.
    #[serde(rename = "secret.list")]
    SecretList(ListSecretsParams),
    /// One secret reference after a fresh probe.
    #[serde(rename = "secret.status")]
    SecretStatus(SecretStatusParams),
    /// Fills, generates, copies, or replaces the value behind a reference.
    #[serde(rename = "secret.set")]
    SecretSet(SetSecretParams),
    /// Reads the value behind a reference, recorded in the audit.
    #[serde(rename = "secret.reveal")]
    SecretReveal(RevealSecretParams),
    /// Deletes a reference and the value behind it.
    #[serde(rename = "secret.delete")]
    SecretDelete(DeleteSecretParams),
    /// Creates an instance and starts the operation that initializes it.
    #[serde(rename = "instance.create")]
    InstanceCreate(CreateInstanceParams),
    /// Starts an instance.
    #[serde(rename = "instance.start")]
    InstanceStart(StartInstanceParams),
    /// Stops an instance.
    #[serde(rename = "instance.stop")]
    InstanceStop(StopInstanceParams),
    /// Restarts a running instance as one operation.
    #[serde(rename = "instance.restart")]
    InstanceRestart(RestartInstanceParams),
    /// Reloads the configuration of a running instance.
    #[serde(rename = "instance.reload")]
    InstanceReload(ReloadInstanceParams),
    /// Sends `SIGKILL` to an instance that is still stopping.
    #[serde(rename = "instance.force_stop")]
    InstanceForceStop(ForceStopInstanceParams),
    /// One instance with its status.
    #[serde(rename = "instance.get")]
    InstanceGet(GetInstanceParams),
    /// One page of instances, retained ones left out.
    #[serde(rename = "instance.list")]
    InstanceList(ListInstancesParams),
    /// The space left on the volume that holds the data root.
    #[serde(rename = "storage.get_capacity")]
    StorageGetCapacity,
    /// The daemon settings (spec 0032).
    #[serde(rename = "settings.get")]
    SettingsGet,
    /// Changes daemon settings (spec 0032).
    #[serde(rename = "settings.update")]
    SettingsUpdate(UpdateSettingsParams),
    /// Every storage location and which one is the default (spec 0032).
    #[serde(rename = "storage.list_locations")]
    StorageListLocations,
    /// Points the default location of a kind at a folder, or back at the
    /// built in one (spec 0032).
    #[serde(rename = "storage.set_location")]
    StorageSetLocation(SetLocationParams),
    /// Resolves a selector and starts the operation that installs the
    /// artifact, or answers that it is installed already.
    #[serde(rename = "runtime.install")]
    RuntimeInstall(InstallRuntimeParams),
    /// Resolves a selector to one artifact without installing it.
    #[serde(rename = "runtime.resolve")]
    RuntimeResolve(InstallRuntimeParams),
    /// One page of runtimes, with the installs that have not ended.
    #[serde(rename = "runtime.list")]
    RuntimeList(ListRuntimesParams),
    /// One runtime in detail.
    #[serde(rename = "runtime.get")]
    RuntimeGet(GetRuntimeParams),
    /// The create form of an installed runtime: the provider, its
    /// endpoints, and its settings fields, typed from the package.
    #[serde(rename = "instance.get_create_form")]
    InstanceGetCreateForm(GetCreateFormParams),
    /// Judges a create draft without any secret value and changes nothing.
    #[serde(rename = "instance.preflight")]
    InstancePreflight(PreflightCreateParams),
    /// The create form and the draft of a duplicate of one instance.
    #[serde(rename = "instance.get_duplicate_form")]
    InstanceGetDuplicateForm(GetDuplicateFormParams),
    /// The detail page facts of one instance: provider, runtime, secrets,
    /// dependents, and connection strings.
    #[serde(rename = "instance.get_detail")]
    InstanceGetDetail(GetInstanceDetailParams),
    /// The size of the data folder of one instance.
    #[serde(rename = "instance.get_data_usage")]
    InstanceGetDataUsage(GetDataUsageParams),
    /// The metrics samples of one running instance after a cursor (spec
    /// 0028).
    #[serde(rename = "instance.get_metrics")]
    InstanceGetMetrics(GetMetricsParams),
    /// The active config revision of one instance with its history, drift,
    /// and non sensitive config files (spec 0029).
    #[serde(rename = "instance.config.get")]
    InstanceConfigGet(GetConfigParams),
    /// Judges a config change and writes nothing (spec 0029).
    #[serde(rename = "instance.config.validate")]
    InstanceConfigValidate(ValidateConfigParams),
    /// Writes a config change as the next revision and makes it take
    /// effect (spec 0029).
    #[serde(rename = "instance.config.apply")]
    InstanceConfigApply(ApplyConfigParams),
    /// Writes the config files of the active revision back over a drift
    /// (spec 0029).
    #[serde(rename = "instance.config.keep_revision")]
    InstanceConfigKeepRevision(KeepRevisionParams),
    /// Renames an instance (spec 0029).
    #[serde(rename = "instance.rename")]
    InstanceRename(RenameInstanceParams),
    /// Sets when an instance starts on its own (spec 0031).
    #[serde(rename = "instance.set_startup")]
    InstanceSetStartup(SetStartupParams),
    /// Reads what Stop All would stop (spec 0031).
    #[serde(rename = "instance.preview_stop_all")]
    InstancePreviewStopAll,
    /// Stops every instance that is wanted or runs (spec 0031).
    #[serde(rename = "instance.stop_all")]
    InstanceStopAll(StopAllParams),
    /// Changes the ports of an instance's IP endpoints (spec 0029).
    #[serde(rename = "instance.change_endpoints")]
    InstanceChangeEndpoints(ChangeEndpointsParams),
    /// One page of an instance's stored log (spec 0027).
    #[serde(rename = "instance.logs_read")]
    InstanceLogsRead(LogsReadParams),
    /// Starts pushing an instance's new log entries as `log` frames.
    #[serde(rename = "instance.logs_follow")]
    InstanceLogsFollow(LogsFollowParams),
    /// Ends a log follow.
    #[serde(rename = "instance.logs_unfollow")]
    InstanceLogsUnfollow(LogsUnfollowParams),
    /// Writes the Terminal script of a `terminal` Open In action (spec
    /// 0070).
    #[serde(rename = "instance.prepare_terminal")]
    InstancePrepareTerminal(PrepareTerminalParams),
    /// Runs the read only checks of the doctor (spec 0034). Not an
    /// operation: it writes nothing and holds no lock.
    #[serde(rename = "doctor.run")]
    DoctorRun(DoctorParams),
}

/// One variant per method, with the same names and tags as [`Call`].
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", content = "content")]
pub enum Reply {
    /// The handshake was accepted.
    #[serde(rename = "daemon.handshake")]
    DaemonHandshake(HandshakeAccepted),
    /// Who the daemon is.
    #[serde(rename = "daemon.get_info")]
    DaemonGetInfo(DaemonInfo),
    /// The daemon's health.
    #[serde(rename = "daemon.get_health")]
    DaemonGetHealth(DaemonHealth),
    /// What the daemon supports.
    #[serde(rename = "daemon.get_capabilities")]
    DaemonGetCapabilities(Capabilities),
    /// The daemon accepted the request and now runs the stop order.
    #[serde(rename = "daemon.shutdown")]
    DaemonShutdown(ShutdownAccepted),
    /// The plan a restart would write and what would refuse it.
    #[serde(rename = "daemon.restart_preview")]
    DaemonRestartPreview(RestartPreview),
    /// The subscription started; its events follow.
    #[serde(rename = "events.subscribe")]
    EventsSubscribe(Subscribed),
    /// The subscription ended.
    #[serde(rename = "events.unsubscribe")]
    EventsUnsubscribe,
    /// The operation with its steps and created items.
    #[serde(rename = "operation.get")]
    OperationGet(Box<OperationDetail>),
    /// One page of operations.
    #[serde(rename = "operation.list")]
    OperationList(OperationPage),
    /// The operation after the cancel, and what the cancel did.
    #[serde(rename = "operation.cancel")]
    OperationCancel(Box<CancelReply>),
    /// The operation, queued again.
    #[serde(rename = "operation.resume")]
    OperationResume(Box<OperationSummary>),
    /// The closed operation and what was left over.
    #[serde(rename = "operation.abandon")]
    OperationAbandon(Box<AbandonReply>),
    /// The engine's counters and limits.
    #[serde(rename = "operation.get_stats")]
    OperationGetStats(OperationStats),
    /// One page of audit rows.
    #[serde(rename = "audit.list")]
    AuditList(AuditPage),
    /// Every catalog entry, in display order.
    #[serde(rename = "catalog.list_providers")]
    CatalogListProviders(ListProvidersReply),
    /// One catalog entry in detail.
    #[serde(rename = "catalog.get_provider")]
    CatalogGetProvider(Box<ProviderDetail>),
    /// The secret references and the store.
    #[serde(rename = "secret.list")]
    SecretList(ListSecretsReply),
    /// The reference after the probe, and the store.
    #[serde(rename = "secret.status")]
    SecretStatus(Box<SecretStatusReply>),
    /// The reference after the write, never the value.
    #[serde(rename = "secret.set")]
    SecretSet(Box<SetSecretReply>),
    /// The value; the one reply that carries one.
    #[serde(rename = "secret.reveal")]
    SecretReveal(RevealSecretReply),
    /// The reference that was deleted.
    #[serde(rename = "secret.delete")]
    SecretDelete(DeleteSecretReply),
    /// The operation that creates the instance.
    #[serde(rename = "instance.create")]
    InstanceCreate(Box<InstanceActionAccepted>),
    /// The operation that starts the instance, or that it already runs.
    #[serde(rename = "instance.start")]
    InstanceStart(Box<InstanceActionAccepted>),
    /// The operation that stops the instance, or that it already stopped.
    #[serde(rename = "instance.stop")]
    InstanceStop(Box<InstanceActionAccepted>),
    /// The operation that restarts the instance.
    #[serde(rename = "instance.restart")]
    InstanceRestart(Box<InstanceActionAccepted>),
    /// The operation that reloads the instance.
    #[serde(rename = "instance.reload")]
    InstanceReload(Box<InstanceActionAccepted>),
    /// The operation that force stops the instance, or that it already
    /// stopped.
    #[serde(rename = "instance.force_stop")]
    InstanceForceStop(Box<InstanceActionAccepted>),
    /// The instance.
    #[serde(rename = "instance.get")]
    InstanceGet(Box<InstanceSummary>),
    /// One page of instances.
    #[serde(rename = "instance.list")]
    InstanceList(Box<InstancePage>),
    /// The space left, read now.
    #[serde(rename = "storage.get_capacity")]
    StorageGetCapacity(CapacityReport),
    /// The daemon settings.
    #[serde(rename = "settings.get")]
    SettingsGet(Box<DaemonSettingsView>),
    /// The settings after the change, or as they were.
    #[serde(rename = "settings.update")]
    SettingsUpdate(Box<DaemonSettingsView>),
    /// The storage locations.
    #[serde(rename = "storage.list_locations")]
    StorageListLocations(LocationList),
    /// The default location of the kind after the change.
    #[serde(rename = "storage.set_location")]
    StorageSetLocation(Box<StorageLocationView>),
    /// The artifact the selector resolved to, and what happened to it.
    #[serde(rename = "runtime.install")]
    RuntimeInstall(Box<InstallRuntimeReply>),
    /// The artifact the selector resolved to.
    #[serde(rename = "runtime.resolve")]
    RuntimeResolve(Box<ResolvedArtifact>),
    /// One page of runtimes.
    #[serde(rename = "runtime.list")]
    RuntimeList(Box<RuntimeListPage>),
    /// The runtime.
    #[serde(rename = "runtime.get")]
    RuntimeGet(Box<RuntimeDetail>),
    /// The form for a new instance.
    #[serde(rename = "instance.get_create_form")]
    InstanceGetCreateForm(Box<CreateForm>),
    /// The preflight rows, the field issues, and the equivalent command.
    #[serde(rename = "instance.preflight")]
    InstancePreflight(Box<CreatePreflight>),
    /// The form and the draft of a duplicate.
    #[serde(rename = "instance.get_duplicate_form")]
    InstanceGetDuplicateForm(Box<DuplicateForm>),
    /// The detail of an instance.
    #[serde(rename = "instance.get_detail")]
    InstanceGetDetail(Box<InstanceDetail>),
    /// The size of a data folder, or null when none is cached.
    #[serde(rename = "instance.get_data_usage")]
    InstanceGetDataUsage(DataUsageReply),
    /// The samples after the cursor and the facts next to them.
    #[serde(rename = "instance.get_metrics")]
    InstanceGetMetrics(Box<MetricsReply>),
    /// The config of the instance.
    #[serde(rename = "instance.config.get")]
    InstanceConfigGet(Box<InstanceConfigView>),
    /// The verdict on a config change.
    #[serde(rename = "instance.config.validate")]
    InstanceConfigValidate(Box<ConfigValidation>),
    /// The operation that applies the change, or that nothing changed.
    #[serde(rename = "instance.config.apply")]
    InstanceConfigApply(Box<ConfigApplyAccepted>),
    /// The operation that keeps the active revision, or that nothing
    /// drifted.
    #[serde(rename = "instance.config.keep_revision")]
    InstanceConfigKeepRevision(Box<InstanceActionAccepted>),
    /// The operation that renames the instance, or that the name is the
    /// same.
    #[serde(rename = "instance.rename")]
    InstanceRename(Box<InstanceActionAccepted>),
    /// The instance after its startup policy changed, or as it was.
    #[serde(rename = "instance.set_startup")]
    InstanceSetStartup(Box<InstanceSummary>),
    /// What Stop All would stop.
    #[serde(rename = "instance.preview_stop_all")]
    InstancePreviewStopAll(Box<StopAllPreview>),
    /// How Stop All ended.
    #[serde(rename = "instance.stop_all")]
    InstanceStopAll(Box<StopAllResult>),
    /// The operation that changes the ports, or the verdict of a dry run.
    #[serde(rename = "instance.change_endpoints")]
    InstanceChangeEndpoints(Box<ChangeEndpointsReply>),
    /// One page of the log.
    #[serde(rename = "instance.logs_read")]
    InstanceLogsRead(Box<LogPage>),
    /// The follow started; its `log` frames follow.
    #[serde(rename = "instance.logs_follow")]
    InstanceLogsFollow(LogsFollowing),
    /// The follow ended.
    #[serde(rename = "instance.logs_unfollow")]
    InstanceLogsUnfollow,
    /// The script is written.
    #[serde(rename = "instance.prepare_terminal")]
    InstancePrepareTerminal(PrepareTerminalReply),
    /// The doctor report, one check per entry.
    #[serde(rename = "doctor.run")]
    DoctorRun(Box<DoctorReport>),
}

/// The method table: one line per method, `name => Variant` or
/// `name => Variant(Params)`, then `recovery allowed` or `recovery refused`.
/// It gives `Call::METHODS`, `Call::method`, `Call::allowed_in_recovery`,
/// `Reply::method`, and the per method params decoder.
macro_rules! methods {
    (@recovery allowed) => {
        true
    };
    (@recovery refused) => {
        false
    };
    (@decode $content:ident, $variant:ident) => {
        json::decode_unit($content).map(|()| Self::$variant)
    };
    (@decode $content:ident, $variant:ident, $params:ty) => {
        json::decode_params::<$params>($content).map(Self::$variant)
    };
    ($($name:literal => $variant:ident $(($params:ty))?, recovery $recovery:ident;)*) => {
        impl Call {
            /// Every method this build knows, in table order.
            pub const METHODS: &[&str] = &[$($name),*];

            /// The method name, such as `daemon.get_info`.
            #[must_use]
            pub const fn method(&self) -> &'static str {
                match self {
                    $(Self::$variant { .. } => $name,)*
                }
            }

            /// Whether the daemon answers this method while the registry is
            /// in recovery mode. Every other method gets
            /// `registry.recovery_mode` before its handler runs (spec 0006
            /// AC-18, spec 0005).
            #[must_use]
            pub const fn allowed_in_recovery(&self) -> bool {
                match self {
                    $(Self::$variant { .. } => methods!(@recovery $recovery),)*
                }
            }

            /// Decodes the params of `method` from the raw `content`, then
            /// runs [`Validate`](crate::json::Validate). `None` means the
            /// method is not in the table.
            pub(crate) fn decode(
                method: &str,
                content: Option<&RawValue>,
            ) -> Option<Result<Self, Vec<FieldError>>> {
                match method {
                    $($name => Some(methods!(@decode content, $variant $(, $params)?)),)*
                    _ => None,
                }
            }
        }

        impl Reply {
            /// The method this reply answers.
            #[must_use]
            pub const fn method(&self) -> &'static str {
                match self {
                    $(Self::$variant { .. } => $name,)*
                }
            }
        }
    };
}

methods! {
    "daemon.handshake" => DaemonHandshake(HandshakeParams), recovery allowed;
    "daemon.get_info" => DaemonGetInfo, recovery allowed;
    "daemon.get_health" => DaemonGetHealth, recovery allowed;
    "daemon.get_capabilities" => DaemonGetCapabilities, recovery allowed;
    "daemon.shutdown" => DaemonShutdown(ShutdownParams), recovery allowed;
    "daemon.restart_preview" => DaemonRestartPreview, recovery allowed;
    "events.subscribe" => EventsSubscribe(SubscribeParams), recovery allowed;
    "events.unsubscribe" => EventsUnsubscribe(UnsubscribeParams), recovery allowed;
    "operation.get" => OperationGet(GetOperationParams), recovery refused;
    "operation.list" => OperationList(ListOperationsParams), recovery refused;
    "operation.cancel" => OperationCancel(CancelOperationParams), recovery refused;
    "operation.resume" => OperationResume(ResumeOperationParams), recovery refused;
    "operation.abandon" => OperationAbandon(AbandonOperationParams), recovery refused;
    "operation.get_stats" => OperationGetStats, recovery refused;
    "audit.list" => AuditList(ListAuditParams), recovery refused;
    "catalog.list_providers" => CatalogListProviders, recovery allowed;
    "catalog.get_provider" => CatalogGetProvider(GetProviderParams), recovery allowed;
    "secret.list" => SecretList(ListSecretsParams), recovery refused;
    "secret.status" => SecretStatus(SecretStatusParams), recovery refused;
    "secret.set" => SecretSet(SetSecretParams), recovery refused;
    "secret.reveal" => SecretReveal(RevealSecretParams), recovery refused;
    "secret.delete" => SecretDelete(DeleteSecretParams), recovery refused;
    "instance.create" => InstanceCreate(CreateInstanceParams), recovery refused;
    "instance.start" => InstanceStart(StartInstanceParams), recovery refused;
    "instance.stop" => InstanceStop(StopInstanceParams), recovery refused;
    "instance.restart" => InstanceRestart(RestartInstanceParams), recovery refused;
    "instance.reload" => InstanceReload(ReloadInstanceParams), recovery refused;
    "instance.force_stop" => InstanceForceStop(ForceStopInstanceParams), recovery refused;
    "instance.get" => InstanceGet(GetInstanceParams), recovery refused;
    "instance.list" => InstanceList(ListInstancesParams), recovery refused;
    "storage.get_capacity" => StorageGetCapacity, recovery allowed;
    "settings.get" => SettingsGet, recovery refused;
    "settings.update" => SettingsUpdate(UpdateSettingsParams), recovery refused;
    "storage.list_locations" => StorageListLocations, recovery refused;
    "storage.set_location" => StorageSetLocation(SetLocationParams), recovery refused;
    "runtime.install" => RuntimeInstall(InstallRuntimeParams), recovery refused;
    "runtime.resolve" => RuntimeResolve(InstallRuntimeParams), recovery allowed;
    "runtime.list" => RuntimeList(ListRuntimesParams), recovery refused;
    "runtime.get" => RuntimeGet(GetRuntimeParams), recovery refused;
    "instance.get_create_form" => InstanceGetCreateForm(GetCreateFormParams), recovery refused;
    "instance.preflight" => InstancePreflight(PreflightCreateParams), recovery refused;
    "instance.get_duplicate_form" => InstanceGetDuplicateForm(GetDuplicateFormParams), recovery refused;
    "instance.get_detail" => InstanceGetDetail(GetInstanceDetailParams), recovery refused;
    "instance.get_data_usage" => InstanceGetDataUsage(GetDataUsageParams), recovery refused;
    "instance.get_metrics" => InstanceGetMetrics(GetMetricsParams), recovery refused;
    "instance.config.get" => InstanceConfigGet(GetConfigParams), recovery refused;
    "instance.config.validate" => InstanceConfigValidate(ValidateConfigParams), recovery refused;
    "instance.config.apply" => InstanceConfigApply(ApplyConfigParams), recovery refused;
    "instance.config.keep_revision" => InstanceConfigKeepRevision(KeepRevisionParams), recovery refused;
    "instance.rename" => InstanceRename(RenameInstanceParams), recovery refused;
    "instance.set_startup" => InstanceSetStartup(SetStartupParams), recovery refused;
    "instance.preview_stop_all" => InstancePreviewStopAll, recovery refused;
    "instance.stop_all" => InstanceStopAll(StopAllParams), recovery refused;
    "instance.change_endpoints" => InstanceChangeEndpoints(ChangeEndpointsParams), recovery refused;
    "instance.logs_read" => InstanceLogsRead(LogsReadParams), recovery refused;
    "instance.logs_follow" => InstanceLogsFollow(LogsFollowParams), recovery refused;
    "instance.logs_unfollow" => InstanceLogsUnfollow(LogsUnfollowParams), recovery refused;
    "instance.prepare_terminal" => InstancePrepareTerminal(PrepareTerminalParams), recovery refused;
    "doctor.run" => DoctorRun(DoctorParams), recovery allowed;
}

impl Reply {
    /// Whether this reply answers `call`. A reply with another tag is a
    /// protocol violation (`ipc.protocol_violation` on the client).
    #[must_use]
    pub fn answers(&self, call: &Call) -> bool {
        self.method() == call.method()
    }
}

/// Every frame the daemon sends. Adding a variant a client may receive
/// without asking is a new major; `log` is sent only on a connection that
/// called `instance.logs_follow`, so it came as a minor (spec 0027).
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", content = "content", rename_all = "snake_case")]
pub enum ServerFrame {
    /// The reply to one request.
    Response(Response),
    /// The error for one request, or for a frame without a readable
    /// request.
    Error(ErrorFrame),
    /// One event for one subscription.
    Event(Event),
    /// Log entries of one follow (spec 0027).
    Log(LogFrame),
}

/// The reply to one request. Replies on one connection may arrive out of
/// order; clients match them by `requestId`.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Response {
    /// The request this answers.
    pub request_id: String,
    /// The reply, tagged like the request's call.
    pub reply: Reply,
}

/// The error for one request.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ErrorFrame {
    /// The request that failed, or null only when its `requestId` could not
    /// be read.
    #[serde(default)]
    pub request_id: Option<String>,
    /// The error.
    pub error: ErrorBody,
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;
    use crate::daemon::ShutdownMode;

    #[test]
    fn every_method_in_the_table_is_the_serde_tag_of_a_call_and_a_reply() {
        for method in Call::METHODS {
            let probe = serde_json::json!({ "type": method, "content": null });
            match serde_json::from_value::<Call>(probe.clone()) {
                Ok(call) => assert_eq!(call.method(), *method),
                Err(err) => assert!(
                    !err.to_string().contains("unknown variant"),
                    "{method}: {err}"
                ),
            }
            match serde_json::from_value::<Reply>(probe) {
                Ok(reply) => assert_eq!(reply.method(), *method),
                Err(err) => assert!(
                    !err.to_string().contains("unknown variant"),
                    "{method}: {err}"
                ),
            }
        }
    }

    #[test]
    fn method_names_are_unique_and_follow_the_area_dot_name_pattern() {
        let unique: HashSet<_> = Call::METHODS.iter().collect();
        assert_eq!(unique.len(), Call::METHODS.len());
        // A method is `area.name`, or `area.group.name` for the config
        // methods of spec 0029 (`instance.config.get`).
        for method in Call::METHODS {
            let parts: Vec<&str> = method.split('.').collect();
            assert!(
                (2..=3).contains(&parts.len())
                    && parts.iter().all(|part| {
                        crate::events::is_kind_filter(&format!("{part}.x")) && !part.is_empty()
                    }),
                "{method}"
            );
        }
    }

    /// The methods the table marks `recovery allowed`.
    fn methods_allowed_in_recovery() -> Vec<&'static str> {
        include_str!("message.rs")
            .lines()
            .filter(|line| line.contains("recovery allowed;"))
            .filter_map(|line| line.trim().split('"').nth(1))
            .collect()
    }

    #[test]
    fn every_method_of_spec_0006_and_the_catalog_is_allowed_in_recovery_and_no_operation_method_is()
    {
        let allowed = [
            Call::DaemonGetInfo,
            Call::DaemonGetHealth,
            Call::DaemonGetCapabilities,
            Call::DaemonShutdown(ShutdownParams::new(ShutdownMode::Stop)),
            Call::DaemonRestartPreview,
            Call::EventsSubscribe(SubscribeParams::default()),
            Call::EventsUnsubscribe(UnsubscribeParams { subscription_id: 1 }),
            Call::CatalogListProviders,
            Call::CatalogGetProvider(GetProviderParams {
                provider: "official/nats".to_owned(),
            }),
            Call::StorageGetCapacity,
            Call::DoctorRun(crate::doctor::DoctorParams::default()),
            Call::RuntimeResolve(InstallRuntimeParams {
                provider: "official/nats".to_owned(),
                selector: "stable".to_owned(),
                expected_sha256: None,
            }),
        ];
        assert!(allowed.iter().all(Call::allowed_in_recovery));
        let operation_id = uuid::Uuid::nil();
        let refused = [
            Call::OperationGet(GetOperationParams { operation_id }),
            Call::OperationList(ListOperationsParams::default()),
            Call::OperationCancel(CancelOperationParams { operation_id }),
            Call::OperationResume(ResumeOperationParams {
                operation_id,
                expected_revision: None,
            }),
            Call::OperationAbandon(AbandonOperationParams {
                operation_id,
                expected_revision: crate::wire::Revision::ZERO,
            }),
            Call::OperationGetStats,
            Call::AuditList(crate::audit::ListAuditParams::default()),
        ];
        assert!(!refused.iter().any(Call::allowed_in_recovery));
        assert!(
            Call::METHODS
                .iter()
                .filter(|method| method.starts_with("secret."))
                .count()
                == 5,
            "every secret method needs the registry, so none is allowed in recovery"
        );
        assert!(
            Call::METHODS
                .iter()
                .filter(|method| method.starts_with("instance."))
                .count()
                == 27
                && Call::METHODS
                    .iter()
                    .filter(|method| method.starts_with("instance."))
                    .all(|method| !methods_allowed_in_recovery().contains(method)),
            "every instance method needs the registry, so none is allowed in recovery"
        );
        assert!(
            ["runtime.install", "runtime.list", "runtime.get"]
                .iter()
                .all(|method| !methods_allowed_in_recovery().contains(method)),
            "installing, listing, and reading runtimes need the registry"
        );
        assert_eq!(
            // The 3 before the logs: `instance.restart`, `instance.reload`,
            // and `instance.force_stop` (spec 0026). Then the 3 log methods
            // (spec 0027), `instance.prepare_terminal` (spec 0070), and
            // `instance.get_metrics` (spec 0028), and the 6 config methods
            // (spec 0029), `instance.set_startup`, `instance.preview_stop_all`, and `instance.stop_all` (spec 0031), and
            // `settings.get`, `settings.update`, `storage.list_locations`, and `storage.set_location` (spec 0032).
            allowed.len() + refused.len() + 1 + 5 + 5 + 3 + 5 + 3 + 3 + 1 + 1 + 6 + 1 + 2 + 4,
            Call::METHODS.len(),
            "a new method needs a line here"
        );
    }

    #[test]
    fn unit_calls_encode_without_content() {
        let json = serde_json::to_string(&Call::DaemonGetInfo).unwrap();
        assert_eq!(json, r#"{"type":"daemon.get_info"}"#);
    }

    #[test]
    fn a_reply_answers_only_the_call_with_its_tag() {
        assert!(
            Reply::EventsUnsubscribe.answers(&Call::EventsUnsubscribe(UnsubscribeParams {
                subscription_id: 1
            }))
        );
        assert!(!Reply::EventsUnsubscribe.answers(&Call::DaemonGetInfo));
    }
}
