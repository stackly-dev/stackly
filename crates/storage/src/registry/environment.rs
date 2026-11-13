//! The environment rows of spec 0044: the `services` snapshot as JSON and
//! `WriteOp::DeleteEnvironment`; and those of spec 0046: the last Up report
//! in `environments.last_up` and the members' `dependency_conditions`.

use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use stackly_domain::{
    DeclaredService, DependencyCondition, DependencyEdge, EnvironmentId, MemberDependency,
    NotDeletableReason, Ownership, RegistryError, ResourceRef, ResourceRevision, ServiceReport,
    UpReport,
};

use super::column::{exists, first, sql_error};

/// One element of `environments.services`.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ServiceJson {
    alias: String,
    provider: String,
    ownership: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    selector: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    instance_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    instance_name: Option<String>,
    depends_on: Vec<EdgeJson>,
}

/// One element of `dependsOn`.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EdgeJson {
    service: String,
    condition: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    timeout_seconds: Option<u32>,
}

/// Writes the snapshot as the JSON text of `environments.services`.
pub(super) fn services_json(declared: &[DeclaredService]) -> Result<String, RegistryError> {
    let rows: Vec<ServiceJson> = declared
        .iter()
        .map(|service| ServiceJson {
            alias: service.alias().to_string(),
            provider: service.provider().to_string(),
            ownership: service.ownership().as_code().to_owned(),
            selector: service.selector().map(ToString::to_string),
            instance_ref: service.instance_ref().map(ToString::to_string),
            instance_name: service.instance_name().map(ToString::to_string),
            depends_on: service
                .depends_on()
                .iter()
                .map(|edge| EdgeJson {
                    service: edge.service().to_string(),
                    condition: edge.condition().as_code().to_owned(),
                    timeout_seconds: edge.timeout_seconds(),
                })
                .collect(),
        })
        .collect();
    serde_json::to_string(&rows).map_err(|err| RegistryError::Io {
        detail: format!("environment services do not serialize: {err}"),
    })
}

/// Reads the JSON text of `environments.services`; the error is the
/// corrupt detail.
pub(super) fn parse_services(text: &str) -> Result<Vec<DeclaredService>, String> {
    let rows: Vec<ServiceJson> = serde_json::from_str(text).map_err(|err| err.to_string())?;
    rows.into_iter()
        .map(|row| {
            let depends_on = row
                .depends_on
                .into_iter()
                .map(|edge| {
                    Ok(DependencyEdge::new(
                        parse(&edge.service)?,
                        parse::<DependencyCondition>(&edge.condition)?,
                        edge.timeout_seconds,
                    ))
                })
                .collect::<Result<Vec<_>, String>>()?;
            Ok(DeclaredService::new(
                parse(&row.alias)?,
                parse(&row.provider)?,
                parse::<Ownership>(&row.ownership)?,
                row.selector.as_deref().map(parse).transpose()?,
                row.instance_ref.as_deref().map(parse).transpose()?,
                row.instance_name.as_deref().map(parse).transpose()?,
                depends_on,
            ))
        })
        .collect()
}

/// One element of `environment_instances.dependency_conditions`.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ConditionJson {
    service: String,
    condition: String,
    timeout_seconds: u32,
}

/// Writes the conditions of a member as the JSON text of
/// `environment_instances.dependency_conditions`.
pub(super) fn conditions_json(conditions: &[MemberDependency]) -> Result<String, RegistryError> {
    let rows: Vec<ConditionJson> = conditions
        .iter()
        .map(|condition| ConditionJson {
            service: condition.service().to_string(),
            condition: condition.condition().as_code().to_owned(),
            timeout_seconds: condition.timeout_seconds(),
        })
        .collect();
    encode("dependency conditions", &rows)
}

/// Reads the JSON text of `environment_instances.dependency_conditions`;
/// the error is the corrupt detail.
pub(super) fn parse_conditions(text: &str) -> Result<Vec<MemberDependency>, String> {
    let rows: Vec<ConditionJson> = serde_json::from_str(text).map_err(|err| err.to_string())?;
    rows.into_iter()
        .map(|row| {
            Ok(MemberDependency::new(
                parse(&row.service)?,
                parse::<DependencyCondition>(&row.condition)?,
                row.timeout_seconds,
            ))
        })
        .collect()
}

/// `environments.last_up`.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LastUpJson {
    operation_id: String,
    state: String,
    started_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    finished_at: Option<String>,
    services: Vec<ServiceReportJson>,
}

/// One element of `services` in `environments.last_up`.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ServiceReportJson {
    alias: String,
    phase: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    stage: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    dependency: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    operation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    references: Vec<String>,
}

