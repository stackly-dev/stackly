//! Down (spec 0047): the `environment_down` coordinator. Its `release`
//! step releases every active lease of the environment and clears
//! `last_up` in one guarded commit; its `stop` step stops, in reverse
//! dependency order, only the confirmed instances no other lease holds,
//! through child `stop_instance` operations ([`DownChildren`]), and ends
//! with one final sweep. Down never deletes data, a runtime, an instance,
//! or a membership, and it records nothing created and never compensates.

pub mod children;
pub mod driver;
pub mod limits;
pub mod order;
pub mod select;

use jiff::Timestamp;
use stackly_domain::{
    DetailKey, EnvironmentId, EnvironmentName, ErrorCodeName, FailureItem, GuardDecision,
    GuardView, Guarded, InstanceId, LeaseFilter, OperationId, ReasonCode, RegistryError,
    ResourceRef, StageName, WriteOp,
};

pub use children::{DownChild, DownChildren};
pub use driver::{DOWN_CHANGED, DownDeps, DownOutcome, DownResult, Driver, DriverEnd};
pub use limits::{DOWN_EXPECTED_IDS_MAX, DOWN_ITEMS_MAX, DOWN_STOP_CONCURRENCY};
pub use order::order;
pub use select::{
    DownBlocked, DownEntry, DownInstance, DownKept, DownMember, DownSelection, DownView, read_view,
    select,
};

use super::children::ChildOrigin;
use super::register::EnvOpError;
use crate::operation::{
    AuditNote, BoxFuture, OperationContext, OperationError, OperationOutcome, OperationWork,
    PartialItem, PartialOutcome, StepSpec, WorkError, WorkOutcome, WorkResult,
};
use crate::secret::SecretCaller;
use crate::supervisor::INSTANCE_IN_USE;

const RELEASE: StageName = StageName::from_static("release");
const STOP: StageName = StageName::from_static("stop");

const DOWN_INCOMPLETE: ErrorCodeName = ErrorCodeName::from_static("env.down_incomplete");

/// Why a Down refused or failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum DownError {
    /// A registry or lookup error shared with import (spec 0044), such as
    /// `env.not_found`.
    #[error(transparent)]
    Environment(EnvOpError),
    /// `env.down_changed`: the stop set differs from the one the client
    /// confirmed (AC-2).
    #[error("what down will stop changed since the preview")]
    Changed,
    /// `env.down_incomplete`: the Down ended with instances that did not
    /// stop (AC-8).
    #[error("some instances of the environment did not stop")]
    Incomplete {
        /// How many instances stopped.
        stopped: usize,
        /// How many instances the Down worked on.
        total: usize,
        /// Every instance that did not stop, by name.
        instances: Vec<DownResult>,
    },
}

fn key(text: &'static str) -> DetailKey {
    text.parse()
        .expect("the detail keys of spec 0047 are camelCase constants")
}

impl OperationError for DownError {
    fn code(&self) -> ErrorCodeName {
        match self {
            Self::Environment(error) => error.code(),
            Self::Changed => DOWN_CHANGED,
            Self::Incomplete { .. } => DOWN_INCOMPLETE,
        }
    }

    fn retryable(&self) -> bool {
        false
    }

    fn details(&self) -> Vec<(DetailKey, String)> {
        match self {
            Self::Environment(error) => error.details(),
            Self::Changed => Vec::new(),
            Self::Incomplete {
                stopped,
                total,
                instances,
            } => vec![
                (key("stopped"), stopped.to_string()),
                (key("total"), total.to_string()),
                (key("count"), instances.len().to_string()),
                (
                    key("omitted"),
                    instances.len().saturating_sub(DOWN_ITEMS_MAX).to_string(),
                ),
            ],
        }
    }

    fn items(&self) -> Vec<FailureItem> {
        match self {
            Self::Environment(error) => error.items(),
            Self::Changed => Vec::new(),
            Self::Incomplete { instances, .. } => instances
                .iter()
                .take(DOWN_ITEMS_MAX)
                .filter_map(instance_item)
                .collect(),
        }
    }
}

fn instance_item(result: &DownResult) -> Option<FailureItem> {
    let mut fields = vec![
        (key("instanceId"), result.instance.to_string()),
        (key("name"), result.name.to_string()),
        (key("outcome"), result.outcome.as_code().to_owned()),
    ];
    if let Some(code) = &result.code {
        fields.push((key("code"), code.to_string()));
    }
    if let Some(operation) = result.operation {
        fields.push((key("operationId"), operation.to_string()));
    }
    FailureItem::new(ReasonCode::from_static("instance"), fields).ok()
}

