//! Reads: stored rows back into aggregates and records, and the queries of
//! spec 0005 step 12.
//!
//! Every function runs inside the read transaction the caller opened, so
//! the rows and the generation come from one snapshot. Aggregates go
//! through their domain `restore` constructor, which checks the invariants
//! between fields.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use jiff::Timestamp;
use rusqlite::{Connection, params_from_iter};
use stackly_domain::{
    ArtifactRecord, Capability, CapabilitySet, CompatibilityRecordRow, DesiredState, Endpoint,
    Environment, EnvironmentId, EnvironmentMember, Generation, InstallDetails, InstallMode,
    Instance, InstanceFilter, InstanceId, InstanceLifecycle, InstancePathEntry, InstancePathKind,
    IpEndpoint, Lease, LeaseFilter, LeaseHolder, LeaseState, LineCompatibility, LogicalRoot,
    Operation, OperationCursor, OperationFailure, OperationId, OperationPage, OperationQuery,
    OperationResource, Ownership, PortRequest, Progress, ProviderRevision, ProviderRevisionKey,
    ProviderRevisionRecord, ProviderSourceId, RegistryError, RelativePath, ResourceRef, Runtime,
    RuntimeId, RuntimeLine, RuntimeReferences, Sha256Digest, StoredEnvironment, StoredInstance,
    StoredLease, StoredOperation, StoredRuntime, Target, Transport, UnixEndpoint,
};

use super::column::{Cells, collect, corrupt, exists, first, inconsistent, sql_error, ts};

/// Reads the registry's generation.
pub(super) fn generation(conn: &Connection) -> Result<Generation, RegistryError> {
    let value: i64 = conn
        .query_row(
            "SELECT generation FROM registry_info WHERE singleton = 1",
            [],
            |row| row.get(0),
        )
        .map_err(sql_error)?;
    u64::try_from(value)
        .map(Generation::new)
        .map_err(|_| corrupt("registry_info", "generation", "is negative"))
}

/// The `WHERE` clause and parameter of a read by key, or of a read of
/// every row when `key` is `None`.
fn by_key(column: &str, key: Option<String>) -> (String, impl rusqlite::Params) {
    let clause = if key.is_some() {
        format!("WHERE {column} = ?1")
    } else {
        String::new()
    };
    (clause, params_from_iter(key))
}

// Provider revisions and artifacts.

pub(super) fn provider_revision(
    conn: &Connection,
    key: &ProviderRevisionKey,
) -> Result<Option<ProviderRevisionRecord>, RegistryError> {
    first(
        conn,
        "SELECT provider, revision, manifest_sha256, software, schema_version, publisher, \
         capabilities, line_compatibility, approval, source_id FROM provider_revisions \
         WHERE provider = ?1 AND revision = ?2 AND manifest_sha256 = ?3",
        (
            key.provider().as_str(),
            key.revision().as_str(),
            key.manifest_sha256().to_string(),
        ),
        "provider_revisions",
        |cells| {
            let key = ProviderRevisionKey::new(
                cells.parse("provider")?,
                cells.parse("revision")?,
                cells.parse("manifest_sha256")?,
            );
            let revision = ProviderRevision::restore(
                key,
                cells.parse("software")?,
                capabilities(cells)?,
                line_compatibility(cells)?,
            );
            let record = ProviderRevisionRecord::new(
                revision,
                cells.opt_text("schema_version")?,
                cells.opt_text("publisher")?,
                cells.opt_text("approval")?,
            );
            Ok(match cells.opt_parse::<ProviderSourceId>("source_id")? {
                Some(source) => record.with_source_id(source),
                None => record,
            })
        },
    )
}

fn capabilities(cells: Cells<'_, '_>) -> Result<CapabilitySet, RegistryError> {
    let codes: Vec<String> = serde_json::from_str(&cells.text("capabilities")?)
        .map_err(|err| cells.corrupt("capabilities", err))?;
    codes
        .iter()
        .map(|code| {
            code.parse::<Capability>()
                .map_err(|err| cells.corrupt("capabilities", err))
        })
        .collect()
}

