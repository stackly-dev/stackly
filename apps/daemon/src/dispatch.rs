//! The method handlers (spec 0006, *Dispatcher*).
//!
//! [`Handlers`] has one method per use case method; each default calls the
//! real use case, so a test can wrap it to hold, slow, or break one method
//! and keep the rest. The connection runs `daemon.handshake`,
//! `events.subscribe`, and `events.unsubscribe` itself, because they act
//! on the connection rather than on a use case. Whether a method is served
//! in recovery mode is its `methods!` table line in `stackly-protocol`
//! ([`Call::allowed_in_recovery`](stackly_protocol::Call::allowed_in_recovery)).
//! The operation methods (spec 0007) live in [`crate::ipc::operation`] and
//! the secret methods (spec 0011) in [`crate::ipc::secret`], and the
//! storage method (spec 0022) in [`crate::storage`].

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Instant;

use stackly_ipc::PeerCred;
use stackly_protocol::ErrorBody;
use stackly_protocol::catalog::{GetProviderParams, ListProvidersReply, ProviderDetail};
use stackly_protocol::codes::daemon;
use stackly_protocol::daemon::{
    Capabilities, DaemonHealth, DaemonInfo, LaunchContextKind, ShutdownAccepted, ShutdownMode,
    ShutdownParams,
};
use stackly_protocol::handshake::ClientInfo;
use stackly_protocol::instance::config::{
    ApplyConfigParams, ChangeEndpointsParams, ChangeEndpointsReply, ConfigApplyAccepted,
    ConfigValidation, GetConfigParams, InstanceConfigView, KeepRevisionParams,
    RenameInstanceParams, ValidateConfigParams,
};
use stackly_protocol::instance::{
    CreateForm, CreateInstanceParams, CreatePreflight, DataUsageReply, DuplicateForm,
    ForceStopInstanceParams, GetCreateFormParams, GetDataUsageParams, GetDuplicateFormParams,
    GetInstanceDetailParams, GetInstanceParams, GetMetricsParams, InstanceActionAccepted,
    InstanceDetail, InstancePage, InstanceSummary, ListInstancesParams, MetricsReply,
    PreflightCreateParams, PrepareTerminalParams, PrepareTerminalReply, ReloadInstanceParams,
    RestartInstanceParams, SetStartupParams, StartInstanceParams, StopInstanceParams,
};
use stackly_protocol::operation::{
    AbandonOperationParams, AbandonReply, CancelOperationParams, CancelReply, GetOperationParams,
    ListOperationsParams, OperationDetail, OperationPage, OperationStats, OperationSummary,
    ResumeOperationParams,
};
use stackly_protocol::restart::RestartPreview;
use stackly_protocol::runtime::{
    GetRuntimeParams, InstallRuntimeParams, InstallRuntimeReply, ListRuntimesParams,
    ResolvedArtifact, RuntimeDetail, RuntimeListPage,
};
use stackly_protocol::secret::{
    DeleteSecretParams, DeleteSecretReply, ListSecretsParams, ListSecretsReply, RevealSecretParams,
    RevealSecretReply, SecretStatusParams, SecretStatusReply, SetSecretParams, SetSecretReply,
};
use stackly_protocol::stop_all::{StopAllParams, StopAllPreview, StopAllResult};
use stackly_protocol::storage::CapacityReport;

use crate::ipc::instance;
use crate::ipc::operation;
use crate::ipc::operation::TestStartingMethod;
use crate::ipc::restart;
use crate::ipc::runtime;
use crate::ipc::secret;
use crate::ipc::stop_all;
use crate::state::DaemonCore;
use crate::{catalog, map, storage};

/// What a handler may know about the request it serves: filled from the
/// accept and the handshake of its connection, so a handler never guesses
/// who called it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct RequestContext {
    /// The connection, a counter per daemon run.
    pub connection_id: u64,
    /// The request's `requestId`.
    pub request_id: String,
    /// Who the peer is, from the accept.
    pub peer: PeerCred,
    /// The client, from the handshake.
    pub client: ClientInfo,
    /// The request's `idempotencyKey`, if it sent one.
    pub idempotency_key: Option<String>,
}

