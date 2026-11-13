//! The DTOs of the `env` area (spec 0044, *API surface*), built from the
//! core validation, plan, and status. A domain type never reaches the wire,
//! and no text of the manifest file reaches it except what core already
//! put in a finding's fixed template, a name, an alias, or a reference.

use stackly_core::diagnostics::HomeMask;
use stackly_core::environment::{
    BlockerKind, DetachedMember as CoreDetached, EnvironmentView, Finding, Plan,
    PlanBlocker as CoreBlocker, PlanItem as CoreItem, SUPPORTED_API_VERSIONS, ServiceStatus,
    Validation,
};
use stackly_domain::{DeclaredService, Environment, PortRequest as DomainPortRequest};
use stackly_protocol::env::{
    DeclaredServiceDto, DependencyConditionKind, DetachedMember, EnvFinding, EnvPlan,
    EnvValidation, EnvironmentRef, EnvironmentService, EnvironmentSummary, PlanBlocker,
    PlanDifference, PlanEndpoint, PlanInstance, PlanItem, PlanRuntime, PlanSecret,
    ServiceDependency, ServiceInstance, ServiceNotReady,
};
use stackly_protocol::instance::{InstanceSummary, PortRequest};
use stackly_protocol::stop_all::InstanceRef;
use stackly_protocol::{Count, ErrorBody, Timestamp};

use crate::ipc::operation::dto::revision;
use crate::map;
use crate::map::catalog::wire;
use crate::membership::env_ownership;

/// One finding on the wire.
fn finding(finding: &Finding) -> EnvFinding {
    EnvFinding {
        severity: wire(finding.severity().as_code()),
        code: finding.code().to_string(),
        location: finding.location().to_string(),
        service: finding.service().map(ToString::to_string),
        message: finding.message().to_owned(),
    }
}

/// One declared service of a snapshot on the wire.
#[must_use]
pub fn declared(service: &DeclaredService) -> DeclaredServiceDto {
    DeclaredServiceDto {
        alias: service.alias().to_string(),
        provider: service.provider().to_string(),
        ownership: env_ownership(service.ownership()),
        selector: service.selector().map(ToString::to_string),
        instance_ref: service.instance_ref().map(ToString::to_string),
        instance_name: service.instance_name().map(ToString::to_string),
        depends_on: service
            .depends_on()
            .iter()
            .map(|edge| ServiceDependency {
                service: edge.service().to_string(),
                condition: DependencyConditionKind::Ready,
                timeout_seconds: edge.timeout_seconds(),
            })
            .collect(),
    }
}

/// The validation of `file`, as the request named it.
#[must_use]
pub fn validation(file: &str, validation: &Validation) -> EnvValidation {
    let findings: Vec<EnvFinding> = validation.findings.list().iter().map(finding).collect();
    EnvValidation {
        file: file.to_owned(),
        valid: validation.is_valid(),
        api_version: validation.api_version.map(str::to_owned),
        supported_api_versions: SUPPORTED_API_VERSIONS
            .iter()
            .map(|&version| version.to_owned())
            .collect(),
        name: validation.name.as_ref().map(ToString::to_string),
        manifest_sha256: Some(validation.sha256.to_string()),
        services: validation.services.iter().map(declared).collect(),
        findings,
        finding_count: Count::saturating(validation.findings.count() as u64),
        omitted: Count::saturating(validation.findings.omitted() as u64),
    }
}

const fn port(request: DomainPortRequest) -> PortRequest {
    match request {
        DomainPortRequest::Fixed(port) => PortRequest::Fixed(port.get()),
        DomainPortRequest::Auto => PortRequest::Auto,
    }
}

