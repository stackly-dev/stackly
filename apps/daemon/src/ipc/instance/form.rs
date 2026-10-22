//! The read methods of the create sheet (spec 0020): `instance.get_create_form`
//! and `instance.preflight`.
//!
//! Neither writes the registry, the disk, or the keychain, spawns, or
//! starts an operation. `preflight` runs the same `gather` and `evaluate`
//! as the precheck of `instance.create`, over a placeholder instance ID, so
//! the sheet, `--dry-run`, and create judge a draft alike (AC-2).

use stackly_core::catalog::CatalogEntry;
use stackly_core::instance::create::{
    CreateDeps, CreateError, FormError, build_form, evaluate, gather, names_of, select_provider,
};
use stackly_domain::{InstanceFilter, ProviderName, Registry, RuntimeId};
use stackly_protocol::ErrorBody;
use stackly_protocol::codes::instance as instance_codes;
use stackly_protocol::instance::{
    CreateForm, CreatePreflight, GetCreateFormParams, PreflightCreateParams,
};

use super::create::catalog_state;
use super::{draft, registry_of, unexpected};
use crate::dispatch::HandlerFuture;
use crate::map;
use crate::state::DaemonCore;

/// `instance.get_create_form`.
pub fn get_create_form(
    core: &DaemonCore,
    params: GetCreateFormParams,
) -> HandlerFuture<'_, Box<CreateForm>> {
    Box::pin(async move {
        core.supervision().supervisor()?;
        let registry = registry_of(core)?;
        let state = catalog_state(core)?;
        let not_found = || {
            Box::new(
                ErrorBody::new(&instance_codes::RUNTIME_NOT_FOUND, "no runtime has that id")
                    .with_detail("runtimeId", params.runtime_id.to_string()),
            )
        };
        let runtime_id = RuntimeId::from_uuid(params.runtime_id).map_err(|_| not_found())?;
        let read = {
            let registry = registry.clone();
            tokio::task::spawn_blocking(move || {
                let runtime = registry.runtime(runtime_id)?.value;
                let instances = registry
                    .instances(InstanceFilter {
                        include_retained: false,
                    })?
                    .value;
                Ok::<_, stackly_domain::RegistryError>((runtime, instances))
            })
            .await
            .map_err(|_| unexpected("the create form task failed"))?
            .map_err(|error| Box::new(map::registry_error(&error)))?
        };
        let (runtime, instances) = read;
        let runtime = runtime.ok_or_else(not_found)?;

        let (entry, meta) =
            select_provider(&state, &runtime, params.provider.as_deref()).map_err(|error| {
                Box::new(form_error(
                    error,
                    &state,
                    &runtime,
                    params.provider.as_deref(),
                ))
            })?;
        let model = build_form(entry, &runtime, &names_of(&instances), meta);
        let summary = crate::ipc::runtime::summary_of_runtime(core, &registry, &runtime).await?;
        let capabilities = map::create::capabilities(&model.capabilities);
        Ok(Box::new(map::create::create_form(
            &model,
            summary,
            capabilities,
        )))
    })
}

/// A form refusal as the error body of the code it stands for.
fn form_error(
    error: FormError,
    state: &stackly_core::catalog::CatalogState,
    runtime: &stackly_domain::Runtime,
    provider: Option<&str>,
) -> ErrorBody {
    let name = provider.map_or_else(|| runtime.software().to_string(), str::to_owned);
    let create = match error {
        FormError::CatalogUnavailable { reason } => CreateError::CatalogUnavailable { reason },
        FormError::ProviderNotFound => CreateError::ProviderNotFound {
            provider: name,
            revision: None,
        },
        FormError::ProviderAmbiguous => CreateError::ProviderAmbiguous {
            provider: name,
            revision: None,
        },
        FormError::ProviderUnavailable { state } => CreateError::ProviderUnavailable {
            provider: name,
            state,
        },
        FormError::RuntimeMismatch => CreateError::RuntimeMismatch {
            runtime_software: runtime.software().to_string(),
            provider_software: provider
                .and_then(|name| name.parse::<ProviderName>().ok())
                .and_then(|id| match state {
                    stackly_core::catalog::CatalogState::Loaded(catalog) => {
                        catalog.entry(&id).and_then(|entry| match entry {
                            CatalogEntry::Provider(entry) => {
                                Some(entry.package.software().to_string())
                            }
                            _ => None,
                        })
                    }
                    _ => None,
                })
                .unwrap_or_default(),
        },
        FormError::RuntimeUnavailable { state } => CreateError::RuntimeUnavailable {
            runtime_id: runtime.id(),
            state,
        },
        _ => {
            return ErrorBody::new(
                &stackly_protocol::codes::internal::UNEXPECTED,
                "the create form was refused for a reason this build cannot name",
            );
        }
    };
    map::create::create_error(&create, &[])
}

/// `instance.preflight`.
pub fn preflight(
    core: &DaemonCore,
    params: PreflightCreateParams,
) -> HandlerFuture<'_, Box<CreatePreflight>> {
    Box::pin(async move {
        core.supervision().supervisor()?;
        let registry = registry_of(core)?;
        let state = catalog_state(core)?;
        let draft = draft::from_preflight(&params);
        let platform = core.platform().clone();
        let prepared = core.prepared().clone();
        let source = core.catalog_source().id();
        let probe = super::create::serialized_probe();
        tokio::task::spawn_blocking(move || {
            let create = CreateDeps {
                registry: &registry,
                catalog: &state,
                source,
                platform: &platform,
                prepared: &prepared,
                probe: &*probe,
            };
            let facts = gather(&create, &draft)?;
            let evaluation = evaluate(&draft, &facts);
            Ok::<_, stackly_core::instance::create::GatherError>(map::create::preflight(
                &evaluation,
                &facts,
            ))
        })
        .await
        .map_err(|_| unexpected("the preflight task failed"))?
        .map(Box::new)
        .map_err(|error| {
            tracing::error!(%error, "cannot read the facts of a preflight");
            unexpected("the facts of the preflight could not be read")
        })
    })
}
