//! The read method of the duplicate sheet (spec 0024): `instance.get_duplicate_form`.
//!
//! provisional: F24 step 4 builds the handler; until then the method
//! answers `request.unknown_method`, so a client that sends it learns the
//! daemon does not serve it yet.

use stackly_protocol::ErrorBody;
use stackly_protocol::codes::request;
use stackly_protocol::instance::{DuplicateForm, GetDuplicateFormParams};

use crate::dispatch::HandlerFuture;
use crate::state::DaemonCore;

/// `instance.get_duplicate_form`.
pub fn get_duplicate_form(
    _core: &DaemonCore,
    _params: GetDuplicateFormParams,
) -> HandlerFuture<'_, Box<DuplicateForm>> {
    Box::pin(async move {
        Err(Box::new(
            ErrorBody::new(
                &request::UNKNOWN_METHOD,
                "the daemon does not serve this method yet",
            )
            .with_detail("method", "instance.get_duplicate_form"),
        ))
    })
}