fn item(item: &CoreItem) -> PlanItem {
    PlanItem {
        alias: item.alias.to_string(),
        ownership: env_ownership(item.ownership),
        action: wire(item.action.as_code()),
        steps: item.steps.iter().map(|step| wire(step.as_code())).collect(),
        instance: PlanInstance {
            id: item.instance_id.map(|id| id.as_uuid()),
            name: item.instance_name.to_string(),
        },
        provider: item.provider.to_string(),
        trust: item.trust.map(|trust| wire(trust.as_code())),
        runtime: item.runtime.as_ref().map(|runtime| PlanRuntime {
            selector: runtime.selector.clone(),
            version: runtime.version.to_string(),
            artifact_id: runtime.artifact_id.clone(),
            artifact_sha256: runtime.artifact_sha256.to_string(),
            install: runtime.install,
            runtime_id: runtime.runtime_id.map(|id| id.as_uuid()),
            download_size: runtime.download_size.map(Count::saturating),
        }),
        endpoints: item
            .endpoints
            .iter()
            .map(|endpoint| PlanEndpoint {
                name: endpoint.name.to_string(),
                host: endpoint.host.to_string(),
                port: port(endpoint.port),
            })
            .collect(),
        secrets: item
            .secrets
            .iter()
            .map(|secret| PlanSecret {
                field: secret.field.clone(),
                reference: secret.reference.as_ref().map(ToString::to_string),
                status: wire(secret.status.as_code()),
            })
            .collect(),
        waits_for: item.waits_for.iter().map(ToString::to_string).collect(),
        differences: item
            .differences
            .iter()
            .map(|difference| PlanDifference {
                field: difference.field.clone(),
                manifest: difference.manifest.clone(),
                instance: difference.instance.clone(),
            })
            .collect(),
        checks: item
            .checks
            .iter()
            .map(|check| map::create::check(check, &[]))
            .collect(),
    }
}

/// `body` with the static hint of its `env.*` code, unless it carries one.
#[must_use]
pub fn with_env_hint(body: ErrorBody) -> ErrorBody {
    if body.hint().is_some() || !body.code().starts_with("env.") {
        return body;
    }
    match stackly_protocol::codes::env::hint(body.code(), body.details()) {
        Some(hint) => body.with_hint(hint),
        None => body,
    }
}

/// The error body of one plan blocker: the code create, resolve, or the
/// registry refuses with, so GUI and CLI show the same message and hint.
fn blocker_error(blocker: &CoreBlocker, items: &[CoreItem], home: &HomeMask) -> ErrorBody {
    use stackly_protocol::codes::env;
    let service = blocker.service.as_ref().map(ToString::to_string);
    let with_service = |body: ErrorBody| match &service {
        Some(alias) => body.with_detail("service", alias.as_str()),
        None => body,
    };
    let body = match &blocker.kind {
        BlockerKind::Create(error) => {
            map::create::with_create_hint(map::create::create_error(error, &[]))
        }
        BlockerKind::Resolve(error) => {
            let item = items
                .iter()
                .find(|item| Some(&item.alias) == blocker.service.as_ref());
            let provider = item
                .map(|item| item.provider.to_string())
                .unwrap_or_default();
            let selector = item
                .and_then(|item| item.runtime.as_ref())
                .map_or_else(|| "stable".to_owned(), |runtime| runtime.selector.clone());
            map::runtime::resolve_error(error, &provider, &selector)
        }
        BlockerKind::NameTaken { name, location } => {
            let body = ErrorBody::new(
                &env::NAME_TAKEN,
                "the environment name is registered at another manifest location",
            )
            .with_detail("name", name.as_str());
            match location {
                Some(location) => {
                    body.with_detail("location", home.mask(location.as_str()).as_ref())
                }
                None => body,
            }
        }
        BlockerKind::ManifestRenamed {
            declared,
            registered,
        } => ErrorBody::new(
            &env::MANIFEST_RENAMED,
            "the file of this environment declares another name",
        )
        .with_detail("declared", declared.as_str())
        .with_detail("registered", registered.as_str()),
        BlockerKind::InstanceRefNotFound { instance_ref } => with_service(ErrorBody::new(
            &env::INSTANCE_REF_NOT_FOUND,
            "no instance has the name this service borrows",
        ))
        .with_detail("instanceRef", instance_ref.as_str()),
        BlockerKind::InstanceRefMismatch { expected, actual } => with_service(ErrorBody::new(
            &env::INSTANCE_REF_MISMATCH,
            "the borrowed instance runs another provider",
        ))
        .with_detail("expected", expected.as_str())
        .with_detail("actual", actual.as_str()),
        BlockerKind::InstanceBlocked { instance, reason } => with_service(ErrorBody::new(
            &env::INSTANCE_BLOCKED,
            "the instance of this service can't be started",
        ))
        .with_detail("instance", instance.as_str())
        .with_detail("reason", reason.as_str()),
        _ => ErrorBody::new(
            &stackly_protocol::codes::internal::UNEXPECTED,
            "the plan found a blocker this build cannot name",
        ),
    };
    with_env_hint(body)
}

