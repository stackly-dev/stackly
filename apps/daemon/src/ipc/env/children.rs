//! The ports of Up in `stacklyd` (spec 0046, *Operasi anak*): `EnvPorts`
//! starts the install, create, and start children through the same
//! request builders as `runtime.install`, `instance.create`, and
//! `instance.start`, so the trust gate, `gather` and `evaluate`,
//! `install_precheck`, and the data check run exactly as for a client
//! request; and it reads the plan and an instance as `env.plan` and the
//! supervisor see them. It holds the core weakly, so a queued Up never
//! keeps it alive past a stop.

use std::sync::{Arc, Weak};

use stackly_core::environment::children::{ChildOrigin, ChildStart};
use stackly_core::environment::up::{
    BlockedItem, InstanceNow, PlanRequest, PlannedUp, UpChildren, UpError, UpReads,
};
use stackly_core::environment::{BlockerKind, EnvOpError, PlanAction};
use stackly_core::instance::create::{CreateDraft, EnvironmentBinding};
use stackly_core::operation::BoxFuture;
use stackly_core::supervisor::StartRequest;
use stackly_domain::{
    DetailKey, ErrorCodeName, FailureError, FailureItem, InstanceBlocker, InstanceId,
    InstanceLifecycle, MemberAlias, NeedsSecretItem, NeedsSecretReason, OperationId, ProviderName,
    ReadinessState, ReasonCode, Registry, RuntimeId, SecretRef, SecretUser, Sha256Digest,
};
use stackly_protocol::ErrorBody;
use stackly_protocol::runtime::InstallRuntimeParams;

use super::{PlanFailure, plan_with};
use crate::ipc::operation::{StartOutcome, start_child};
use crate::state::DaemonCore;

/// The ports of Up on this daemon.
#[derive(Debug)]
pub struct EnvPorts {
    core: Weak<DaemonCore>,
}

impl EnvPorts {
    /// The ports of `core`.
    #[must_use]
    pub fn new(core: &Arc<DaemonCore>) -> Self {
        Self {
            core: Arc::downgrade(core),
        }
    }

    /// Loads the membership cache again once the Up `operation` ends, so
    /// the instance summaries name the members it bound (spec 0044 AC-15).
    pub(crate) fn reload_membership_after(&self, operation: uuid::Uuid) {
        let core = self.core.clone();
        tokio::spawn(async move {
            let Some(engine) = core
                .upgrade()
                .and_then(|core| core.operations().writer().ok())
            else {
                return;
            };
            let Ok(id) = OperationId::from_uuid(operation) else {
                return;
            };
            let _ = engine.wait_final(id).await;
            if let Some(core) = core.upgrade() {
                crate::membership::reload(&core).await;
            }
        });
    }

    fn core(&self) -> Result<Arc<DaemonCore>, FailureError> {
        self.core.upgrade().ok_or_else(|| {
            failure(
                ErrorCodeName::from_static("daemon.shutting_down"),
                "the daemon is shutting down",
            )
        })
    }
}

fn failure(code: ErrorCodeName, message: &str) -> FailureError {
    FailureError::new(code, message, Vec::new())
        .expect("a fixed failure message is 1 to 200 characters")
}

/// The stored form of a refusal the daemon answered with: its code,
/// message, details, and items, so a service's report names the code a
/// client would have seen.
pub(crate) fn failure_of(body: &ErrorBody) -> FailureError {
    let code = body
        .code()
        .parse::<ErrorCodeName>()
        .unwrap_or(stackly_core::operation::codes::INTERNAL_UNEXPECTED);
    let message: String = body
        .message()
        .chars()
        .take(FailureError::MESSAGE_MAX)
        .collect();
    let message = if message.is_empty() {
        "the child operation was refused".to_owned()
    } else {
        message
    };
    let details: Vec<(DetailKey, String)> = body
        .details()
        .iter()
        .filter_map(|detail| {
            let key = detail.key().parse::<DetailKey>().ok()?;
            let value: String = detail
                .value()
                .chars()
                .take(FailureError::DETAIL_VALUE_MAX / 4)
                .collect();
            Some((key, value))
        })
        .collect();
    let items: Vec<FailureItem> = body
        .items()
        .iter()
        .take(FailureError::ITEMS_MAX)
        .filter_map(|item| {
            let kind = item.kind().parse::<ReasonCode>().ok()?;
            let fields = item
                .fields()
                .iter()
                .filter_map(|field| Some((field.key().parse().ok()?, field.value().to_owned())))
                .collect();
            FailureItem::new(kind, fields).ok()
        })
        .collect();
    let fallback = || failure(code.clone(), "the child operation was refused");
    match FailureError::new(code.clone(), message, details) {
        Ok(error) => error.with_items(items).unwrap_or_else(|_| fallback()),
        Err(_) => fallback(),
    }
}

