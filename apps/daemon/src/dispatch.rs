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
//! the secret methods (spec 0011) in [`crate::ipc::secret`].

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Instant;

use stackly_ipc::PeerCred;
use stackly_protocol::ErrorBody;
use stackly_protocol::catalog::{GetProviderParams, ListProvidersReply, ProviderDetail};
use stackly_protocol::daemon::{Capabilities, DaemonHealth, DaemonInfo};
use stackly_protocol::handshake::ClientInfo;
use stackly_protocol::operation::{
    AbandonOperationParams, AbandonReply, CancelOperationParams, CancelReply, GetOperationParams,
    ListOperationsParams, OperationDetail, OperationPage, OperationStats, OperationSummary,
    ResumeOperationParams,
};
use stackly_protocol::secret::{
    DeleteSecretParams, DeleteSecretReply, ListSecretsParams, ListSecretsReply, RevealSecretParams,
    RevealSecretReply, SecretStatusParams, SecretStatusReply, SetSecretParams, SetSecretReply,
};

use crate::ipc::operation::{self, TestStartingMethod};
use crate::ipc::secret;
use crate::state::DaemonCore;
use crate::{catalog, map};

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
        }
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

    /// A method outside the contract that starts an operation, which only
    /// tests register; the daemon serves none. No contract method starts an
    /// operation before feature 10, so this is how the integration tests
    /// reach [`operation::start_operation`] over the socket (spec 0007 build
    /// step 15). provisional: F10 (the first contract method that starts
    /// an operation replaces it, and its tests move there)
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

/// The real `daemon.get_health`: the state right now.
pub fn get_health(core: &DaemonCore) -> HandlerFuture<'_, DaemonHealth> {
    Box::pin(async move { Ok(core.health(Instant::now())) })
}

/// The real `daemon.get_capabilities`.
pub fn get_capabilities(core: &DaemonCore) -> HandlerFuture<'_, Capabilities> {
    Box::pin(async move { Ok(map::capabilities(core.platform().info())) })
}
