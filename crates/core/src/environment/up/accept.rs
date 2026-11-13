//! The guard of the `accept` step (spec 0046 AC-4): in one guarded commit
//! it registers the environment when it is new, writes the snapshot, the
//! lock digest, `shared` members for `instanceRef`, the dependency
//! conditions, one environment lease per member whose alias the manifest
//! names, and a running `last_up`. Every write of the environment is one
//! `UpdateEnvironment` at the revision just written, because the registry
//! takes exactly one revision step per update.

use std::collections::BTreeMap;

use jiff::Timestamp;
use stackly_domain::{
    DeclaredService, Environment, EnvironmentId, EnvironmentMember, EnvironmentName, GuardView,
    InstanceFilter, InstanceId, InstanceLifecycle, Lease, LeaseFilter, LeaseHolder, LeaseId,
    LeaseState, ManifestLocation, MemberAlias, MemberDependency, OperationId, Ownership,
    ProviderName, RegistryError, Sha256Digest, WriteOp,
};

use super::report::UpError;
use crate::diagnostics::HomeMask;
use crate::environment::register::{EnvOpError, RegisterDecision, RegisterInput, register};

/// One service of the plan as `accept` checks it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptItem {
    /// The alias.
    pub alias: MemberAlias,
    /// The instance the plan found for it; `None` for a `create` item.
    pub instance: Option<InstanceId>,
    /// The provider the manifest names.
    pub provider: ProviderName,
    /// Whether the service borrows its instance through `instanceRef`.
    pub borrowed: bool,
}

/// What `accept` writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptInput {
    /// The environment the operation runs on.
    pub environment: EnvironmentId,
    /// `metadata.name`.
    pub name: EnvironmentName,
    /// Where the manifest lives.
    pub location: ManifestLocation,
    /// The digest of the bytes read in this step.
    pub sha256: Sha256Digest,
    /// The snapshot of its services.
    pub declared: Vec<DeclaredService>,
    /// `EnvPlan.lock.sha256` (spec 0045).
    pub lock_sha256: Option<Sha256Digest>,
    /// Every service of the plan.
    pub items: Vec<AcceptItem>,
    /// The edges of each service with their effective timeouts.
    pub conditions: BTreeMap<MemberAlias, Vec<MemberDependency>>,
    /// The `environment_up` operation.
    pub operation: OperationId,
    /// When the step commits.
    pub now: Timestamp,
}

/// Records one change of `environment` as an update at the revision it had
/// before `change`, and keeps it current.
fn update(
    ops: &mut Vec<WriteOp>,
    environment: &mut Environment,
    change: impl FnOnce(&mut Environment) -> Result<(), UpError>,
) -> Result<(), UpError> {
    let expected = environment.revision();
    change(environment)?;
    if environment.revision() != expected {
        ops.push(WriteOp::UpdateEnvironment {
            environment: environment.clone(),
            expected,
        });
    }
    Ok(())
}

fn broken(_: impl std::fmt::Debug) -> UpError {
    UpError::Environment(EnvOpError::Internal(
        "a planned change broke an environment invariant",
    ))
}

/// The guard: `Ok(Err(..))` refuses with the error the operation fails
/// with, writing nothing.
///
/// # Errors
///
/// Returns a [`RegistryError`] when the view can't be read.
pub fn accept(
    view: &dyn GuardView,
    input: &AcceptInput,
    home: &HomeMask,
) -> Result<Result<Vec<WriteOp>, UpError>, RegistryError> {
    let register_input = RegisterInput {
        id: input.environment,
        name: input.name.clone(),
        location: input.location.clone(),
        sha256: input.sha256,
        declared: input.declared.clone(),
        relink: None,
    };
    let mut ops = match register(view, &register_input, home)? {
        RegisterDecision::Write(ops) => ops,
        RegisterDecision::Unchanged => Vec::new(),
        RegisterDecision::Refuse(error) => return Ok(Err(UpError::Environment(error))),
    };
    // The environment as the ops so far leave it.
    let written = ops.iter().rev().find_map(|op| match op {
        WriteOp::InsertEnvironment(environment)
        | WriteOp::UpdateEnvironment { environment, .. }
            if environment.id() == input.environment =>
        {
            Some(environment.clone())
        }
        _ => None,
    });
    let stored = view
        .environments()?
        .into_iter()
        .find(|environment| environment.id() == input.environment);
    let Some(mut environment) = written.or(stored) else {
        return Ok(Err(UpError::Environment(EnvOpError::NotFound)));
    };

    // Every instance a service of the manifest uses is still there, not
    // retained, and runs the provider the manifest names.
    let instances = view.instances(InstanceFilter {
        include_retained: true,
    })?;
    for item in &input.items {
        let Some(id) = item.instance else {
            continue;
        };
        let fits = instances.iter().any(|instance| {
            instance.id() == id
                && instance.lifecycle() != InstanceLifecycle::Retained
                && instance.provider_revision().provider() == &item.provider
        });
        if !fits {
            return Ok(Err(UpError::PlanChanged));
        }
    }

    let result = (|| {
        if environment.lock_sha256() != input.lock_sha256.as_ref() {
            update(&mut ops, &mut environment, |environment| {
                let location = input.location.clone();
                environment
                    .set_manifest(
                        location,
                        input.sha256,
                        input.declared.clone(),
                        input.lock_sha256,
                    )
                    .map_err(broken)
            })?;
        }
        for item in input.items.iter().filter(|item| item.borrowed) {
            let Some(id) = item.instance else {
                continue;
            };
            if environment.member(&item.alias).is_some() {
                continue;
            }
            update(&mut ops, &mut environment, |environment| {
                environment
                    .bind_member(EnvironmentMember::new(
                        item.alias.clone(),
                        id,
                        Ownership::Shared,
                    ))
                    .map_err(|_| UpError::PlanChanged)
            })?;
        }
        for (alias, conditions) in &input.conditions {
            let Some(member) = environment.member(alias) else {
                continue;
            };
            if member.dependency_conditions() == conditions.as_slice() {
                continue;
            }
            update(&mut ops, &mut environment, |environment| {
                environment
                    .set_member_conditions(alias, conditions.clone())
                    .map_err(broken)
            })?;
        }
        update(&mut ops, &mut environment, |environment| {
            environment.begin_up(input.operation, input.now);
            Ok(())
        })?;
        Ok::<_, UpError>(())
    })();
    if let Err(error) = result {
        return Ok(Err(error));
    }

    // One `active` lease per member the manifest names that has none from
    // this environment; a `detached` member gets none.
    let holder = LeaseHolder::Environment(input.environment);
    for member in environment.members() {
        if !input.items.iter().any(|item| &item.alias == member.alias()) {
            continue;
        }
        let leased = view
            .leases(member.instance_id(), LeaseFilter::Unreleased)?
            .iter()
            .any(|lease| lease.holder() == holder && lease.state() == LeaseState::Active);
        if leased {
            continue;
        }
        let lease = Lease::new(
            LeaseId::generate(),
            member.instance_id(),
            holder,
            None,
            input.now,
        )
        .map_err(|error| RegistryError::Io {
            detail: format!("an environment lease broke an invariant: {error}"),
        })?;
        ops.push(WriteOp::InsertLease(lease));
    }
    Ok(Ok(ops))
}
