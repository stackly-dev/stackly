//! `settings.get`, `settings.update`, `storage.list_locations`, and
//! `storage.set_location` (spec 0032).

use stackly_protocol::ErrorBody;
use stackly_protocol::codes::internal;
use stackly_protocol::settings::{
    DaemonSettingsView, LocationList, SetLocationParams, StorageLocationView, UpdateSettingsParams,
};

use crate::dispatch::{HandlerFuture, RequestContext};
use crate::state::DaemonCore;

// provisional: F32 step 4, the handlers below answer once the use cases of
// step 3 exist.
fn not_built(method: &str) -> Box<ErrorBody> {
    Box::new(ErrorBody::new(
        &internal::UNEXPECTED,
        format!("{method} is not built yet"),
    ))
}

/// `settings.get`.
pub fn get(core: &DaemonCore) -> HandlerFuture<'_, Box<DaemonSettingsView>> {
    let _ = core;
    Box::pin(async { Err(not_built("settings.get")) })
}

/// `settings.update`.
pub fn update<'a>(
    core: &'a DaemonCore,
    context: &RequestContext,
    params: UpdateSettingsParams,
) -> HandlerFuture<'a, Box<DaemonSettingsView>> {
    let _ = (core, context, params);
    Box::pin(async { Err(not_built("settings.update")) })
}

/// `storage.list_locations`.
pub fn list_locations(core: &DaemonCore) -> HandlerFuture<'_, LocationList> {
    let _ = core;
    Box::pin(async { Err(not_built("storage.list_locations")) })
}

/// `storage.set_location`.
pub fn set_location<'a>(
    core: &'a DaemonCore,
    context: &RequestContext,
    params: SetLocationParams,
) -> HandlerFuture<'a, Box<StorageLocationView>> {
    let _ = (core, context, params);
    Box::pin(async { Err(not_built("storage.set_location")) })
}