fn line_compatibility(cells: Cells<'_, '_>) -> Result<LineCompatibility, RegistryError> {
    let map: BTreeMap<String, Vec<String>> =
        serde_json::from_str(&cells.text("line_compatibility")?)
            .map_err(|err| cells.corrupt("line_compatibility", err))?;
    let line = |text: &str| {
        text.parse::<RuntimeLine>()
            .map_err(|err| cells.corrupt("line_compatibility", err))
    };
    let mut lines = BTreeMap::new();
    for (data_line, runtime_lines) in &map {
        let runtime_lines = runtime_lines
            .iter()
            .map(|text| line(text))
            .collect::<Result<BTreeSet<_>, _>>()?;
        lines.insert(line(data_line)?, runtime_lines);
    }
    Ok(LineCompatibility::from_map(lines))
}

pub(super) fn artifact(
    conn: &Connection,
    sha256: &Sha256Digest,
) -> Result<Option<ArtifactRecord>, RegistryError> {
    let key = sha256.to_string();
    let found = first(
        conn,
        "SELECT sha256, software, version, os, arch, minimum_os, source, signature_info, \
         dependency_manifest, catalog_id, build, format, size_bytes FROM artifacts \
         WHERE sha256 = ?1",
        [&key],
        "artifacts",
        |cells| {
            let build = cells
                .opt_int("build")?
                .map(|build| {
                    u32::try_from(build).map_err(|_| cells.corrupt("build", "is out of range"))
                })
                .transpose()?;
            let details = InstallDetails {
                catalog_id: cells.opt_text("catalog_id")?,
                build,
                format: cells.opt_parse("format")?,
                size: cells.opt_u64("size_bytes")?,
                signature_info: cells.opt_parse("signature_info")?,
                dependency_manifest: cells.opt_parse("dependency_manifest")?,
                compatibility: Vec::new(),
            };
            Ok(ArtifactRecord::new(
                cells.parse("sha256")?,
                cells.parse("software")?,
                cells.parse("version")?,
                target(cells)?,
                cells.opt_text("minimum_os")?,
                cells.opt_text("source")?,
            )
            .with_install_details(details))
        },
    )?;
    let Some(mut record) = found else {
        return Ok(None);
    };
    record.compatibility = collect(
        conn,
        "SELECT scope, provider, provider_revision, manifest_sha256, os, os_version, os_build, \
         arch, result, checks, known_limitations, summary, tested_at, evidence \
         FROM compatibility_records WHERE artifact_sha256 = ?1 ORDER BY position",
        [&key],
        "compatibility_records",
        compatibility_row,
    )?;
    Ok(Some(record))
}

fn compatibility_row(cells: Cells<'_, '_>) -> Result<CompatibilityRecordRow, RegistryError> {
    let revision = match (
        cells.opt_parse("provider")?,
        cells.opt_parse("provider_revision")?,
        cells.opt_parse("manifest_sha256")?,
    ) {
        (Some(provider), Some(revision), Some(digest)) => {
            Some(ProviderRevisionKey::new(provider, revision, digest))
        }
        (None, None, None) => None,
        _ => return Err(cells.corrupt("provider", "is set without its revision columns")),
    };
    let codes = |column: &'static str| -> Result<Vec<String>, RegistryError> {
        serde_json::from_str(&cells.text(column)?).map_err(|err| cells.corrupt(column, err))
    };
    CompatibilityRecordRow::new(
        cells.parse("scope")?,
        revision,
        cells.parse("os")?,
        cells.parse("os_version")?,
        cells.opt_text("os_build")?,
        cells.parse("arch")?,
        cells.parse("result")?,
        codes("checks")?,
        codes("known_limitations")?,
        cells.opt_text("summary")?,
        cells.ts("tested_at")?,
        cells.text("evidence")?,
    )
    .map_err(|err| cells.corrupt("*", err))
}

