//! Commits: a [`WriteBatch`] applied in one `BEGIN IMMEDIATE` transaction.
//!
//! The check order is the one the `Registry` trait documents and the fake
//! follows: `expected_generation` first, then each op in order, and within
//! an op the value shape, existence, the revision, references, and last the
//! unique rules. Every rule is checked with a `SELECT` in the same
//! transaction, so a SQL constraint that still refuses a write means a
//! check was missed (`Integrity`).

use jiff::Timestamp;
use rusqlite::{Connection, TransactionBehavior, params};
use stackly_domain::{
    ArtifactRecord, BatchError, Committed, Endpoint, Environment, Generation, InstallMode,
    Instance, Lease, LeaseHolder, LeaseState, Operation, Ownership, PortRequest, Progress,
    ProviderRevisionKey, ProviderRevisionRecord, RegistryError, ResourceRef, ResourceRevision,
    Runtime, SoftwareName, WriteBatch, WriteOp,
};
use stackly_platform::instance_path_entries;

use super::column::{corrupt, exists, first, int, opt_ts, sql_error, ts};
use super::read::{self, resource_columns};

/// Applies `batch` in one transaction stamped `now`: all of it, or none.
pub(super) fn commit(
    conn: &mut Connection,
    batch: &WriteBatch,
    now: Timestamp,
) -> Result<Committed, BatchError> {
    let batch_error = |err| BatchError::batch(sql_error(err));
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(batch_error)?;
    let current = read::generation(&tx).map_err(BatchError::batch)?;
    if let Some(expected) = batch.expected_generation
        && expected != current
    {
        return Err(BatchError::batch(RegistryError::GenerationConflict {
            expected,
            actual: current,
        }));
    }
    let stamp = ts(now);
    let mut revisions = Vec::new();
    for (index, op) in batch.ops.iter().enumerate() {
        apply(&tx, op, &stamp).map_err(|err| locate(index, err))?;
        if let Some(revision) = op.revision() {
            revisions.push((op.resource(), revision));
        }
    }
    let generation = current.next();
    write_generation(&tx, generation).map_err(BatchError::batch)?;
    tx.commit().map_err(batch_error)?;
    Ok(Committed {
        generation,
        revisions,
    })
}

/// Places an error of op `index`: the errors of the batch as a whole carry
/// no op index.
fn locate(index: usize, error: RegistryError) -> BatchError {
    match error {
        RegistryError::GenerationConflict { .. }
        | RegistryError::Busy
        | RegistryError::NoSpace
        | RegistryError::Unavailable
        | RegistryError::Io { .. } => BatchError::batch(error),
        _ => BatchError::at(index, error),
    }
}

fn write_generation(conn: &Connection, generation: Generation) -> Result<(), RegistryError> {
    conn.execute(
        "UPDATE registry_info SET generation = ?1 WHERE singleton = 1",
        [int(generation.get(), "generation")?],
    )
    .map_err(sql_error)?;
    Ok(())
}

