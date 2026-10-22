//! The read methods of the detail page (spec 0021): `instance.get_detail`
//! and `instance.get_data_usage`.
//!
//! provisional: F21 step 7 builds the handlers; until then both methods
//! answer `request.unknown_method`, so a client that sends one learns the
//! daemon does not serve it yet.

use stackly_protocol::ErrorBody;
use stackly_protocol::codes::request;
use stackly_protocol::instance::{
    DataUsageReply, GetDataUsageParams, GetInstanceDetailParams, InstanceDetail,
};

use crate::dispatch::HandlerFuture;
use crate::state::DaemonCore;

/// The refusal both methods give until their handlers exist.
fn not_served(method: &'static str) -> Box<ErrorBody> {
    Box::new(
        ErrorBody::new(
            &request::UNKNOWN_METHOD,
            "the daemon does not serve this method yet",
        )
        .with_detail("method", method),
    )
}

/// `instance.get_detail`.
pub fn get_instance_detail(
    _core: &DaemonCore,
    _params: GetInstanceDetailParams,
) -> HandlerFuture<'_, Box<InstanceDetail>> {
    Box::pin(async move { Err(not_served("instance.get_detail")) })
}

/// `instance.get_data_usage`.
pub fn get_data_usage(
    _core: &DaemonCore,
    _params: GetDataUsageParams,
) -> HandlerFuture<'_, DataUsageReply> {
    Box::pin(async move { Err(not_served("instance.get_data_usage")) })
}
