//! The read methods of the detail page (spec 0021): `instance.get_detail`
//! and `instance.get_data_usage`.
//!
//! Neither method writes, spawns, reads the Keychain, or looks at the
//! catalog: the detail comes from the registry and the pinned package in
//! the layout, and the size from the data folder of the instance, so both
//! work offline (REQ-046).

use stackly_core::instance::detail::{DetailError, DetailSnapshot, build};
use stackly_domain::{
    Environment, InstanceId, InstancePathEntry, InstancePathKind, Lease, LeaseFilter, Registry,
    Runtime, SecretRef, SecretStatus, SecretUse, SecretUser,
};
use stackly_platform::Platform;
use stackly_protocol::ErrorBody;
use stackly_protocol::instance::{
    DataUsageReply, GetDataUsageParams, GetInstanceDetailParams, InstanceDetail,
};
use stackly_storage::SqliteRegistry;

use super::{instance_id, registry_of, unexpected};
use crate::detail::UsageRequest;
use crate::dispatch::HandlerFuture;
use crate::map;
use crate::state::DaemonCore;

/// What one read of the registry returned for a detail.
struct Facts {
    instance: stackly_domain::Instance,
    runtime: Runtime,
    paths: Vec<InstancePathEntry>,
    uses: Vec<SecretUse>,
    statuses: Vec<(SecretRef, SecretStatus)>,
    environments: Vec<Environment>,
    leases: Vec<Lease>,
}

/// Why the facts of a detail could not be read.
enum Unreadable {
    /// No instance has the ID.
    NoInstance,
    /// The registry could not answer.
    Registry(stackly_domain::RegistryError),
    /// The instance has no runtime row, which the registry forbids.
    NoRuntime,
}

impl From<stackly_domain::RegistryError> for Unreadable {
    fn from(error: stackly_domain::RegistryError) -> Self {
        Self::Registry(error)
    }
}

/// `instance.get_detail`.
pub fn get_instance_detail(
    core: &DaemonCore,
    params: GetInstanceDetailParams,
) -> HandlerFuture<'_, Box<InstanceDetail>> {
    Box::pin(async move {
        core.supervision().supervisor()?;
        let registry = registry_of(core)?;
        let id = instance_id(params.instance_id)?;
        let platform = core.platform().clone();
        let packages = core.pinned_packages().clone();
        let layout = core.prepared().layout().clone();
        let built = tokio::task::spawn_blocking(move || {
            let facts = read_facts(&registry, id)?;
            let package = packages.get(&platform, &layout, facts.instance.provider_revision());
            let snapshot = DetailSnapshot::new(
                &facts.instance,
                &facts.runtime,
                &facts.paths,
                &facts.uses,
                &facts.statuses,
                &facts.environments,
                &facts.leases,
                package.as_deref(),
                &layout,
            );
            Ok::<_, Unreadable>(build(&snapshot))
        })
        .await
        .map_err(|_| unexpected("the instance detail task failed"))?
        .map_err(|why| unreadable(why, params.instance_id))?
        .map_err(|error| detail_error(&error))?;
        Ok(Box::new(map::detail::instance_detail(&built)))
    })
}

/// `instance.get_data_usage`.
pub fn get_data_usage(
    core: &DaemonCore,
    params: GetDataUsageParams,
) -> HandlerFuture<'_, DataUsageReply> {
    Box::pin(async move {
        core.supervision().supervisor()?;
        let registry = registry_of(core)?;
        let id = instance_id(params.instance_id)?;
        let layout = core.prepared().layout().clone();
        let path = tokio::task::spawn_blocking(move || {
            let paths = registry.instance_paths(id)?.value;
            match registry.instance(id)?.value {
                Some(_) => Ok(data_folder(&layout, paths.as_deref().unwrap_or_default())),
                None => Err(Unreadable::NoInstance),
            }
        })
        .await
        .map_err(|_| unexpected("the data folder lookup task failed"))?
        .map_err(|why| unreadable(why, params.instance_id))?
        .ok_or_else(|| unexpected("the registry holds no data path for the instance"))?;
        let platform: Platform = core.platform().clone();
        let usage = core
            .data_usage()
            .usage(
                &platform,
                id,
                path,
                UsageRequest {
                    refresh: params.refresh,
                    cached_only: params.cached_only,
                },
            )
            .await
            .map_err(|failure| Box::new(map::detail::usage_failure(failure)))?;
        Ok(DataUsageReply {
            usage: usage.as_ref().map(map::detail::data_usage),
        })
    })
}

fn read_facts(registry: &SqliteRegistry, id: InstanceId) -> Result<Facts, Unreadable> {
    let instance = registry.instance(id)?.value.ok_or(Unreadable::NoInstance)?;
    let runtime = registry
        .runtime(instance.runtime_id())?
        .value
        .ok_or(Unreadable::NoRuntime)?;
    let paths = registry.instance_paths(id)?.value.unwrap_or_default();
    let uses = registry.secret_uses_by(SecretUser::Instance(id))?.value;
    let mut statuses = Vec::new();
    for used in &uses {
        if let Some(view) = registry.secret_reference(&used.reference)?.value {
            statuses.push((used.reference.clone(), view.record.status));
        }
    }
    let environments = registry.environments()?.value;
    let leases = registry.leases(id, LeaseFilter::Unreleased)?.value;
    Ok(Facts {
        instance,
        runtime,
        paths,
        uses,
        statuses,
        environments,
        leases,
    })
}

/// The absolute data folder of an instance from its stored paths.
fn data_folder(
    layout: &stackly_platform::DirectoryLayout,
    paths: &[InstancePathEntry],
) -> Option<std::path::PathBuf> {
    paths
        .iter()
        .find(|entry| entry.kind() == InstancePathKind::Data)
        .map(|entry| layout.roots().resolve(entry.root(), entry.path()))
}

fn unreadable(why: Unreadable, id: uuid::Uuid) -> Box<ErrorBody> {
    match why {
        Unreadable::NoInstance => Box::new(
            ErrorBody::new(
                &stackly_protocol::codes::resource::NOT_FOUND,
                "the instance does not exist",
            )
            .with_resource(stackly_protocol::ResourceRef {
                kind: stackly_protocol::ResourceKind::Instance,
                id: id.to_string(),
            }),
        ),
        Unreadable::Registry(error) => Box::new(map::registry_error(&error)),
        Unreadable::NoRuntime => unexpected("the registry holds no runtime for the instance"),
    }
}

fn detail_error(error: &DetailError) -> Box<ErrorBody> {
    tracing::debug!(%error, "no instance detail");
    unexpected("the registry holds no data path for the instance")
}