fn started<T>(outcome: &StartOutcome) -> Option<ChildStart<T>> {
    match outcome {
        StartOutcome::Accepted(accepted) => OperationId::from_uuid(accepted.operation.id)
            .ok()
            .map(|operation| ChildStart::Started { operation }),
        StartOutcome::NothingToDo => None,
    }
}

impl UpChildren for EnvPorts {
    fn install(
        &self,
        parent: &ChildOrigin,
        provider: &ProviderName,
        artifact: Sha256Digest,
    ) -> BoxFuture<'_, Result<ChildStart<RuntimeId>, FailureError>> {
        let parent = parent.clone();
        let provider = provider.clone();
        Box::pin(async move {
            let core = self.core()?;
            let params = InstallRuntimeParams {
                provider: provider.to_string(),
                selector: format!("sha256:{artifact}"),
                expected_sha256: Some(artifact.to_string()),
            };
            let prepared = crate::ipc::runtime::install_request(&core, &params)
                .map_err(|body| failure_of(&body))?;
            let outcome = start_child(
                &core,
                parent.operation.clone(),
                "runtime.install",
                prepared.request,
            )
            .await
            .map_err(|body| failure_of(&body))?;
            match started(&outcome) {
                Some(start) => Ok(start),
                None => crate::ipc::runtime::installed_runtime_id(&core, &artifact)
                    .await
                    .map(ChildStart::AlreadyDone)
                    .map_err(|body| failure_of(&body)),
            }
        })
    }

    fn create(
        &self,
        parent: &ChildOrigin,
        draft: CreateDraft,
        binding: EnvironmentBinding,
    ) -> BoxFuture<'_, Result<ChildStart<InstanceId>, FailureError>> {
        let parent = parent.clone();
        Box::pin(async move {
            let core = self.core()?;
            let prepared = crate::ipc::instance::create_request(
                &core,
                draft,
                Vec::new(),
                parent.caller.clone(),
                Some(binding),
            )
            .await
            .map_err(|body| failure_of(&body))?;
            let outcome = start_child(
                &core,
                parent.operation.clone(),
                "instance.create",
                prepared.request,
            )
            .await
            .map_err(|body| {
                failure_of(&crate::ipc::instance::with_issues(body, &prepared.refused))
            })?;
            started(&outcome).ok_or_else(|| {
                failure(
                    stackly_core::operation::codes::INTERNAL_UNEXPECTED,
                    "a create answered without an operation",
                )
            })
        })
    }

    fn start(
        &self,
        parent: &ChildOrigin,
        instance: InstanceId,
    ) -> BoxFuture<'_, Result<ChildStart<InstanceId>, FailureError>> {
        let parent = parent.clone();
        Box::pin(async move {
            let core = self.core()?;
            let request = crate::ipc::instance::start_request(
                &core,
                instance,
                None,
                StartRequest::environment(instance, parent.caller.clone()),
            )
            .map_err(|body| failure_of(&body))?;
            let outcome = start_child(&core, parent.operation.clone(), "instance.start", request)
                .await
                .map_err(|body| failure_of(&body))?;
            crate::ipc::instance::announce(&core, instance, &outcome).await;
            Ok(started(&outcome).unwrap_or(ChildStart::AlreadyDone(instance)))
        })
    }
}

impl UpReads for EnvPorts {
    fn plan(&self, request: &PlanRequest) -> BoxFuture<'_, Result<PlannedUp, UpError>> {
        let request = request.clone();
        Box::pin(async move {
            let core = self.core().map_err(|_| {
                UpError::Environment(EnvOpError::Internal("the daemon is shutting down"))
            })?;
            planned_up(&core, &request)
                .await
                .map_err(|error| match error {
                    PlanFailure::Unreadable(reason) => {
                        UpError::Environment(EnvOpError::Unreadable(reason))
                    }
                    PlanFailure::Body(body) => {
                        tracing::warn!(code = body.code(), "the plan of an Up could not be read");
                        UpError::Environment(EnvOpError::Internal("the plan could not be read"))
                    }
                })
        })
    }

    fn instance(&self, id: InstanceId) -> BoxFuture<'_, Option<InstanceNow>> {
        Box::pin(async move {
            let core = self.core.upgrade()?;
            instance_now(&core, id).await
        })
    }
}