impl RequestContext {
    /// Bundles the context of one request.
    #[must_use]
    pub const fn new(
        connection_id: u64,
        request_id: String,
        peer: PeerCred,
        client: ClientInfo,
    ) -> Self {
        Self {
            connection_id,
            request_id,
            peer,
            client,
            idempotency_key: None,
        }
    }

    /// Returns the context with the request's `idempotencyKey`.
    #[must_use]
    pub fn with_idempotency_key(mut self, key: Option<String>) -> Self {
        self.idempotency_key = key;
        self
    }
}

/// What a handler returns: its reply, or the error to send.
pub type HandlerFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, Box<ErrorBody>>> + Send + 'a>>;

/// One method per use case method. Every default is the daemon's real
/// behaviour; the daemon runs [`DefaultHandlers`].
pub trait Handlers: Send + Sync + 'static {
    /// `daemon.get_info`.
    fn get_info<'a>(
        &'a self,
        core: &'a DaemonCore,
        context: &'a RequestContext,
    ) -> HandlerFuture<'a, DaemonInfo> {
        let _ = context;
        get_info(core)
    }

    /// `daemon.get_health`.
    fn get_health<'a>(
        &'a self,
        core: &'a DaemonCore,
        context: &'a RequestContext,
    ) -> HandlerFuture<'a, DaemonHealth> {
        let _ = context;
        get_health(core)
    }

    /// `daemon.get_capabilities`.
    fn get_capabilities<'a>(
        &'a self,
        core: &'a DaemonCore,
        context: &'a RequestContext,
    ) -> HandlerFuture<'a, Capabilities> {
        let _ = context;
        get_capabilities(core)
    }

    /// `daemon.shutdown`.
    fn shutdown<'a>(
        &'a self,
        core: &'a DaemonCore,
        context: &'a RequestContext,
        params: ShutdownParams,
    ) -> HandlerFuture<'a, ShutdownAccepted> {
        shutdown(core, context, params)
    }

    /// `daemon.restart_preview` (spec 0030).
    fn restart_preview<'a>(
        &'a self,
        core: &'a DaemonCore,
        context: &'a RequestContext,
    ) -> HandlerFuture<'a, RestartPreview> {
        let _ = context;
        restart::preview(core)
    }

    /// `catalog.list_providers`.
    fn list_providers<'a>(
        &'a self,
        core: &'a DaemonCore,
        context: &'a RequestContext,
    ) -> HandlerFuture<'a, ListProvidersReply> {
        let _ = context;
        catalog::list_providers(core)
    }

    /// `catalog.get_provider`.
    fn get_provider<'a>(
        &'a self,
        core: &'a DaemonCore,
        context: &'a RequestContext,
        params: GetProviderParams,
    ) -> HandlerFuture<'a, Box<ProviderDetail>> {
        let _ = context;
        catalog::get_provider(core, params)
    }

    /// `operation.get`.
    fn get_operation<'a>(
        &'a self,
        core: &'a DaemonCore,
        context: &'a RequestContext,
        params: GetOperationParams,
    ) -> HandlerFuture<'a, OperationDetail> {
        let _ = context;
        operation::get(core, params)
    }

    /// `operation.list`.
    fn list_operations<'a>(
        &'a self,
        core: &'a DaemonCore,
        context: &'a RequestContext,
        params: ListOperationsParams,
    ) -> HandlerFuture<'a, OperationPage> {
        let _ = context;
        operation::list(core, params)
    }

    /// `operation.cancel`.
    fn cancel_operation<'a>(
        &'a self,
        core: &'a DaemonCore,
        context: &'a RequestContext,
        params: CancelOperationParams,
    ) -> HandlerFuture<'a, CancelReply> {
        let _ = context;
        operation::cancel(core, params)
    }

    /// `operation.resume`.
    fn resume_operation<'a>(
        &'a self,
        core: &'a DaemonCore,
        context: &'a RequestContext,
        params: ResumeOperationParams,
    ) -> HandlerFuture<'a, OperationSummary> {
        let _ = context;
        operation::resume(core, params)
    }

    /// `operation.abandon`.
    fn abandon_operation<'a>(
        &'a self,
        core: &'a DaemonCore,
        context: &'a RequestContext,
        params: AbandonOperationParams,
    ) -> HandlerFuture<'a, AbandonReply> {
        let _ = context;
        operation::abandon(core, params)
    }

    /// `operation.get_stats`.
    fn get_operation_stats<'a>(
        &'a self,
        core: &'a DaemonCore,
        context: &'a RequestContext,
    ) -> HandlerFuture<'a, OperationStats> {
        let _ = context;
        operation::get_stats(core)
    }

    /// `secret.list`.
    fn list_secrets<'a>(
        &'a self,
        core: &'a DaemonCore,
        context: &'a RequestContext,
        params: ListSecretsParams,
    ) -> HandlerFuture<'a, ListSecretsReply> {
        let _ = context;
        secret::list(core, params)
    }

    /// `secret.status`.
    fn secret_status<'a>(
        &'a self,
        core: &'a DaemonCore,
        context: &'a RequestContext,
        params: SecretStatusParams,
    ) -> HandlerFuture<'a, SecretStatusReply> {
        let _ = context;
        secret::status(core, params)
    }

    /// `secret.set`.
    fn set_secret<'a>(
        &'a self,
        core: &'a DaemonCore,
        context: &'a RequestContext,
        params: SetSecretParams,
    ) -> HandlerFuture<'a, SetSecretReply> {
        secret::set(core, context, params)
    }

    /// `secret.reveal`.
    fn reveal_secret<'a>(
        &'a self,
        core: &'a DaemonCore,
        context: &'a RequestContext,
        params: RevealSecretParams,
    ) -> HandlerFuture<'a, RevealSecretReply> {
        secret::reveal(core, context, params)
    }

    /// `secret.delete`.
    fn delete_secret<'a>(
        &'a self,
        core: &'a DaemonCore,
        context: &'a RequestContext,
        params: DeleteSecretParams,
    ) -> HandlerFuture<'a, DeleteSecretReply> {
        secret::delete(core, context, params)
    }

    /// `instance.create`.
    fn create_instance<'a>(
        &'a self,
        core: &'a DaemonCore,
        context: &'a RequestContext,
        params: CreateInstanceParams,
    ) -> HandlerFuture<'a, InstanceActionAccepted> {
        instance::create(core, context, params)
    }

    /// `instance.get_create_form`.
    fn get_create_form<'a>(
        &'a self,
        core: &'a DaemonCore,
        context: &'a RequestContext,
        params: GetCreateFormParams,
    ) -> HandlerFuture<'a, Box<CreateForm>> {
        let _ = context;
        instance::get_create_form(core, params)
    }

    /// `instance.preflight`.
    fn preflight_create<'a>(
        &'a self,
        core: &'a DaemonCore,
        context: &'a RequestContext,
        params: PreflightCreateParams,
    ) -> HandlerFuture<'a, Box<CreatePreflight>> {
        let _ = context;
        instance::preflight(core, params)
    }

    /// `instance.get_duplicate_form`.
    fn get_duplicate_form<'a>(
        &'a self,
        core: &'a DaemonCore,
        context: &'a RequestContext,
        params: GetDuplicateFormParams,
    ) -> HandlerFuture<'a, Box<DuplicateForm>> {
        let _ = context;
        instance::get_duplicate_form(core, params)
    }

    /// `instance.get_detail`.
    fn get_instance_detail<'a>(
        &'a self,
        core: &'a DaemonCore,
        context: &'a RequestContext,
        params: GetInstanceDetailParams,
    ) -> HandlerFuture<'a, Box<InstanceDetail>> {
        let _ = context;
        instance::get_instance_detail(core, params)
    }

    /// `instance.prepare_terminal` (spec 0070).
    fn prepare_terminal<'a>(
        &'a self,
        core: &'a DaemonCore,
        context: &'a RequestContext,
        params: PrepareTerminalParams,
    ) -> HandlerFuture<'a, PrepareTerminalReply> {
        let _ = context;
        instance::prepare_terminal(core, params)
    }

    /// `instance.get_data_usage`.
    fn get_data_usage<'a>(
        &'a self,
        core: &'a DaemonCore,
        context: &'a RequestContext,
        params: GetDataUsageParams,
    ) -> HandlerFuture<'a, DataUsageReply> {
        let _ = context;
        instance::get_data_usage(core, params)
    }

    /// `instance.get_metrics`.
    fn get_metrics<'a>(
        &'a self,
        core: &'a DaemonCore,
        context: &'a RequestContext,
        params: GetMetricsParams,
    ) -> HandlerFuture<'a, Box<MetricsReply>> {
        let _ = context;
        instance::get_metrics(core, params)
    }

    /// `instance.config.get` (spec 0029).
    fn get_config<'a>(
        &'a self,
        core: &'a DaemonCore,
        context: &'a RequestContext,
        params: GetConfigParams,
    ) -> HandlerFuture<'a, Box<InstanceConfigView>> {
        let _ = context;
        instance::get_config(core, params)
    }

    /// `instance.config.validate` (spec 0029).
    fn validate_config<'a>(
        &'a self,
        core: &'a DaemonCore,
        context: &'a RequestContext,
        params: ValidateConfigParams,
    ) -> HandlerFuture<'a, Box<ConfigValidation>> {
        let _ = context;
        instance::validate_config(core, params)
    }

    /// `instance.config.apply` (spec 0029).
    fn apply_config<'a>(
        &'a self,
        core: &'a DaemonCore,
        context: &'a RequestContext,
        params: ApplyConfigParams,
    ) -> HandlerFuture<'a, Box<ConfigApplyAccepted>> {
        instance::apply_config(core, context, params)
    }

    /// `instance.config.keep_revision` (spec 0029).
    fn keep_revision<'a>(
        &'a self,
        core: &'a DaemonCore,
        context: &'a RequestContext,
        params: KeepRevisionParams,
    ) -> HandlerFuture<'a, InstanceActionAccepted> {
        instance::keep_revision(core, context, params)
    }

    /// `instance.rename` (spec 0029).
    fn rename_instance<'a>(
        &'a self,
        core: &'a DaemonCore,
        context: &'a RequestContext,
        params: RenameInstanceParams,
    ) -> HandlerFuture<'a, InstanceActionAccepted> {
        instance::rename(core, context, params)
    }

    /// `instance.preview_stop_all` (spec 0031).
    fn preview_stop_all<'a>(
        &'a self,
        core: &'a DaemonCore,
        context: &'a RequestContext,
    ) -> HandlerFuture<'a, Box<StopAllPreview>> {
        let _ = context;
        stop_all::preview(core)
    }

    /// `instance.stop_all` (spec 0031).
    fn stop_all<'a>(
        &'a self,
        core: &'a DaemonCore,
        context: &'a RequestContext,
        params: StopAllParams,
    ) -> HandlerFuture<'a, Box<StopAllResult>> {
        stop_all::stop_all(core, context, params)
    }

    /// `instance.set_startup` (spec 0031).
    fn set_startup<'a>(
        &'a self,
        core: &'a DaemonCore,
        context: &'a RequestContext,
        params: SetStartupParams,
    ) -> HandlerFuture<'a, InstanceSummary> {
        instance::set_startup(core, context, params)
    }

    /// `instance.change_endpoints` (spec 0029).
    fn change_endpoints<'a>(
        &'a self,
        core: &'a DaemonCore,
        context: &'a RequestContext,
        params: ChangeEndpointsParams,
    ) -> HandlerFuture<'a, Box<ChangeEndpointsReply>> {
        instance::change_endpoints(core, context, params)
    }

    /// `instance.start`.
    fn start_instance<'a>(
        &'a self,
        core: &'a DaemonCore,
        context: &'a RequestContext,
        params: StartInstanceParams,
    ) -> HandlerFuture<'a, InstanceActionAccepted> {
        instance::start(core, context, params)
    }

    /// `instance.stop`.
    fn stop_instance<'a>(
        &'a self,
        core: &'a DaemonCore,
        context: &'a RequestContext,
        params: StopInstanceParams,
    ) -> HandlerFuture<'a, InstanceActionAccepted> {
        instance::stop(core, context, params)
    }

    /// `instance.restart`.
    fn restart_instance<'a>(
        &'a self,
        core: &'a DaemonCore,
        context: &'a RequestContext,
        params: RestartInstanceParams,
    ) -> HandlerFuture<'a, InstanceActionAccepted> {
        instance::restart(core, context, params)
    }

    /// `instance.reload`.
    fn reload_instance<'a>(
        &'a self,
        core: &'a DaemonCore,
        context: &'a RequestContext,
        params: ReloadInstanceParams,
    ) -> HandlerFuture<'a, InstanceActionAccepted> {
        instance::reload(core, context, params)
    }

    /// `instance.force_stop`.
    fn force_stop_instance<'a>(
        &'a self,
        core: &'a DaemonCore,
        context: &'a RequestContext,
        params: ForceStopInstanceParams,
    ) -> HandlerFuture<'a, InstanceActionAccepted> {
        instance::force_stop(core, context, params)
    }

    /// `instance.get`.
    fn get_instance<'a>(
        &'a self,
        core: &'a DaemonCore,
        context: &'a RequestContext,
        params: GetInstanceParams,
    ) -> HandlerFuture<'a, InstanceSummary> {
        let _ = context;
        instance::get(core, params)
    }

    /// `instance.list`.
    fn list_instances<'a>(
        &'a self,
        core: &'a DaemonCore,
        context: &'a RequestContext,
        params: ListInstancesParams,
    ) -> HandlerFuture<'a, InstancePage> {
        let _ = context;
        instance::list(core, params)
    }

    /// `storage.get_capacity`.
    fn get_capacity<'a>(
        &'a self,
        core: &'a DaemonCore,
        context: &'a RequestContext,
    ) -> HandlerFuture<'a, CapacityReport> {
        let _ = context;
        storage::get_capacity(core)
    }

    /// `runtime.install`.
    fn install_runtime<'a>(
        &'a self,
        core: &'a DaemonCore,
        context: &'a RequestContext,
        params: InstallRuntimeParams,
    ) -> HandlerFuture<'a, Box<InstallRuntimeReply>> {
        runtime::install(core, context, params)
    }

    /// `runtime.resolve`.
    fn resolve_runtime<'a>(
        &'a self,
        core: &'a DaemonCore,
        context: &'a RequestContext,
        params: InstallRuntimeParams,
    ) -> HandlerFuture<'a, Box<ResolvedArtifact>> {
        let _ = context;
        runtime::resolve_runtime(core, params)
    }

    /// `runtime.list`.
    fn list_runtimes<'a>(
        &'a self,
        core: &'a DaemonCore,
        context: &'a RequestContext,
        params: ListRuntimesParams,
    ) -> HandlerFuture<'a, Box<RuntimeListPage>> {
        let _ = context;
        runtime::list(core, params)
    }

    /// `runtime.get`.
    fn get_runtime<'a>(
        &'a self,
        core: &'a DaemonCore,
        context: &'a RequestContext,
        params: GetRuntimeParams,
    ) -> HandlerFuture<'a, Box<RuntimeDetail>> {
        let _ = context;
        runtime::get(core, params)
    }

    /// A method outside the contract that starts an operation, which only
    /// tests register; the daemon serves none. The engine tests need work
    /// they can script (hold, fail, count steps), which no contract method
    /// offers, so this is how they reach
    /// [`operation::start_operation`] over the socket (spec 0007 build step
    /// 15). The daemon never registers one. It stays in the library
    /// because the integration tests are separate crates and `cargo xtask
    /// check-deps` forbids the crate a dev dependency on itself, which a
    /// feature gate would need.
    #[doc(hidden)]
    fn test_starting_method(&self) -> Option<Arc<dyn TestStartingMethod>> {
        None
    }
}