/// What a Down ends with for `results`, the outcome of every instance: the
/// stopped ones as resources and one skipped item per kept one, or
/// `env.down_incomplete` when one failed, was cancelled, or was left
/// running.
///
/// # Errors
///
/// Returns [`DownError::Incomplete`] when not every instance settled.
pub fn outcome_of(results: &[DownResult]) -> Result<OperationOutcome, DownError> {
    let stopped = results
        .iter()
        .filter(|result| result.outcome == DownOutcome::Stopped)
        .count();
    let open: Vec<DownResult> = results
        .iter()
        .filter(|result| result.outcome.is_incomplete())
        .cloned()
        .collect();
    if !open.is_empty() {
        return Err(DownError::Incomplete {
            stopped,
            total: results.len(),
            instances: open,
        });
    }
    Ok(OperationOutcome {
        resources: results
            .iter()
            .filter(|result| result.outcome == DownOutcome::Stopped)
            .map(|result| ResourceRef::Instance(result.instance))
            .collect(),
        partial: results
            .iter()
            .filter(|result| result.outcome == DownOutcome::Kept)
            .map(|result| PartialItem {
                item: result.name.to_string(),
                resource: Some(ResourceRef::Instance(result.instance)),
                outcome: PartialOutcome::Skipped,
                code: Some(INSTANCE_IN_USE.to_string()),
            })
            .collect(),
    })
}

/// What the `release` guard writes, and how many leases it releases.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    /// One `UpdateLease` per active lease, then the environment update.
    pub ops: Vec<WriteOp>,
    /// How many leases it releases.
    pub leases: usize,
}

/// The guard of the `release` step (AC-4): releases every active lease the
/// environment holds, `detached` members' included, clears `last_up`, and
/// records `operation`. `Ok(Err(..))` refuses with `env.not_found` when
/// the environment is gone, writing nothing.
///
/// # Errors
///
/// Returns a [`RegistryError`] when the view can't be read.
pub fn release(
    view: &dyn GuardView,
    environment: EnvironmentId,
    operation: OperationId,
    now: Timestamp,
) -> Result<Result<Release, DownError>, RegistryError> {
    let Some(mut current) = view
        .environments()?
        .into_iter()
        .find(|candidate| candidate.id() == environment)
    else {
        return Ok(Err(DownError::Environment(EnvOpError::NotFound)));
    };
    let mut ops = Vec::new();
    for mut lease in view.leases_of_environment(environment, LeaseFilter::Unreleased)? {
        if !lease.is_active() {
            continue;
        }
        lease.release(now).map_err(|error| RegistryError::Io {
            detail: format!("an environment lease broke an invariant: {error}"),
        })?;
        ops.push(WriteOp::UpdateLease(lease));
    }
    let leases = ops.len();
    let expected = current.revision();
    current.begin_down(operation);
    ops.push(WriteOp::UpdateEnvironment {
        environment: current,
        expected,
    });
    Ok(Ok(Release { ops, leases }))
}

/// What one `environment_down` works on (*Precheck Down* step 5).
#[derive(Debug, Clone)]
pub struct DownRequest {
    /// The environment.
    pub environment: EnvironmentId,
    /// Its name, as the precheck read it.
    pub name: EnvironmentName,
    /// The confirmed stop set, as the journal input records it.
    pub stop: Vec<InstanceId>,
    /// The secret caller of the request, carried to the children's origin.
    pub caller: SecretCaller,
}

/// The `environment_down` work for `request`.
#[must_use]
pub fn down_work(request: DownRequest, deps: DownDeps) -> Box<dyn OperationWork> {
    Box::new(DownWork { request, deps })
}

struct DownWork {
    request: DownRequest,
    deps: DownDeps,
}

impl OperationWork for DownWork {
    fn run(self: Box<Self>, ctx: OperationContext) -> BoxFuture<'static, WorkResult> {
        Box::pin(async move { run(self.request, self.deps, ctx).await })
    }
}

fn failed(error: DownError) -> WorkError {
    WorkError::Failed(Box::new(error))
}

