//! The read method of the duplicate sheet (spec 0024): `instance.get_duplicate_form`.
//!
//! It reads the registry, the loaded catalog, and the pinned package of the
//! source instance, writes nothing, and never opens the data, log, or run
//! folder of the source. The draft it returns goes through `instance.preflight`
//! and `instance.create` of spec 0020 like any other.

use stackly_core::instance::duplicate::{DuplicateDeps, build_duplicate, gather_duplicate};
use stackly_protocol::ErrorBody;
use stackly_protocol::instance::{DuplicateForm, GetDuplicateFormParams};

use super::create::catalog_state;
use super::{instance_id, registry_of, unexpected};
use crate::dispatch::HandlerFuture;
use crate::map;
use crate::state::DaemonCore;

/// `instance.get_duplicate_form`.
pub fn get_duplicate_form(
    core: &DaemonCore,
    params: GetDuplicateFormParams,
) -> HandlerFuture<'_, Box<DuplicateForm>> {
    Box::pin(async move {
        core.supervision().supervisor()?;
        let registry = registry_of(core)?;
        let state = catalog_state(core)?;
        let source = instance_id(params.instance_id)?;
        let platform = core.platform().clone();
        let prepared = core.prepared().clone();
        let (built, runtime) = tokio::task::spawn_blocking(move || {
            let deps = DuplicateDeps {
                registry: &registry,
                catalog: &state,
                platform: &platform,
                prepared: &prepared,
            };
            gather_duplicate(&deps, source).map(|facts| {
                let model = build_duplicate(&facts);
                (model, facts.runtime)
            })
        })
        .await
        .map_err(|_| unexpected("the duplicate form task failed"))?
        .map_err(|error| {
            tracing::debug!(instance = %params.instance_id, %error, "no duplicate form");
            Box::new(map::duplicate::duplicate_error(&error, params.instance_id))
        })?;
        let registry = registry_of(core)?;
        let runtime = crate::ipc::runtime::summary_of_runtime(core, &registry, &runtime).await?;
        let capabilities = map::create::capabilities(&built.form.capabilities);
        Ok::<_, Box<ErrorBody>>(Box::new(map::duplicate::duplicate_form(
            &built,
            runtime,
            capabilities,
        )))
    })
}