fn apply(conn: &Connection, op: &WriteOp, now: &str) -> Result<(), RegistryError> {
    op.check_shape()?;
    match op {
        WriteOp::InsertProviderRevision(record) => insert_provider_revision(conn, record, now),
        WriteOp::InsertArtifact(record) => insert_artifact(conn, record, now),
        WriteOp::InsertRuntime(runtime) => {
            expect_new(conn, "runtimes", &runtime.id().to_string(), op)?;
            check_runtime(conn, runtime)?;
            write_runtime(conn, runtime, now, Write::Insert)
        }
        WriteOp::UpdateRuntime { runtime, expected } => {
            expect_revision(conn, "runtimes", &runtime.id().to_string(), op, *expected)?;
            check_runtime(conn, runtime)?;
            write_runtime(conn, runtime, now, Write::Update)
        }
        WriteOp::InsertInstance(instance) => {
            expect_new(conn, "instances", &instance.id().to_string(), op)?;
            let pk = check_instance(conn, instance, Write::Insert)?;
            write_instance(conn, instance, pk, now, Write::Insert)
        }
        WriteOp::UpdateInstance { instance, expected } => {
            expect_revision(conn, "instances", &instance.id().to_string(), op, *expected)?;
            let pk = check_instance(conn, instance, Write::Update)?;
            write_instance(conn, instance, pk, now, Write::Update)
        }
        WriteOp::InsertEnvironment(environment) => {
            expect_new(conn, "environments", &environment.id().to_string(), op)?;
            check_environment(conn, environment)?;
            write_environment(conn, environment, now, Write::Insert)
        }
        WriteOp::UpdateEnvironment {
            environment,
            expected,
        } => {
            let id = environment.id().to_string();
            expect_revision(conn, "environments", &id, op, *expected)?;
            check_environment(conn, environment)?;
            write_environment(conn, environment, now, Write::Update)
        }
        WriteOp::InsertLease(lease) => {
            expect_new(conn, "instance_demands", &lease.id().to_string(), op)?;
            check_lease(conn, lease)?;
            write_lease(conn, lease, Write::Insert)
        }
        WriteOp::UpdateLease(lease) => {
            let id = lease.id().to_string();
            if !exists(conn, "SELECT 1 FROM instance_demands WHERE id = ?1", [&id])? {
                return Err(RegistryError::NotFound {
                    resource: op.resource(),
                });
            }
            check_lease(conn, lease)?;
            write_lease(conn, lease, Write::Update)
        }
        WriteOp::InsertOperation(operation) => {
            expect_new(conn, "operations", &operation.id().to_string(), op)?;
            write_operation(conn, operation, Write::Insert)
        }
        WriteOp::UpdateOperation {
            operation,
            expected,
        } => {
            expect_revision(
                conn,
                "operations",
                &operation.id().to_string(),
                op,
                *expected,
            )?;
            write_operation(conn, operation, Write::Update)
        }
        _ => Err(RegistryError::Integrity {
            detail: "the SQLite registry does not support this write op yet".to_owned(),
        }),
    }
}

/// Whether an op creates its row or rewrites it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Write {
    Insert,
    Update,
}

/// Existence for an insert: no row with the ID yet.
fn expect_new(
    conn: &Connection,
    table: &'static str,
    id: &str,
    op: &WriteOp,
) -> Result<(), RegistryError> {
    let sql = format!("SELECT 1 FROM {table} WHERE id = ?1");
    if exists(conn, &sql, [id])? {
        return Err(RegistryError::DuplicateResource {
            resource: op.resource(),
        });
    }
    Ok(())
}

/// Existence and revision for an update: the row exists and is still at
/// `expected`.
fn expect_revision(
    conn: &Connection,
    table: &'static str,
    id: &str,
    op: &WriteOp,
    expected: ResourceRevision,
) -> Result<(), RegistryError> {
    let sql = format!("SELECT revision FROM {table} WHERE id = ?1");
    let stored = first(conn, &sql, [id], table, |cells| cells.revision())?;
    match stored {
        None => Err(RegistryError::NotFound {
            resource: op.resource(),
        }),
        Some(actual) if actual != expected => Err(RegistryError::RevisionConflict {
            resource: op.resource(),
            expected,
            actual,
        }),
        Some(_) => Ok(()),
    }
}

/// Finds a row's text ID with a query by key.
fn find_id<T>(
    conn: &Connection,
    sql: &str,
    params: impl rusqlite::Params,
    table: &'static str,
    column: &'static str,
) -> Result<Option<T>, RegistryError>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    first(conn, sql, params, table, |cells| cells.parse(column))
}

/// The stored `position` of an endpoint or member: its index in the
/// aggregate's list.
fn position_of(index: usize) -> Result<i64, RegistryError> {
    i64::try_from(index).map_err(|_| RegistryError::Integrity {
        detail: "a list position is larger than the registry can store".to_owned(),
    })
}

// Provider revisions and artifacts.

