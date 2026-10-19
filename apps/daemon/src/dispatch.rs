//! The method handlers (spec 0006, *Dispatcher*).
//!
//! [`Handlers`] has one method per use case method; each default calls the
//! real use case, so a test can wrap it to hold, slow, or break one method
//! and keep the rest. The connection runs `daemon.handshake`,
//! `events.subscribe`, and `events.unsubscribe` itself, because they act
//! on the connection rather than on a use case. Whether a method is served
//! in recovery mode is its `methods!` table line in `stackly-protocol`
//! ([`Call::allowed_in_recovery`](stackly_protocol::Call::allowed_in_recovery)).

use std::future::Future;
use std::pin::Pin;
use std::time::Instant;

use stackly_ipc::PeerCred;
use stackly_protocol::ErrorBody;
use stackly_protocol::daemon::{Capabilities, DaemonHealth, DaemonInfo};
use stackly_protocol::handshake::ClientInfo;

use crate::map;
use crate::state::DaemonCore;

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
