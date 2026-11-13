//! Up (spec 0046): the `environment_up` coordinator. Its `accept` step
//! reads the manifest again, plans it again, and in one guarded commit
//! registers the environment, writes the snapshot, the members borrowed
//! through `instanceRef`, the dependency conditions, the leases, and a
//! running `last_up`. Its `run` step installs, creates, and starts through
//! child operations ([`UpChildren`]) and waits for them; a closing commit
//! stores the per service report in `last_up` on every path but a daemon
//! shutdown. The coordinator itself records nothing created and never
//! compensates (AC-12).

pub mod accept;
pub mod board;
pub mod children;
pub mod driver;
pub mod graph;
pub mod limits;
pub mod precheck;
pub mod report;

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use stackly_domain::{
    EnvironmentId, EnvironmentName, GuardDecision, Guarded, MemberAlias, MemberDependency,
    Sha256Digest, StageName, UpReport, UpState, WriteOp,
};

pub use accept::{AcceptInput, AcceptItem, accept};
pub use board::{LiveService, LiveUp, UpBoard};
pub use children::{InstanceNow, PlanRequest, PlannedUp, UpChildren, UpReads};
pub use driver::{CreateJob, DriverDeps, Ending, Service};
pub use graph::{Decision, Edge, Graph, NodeState};
pub use limits::{
    CHILD_QUEUE_TIMEOUT_SECONDS, DEFAULT_DEPENDENCY_TIMEOUT_SECONDS,
    SERVICE_SETTLE_TIMEOUT_SECONDS, UP_INSTALL_CONCURRENCY, UP_POLL_INTERVAL, UP_START_CONCURRENCY,
};
pub use precheck::check;
pub use report::{BlockedItem, UP_ITEMS_MAX, UpError, incomplete};

use super::children::ChildOrigin;
use super::plan::{PlanAction, create_draft};
use super::register::EnvOpError;
use super::{EnvironmentManifest, Severity};
use crate::diagnostics::HomeMask;
use crate::instance::create::EnvironmentBinding;
use crate::operation::{
    AuditNote, BoxFuture, OperationContext, OperationOutcome, OperationWork, StepSpec, WorkError,
    WorkOutcome, WorkResult,
};
use crate::secret::SecretCaller;
use stackly_domain::ResourceRef;

const ACCEPT: StageName = StageName::from_static("accept");
const RUN: StageName = StageName::from_static("run");

/// What one `environment_up` works on (spec 0046 *Precheck Up* step 6).
#[derive(Debug, Clone)]
pub struct UpRequest {
    /// The environment: the registered one of the name, or a new ID.
    pub environment: EnvironmentId,
    /// Its name, as the precheck's plan read it.
    pub name: EnvironmentName,
    /// The manifest file and how to plan it.
    pub plan: PlanRequest,
    /// The digest of the bytes the precheck planned.
    pub manifest_sha256: Sha256Digest,
    /// The secret caller of the request: creates bind with it, and starts
    /// use it for their first try.
    pub caller: SecretCaller,
    /// The home folder, written `~` in notes and details.
    pub home: HomeMask,
}

/// The `environment_up` work for `request`.
#[must_use]
pub fn up_work(request: UpRequest, deps: DriverDeps) -> Box<dyn OperationWork> {
    Box::new(UpWork { request, deps })
}

struct UpWork {
    request: UpRequest,
    deps: DriverDeps,
}

impl OperationWork for UpWork {
    fn run(self: Box<Self>, ctx: OperationContext) -> BoxFuture<'static, WorkResult> {
        Box::pin(async move { run(self.request, self.deps, ctx).await })
    }
}

fn failed(error: UpError) -> WorkError {
    WorkError::Failed(Box::new(error))
}

/// The edges of every service of `manifest`, with their effective timeouts.
fn conditions(manifest: &EnvironmentManifest) -> BTreeMap<MemberAlias, Vec<MemberDependency>> {
    manifest
        .services
        .iter()
        .map(|service| {
            let edges = service
                .depends_on
                .iter()
                .map(|edge| {
                    MemberDependency::new(
                        edge.service().clone(),
                        edge.condition(),
                        edge.timeout_seconds()
                            .unwrap_or(DEFAULT_DEPENDENCY_TIMEOUT_SECONDS),
                    )
                })
                .collect();
            (service.alias.clone(), edges)
        })
        .collect()
}