fn insert_provider_revision(
    conn: &Connection,
    record: &ProviderRevisionRecord,
    now: &str,
) -> Result<(), RegistryError> {
    let key = record.revision.key();
    if provider_revision_pk(conn, key)?.is_some() {
        return Err(RegistryError::DuplicateResource {
            resource: ResourceRef::ProviderRevision(Box::new(key.clone())),
        });
    }
    let mut capabilities: Vec<&str> = record
        .revision
        .capabilities()
        .iter()
        .map(|capability| capability.as_code())
        .collect();
    capabilities.sort_unstable();
    let lines: serde_json::Map<String, serde_json::Value> = record
        .revision
        .line_compatibility()
        .iter()
        .map(|(data_line, runtime_lines)| {
            let runtime_lines = runtime_lines
                .iter()
                .map(|line| serde_json::Value::from(line.as_str()))
                .collect();
            (
                data_line.to_string(),
                serde_json::Value::Array(runtime_lines),
            )
        })
        .collect();
    conn.execute(
        "INSERT INTO provider_revisions (provider, revision, manifest_sha256, software, \
         schema_version, publisher, capabilities, line_compatibility, approval, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            key.provider().as_str(),
            key.revision().as_str(),
            key.manifest_sha256().to_string(),
            record.revision.software().as_str(),
            record.schema_version,
            record.publisher,
            json_text(&capabilities)?,
            json_text(&lines)?,
            record.approval,
            now,
        ],
    )
    .map_err(sql_error)?;
    Ok(())
}

/// The JSON text of a column whose object keys and arrays are already in
/// order.
fn json_text(value: &impl serde::Serialize) -> Result<String, RegistryError> {
    serde_json::to_string(value).map_err(|err| RegistryError::Integrity {
        detail: format!("cannot encode a JSON column: {err}"),
    })
}

/// The internal key of a provider revision.
fn provider_revision_pk(
    conn: &Connection,
    key: &ProviderRevisionKey,
) -> Result<Option<(i64, String)>, RegistryError> {
    first(
        conn,
        "SELECT pk, software FROM provider_revisions \
         WHERE provider = ?1 AND revision = ?2 AND manifest_sha256 = ?3",
        (
            key.provider().as_str(),
            key.revision().as_str(),
            key.manifest_sha256().to_string(),
        ),
        "provider_revisions",
        |cells| {
            let pk = cells
                .opt_int("pk")?
                .ok_or_else(|| cells.corrupt("pk", "is null"))?;
            Ok((pk, cells.text("software")?))
        },
    )
}

fn insert_artifact(
    conn: &Connection,
    record: &ArtifactRecord,
    now: &str,
) -> Result<(), RegistryError> {
    let sha256 = record.sha256.to_string();
    if exists(conn, "SELECT 1 FROM artifacts WHERE sha256 = ?1", [&sha256])? {
        return Err(RegistryError::DuplicateResource {
            resource: ResourceRef::Artifact(record.sha256),
        });
    }
    conn.execute(
        "INSERT INTO artifacts (sha256, software, version, os, arch, minimum_os, source, \
         created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            sha256,
            record.software.as_str(),
            record.version.as_str(),
            record.target.os().as_code(),
            record.target.arch().as_code(),
            record.minimum_os,
            record.source,
            now,
        ],
    )
    .map_err(sql_error)?;
    Ok(())
}

// Runtimes.

/// References and unique rules of a runtime insert or update.
fn check_runtime(conn: &Connection, runtime: &Runtime) -> Result<(), RegistryError> {
    let id = runtime.id().to_string();
    if let InstallMode::Managed { artifact_sha256 } = runtime.install()
        && !exists(
            conn,
            "SELECT 1 FROM artifacts WHERE sha256 = ?1",
            [artifact_sha256.to_string()],
        )?
    {
        return Err(RegistryError::MissingReference {
            resource: ResourceRef::Artifact(*artifact_sha256),
        });
    }
    for dependency in runtime.depends_on() {
        if *dependency == runtime.id() {
            return Err(RegistryError::Integrity {
                detail: "a runtime cannot depend on itself".to_owned(),
            });
        }
        if !exists(
            conn,
            "SELECT 1 FROM runtimes WHERE id = ?1",
            [dependency.to_string()],
        )? {
            return Err(RegistryError::MissingReference {
                resource: ResourceRef::Runtime(*dependency),
            });
        }
    }
    let existing = match runtime.install() {
        InstallMode::Managed { artifact_sha256 } => find_id(
            conn,
            "SELECT id FROM runtimes WHERE install_mode = 'managed' AND artifact_sha256 = ?1 \
             AND id <> ?2",
            (artifact_sha256.to_string(), &id),
            "runtimes",
            "id",
        )?,
        InstallMode::External { root, .. } => find_id(
            conn,
            "SELECT id FROM runtimes WHERE install_mode = 'external' AND external_root = ?1 \
             AND id <> ?2",
            (utf8_root(root)?, &id),
            "runtimes",
            "id",
        )?,
    };
    match existing {
        Some(existing) => Err(RegistryError::DuplicateRuntime { existing }),
        None => Ok(()),
    }
}

