//! The read methods of the create sheet (spec 0020): `instance.get_create_form`
//! and `instance.preflight`.
//!
//! Neither writes the registry, the disk, or the keychain, spawns, or
//! starts an operation. `preflight` runs the same `gather` and `evaluate`
//! as the precheck of `instance.create`, over a placeholder instance ID, so
//! the sheet, `--dry-run`, and create judge a draft alike (AC-2).

use stackly_core::catalog::{CatalogEntry, CatalogState};
use stackly_core::instance::create::{
    CreateDeps, CreateError, CreateFormModel, FormError, ProviderProblem, build_form,
    build_form_from_package, evaluate, gather, names_of, resolve_package, select_provider,
    suggested_name,
};
use stackly_domain::{
    Instance, InstanceFilter, ProviderName, Registry, Runtime, RuntimeId, RuntimeState,
};
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

        let model = match params.revision.clone() {
            None => {
                let (entry, meta) = select_provider(&state, &runtime, params.provider.as_deref())
                    .map_err(|error| {
                    Box::new(form_error(
                        error,
                        &state,
                        &runtime,
                        params.provider.as_deref(),
                        None,
                        None,
                    ))
                })?;
                build_form(entry, &runtime, &names_of(&instances), meta)
            }
            Some(revision) => {
                pinned_form(
                    core,
                    &registry,
                    &state,
                    runtime.clone(),
                    &instances,
                    &params,
                    revision,
                )
                .await?
            }
        };
        let summary = crate::ipc::runtime::summary_of_runtime(core, &registry, &runtime).await?;
        let capabilities = map::create::capabilities(&model.capabilities);
        Ok(Box::new(map::create::create_form(
            &model,
            summary,
            capabilities,
        )))
    })
}

/// The form when the request names a revision (AC-1): the runtime, then the
/// provider name, then the package through the [`resolve_package`] create
/// uses, so form and create pick the same package and refuse alike.
async fn pinned_form(
    core: &DaemonCore,
    registry: &stackly_storage::SqliteRegistry,
    state: &CatalogState,
    runtime: Runtime,
    instances: &[Instance],
    params: &GetCreateFormParams,
    revision: String,
) -> Result<CreateFormModel, Box<ErrorBody>> {
    let refuse = |error, name: Option<&str>, software: Option<String>| {
        Box::new(form_error(
            error,
            state,
            &runtime,
            name,
            Some(&revision),
            software,
        ))
    };
    if runtime.state() != RuntimeState::Installed {
        return Err(refuse(
            FormError::RuntimeUnavailable {
                state: runtime.state().as_code(),
            },
            params.provider.as_deref(),
            None,
        ));
    }
    let name = match &params.provider {
        Some(name) => name.clone(),
        None => select_provider(state, &runtime, None)
            .map_err(|error| refuse(error, None, None))?
            .0
            .package
            .key()
            .provider()
            .to_string(),
    };
    let found = {
        let registry = registry.clone();
        let state = state.clone();
        let platform = core.platform().clone();
        let prepared = core.prepared().clone();
        let (name, revision) = (name.clone(), revision.clone());
        tokio::task::spawn_blocking(move || {
            resolve_package(
                &state,
                &registry,
                &platform,
                prepared.layout(),
                &name,
                Some(&revision),
            )
        })
        .await
        .map_err(|_| unexpected("the create form task failed"))?
        .map_err(|error| {
            tracing::error!(%error, "cannot resolve the package of a create form");
            unexpected("the package of the create form could not be read")
        })?
    };
    let found = found.map_err(|problem| {
        let error = match problem {
            ProviderProblem::CatalogUnavailable { reason } => {
                FormError::CatalogUnavailable { reason }
            }
            ProviderProblem::NotFound => FormError::ProviderNotFound,
            ProviderProblem::Ambiguous => FormError::ProviderAmbiguous,
            ProviderProblem::Unavailable { state } => FormError::ProviderUnavailable { state },
        };
        refuse(error, Some(&name), None)
    })?;
    let software = found.package.software();
    if software != runtime.software() {
        return Err(refuse(
            FormError::RuntimeMismatch,
            Some(&name),
            Some(software.to_string()),
        ));
    }
    let suggested = suggested_name(
        software.as_str(),
        runtime.line().as_str(),
        &names_of(instances),
    );
    Ok(build_form_from_package(
        &found.package,
        found.source,
        &runtime,
        suggested,
    ))
}

/// A form refusal as the error body of the code it stands for. `revision`
/// is the one the request named; `package_software` is the software of the
/// package a revision resolved to, read for `instance.runtime_mismatch`
/// instead of the catalog.
fn form_error(
    error: FormError,
    state: &CatalogState,
    runtime: &Runtime,
    provider: Option<&str>,
    revision: Option<&str>,
    package_software: Option<String>,
) -> ErrorBody {
    let name = provider.map_or_else(|| runtime.software().to_string(), str::to_owned);
    let revision = revision.map(str::to_owned);
    let create = match error {
        FormError::CatalogUnavailable { reason } => CreateError::CatalogUnavailable { reason },
        FormError::ProviderNotFound => CreateError::ProviderNotFound {
            provider: name,
            revision,
        },
        FormError::ProviderAmbiguous => CreateError::ProviderAmbiguous {
            provider: name,
            revision,
        },
        FormError::ProviderUnavailable { state } => CreateError::ProviderUnavailable {
            provider: name,
            state,
        },
        FormError::RuntimeMismatch => CreateError::RuntimeMismatch {
            runtime_software: runtime.software().to_string(),
            provider_software: package_software
                .or_else(|| catalog_software(state, provider))
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

/// The software of the catalog entry named `provider`, if it is a provider.
fn catalog_software(state: &CatalogState, provider: Option<&str>) -> Option<String> {
    let id = provider?.parse::<ProviderName>().ok()?;
    let CatalogState::Loaded(catalog) = state else {
        return None;
    };
    match catalog.entry(&id)? {
        CatalogEntry::Provider(entry) => Some(entry.package.software().to_string()),
        _ => None,
    }
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