/// The daemon's handlers: every default.
#[derive(Debug, Clone, Copy, Default)]
pub struct DefaultHandlers;

impl Handlers for DefaultHandlers {}

/// The real `daemon.get_info`: the facts recorded at start, plus the
/// registry schema once the registry opened ready.
pub fn get_info(core: &DaemonCore) -> HandlerFuture<'_, DaemonInfo> {
    Box::pin(async move { Ok(core.info()) })
}

/// The real `daemon.shutdown` (spec 0015): records the mode, which wakes
/// the loop in `startup` that runs the stop order. `restart` is refused for
/// a daemon that launchd did not start, because nothing would start it
/// again. Only the first request counts; a later one gets
/// `daemon.shutting_down`.
pub fn shutdown<'a>(
    core: &'a DaemonCore,
    context: &RequestContext,
    params: ShutdownParams,
) -> HandlerFuture<'a, ShutdownAccepted> {
    if params.mode == ShutdownMode::Restart
        && restart::launch_context(core) == LaunchContextKind::Launchd
    {
        return restart::restart(core, context, params);
    }
    Box::pin(async move {
        let mode = params.mode;
        if mode == ShutdownMode::Restart {
            let context = core
                .start_info()
                .launch
                .as_ref()
                .map_or(LaunchContextKind::Manual, |launch| launch.context);
            if context != LaunchContextKind::Launchd {
                return Err(Box::new(
                    ErrorBody::new(
                        &daemon::RESTART_UNAVAILABLE,
                        "the daemon was not started by launchd, so a restart would not start it again",
                    )
                    .with_detail("launchContext", launch_context_code(context)),
                ));
            }
        }
        if core.shutdown_pending() {
            return Err(Box::new(ErrorBody::new(
                &daemon::SHUTTING_DOWN,
                "the daemon is already stopping",
            )));
        }
        // A planned restart is quiescing or stopping instances (spec 0030).
        if core
            .operations()
            .engine()
            .is_some_and(|engine| engine.is_quiesced())
        {
            return Err(restart::restart_pending(None));
        }
        if !core.request_shutdown(mode) {
            return Err(Box::new(ErrorBody::new(
                &daemon::SHUTTING_DOWN,
                "the daemon is already stopping",
            )));
        }
        Ok(ShutdownAccepted {
            server_instance_id: core.start_info().server_instance_id,
            mode,
            plan_id: None,
        })
    })
}

/// The wire code of a launch context, for error details.
const fn launch_context_code(context: LaunchContextKind) -> &'static str {
    match context {
        LaunchContextKind::Launchd => "launchd",
        LaunchContextKind::Manual => "manual",
        LaunchContextKind::Unrecognized => "unrecognized",
    }
}

/// The real `daemon.get_health`: the state right now.
pub fn get_health(core: &DaemonCore) -> HandlerFuture<'_, DaemonHealth> {
    Box::pin(async move { Ok(core.health(Instant::now())) })
}

/// The real `daemon.get_capabilities`.
pub fn get_capabilities(core: &DaemonCore) -> HandlerFuture<'_, Capabilities> {
    Box::pin(async move { Ok(map::capabilities(core.platform().info())) })
}