/// The text of an external root. [`WriteOp::check_shape`] already
/// rejected one that is not UTF-8.
fn utf8_root(root: &std::path::Path) -> Result<&str, RegistryError> {
    root.to_str().ok_or(RegistryError::NotUtf8Path {
        field: "external_root",
    })
}

fn write_runtime(
    conn: &Connection,
    runtime: &Runtime,
    now: &str,
    write: Write,
) -> Result<(), RegistryError> {
    let id = runtime.id().to_string();
    let (mode, artifact, root, observed) = match runtime.install() {
        InstallMode::Managed { artifact_sha256 } => {
            ("managed", Some(artifact_sha256.to_string()), None, None)
        }
        InstallMode::External {
            root,
            observed_sha256,
        } => (
            "external",
            None,
            Some(utf8_root(root)?),
            observed_sha256.map(|sha| sha.to_string()),
        ),
    };
    let values = params![
        id,
        runtime.software().as_str(),
        runtime.version().as_str(),
        runtime.line().as_str(),
        runtime.target().os().as_code(),
        runtime.target().arch().as_code(),
        mode,
        artifact,
        root,
        observed,
        runtime.state().as_code(),
        opt_ts(runtime.installed_at()),
        opt_ts(runtime.last_verified_at()),
        int(runtime.revision().get(), "revision")?,
        now,
    ];
    let sql = match write {
        Write::Insert => {
            "INSERT INTO runtimes (id, software, version, line, os, arch, install_mode, \
             artifact_sha256, external_root, observed_sha256, state, installed_at, \
             last_verified_at, revision, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?15)"
        }
        Write::Update => {
            "UPDATE runtimes SET software = ?2, version = ?3, line = ?4, os = ?5, arch = ?6, \
             install_mode = ?7, artifact_sha256 = ?8, external_root = ?9, \
             observed_sha256 = ?10, state = ?11, installed_at = ?12, last_verified_at = ?13, \
             revision = ?14, updated_at = ?15 WHERE id = ?1"
        }
    };
    conn.execute(sql, values).map_err(sql_error)?;
    if write == Write::Update {
        conn.execute(
            "DELETE FROM runtime_dependencies WHERE runtime_id = ?1",
            [&id],
        )
        .map_err(sql_error)?;
    }
    for dependency in runtime.depends_on() {
        conn.execute(
            "INSERT INTO runtime_dependencies (runtime_id, dependency_runtime_id) \
             VALUES (?1, ?2)",
            (&id, dependency.to_string()),
        )
        .map_err(sql_error)?;
    }
    Ok(())
}

// Instances.