fn detached(member: &CoreDetached) -> DetachedMember {
    DetachedMember {
        alias: member.alias.to_string(),
        instance: InstanceRef {
            instance_id: member.instance_id.as_uuid(),
            name: member
                .instance_name
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_default(),
        },
        ownership: env_ownership(member.ownership),
    }
}

/// The plan on the wire, with the validation it was built from.
#[must_use]
pub fn plan(validation: EnvValidation, plan: &Plan, home: &HomeMask) -> EnvPlan {
    EnvPlan {
        validation,
        registration: plan
            .registration
            .map(|registration| wire(registration.as_code())),
        environment: plan.environment.as_ref().map(|env| EnvironmentRef {
            id: env.id.as_uuid(),
            name: env.name.to_string(),
            revision: revision(env.revision),
        }),
        registered_location: plan
            .registered_location
            .as_ref()
            .map(|location| location.as_str().to_owned()),
        applicable: plan.applicable,
        plan_digest: plan.digest.map(|digest| digest.to_string()),
        items: plan.items.iter().map(item).collect(),
        order: plan.order.iter().map(ToString::to_string).collect(),
        blockers: plan
            .blockers
            .iter()
            .map(|blocker| PlanBlocker {
                service: blocker.service.as_ref().map(ToString::to_string),
                error: blocker_error(blocker, &plan.items, home),
            })
            .collect(),
        detached: plan.detached.iter().map(detached).collect(),
        needs_secrets: Count::saturating(plan.needs_secrets as u64),
    }
}

/// The summary of `environment` with its status.
#[must_use]
pub fn summary(
    environment: &Environment,
    view: &EnvironmentView,
    updated_at: Timestamp,
) -> EnvironmentSummary {
    EnvironmentSummary {
        id: environment.id().as_uuid(),
        name: environment.name().to_string(),
        revision: revision(environment.revision()),
        manifest_location: environment
            .manifest_location()
            .map(|location| location.as_str().to_owned()),
        manifest_sha256: environment.manifest_sha256().map(ToString::to_string),
        status: wire(view.status.as_code()),
        ready_count: Count::saturating(view.ready_count as u64),
        service_count: Count::saturating(view.service_count as u64),
        updated_at,
    }
}

/// One service of the snapshot with its instance, whose lifecycle,
/// readiness, and health come from the same summary `instance.get` sends.
#[must_use]
pub fn service(status: &ServiceStatus, summaries: &[InstanceSummary]) -> EnvironmentService {
    EnvironmentService {
        alias: status.alias.to_string(),
        provider: status.provider.to_string(),
        ownership: env_ownership(status.ownership),
        selector: status.selector.as_ref().map(ToString::to_string),
        instance_ref: status.instance_ref.as_ref().map(ToString::to_string),
        instance: status
            .instance
            .as_ref()
            .and_then(|view| summaries.iter().find(|s| s.id == view.id.as_uuid()))
            .map(|summary| ServiceInstance {
                id: summary.id,
                name: summary.name.clone(),
                runtime_version: summary.runtime_version.clone(),
                lifecycle: summary.lifecycle,
                readiness: summary.readiness.clone(),
                health: summary.health.clone(),
            }),
        not_ready: status
            .not_ready
            .as_ref()
            .map(|(reason, detail)| ServiceNotReady {
                reason: wire(reason.as_code()),
                detail: detail.clone(),
            }),
    }
}

/// The members a snapshot no longer declares.
#[must_use]
pub fn detached_members(view: &EnvironmentView) -> Vec<DetachedMember> {
    view.detached.iter().map(detached).collect()
}
