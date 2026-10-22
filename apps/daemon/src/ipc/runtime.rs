//! The `runtime` methods (spec 0018).
//!
//! provisional: F18 step 11 builds the handlers; until then each method
//! answers `request.unknown_method`, so a client that sends one learns the
//! daemon does not serve it yet.

use stackly_protocol::ErrorBody;
use stackly_protocol::codes::request;
use stackly_protocol::runtime::{
    GetRuntimeParams, InstallRuntimeParams, InstallRuntimeReply, ListRuntimesParams,
    ResolvedArtifact, RuntimeDetail, RuntimeListPage,
};

use crate::dispatch::HandlerFuture;
use crate::state::DaemonCore;

fn not_served<T: Send + 'static>(method: &'static str) -> HandlerFuture<'static, T> {
    Box::pin(async move {
        Err(Box::new(
            ErrorBody::new(
                &request::UNKNOWN_METHOD,
                "the daemon does not serve this method yet",
            )
            .with_detail("method", method),
        ))
    })
}

/// `runtime.install`.
pub fn install(
    _core: &DaemonCore,
    _params: InstallRuntimeParams,
) -> HandlerFuture<'_, Box<InstallRuntimeReply>> {
    not_served("runtime.install")
}

/// `runtime.resolve`.
pub fn resolve(
    _core: &DaemonCore,
    _params: InstallRuntimeParams,
) -> HandlerFuture<'_, Box<ResolvedArtifact>> {
    not_served("runtime.resolve")
}

/// `runtime.list`.
pub fn list(
    _core: &DaemonCore,
    _params: ListRuntimesParams,
) -> HandlerFuture<'_, Box<RuntimeListPage>> {
    not_served("runtime.list")
}

/// `runtime.get`.
pub fn get(_core: &DaemonCore, _params: GetRuntimeParams) -> HandlerFuture<'_, Box<RuntimeDetail>> {
    not_served("runtime.get")
}