fn target(cells: Cells<'_, '_>) -> Result<Target, RegistryError> {
    Ok(Target::new(cells.parse("os")?, cells.parse("arch")?))
}

// Runtimes.

/// Reads one runtime, or every runtime ordered by ID.
pub(super) fn runtimes(
    conn: &Connection,
    id: Option<RuntimeId>,
) -> Result<Vec<Runtime>, RegistryError> {
    let key = id.map(|id| id.to_string());
    let mut depends_on = runtime_dependencies(conn, key.clone())?;
    let (clause, params) = by_key("id", key);
    collect(
        conn,
        &format!(
            "SELECT id, software, version, line, os, arch, install_mode, artifact_sha256, \
             external_root, observed_sha256, state, installed_at, last_verified_at, revision \
             FROM runtimes {clause} ORDER BY id"
        ),
        params,
        "runtimes",
        |cells| {
            let id: RuntimeId = cells.parse("id")?;
            let install = match cells.text("install_mode")?.as_str() {
                "managed" => InstallMode::Managed {
                    artifact_sha256: cells.parse("artifact_sha256")?,
                },
                "external" => InstallMode::External {
                    root: PathBuf::from(cells.text("external_root")?),
                    observed_sha256: cells.opt_parse("observed_sha256")?,
                },
                _ => return Err(cells.corrupt("install_mode", "is not managed or external")),
            };
            let stored = StoredRuntime {
                id,
                software: cells.parse("software")?,
                version: cells.parse("version")?,
                line: cells.parse("line")?,
                target: target(cells)?,
                install,
                state: cells.parse("state")?,
                installed_at: cells.opt_ts("installed_at")?,
                last_verified_at: cells.opt_ts("last_verified_at")?,
                depends_on: depends_on.remove(&id).unwrap_or_default(),
                revision: cells.revision()?,
            };
            Runtime::restore(stored).map_err(|err| inconsistent("runtimes", &err))
        },
    )
}

fn runtime_dependencies(
    conn: &Connection,
    key: Option<String>,
) -> Result<BTreeMap<RuntimeId, BTreeSet<RuntimeId>>, RegistryError> {
    let (clause, params) = by_key("runtime_id", key);
    let pairs = collect(
        conn,
        &format!("SELECT runtime_id, dependency_runtime_id FROM runtime_dependencies {clause}"),
        params,
        "runtime_dependencies",
        |cells| {
            Ok((
                cells.parse::<RuntimeId>("runtime_id")?,
                cells.parse::<RuntimeId>("dependency_runtime_id")?,
            ))
        },
    )?;
    let mut map: BTreeMap<RuntimeId, BTreeSet<RuntimeId>> = BTreeMap::new();
    for (runtime, dependency) in pairs {
        map.entry(runtime).or_default().insert(dependency);
    }
    Ok(map)
}

/// Everything that still refers to a runtime, each list ordered by ID.
pub(super) fn runtime_references(
    conn: &Connection,
    id: RuntimeId,
) -> Result<RuntimeReferences, RegistryError> {
    let key = id.to_string();
    if !exists(conn, "SELECT 1 FROM runtimes WHERE id = ?1", [&key])? {
        return Err(RegistryError::NotFound {
            resource: ResourceRef::Runtime(id),
        });
    }
    Ok(RuntimeReferences {
        instances: ids(
            conn,
            "SELECT id FROM instances WHERE runtime_id = ?1 ORDER BY id",
            &key,
            "instances",
            "id",
        )?,
        dependent_runtimes: ids(
            conn,
            "SELECT runtime_id FROM runtime_dependencies WHERE dependency_runtime_id = ?1 \
         ORDER BY runtime_id",
            &key,
            "runtime_dependencies",
            "runtime_id",
        )?,
        active_operations: ids(
            conn,
            &format!(
                "SELECT id FROM operations WHERE resource_kind = 'runtime' AND resource_id = ?1 \
             AND state NOT IN {FINAL_OPERATION_STATES} ORDER BY id"
            ),
            &key,
            "operations",
            "id",
        )?,
        recovery_snapshots: ids(
            conn,
            "SELECT id FROM backups WHERE runtime_id = ?1 ORDER BY id",
            &key,
            "backups",
            "id",
        )?,
    })
}

