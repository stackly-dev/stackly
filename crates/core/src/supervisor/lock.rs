//! The instance lock and the guard that backs it up (spec 0010, *Lock
//! instance*): init, start, and every respawn take `instance.lock` before
//! their first side effect, and refuse with `supervisor.data_in_use` when
//! another process holds it or when the last observed process still runs.

use std::sync::Arc;

use stackly_domain::{InstanceId, ProcessObservation};
use stackly_platform::{InstanceLock, LockOutcome, ProcessError};

use super::SupervisorDeps;
use super::classify::{identity_checks, six_checks_passed};
use super::error::SupervisorError;
use super::persist::blocking;
use super::prepare::Prepared;

/// Takes the exclusive lock on the instance's data without waiting.
///
/// # Errors
///
/// Returns [`SupervisorError::DataInUse`] when another process holds it, and
/// [`SupervisorError::Internal`] when the lock file is invalid or the call
/// fails.
pub(crate) async fn take_lock(
    deps: &SupervisorDeps,
    prepared: &Prepared,
) -> Result<Arc<InstanceLock>, SupervisorError> {
    let platform = deps.platform.clone();
    let path = prepared.paths.lock.clone();
    let outcome = blocking(move || platform.processes().lock_instance(&path)).await?;
    match outcome {
        Ok(LockOutcome::Acquired(lock)) => Ok(Arc::new(lock)),
        Ok(LockOutcome::Held) => Err(SupervisorError::DataInUse {
            lock_path: prepared.paths.lock.display().to_string(),
            pid: None,
        }),
        Err(error) => Err(lock_failure(&error)),
    }
}

fn lock_failure(error: &ProcessError) -> SupervisorError {
    match error {
        ProcessError::NotSupported { .. } => SupervisorError::from_process(error),
        other => SupervisorError::internal(format!("the instance lock failed: {other}")),
    }
}

/// The second guard against a second server on the same data: the last
/// observed process, when it still runs with a matching identity, refuses
/// the start even though the lock could be taken (the user deleted
/// `instance.lock`, for example).
///
/// It uses [`identity_checks`] on the facts of `inspect` and never
/// `probe_lock` or `holds_file`, because the lock is already held. An error
/// while gathering the evidence refuses the start as well.
///
/// # Errors
///
/// Returns [`SupervisorError::DataInUse`] naming the process.
pub(crate) async fn guard_last_observation(
    deps: &SupervisorDeps,
    prepared: &Prepared,
    id: InstanceId,
) -> Result<(), SupervisorError> {
    let registry = deps.registry.clone();
    let recorded = blocking(move || registry.latest_process_observation(id))
        .await?
        .map_err(SupervisorError::Registry)?
        .value;
    let Some(ProcessObservation { identity, .. }) = recorded else {
        return Ok(());
    };
    let in_use = |pid| SupervisorError::DataInUse {
        lock_path: prepared.paths.lock.display().to_string(),
        pid: Some(pid),
    };
    let platform = deps.platform.clone();
    let recorded = identity.clone();
    let answers = blocking(move || {
        let processes = platform.processes();
        let boot = processes.boot_id()?;
        let facts = if boot == recorded.boot {
            processes.inspect(recorded.pid)?
        } else {
            None
        };
        let uid = platform
            .identity()
            .current_user()
            .map_err(|error| ProcessError::Io {
                op: "current_user",
                path: None,
                kind: std::io::ErrorKind::Other,
                message: error.to_string(),
            })?;
        Ok::<_, ProcessError>(identity_checks(&recorded, boot, facts.as_ref(), &uid))
    })
    .await?;
    match answers {
        Ok(evidence) if six_checks_passed(&evidence) => Err(in_use(identity.pid)),
        Ok(_) => Ok(()),
        Err(_) => Err(in_use(identity.pid)),
    }
}