/// Writes an Up report as the JSON text of `environments.last_up`.
pub(super) fn last_up_json(report: &UpReport) -> Result<String, RegistryError> {
    let row = LastUpJson {
        operation_id: report.operation().to_string(),
        state: report.state().as_code().to_owned(),
        started_at: super::column::ts(report.started_at()),
        finished_at: report.finished_at().map(super::column::ts),
        services: report
            .services()
            .iter()
            .map(|service| ServiceReportJson {
                alias: service.alias().to_string(),
                phase: service.phase().as_code().to_owned(),
                stage: service.stage().map(|stage| stage.as_code().to_owned()),
                code: service.code().map(ToString::to_string),
                dependency: service.dependency().map(ToString::to_string),
                reason: service.reason().map(|reason| reason.as_code().to_owned()),
                operation_id: service.operation().map(|id| id.to_string()),
                references: service
                    .references()
                    .iter()
                    .map(ToString::to_string)
                    .collect(),
            })
            .collect(),
    };
    encode("the last up report", &row)
}

/// Reads `environments.last_up`, `None` when the column is null; the error
/// is the corrupt detail.
pub(super) fn parse_last_up(text: Option<&str>) -> Result<Option<UpReport>, String> {
    let Some(text) = text else {
        return Ok(None);
    };
    let row: LastUpJson = serde_json::from_str(text).map_err(|err| err.to_string())?;
    let services = row
        .services
        .into_iter()
        .map(|service| {
            let mut report = ServiceReport::new(parse(&service.alias)?, parse(&service.phase)?);
            if let Some(stage) = service.stage {
                report = report.with_stage(parse(&stage)?);
            }
            if let Some(code) = service.code {
                report = report.with_code(parse(&code)?);
            }
            match (service.dependency, service.reason) {
                (Some(dependency), Some(reason)) => {
                    report = report.with_dependency(parse(&dependency)?, parse(&reason)?);
                }
                (None, None) => {}
                _ => return Err("dependency and reason are not set together".to_owned()),
            }
            if let Some(operation) = service.operation_id {
                report = report.with_operation(parse(&operation)?);
            }
            let references = service
                .references
                .iter()
                .map(|reference| parse(reference))
                .collect::<Result<Vec<_>, String>>()?;
            Ok(report.with_references(references))
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(Some(UpReport::new(
        parse(&row.operation_id)?,
        parse(&row.state)?,
        parse(&row.started_at)?,
        row.finished_at.as_deref().map(parse).transpose()?,
        services,
    )))
}

fn encode<T: Serialize>(what: &str, value: &T) -> Result<String, RegistryError> {
    serde_json::to_string(value).map_err(|err| RegistryError::Io {
        detail: format!("{what} do not serialize: {err}"),
    })
}

fn parse<T>(text: &str) -> Result<T, String>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    text.parse().map_err(|err: T::Err| err.to_string())
}

/// `WriteOp::DeleteEnvironment`: refuses while a demand row of the
/// environment is `active`, then deletes its `released` demand rows and
/// its secret uses, moves the secret references it owns to the user, and
/// deletes the row (the members cascade).
pub(super) fn delete(
    conn: &Connection,
    id: EnvironmentId,
    expected: ResourceRevision,
    now: &str,
) -> Result<(), RegistryError> {
    let key = id.to_string();
    let resource = ResourceRef::Environment(id);
    let stored = first(
        conn,
        "SELECT revision FROM environments WHERE id = ?1",
        [&key],
        "environments",
        |cells| cells.revision(),
    )?;
    match stored {
        None => return Err(RegistryError::NotFound { resource }),
        Some(actual) if actual != expected => {
            return Err(RegistryError::RevisionConflict {
                resource,
                expected,
                actual,
            });
        }
        Some(_) => {}
    }
    if exists(
        conn,
        "SELECT 1 FROM instance_demands WHERE source_kind = 'environment' AND source_id = ?1 \
         AND state <> 'released'",
        [&key],
    )? {
        return Err(RegistryError::NotDeletable {
            resource,
            reason: NotDeletableReason::ActiveLease,
        });
    }
    conn.execute(
        "DELETE FROM instance_demands WHERE source_kind = 'environment' AND source_id = ?1",
        [&key],
    )
    .map_err(sql_error)?;
    conn.execute(
        "DELETE FROM secret_reference_uses WHERE resource_kind = 'environment' \
         AND resource_id = ?1",
        [&key],
    )
    .map_err(sql_error)?;
    conn.execute(
        "UPDATE secret_references SET owner_kind = 'user', owner_id = NULL, \
         revision = revision + 1, updated_at = ?2 \
         WHERE owner_kind = 'environment' AND owner_id = ?1",
        params![key, now],
    )
    .map_err(sql_error)?;
    conn.execute("DELETE FROM environments WHERE id = ?1", [&key])
        .map_err(sql_error)?;
    Ok(())
}
