//! The read methods of the create sheet (spec 0020): `instance.get_create_form`
//! and `instance.preflight`.
//!
//! provisional: F20 step 9 builds the handlers; until then each method
//! answers `request.unknown_method`, so a client that sends one learns the
//! daemon does not serve it yet.

use stackly_protocol::ErrorBody;
use stackly_protocol::codes::request;
use stackly_protocol::instance::{
    CreateForm, CreatePreflight, GetCreateFormParams, PreflightCreateParams,
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

/// `instance.get_create_form`.
pub fn get_create_form(
    _core: &DaemonCore,
    _params: GetCreateFormParams,
) -> HandlerFuture<'_, Box<CreateForm>> {
    not_served("instance.get_create_form")
}

/// `instance.preflight`.
pub fn preflight(
    _core: &DaemonCore,
    _params: PreflightCreateParams,
) -> HandlerFuture<'_, Box<CreatePreflight>> {
    not_served("instance.preflight")
}