/// The IDs one column of a query by key returns, in query order.
fn ids<T>(
    conn: &Connection,
    sql: &str,
    key: &str,
    table: &'static str,
    column: &'static str,
) -> Result<Vec<T>, RegistryError>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    collect(conn, sql, [key], table, |cells| cells.parse(column))
}

// Instances.

/// Which instances an instance read returns.
pub(super) enum InstanceSelect<'a> {
    /// One instance by ID, retained or not.
    Id(InstanceId),
    /// The instance that is not retained and has this name.
    Name(&'a str),
    /// A list, ordered by name and then ID.
    List(InstanceFilter),
}

/// Reads instances. Readiness and health come back `unknown`, reason
/// `daemon_restarted`, observed at `loaded_at`.
pub(super) fn instances(
    conn: &Connection,
    select: &InstanceSelect<'_>,
    loaded_at: Timestamp,
) -> Result<Vec<Instance>, RegistryError> {
    let (clause, key) = match select {
        InstanceSelect::Id(id) => ("WHERE i.id = ?1", Some(id.to_string())),
        InstanceSelect::Name(name) => (
            "WHERE i.name = ?1 AND i.retained_at IS NULL",
            Some((*name).to_owned()),
        ),
        InstanceSelect::List(filter) if filter.include_retained => ("", None),
        InstanceSelect::List(_) => ("WHERE i.retained_at IS NULL", None),
    };
    let only = match select {
        InstanceSelect::Id(id) => Some(id.to_string()),
        InstanceSelect::Name(_) | InstanceSelect::List(_) => None,
    };
    let mut endpoints = endpoints(conn, only)?;
    collect(
        conn,
        &format!(
            "SELECT i.id AS id, i.name AS name, i.display_name AS display_name, \
             p.provider AS provider, p.revision AS provider_revision, \
             p.manifest_sha256 AS manifest_sha256, i.software AS software, \
             i.runtime_id AS runtime_id, i.data_line AS data_line, \
             i.data_version AS data_version, i.data_initialized_at AS data_initialized_at, \
             i.desired AS desired, i.startup AS startup, i.lifecycle AS lifecycle, \
             i.last_failure AS last_failure, i.revision AS revision, \
             i.created_at AS created_at, i.created_by_operation AS created_by_operation, \
             i.retained_at AS retained_at, i.retained_by_operation AS retained_by_operation \
             FROM instances i JOIN provider_revisions p ON p.pk = i.provider_revision_pk \
             {clause} ORDER BY i.name, i.id"
        ),
        params_from_iter(key),
        "instances",
        |cells| {
            let id: InstanceId = cells.parse("id")?;
            let provider = cells.of("provider_revisions");
            let provider_revision = ProviderRevisionKey::new(
                provider.parse("provider")?,
                provider.parse("provider_revision")?,
                provider.parse("manifest_sha256")?,
            );
            let stored = StoredInstance {
                id,
                name: cells.parse("name")?,
                display_name: cells.opt_parse("display_name")?,
                provider_revision,
                software: cells.parse("software")?,
                runtime_id: cells.parse("runtime_id")?,
                data_line: cells.parse("data_line")?,
                data_version: cells.parse("data_version")?,
                data_initialized_at: cells.opt_ts("data_initialized_at")?,
                endpoints: endpoints.remove(&id).unwrap_or_default(),
                desired: cells.parse::<DesiredState>("desired")?,
                startup: cells.parse("startup")?,
                lifecycle: cells.parse::<InstanceLifecycle>("lifecycle")?,
                last_failure: cells.opt_parse("last_failure")?,
                revision: cells.revision()?,
                created_at: cells.ts("created_at")?,
                created_by: cells.parse("created_by_operation")?,
                retained_at: cells.opt_ts("retained_at")?,
                retained_by: cells.opt_parse("retained_by_operation")?,
            };
            Instance::restore(stored, loaded_at).map_err(|err| inconsistent("instances", &err))
        },
    )
}