fn graph_of(
    order: Vec<MemberAlias>,
    conditions: &BTreeMap<MemberAlias, Vec<MemberDependency>>,
) -> Graph {
    let edges = conditions
        .iter()
        .map(|(service, list)| {
            let edges = list
                .iter()
                .map(|condition| Edge {
                    dependency: condition.service().clone(),
                    timeout: Duration::from_secs(u64::from(condition.timeout_seconds())),
                })
                .collect();
            (service.clone(), edges)
        })
        .collect();
    Graph::new(order, edges)
}

async fn run(request: UpRequest, deps: DriverDeps, mut ctx: OperationContext) -> WorkResult {
    let masked = request
        .home
        .mask(&request.plan.path.to_string_lossy())
        .into_owned();
    let note = |extra: Option<(usize, usize, String)>| {
        let note = AuditNote::new(None)
            .with("name", &request.name)
            .with("manifestSha256", request.manifest_sha256)
            .with("location", &masked);
        match extra {
            Some((ready, total, not_ready)) => note
                .with("ready", ready)
                .with("total", total)
                .with("notReady", not_ready),
            None => note,
        }
    };
    ctx.set_audit_note(note(None));

    // accept: read and plan again, then one guarded commit.
    ctx.begin_step(StepSpec::new(ACCEPT).irreversible()).await?;
    ctx.set_stage(ACCEPT).await?;
    let planned = deps.reads.plan(&request.plan).await.map_err(failed)?;
    if planned.validation.sha256 != request.manifest_sha256 {
        return Err(failed(UpError::Environment(EnvOpError::Changed)));
    }
    let Some(manifest) = planned.validation.manifest.clone() else {
        let errors = planned.validation.findings.count_of(Severity::Error);
        return Err(failed(UpError::Environment(EnvOpError::Invalid { errors })));
    };
    if !planned.plan.applicable {
        return Err(failed(UpError::PlanBlocked {
            blockers: planned.blocked.clone(),
        }));
    }
    let conditions = conditions(&manifest);
    let operation = ctx.id();
    let now = ctx.platform().clock().now();
    let input = AcceptInput {
        environment: request.environment,
        name: manifest.name.clone(),
        location: planned.location.clone(),
        sha256: planned.validation.sha256,
        declared: planned.validation.services.clone(),
        lock_sha256: planned.plan.lock.sha256,
        items: planned
            .plan
            .items
            .iter()
            .map(|item| AcceptItem {
                alias: item.alias.clone(),
                instance: item.instance_id,
                provider: item.provider.clone(),
                borrowed: manifest
                    .service(&item.alias)
                    .is_some_and(|service| service.instance_ref.is_some()),
            })
            .collect(),
        conditions: conditions.clone(),
        operation,
        now,
    };
    let home = request.home.clone();
    let outcome = ctx
        .commit_guarded(Box::new(move |view| {
            Ok(match accept(view, &input, &home)? {
                Ok(ops) => GuardDecision::Write(ops),
                Err(error) => GuardDecision::Refuse(Box::new(error)),
            })
        }))
        .await
        .map_err(|error| match error {
            WorkError::Engine(crate::operation::EngineError::Registry(error)) => {
                failed(UpError::Environment(EnvOpError::Registry(error)))
            }
            other => other,
        })?;
    if let Guarded::Refused(reason) = outcome {
        return Err(failed(match reason.downcast::<UpError>() {
            Ok(error) => *error,
            Err(_) => UpError::Environment(EnvOpError::Internal("the accept guard refused")),
        }));
    }
    ctx.finish_step(None).await?;

    // run: install, create, start.
    ctx.begin_step(StepSpec::new(RUN).irreversible()).await?;
    let mut services = Vec::with_capacity(planned.plan.items.len());
    for item in &manifest.services {
        let Some(planned_item) = planned
            .plan
            .items
            .iter()
            .find(|candidate| candidate.alias == item.alias)
        else {
            continue;
        };
        let create = (planned_item.action == PlanAction::Create).then(|| {
            let runtime = planned_item.runtime.as_ref();
            let runtime_id = runtime
                .and_then(|runtime| runtime.runtime_id)
                .unwrap_or_else(super::plan::planned_runtime_placeholder_id);
            CreateJob {
                draft: create_draft(item, planned_item, runtime_id),
                binding: EnvironmentBinding {
                    environment: request.environment,
                    alias: item.alias.clone(),
                    conditions: conditions.get(&item.alias).cloned().unwrap_or_default(),
                },
                install: runtime
                    .filter(|runtime| runtime.install)
                    .map(|runtime| (planned_item.provider.clone(), runtime.artifact_sha256)),
            }
        });
        services.push(Service::new(
            item.alias.clone(),
            planned_item.instance_id,
            create,
        ));
    }
    let origin = ChildOrigin {
        operation: crate::operation::ChildOrigin {
            parent: operation,
            trigger: ctx.trigger().clone(),
        },
        caller: request.caller.clone(),
    };
    let graph = graph_of(planned.plan.order.clone(), &conditions);
    let board = Arc::clone(&deps.board);
    let (ending, reports) = {
        let mut driver = driver::Driver::new(
            &mut ctx,
            deps,
            origin,
            request.environment,
            now,
            graph,
            services,
        );
        let ending = driver.run().await?;
        (ending, driver.reports())
    };
    if ending == Ending::Abandoned {
        // The daemon stops: no closing commit, `last_up` stays running.
        return Err(WorkError::Abandoned);
    }

    // The closing commit, on every other path, also after a cancel.
    let state = if ending == Ending::Cancelled {
        UpState::Cancelled
    } else if reports
        .iter()
        .all(|report| report.phase() == stackly_domain::UpPhase::Ready)
    {
        UpState::Succeeded
    } else {
        UpState::Failed
    };
    let finished = ctx.platform().clock().now();
    let report = UpReport::new(operation, state, now, Some(finished), reports.clone());
    let environment = request.environment;
    let closing = ctx
        .commit_guarded(Box::new(move |view| {
            let Some(mut current) = view
                .environments()?
                .into_iter()
                .find(|candidate| candidate.id() == environment)
            else {
                return Ok(GuardDecision::Write(Vec::new()));
            };
            let expected = current.revision();
            current.finish_up(report);
            Ok(GuardDecision::Write(vec![WriteOp::UpdateEnvironment {
                environment: current,
                expected,
            }]))
        }))
        .await;
    board.remove(environment, operation);
    // A shutdown that closed the engine under the closing commit is an
    // abandon, never a failed Up (AC-15).
    let _ = closing.map_err(|error| match error {
        WorkError::Engine(crate::operation::EngineError::ShuttingDown) => WorkError::Abandoned,
        other => other,
    })?;

    let ready = reports
        .iter()
        .filter(|report| report.phase() == stackly_domain::UpPhase::Ready)
        .count();
    let not_ready: Vec<String> = reports
        .iter()
        .filter(|report| report.phase() != stackly_domain::UpPhase::Ready)
        .map(|report| report.alias().to_string())
        .collect();
    ctx.set_audit_note(note(Some((ready, reports.len(), not_ready.join(",")))));
    if ending == Ending::Cancelled {
        return Err(WorkError::Cancelled);
    }
    // The stage of the first service that is not ready, by start order.
    let first = planned.plan.order.iter().find_map(|alias| {
        reports.iter().find(|report| {
            report.alias() == alias && report.phase() != stackly_domain::UpPhase::Ready
        })
    });
    if let Some(stage) = first.and_then(stackly_domain::ServiceReport::stage) {
        ctx.set_stage(StageName::from_static(stage.as_code()))
            .await?;
    }
    if let Some(error) = incomplete(&reports) {
        return Err(failed(error));
    }
    ctx.finish_step(None).await?;
    Ok(WorkOutcome {
        result: OperationOutcome {
            resources: vec![ResourceRef::Environment(environment)],
            partial: Vec::new(),
        },
        finalize: None,
    })
}