/// References and unique rules of an instance insert or update, and the
/// internal key of its provider revision.
fn check_instance(
    conn: &Connection,
    instance: &Instance,
    write: Write,
) -> Result<i64, RegistryError> {
    let id = instance.id().to_string();
    let runtime_software: Option<String> = first(
        conn,
        "SELECT software FROM runtimes WHERE id = ?1",
        [instance.runtime_id().to_string()],
        "runtimes",
        |cells| cells.text("software"),
    )?;
    let runtime_software = runtime_software.ok_or(RegistryError::MissingReference {
        resource: ResourceRef::Runtime(instance.runtime_id()),
    })?;
    let key = instance.provider_revision();
    let (pk, provider_software) =
        provider_revision_pk(conn, key)?.ok_or_else(|| RegistryError::MissingReference {
            resource: ResourceRef::ProviderRevision(Box::new(key.clone())),
        })?;
    if instance.retained_at().is_none()
        && let Some(existing) = find_id(
            conn,
            "SELECT id FROM instances WHERE name = ?1 AND retained_at IS NULL AND id <> ?2",
            (instance.name().as_str(), &id),
            "instances",
            "id",
        )?
    {
        return Err(RegistryError::NameTaken {
            name: instance.name().clone(),
            existing,
        });
    }
    if write == Write::Insert {
        let key = instance.id().short_key();
        if let Some(existing) = find_id(
            conn,
            "SELECT id FROM instances WHERE substr(id, 29) = ?1 AND id <> ?2",
            (key.to_string(), &id),
            "instances",
            "id",
        )? {
            return Err(RegistryError::ShortKeyTaken { key, existing });
        }
        for entry in instance_path_entries(instance.id()) {
            if let Some(existing) = find_id(
                conn,
                "SELECT instance_id FROM instance_paths \
                 WHERE logical_root = ?1 AND relative_path = ?2 AND instance_id <> ?3",
                (entry.root().as_code(), entry.path().as_str(), &id),
                "instance_paths",
                "instance_id",
            )? {
                return Err(RegistryError::PathTaken {
                    root: entry.root(),
                    path: entry.path().clone(),
                    existing,
                });
            }
        }
    }
    let software = instance.software().as_str();
    if software != runtime_software || software != provider_software {
        let parse = |text: &str, table| {
            text.parse::<SoftwareName>()
                .map_err(|err| corrupt(table, "software", err))
        };
        return Err(RegistryError::SoftwareMismatch {
            instance: instance.software().clone(),
            runtime: parse(&runtime_software, "runtimes")?,
            provider_revision: parse(&provider_software, "provider_revisions")?,
        });
    }
    Ok(pk)
}

fn write_instance(
    conn: &Connection,
    instance: &Instance,
    provider_revision_pk: i64,
    now: &str,
    write: Write,
) -> Result<(), RegistryError> {
    let id = instance.id().to_string();
    let values = params![
        id,
        instance.name().as_str(),
        instance.display_name().map(|name| name.as_str()),
        provider_revision_pk,
        instance.software().as_str(),
        instance.runtime_id().to_string(),
        instance.data_line().as_str(),
        instance.data_version().as_str(),
        opt_ts(instance.data_initialized_at()),
        instance.desired().as_code(),
        instance.startup().as_code(),
        instance.lifecycle().as_code(),
        instance.last_failure().map(|code| code.as_str()),
        opt_ts(instance.retained_at()),
        instance.retained_by().map(|id| id.to_string()),
        int(instance.revision().get(), "revision")?,
        now,
        instance.created_by().to_string(),
        ts(instance.created_at()),
    ];
    match write {
        Write::Insert => {
            conn.execute(
                "INSERT INTO instances (id, name, display_name, provider_revision_pk, software, \
                 runtime_id, data_line, data_version, data_initialized_at, desired, startup, \
                 lifecycle, last_failure, retained_at, retained_by_operation, revision, \
                 updated_at, created_by_operation, created_at, ownership) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, \
                 ?17, ?18, ?19, 'created')",
                values,
            )
            .map_err(sql_error)?;
            for entry in instance_path_entries(instance.id()) {
                conn.execute(
                    "INSERT INTO instance_paths (instance_id, kind, logical_root, \
                     relative_path, managed) VALUES (?1, ?2, ?3, ?4, 1)",
                    (
                        &id,
                        entry.kind().as_code(),
                        entry.root().as_code(),
                        entry.path().as_str(),
                    ),
                )
                .map_err(sql_error)?;
            }
        }
        Write::Update => {
            // Ownership, config, and paths never change through an update;
            // the creation fields come from the aggregate, which keeps them.
            conn.execute(
                "UPDATE instances SET name = ?2, display_name = ?3, provider_revision_pk = ?4, \
                 software = ?5, runtime_id = ?6, data_line = ?7, data_version = ?8, \
                 data_initialized_at = ?9, desired = ?10, startup = ?11, lifecycle = ?12, \
                 last_failure = ?13, retained_at = ?14, retained_by_operation = ?15, \
                 revision = ?16, updated_at = ?17, created_by_operation = ?18, \
                 created_at = ?19 WHERE id = ?1",
                values,
            )
            .map_err(sql_error)?;
            conn.execute(
                "DELETE FROM instance_endpoints WHERE instance_id = ?1",
                [&id],
            )
            .map_err(sql_error)?;
        }
    }
    for (position, endpoint) in instance.endpoints().iter().enumerate() {
        write_endpoint(conn, &id, position, endpoint)?;
    }
    Ok(())
}