fn endpoints(
    conn: &Connection,
    key: Option<String>,
) -> Result<BTreeMap<InstanceId, Vec<Endpoint>>, RegistryError> {
    let (clause, params) = by_key("instance_id", key);
    let rows = collect(
        conn,
        &format!(
            "SELECT instance_id, name, kind, transport, address, port_request, fixed_port, \
             bound_port, socket_name FROM instance_endpoints {clause} \
             ORDER BY instance_id, position"
        ),
        params,
        "instance_endpoints",
        |cells| {
            let instance: InstanceId = cells.parse("instance_id")?;
            let name = cells.parse("name")?;
            let endpoint = match cells.text("kind")?.as_str() {
                "ip" => {
                    let request = match cells.text("port_request")?.as_str() {
                        "fixed" => PortRequest::Fixed(
                            cells
                                .opt_port("fixed_port")?
                                .ok_or_else(|| cells.corrupt("fixed_port", "is null"))?,
                        ),
                        "auto" => PortRequest::Auto,
                        _ => return Err(cells.corrupt("port_request", "is not fixed or auto")),
                    };
                    Endpoint::Ip(IpEndpoint::new(
                        name,
                        cells.parse::<Transport>("transport")?,
                        cells.parse("address")?,
                        request,
                        cells.opt_port("bound_port")?,
                    ))
                }
                "unix" => Endpoint::Unix(UnixEndpoint::new(name, cells.parse("socket_name")?)),
                _ => return Err(cells.corrupt("kind", "is not ip or unix")),
            };
            Ok((instance, endpoint))
        },
    )?;
    let mut map: BTreeMap<InstanceId, Vec<Endpoint>> = BTreeMap::new();
    for (instance, endpoint) in rows {
        map.entry(instance).or_default().push(endpoint);
    }
    Ok(map)
}

/// The ID of every instance, retained ones included, ordered by ID.
pub(super) fn known_instance_ids(conn: &Connection) -> Result<Vec<InstanceId>, RegistryError> {
    collect(
        conn,
        "SELECT id FROM instances ORDER BY id",
        [],
        "instances",
        |cells| cells.parse("id"),
    )
}

/// The seven stored paths of an instance in [`InstancePathKind::ALL`]
/// order, or `None` for an unknown instance.
pub(super) fn instance_paths(
    conn: &Connection,
    id: InstanceId,
) -> Result<Option<Vec<InstancePathEntry>>, RegistryError> {
    let key = id.to_string();
    if !exists(conn, "SELECT 1 FROM instances WHERE id = ?1", [&key])? {
        return Ok(None);
    }
    let mut entries = collect(
        conn,
        "SELECT kind, logical_root, relative_path FROM instance_paths WHERE instance_id = ?1",
        [&key],
        "instance_paths",
        |cells| {
            Ok(InstancePathEntry::new(
                cells.parse::<InstancePathKind>("kind")?,
                cells.parse::<LogicalRoot>("logical_root")?,
                cells.parse::<RelativePath>("relative_path")?,
            ))
        },
    )?;
    entries.sort_by_key(InstancePathEntry::kind);
    Ok(Some(entries))
}

// Environments.

/// Reads one environment, or every environment ordered by ID.
pub(super) fn environments(
    conn: &Connection,
    id: Option<EnvironmentId>,
) -> Result<Vec<Environment>, RegistryError> {
    let key = id.map(|id| id.to_string());
    let mut members = members(conn, key.clone())?;
    let (clause, params) = by_key("id", key);
    collect(
        conn,
        &format!(
            "SELECT id, name, manifest_sha256, revision FROM environments {clause} ORDER BY id"
        ),
        params,
        "environments",
        |cells| {
            let id: EnvironmentId = cells.parse("id")?;
            let stored = StoredEnvironment {
                id,
                name: cells.parse("name")?,
                manifest_sha256: cells.opt_parse("manifest_sha256")?,
                members: members.remove(&id).unwrap_or_default(),
                revision: cells.revision()?,
            };
            Environment::restore(stored).map_err(|err| inconsistent("environments", &err))
        },
    )
}

