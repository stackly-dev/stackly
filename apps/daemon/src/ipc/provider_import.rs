//! The import methods of local providers (spec 0037, *API surface*):
//! `provider.preview_import`, `provider.import`, `provider.approve`,
//! `provider.revoke`, and `provider.remove`.

use stackly_protocol::ErrorBody;
use stackly_protocol::catalog::ProviderDetail;
use stackly_protocol::codes::request;
use stackly_protocol::provider::{
    DecideProviderParams, ImportPreview, ImportProviderParams, PreviewImportParams,
    ProviderTargetParams, RemovedProvider,
};

use crate::dispatch::{HandlerFuture, RequestContext};
use crate::state::DaemonCore;

/// The answer of a method this build does not serve yet.
// provisional: F37 step 5 (preview, import, remove) and step 6 (approve, revoke)
fn not_served(method: &'static str) -> Box<ErrorBody> {
    Box::new(
        ErrorBody::new(
            &request::UNKNOWN_METHOD,
            format!("{method} is not served yet"),
        )
        .with_detail("method", method),
    )
}

/// `provider.preview_import`.
pub fn preview<'a>(
    _core: &'a DaemonCore,
    _params: PreviewImportParams,
) -> HandlerFuture<'a, Box<ImportPreview>> {
    Box::pin(async move { Err(not_served("provider.preview_import")) })
}

/// `provider.import`.
pub fn import<'a>(
    _core: &'a DaemonCore,
    _context: &RequestContext,
    _params: ImportProviderParams,
) -> HandlerFuture<'a, Box<ProviderDetail>> {
    Box::pin(async move { Err(not_served("provider.import")) })
}

/// `provider.approve`.
pub fn approve<'a>(
    _core: &'a DaemonCore,
    _context: &RequestContext,
    _params: DecideProviderParams,
) -> HandlerFuture<'a, Box<ProviderDetail>> {
    Box::pin(async move { Err(not_served("provider.approve")) })
}

/// `provider.revoke`.
pub fn revoke<'a>(
    _core: &'a DaemonCore,
    _context: &RequestContext,
    _params: ProviderTargetParams,
) -> HandlerFuture<'a, Box<ProviderDetail>> {
    Box::pin(async move { Err(not_served("provider.revoke")) })
}

/// `provider.remove`.
pub fn remove<'a>(
    _core: &'a DaemonCore,
    _context: &RequestContext,
    _params: ProviderTargetParams,
) -> HandlerFuture<'a, RemovedProvider> {
    Box::pin(async move { Err(not_served("provider.remove")) })
}