fn write_endpoint(
    conn: &Connection,
    instance: &str,
    position: usize,
    endpoint: &Endpoint,
) -> Result<(), RegistryError> {
    let position = position_of(position)?;
    match endpoint {
        Endpoint::Ip(ip) => {
            let (request, fixed) = match ip.request() {
                PortRequest::Fixed(port) => ("fixed", Some(port.get())),
                PortRequest::Auto => ("auto", None),
            };
            conn.execute(
                "INSERT INTO instance_endpoints (instance_id, position, name, kind, transport, \
                 address, port_request, fixed_port, bound_port, enabled) \
                 VALUES (?1, ?2, ?3, 'ip', ?4, ?5, ?6, ?7, ?8, 1)",
                params![
                    instance,
                    position,
                    ip.name().as_str(),
                    ip.transport().as_code(),
                    ip.address().to_string(),
                    request,
                    fixed,
                    ip.bound().map(|port| port.get()),
                ],
            )
        }
        Endpoint::Unix(unix) => conn.execute(
            "INSERT INTO instance_endpoints (instance_id, position, name, kind, socket_name, \
             enabled) VALUES (?1, ?2, ?3, 'unix', ?4, 1)",
            params![
                instance,
                position,
                unix.name().as_str(),
                unix.socket().as_str()
            ],
        ),
    }
    .map_err(sql_error)?;
    Ok(())
}

// Environments.

/// References and unique rules of an environment insert or update.
fn check_environment(conn: &Connection, environment: &Environment) -> Result<(), RegistryError> {
    for member in environment.members() {
        if !exists(
            conn,
            "SELECT 1 FROM instances WHERE id = ?1",
            [member.instance_id().to_string()],
        )? {
            return Err(RegistryError::MissingReference {
                resource: ResourceRef::Instance(member.instance_id()),
            });
        }
    }
    for member in environment.members() {
        if member.ownership() != Ownership::Dedicated {
            continue;
        }
        if let Some(owner) = find_id(
            conn,
            "SELECT environment_id FROM environment_instances \
             WHERE instance_id = ?1 AND ownership = 'dedicated' AND environment_id <> ?2",
            (
                member.instance_id().to_string(),
                environment.id().to_string(),
            ),
            "environment_instances",
            "environment_id",
        )? {
            return Err(RegistryError::DedicatedOwnerExists {
                instance: member.instance_id(),
                environment: owner,
            });
        }
    }
    Ok(())
}

fn write_environment(
    conn: &Connection,
    environment: &Environment,
    now: &str,
    write: Write,
) -> Result<(), RegistryError> {
    let id = environment.id().to_string();
    let values = params![
        id,
        environment.name().as_str(),
        environment.manifest_sha256().map(ToString::to_string),
        int(environment.revision().get(), "revision")?,
        now,
    ];
    match write {
        Write::Insert => conn.execute(
            "INSERT INTO environments (id, name, manifest_sha256, revision, created_at, \
             updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
            values,
        ),
        Write::Update => conn
            .execute(
                "UPDATE environments SET name = ?2, manifest_sha256 = ?3, revision = ?4, \
                 updated_at = ?5 WHERE id = ?1",
                values,
            )
            .and_then(|_| {
                conn.execute(
                    "DELETE FROM environment_instances WHERE environment_id = ?1",
                    [&id],
                )
            }),
    }
    .map_err(sql_error)?;
    for (position, member) in environment.members().iter().enumerate() {
        conn.execute(
            "INSERT INTO environment_instances (environment_id, position, alias, instance_id, \
             ownership) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                id,
                position_of(position)?,
                member.alias().as_str(),
                member.instance_id().to_string(),
                member.ownership().as_code(),
            ],
        )
        .map_err(sql_error)?;
    }
    Ok(())
}

// Leases.