fn members(
    conn: &Connection,
    key: Option<String>,
) -> Result<BTreeMap<EnvironmentId, Vec<EnvironmentMember>>, RegistryError> {
    let (clause, params) = by_key("environment_id", key);
    let rows = collect(
        conn,
        &format!(
            "SELECT environment_id, alias, instance_id, ownership FROM environment_instances \
             {clause} ORDER BY environment_id, position"
        ),
        params,
        "environment_instances",
        |cells| {
            Ok((
                cells.parse::<EnvironmentId>("environment_id")?,
                EnvironmentMember::new(
                    cells.parse("alias")?,
                    cells.parse("instance_id")?,
                    cells.parse::<Ownership>("ownership")?,
                ),
            ))
        },
    )?;
    let mut map: BTreeMap<EnvironmentId, Vec<EnvironmentMember>> = BTreeMap::new();
    for (environment, member) in rows {
        map.entry(environment).or_default().push(member);
    }
    Ok(map)
}

// Leases.

/// The leases of an instance that `filter` selects, ordered by
/// `acquired_at` and then ID.
pub(super) fn leases(
    conn: &Connection,
    instance: InstanceId,
    filter: LeaseFilter,
) -> Result<Vec<Lease>, RegistryError> {
    let states = match filter {
        LeaseFilter::All => "",
        LeaseFilter::Unreleased => "AND state IN ('active', 'suppressed')",
    };
    collect(
        conn,
        &format!(
            "SELECT id, instance_id, source_kind, source_id, session_id, state, \
             state_changed_at, acquired_at FROM instance_demands WHERE instance_id = ?1 \
             {states} ORDER BY acquired_at, id"
        ),
        [instance.to_string()],
        "instance_demands",
        |cells| {
            let holder = match cells.text("source_kind")?.as_str() {
                "environment" => LeaseHolder::Environment(cells.parse("source_id")?),
                "manual" => LeaseHolder::Manual,
                "startup" => LeaseHolder::Startup,
                _ => return Err(cells.corrupt("source_kind", "is not a lease holder")),
            };
            let changed_at = || {
                cells
                    .opt_ts("state_changed_at")?
                    .ok_or_else(|| cells.corrupt("state_changed_at", "is null"))
            };
            let state = match cells.text("state")?.as_str() {
                "active" => LeaseState::Active,
                "suppressed" => LeaseState::Suppressed { at: changed_at()? },
                "released" => LeaseState::Released { at: changed_at()? },
                _ => return Err(cells.corrupt("state", "is not a lease state")),
            };
            let stored = StoredLease {
                id: cells.parse("id")?,
                instance_id: cells.parse("instance_id")?,
                holder,
                session: cells.opt_parse("session_id")?,
                state,
                acquired_at: cells.ts("acquired_at")?,
            };
            Lease::restore(stored).map_err(|err| inconsistent("instance_demands", &err))
        },
    )
}

// Operations.

pub(super) const OPERATION_COLUMNS: &str = "id, kind, resource_kind, resource_id, idempotency_key, state, \
     stage, progress_done, progress_total, failure_code, failure_retryable, recovery_hint, \
     started_at, revision, created_at, updated_at";

/// Final operation states, as SQL.
pub(super) const FINAL_OPERATION_STATES: &str = "('succeeded', 'failed', 'cancelled')";

pub(super) fn operation(
    conn: &Connection,
    id: OperationId,
) -> Result<Option<Operation>, RegistryError> {
    first(
        conn,
        &format!("SELECT {OPERATION_COLUMNS} FROM operations WHERE id = ?1"),
        [id.to_string()],
        "operations",
        stored_operation,
    )
}