/// One instance now: its registry row and the supervisor's status.
pub(crate) async fn instance_now(core: &DaemonCore, id: InstanceId) -> Option<InstanceNow> {
    let registry = core.registry()?;
    let supervisor = core.supervision().supervisor().ok()?;
    let row = tokio::task::spawn_blocking(move || registry.instance(id))
        .await
        .ok()?
        .ok()?
        .value?;
    let status = supervisor.status(id);
    Some(InstanceNow {
        lifecycle: super::shown_lifecycle(row.lifecycle(), &status),
        ready: row.lifecycle() == InstanceLifecycle::Running
            && status.readiness.state() == ReadinessState::Ready,
        refuses_start: matches!(
            status.blocker,
            Some(
                InstanceBlocker::DataInUse { .. }
                    | InstanceBlocker::IdentityUnproven { .. }
                    | InstanceBlocker::ConfigDrift { .. }
            )
        ),
    })
}

/// The plan of `request` with what the precheck and `accept` need beyond
/// it: the message of each blocker, the references of members blocked for
/// a secret, and the references of `create` items that have no value.
pub(crate) async fn planned_up(
    core: &DaemonCore,
    request: &PlanRequest,
) -> Result<PlannedUp, PlanFailure> {
    let file = request.path.to_string_lossy().into_owned();
    let planned = plan_with(core, &file, request.registered, request.locked).await?;
    let home = core.home_mask().clone();
    let blocked = planned
        .plan
        .blockers
        .iter()
        .map(|blocker| {
            let body = super::dto::blocker_error(blocker, &planned.plan.items, &home);
            BlockedItem {
                service: blocker.service.clone(),
                code: body.code().to_owned(),
                message: body.message().to_owned(),
            }
        })
        .collect();
    let supervisor = core.supervision().supervisor()?;
    let mut instance_needs: Vec<(MemberAlias, Vec<NeedsSecretItem>)> = Vec::new();
    for blocker in &planned.plan.blockers {
        let (Some(alias), BlockerKind::InstanceBlocked { reason, .. }) =
            (&blocker.service, &blocker.kind)
        else {
            continue;
        };
        if reason != "needs_secret" {
            continue;
        }
        let Some(id) = planned
            .plan
            .items
            .iter()
            .find(|item| &item.alias == alias)
            .and_then(|item| item.instance_id)
        else {
            continue;
        };
        if let Some(InstanceBlocker::NeedsSecret { references, .. }) = supervisor.status(id).blocker
        {
            instance_needs.push((alias.clone(), references));
        }
    }
    let missing: Vec<SecretRef> = planned
        .plan
        .items
        .iter()
        .filter(|item| item.action == PlanAction::Create)
        .flat_map(|item| item.secrets.iter())
        .filter(|secret| secret.status.needs_secret())
        .filter_map(|secret| secret.reference.clone())
        .collect();
    let create_needs = if missing.is_empty() {
        Vec::new()
    } else {
        let registry = super::registry_of(core)?;
        tokio::task::spawn_blocking(move || {
            missing
                .into_iter()
                .map(|reference| {
                    let allowed = generate_allowed(&registry, &reference);
                    NeedsSecretItem::new(reference, NeedsSecretReason::Missing, allowed)
                })
                .collect()
        })
        .await
        .map_err(|_| super::unexpected("a secret read task of an Up failed"))?
    };
    Ok(PlannedUp {
        validation: planned.validation,
        location: planned.location,
        plan: planned.plan,
        blocked,
        instance_needs,
        create_needs,
    })
}

/// Whether a generated value could work for `reference` (AC-2): always
/// for a reference no instance uses yet; otherwise not once the data of an
/// instance that uses it is initialized, because such a field may not
/// change any more. The secret service keeps its exact field rules
/// private, so this reads the uses from the registry and errs towards
/// false.
fn generate_allowed(registry: &dyn Registry, reference: &SecretRef) -> bool {
    let Ok(view) = registry.secret_reference(reference) else {
        return false;
    };
    let Some(view) = view.value else {
        return true;
    };
    view.used_by.iter().all(|used| match used.user {
        SecretUser::Instance(id) => registry
            .instance(id)
            .ok()
            .and_then(|row| row.value)
            .is_none_or(|instance| instance.data_initialized_at().is_none()),
        _ => true,
    })
}