/// References and unique rules of a lease insert or update.
fn check_lease(conn: &Connection, lease: &Lease) -> Result<(), RegistryError> {
    if !exists(
        conn,
        "SELECT 1 FROM instances WHERE id = ?1",
        [lease.instance_id().to_string()],
    )? {
        return Err(RegistryError::MissingReference {
            resource: ResourceRef::Instance(lease.instance_id()),
        });
    }
    let LeaseHolder::Environment(environment) = lease.holder() else {
        return Ok(());
    };
    if !exists(
        conn,
        "SELECT 1 FROM environments WHERE id = ?1",
        [environment.to_string()],
    )? {
        return Err(RegistryError::MissingReference {
            resource: ResourceRef::Environment(environment),
        });
    }
    if lease.is_active()
        && exists(
            conn,
            "SELECT 1 FROM instance_demands WHERE instance_id = ?1 AND source_id = ?2 \
             AND source_kind = 'environment' AND state = 'active' AND id <> ?3",
            (
                lease.instance_id().to_string(),
                environment.to_string(),
                lease.id().to_string(),
            ),
        )?
    {
        return Err(RegistryError::ActiveLeaseExists {
            instance: lease.instance_id(),
            environment,
        });
    }
    Ok(())
}

fn write_lease(conn: &Connection, lease: &Lease, write: Write) -> Result<(), RegistryError> {
    let (source_kind, source_id) = match lease.holder() {
        LeaseHolder::Environment(id) => ("environment", Some(id.to_string())),
        LeaseHolder::Manual => ("manual", None),
        LeaseHolder::Startup => ("startup", None),
    };
    let changed_at = match lease.state() {
        LeaseState::Active => None,
        LeaseState::Suppressed { at } | LeaseState::Released { at } => Some(ts(at)),
    };
    let values = params![
        lease.id().to_string(),
        lease.instance_id().to_string(),
        source_kind,
        source_id,
        lease.session().map(|session| session.as_str()),
        lease.state().as_code(),
        changed_at,
        ts(lease.acquired_at()),
    ];
    let sql = match write {
        Write::Insert => {
            "INSERT INTO instance_demands (id, instance_id, source_kind, source_id, session_id, \
             state, state_changed_at, acquired_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)"
        }
        Write::Update => {
            "UPDATE instance_demands SET instance_id = ?2, source_kind = ?3, source_id = ?4, \
             session_id = ?5, state = ?6, state_changed_at = ?7, acquired_at = ?8 WHERE id = ?1"
        }
    };
    conn.execute(sql, values).map_err(sql_error)?;
    Ok(())
}

// Operations.

fn write_operation(
    conn: &Connection,
    operation: &Operation,
    write: Write,
) -> Result<(), RegistryError> {
    let (resource_kind, resource_id) = resource_columns(operation.resource());
    let (done, total) = match operation.progress() {
        Progress::Indeterminate => (None, None),
        Progress::Counted { done, total } => (
            Some(int(done, "progress_done")?),
            Some(int(total, "progress_total")?),
        ),
    };
    let failure = operation.failure();
    let values = params![
        operation.id().to_string(),
        operation.kind().as_code(),
        resource_kind,
        resource_id,
        operation.idempotency_key().map(|key| key.as_str()),
        operation.state().as_code(),
        operation.stage().map(|stage| stage.as_str()),
        done,
        total,
        failure.map(|failure| failure.code().as_str()),
        failure.map(|failure| failure.retryable()),
        opt_ts(operation.started_at()),
        int(operation.revision().get(), "revision")?,
        ts(operation.created_at()),
        ts(operation.updated_at()),
    ];
    let sql = match write {
        Write::Insert => {
            "INSERT INTO operations (id, kind, resource_kind, resource_id, idempotency_key, \
             state, stage, progress_done, progress_total, failure_code, failure_retryable, \
             started_at, revision, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)"
        }
        Write::Update => {
            "UPDATE operations SET kind = ?2, resource_kind = ?3, resource_id = ?4, \
             idempotency_key = ?5, state = ?6, stage = ?7, progress_done = ?8, \
             progress_total = ?9, failure_code = ?10, failure_retryable = ?11, \
             started_at = ?12, revision = ?13, created_at = ?14, updated_at = ?15 WHERE id = ?1"
        }
    };
    conn.execute(sql, values).map_err(sql_error)?;
    Ok(())
}