/// One page of operations, newest `(created_at, id)` first. The query was
/// validated by the caller.
pub(super) fn operations(
    conn: &Connection,
    query: &OperationQuery,
) -> Result<OperationPage, RegistryError> {
    let mut clauses = Vec::new();
    let mut params: Vec<String> = Vec::new();
    if query.active_only {
        clauses.push(format!("state NOT IN {FINAL_OPERATION_STATES}"));
    }
    match query.resource {
        None => {}
        Some(OperationResource::Manager) => clauses.push("resource_kind = 'manager'".to_owned()),
        Some(resource) => {
            let (kind, id) = resource_columns(resource);
            params.push(kind.to_owned());
            params.push(id.unwrap_or_default());
            clauses.push(format!(
                "resource_kind = ?{} AND resource_id = ?{}",
                params.len() - 1,
                params.len()
            ));
        }
    }
    if let Some(before) = query.before {
        params.push(ts(before.created_at));
        params.push(before.id.to_string());
        clauses.push(format!(
            "(created_at, id) < (?{}, ?{})",
            params.len() - 1,
            params.len()
        ));
    }
    let clause = if clauses.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", clauses.join(" AND "))
    };
    // One row more than the page shows whether another page follows.
    let limit = usize::from(query.limit);
    let mut items = collect(
        conn,
        &format!(
            "SELECT {OPERATION_COLUMNS} FROM operations {clause} \
             ORDER BY created_at DESC, id DESC LIMIT {}",
            limit + 1
        ),
        params_from_iter(params),
        "operations",
        stored_operation,
    )?;
    let next = if items.len() > limit {
        items.truncate(limit);
        items.last().map(OperationCursor::of)
    } else {
        None
    };
    Ok(OperationPage { items, next })
}

/// The `resource_kind` and `resource_id` columns of an operation resource.
pub(super) fn resource_columns(resource: OperationResource) -> (&'static str, Option<String>) {
    match resource {
        OperationResource::Runtime(id) => ("runtime", Some(id.to_string())),
        OperationResource::Instance(id) => ("instance", Some(id.to_string())),
        OperationResource::Environment(id) => ("environment", Some(id.to_string())),
        OperationResource::Manager => ("manager", None),
    }
}

pub(super) fn stored_operation(cells: Cells<'_, '_>) -> Result<Operation, RegistryError> {
    let resource = match cells.text("resource_kind")?.as_str() {
        "runtime" => OperationResource::Runtime(cells.parse("resource_id")?),
        "instance" => OperationResource::Instance(cells.parse("resource_id")?),
        "environment" => OperationResource::Environment(cells.parse("resource_id")?),
        "manager" => OperationResource::Manager,
        _ => return Err(cells.corrupt("resource_kind", "is not an operation resource")),
    };
    let progress = match (
        cells.opt_u64("progress_done")?,
        cells.opt_u64("progress_total")?,
    ) {
        (None, None) => Progress::Indeterminate,
        (Some(done), Some(total)) => Progress::Counted { done, total },
        _ => return Err(cells.corrupt("progress_total", "is set without progress_done")),
    };
    let failure = match (
        cells.opt_parse("failure_code")?,
        cells.opt_bool("failure_retryable")?,
    ) {
        (None, None) => None,
        (Some(code), Some(retryable)) => Some(OperationFailure::new(code, retryable)),
        _ => return Err(cells.corrupt("failure_retryable", "is set without failure_code")),
    };
    let stored = StoredOperation {
        id: cells.parse("id")?,
        kind: cells.parse("kind")?,
        resource,
        idempotency_key: cells.opt_parse("idempotency_key")?,
        state: cells.parse("state")?,
        started_at: cells.opt_ts("started_at")?,
        stage: cells.opt_parse("stage")?,
        progress,
        failure,
        recovery_hint: cells.opt_parse("recovery_hint")?,
        created_at: cells.ts("created_at")?,
        updated_at: cells.ts("updated_at")?,
        revision: cells.revision()?,
    };
    Operation::restore(stored).map_err(|err| inconsistent("operations", &err))
}