fn names(results: &[DownResult], outcome: impl Fn(DownOutcome) -> bool) -> String {
    results
        .iter()
        .filter(|result| outcome(result.outcome))
        .map(|result| result.name.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

async fn run(request: DownRequest, deps: DownDeps, mut ctx: OperationContext) -> WorkResult {
    ctx.set_audit_note(AuditNote::new(None).with("name", &request.name));
    ctx.check_cancel()?;

    // release: one guarded commit.
    ctx.begin_step(StepSpec::new(RELEASE).irreversible())
        .await?;
    ctx.set_stage(RELEASE).await?;
    let environment = request.environment;
    let operation = ctx.id();
    let now = ctx.platform().clock().now();
    let released = std::sync::Arc::new(std::sync::Mutex::new(0_usize));
    let count = std::sync::Arc::clone(&released);
    let outcome = ctx
        .commit_guarded(Box::new(move |view| {
            Ok(match release(view, environment, operation, now)? {
                Ok(release) => {
                    if let Ok(mut count) = count.lock() {
                        *count = release.leases;
                    }
                    GuardDecision::Write(release.ops)
                }
                Err(error) => GuardDecision::Refuse(Box::new(error)),
            })
        }))
        .await
        .map_err(|error| match error {
            WorkError::Engine(crate::operation::EngineError::Registry(error)) => {
                failed(DownError::Environment(EnvOpError::Registry(error)))
            }
            other => other,
        })?;
    if let Guarded::Refused(reason) = outcome {
        return Err(failed(match reason.downcast::<DownError>() {
            Ok(error) => *error,
            Err(_) => DownError::Environment(EnvOpError::Internal("the release guard refused")),
        }));
    }
    let released = released.lock().map_or(0, |count| *count);
    ctx.finish_step(None).await?;

    // stop: the confirmed set, in reverse dependency order.
    ctx.begin_step(StepSpec::new(STOP).irreversible()).await?;
    ctx.set_stage(STOP).await?;
    let origin = ChildOrigin {
        operation: crate::operation::ChildOrigin {
            parent: operation,
            trigger: ctx.trigger().clone(),
        },
        caller: request.caller.clone(),
    };
    let (ending, results) = {
        let mut driver = Driver::new(&mut ctx, deps, origin, environment, request.stop.clone());
        let ending = driver.run().await?;
        (ending, driver.results())
    };
    if ending == DriverEnd::Abandoned {
        return Err(WorkError::Abandoned);
    }
    ctx.set_audit_note(
        AuditNote::new(None)
            .with("name", &request.name)
            .with("released", released)
            .with("stopped", names(&results, |o| o == DownOutcome::Stopped))
            .with("kept", names(&results, |o| o == DownOutcome::Kept))
            .with("failed", names(&results, DownOutcome::is_incomplete)),
    );
    if ending == DriverEnd::Cancelled {
        return Err(WorkError::Cancelled);
    }
    let result = outcome_of(&results).map_err(failed)?;
    ctx.finish_step(None).await?;
    Ok(WorkOutcome {
        result,
        finalize: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result(name: &str, outcome: DownOutcome) -> DownResult {
        DownResult {
            instance: InstanceId::generate(),
            name: name.parse().unwrap(),
            outcome,
            code: None,
            operation: None,
        }
    }

    #[test]
    fn a_down_whose_instances_all_settled_succeeds_with_stopped_and_kept() {
        let results = [
            result("db", DownOutcome::Stopped),
            result("cache", DownOutcome::Kept),
            result("web", DownOutcome::AlreadyStopped),
        ];

        let outcome = outcome_of(&results).unwrap();

        assert_eq!(
            outcome.resources,
            [ResourceRef::Instance(results[0].instance)]
        );
        assert_eq!(outcome.partial.len(), 1);
        assert_eq!(outcome.partial[0].item, "cache");
        assert_eq!(outcome.partial[0].outcome, PartialOutcome::Skipped);
        assert_eq!(
            outcome.partial[0].code.as_deref(),
            Some("env.instance_in_use")
        );
    }

    #[test]
    fn a_failed_cancelled_or_left_running_instance_makes_it_incomplete() {
        for open in [
            DownOutcome::Failed,
            DownOutcome::Cancelled,
            DownOutcome::LeftRunning,
        ] {
            let results = [
                result("db", DownOutcome::Stopped),
                result("api", open),
                result("cache", DownOutcome::Kept),
            ];

            let error = outcome_of(&results).unwrap_err();

            assert_eq!(error.code().as_str(), "env.down_incomplete");
            let details: Vec<(String, String)> = error
                .details()
                .into_iter()
                .map(|(key, value)| (key.as_str().to_owned(), value))
                .collect();
            assert!(details.contains(&("stopped".to_owned(), "1".to_owned())));
            assert!(details.contains(&("total".to_owned(), "3".to_owned())));
            let items = error.items();
            assert_eq!(items.len(), 1, "{open:?}");
            assert_eq!(items[0].kind().as_str(), "instance");
            assert_eq!(items[0].fields()[1].1, "api");
            assert_eq!(items[0].fields()[2].1, open.as_code());
        }
    }
}
