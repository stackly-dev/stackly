//! Login session rotation (spec 0031, *Rotasi sesi*): what the daemon
//! writes when it starts inside a login session, so a new session turns the
//! startup policy of every instance into desired state and a restart in the
//! same session changes nothing.
//!
//! [`plan_session_rotation`] is the pure rule; [`rotate_session`] runs it as
//! a guarded commit on the registry's thread. Rotation only writes desired
//! state, leases, and session rows. It never starts or stops a process.

use std::sync::{Arc, Mutex, PoisonError};

use jiff::Timestamp;
use stackly_domain::{
    BatchError, DaemonSession, DesiredState, GuardDecision, GuardView, Guarded, Instance,
    InstanceFilter, InstanceId, InstanceLifecycle, Lease, LeaseFilter, LeaseHolder, LeaseId,
    LeaseState, Registry, RegistryError, SESSIONS_KEPT, SessionEndReason, SessionId, StartupPolicy,
    WriteOp,
};
use stackly_platform::LoginSession;

/// How this daemon run relates to the session rows (spec 0031, AC-2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RotationKind {
    /// The registry had no session row: the first run of a daemon that
    /// knows sessions. Nothing but the row is written (AC-3).
    First,
    /// The newest row is another session: a new login.
    New,
    /// The newest row is this session: the daemon came back.
    Continued,
}

impl RotationKind {
    /// Returns the stable snake_case code: `first`, `new`, or `continued`.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::First => "first",
            Self::New => "new",
            Self::Continued => "continued",
        }
    }
}

/// What [`plan_session_rotation`] decided.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct SessionRotation {
    /// How the run relates to the session rows.
    pub kind: RotationKind,
    /// The writes, in order, for one transaction.
    pub ops: Vec<WriteOp>,
    /// The instances that got a startup lease and desired `running`, by
    /// name order of the input.
    pub started: Vec<InstanceId>,
}

/// Whether an instance can get a startup lease: not retained, not unknown,
/// and its data initialized (spec 0031, *Rotasi sesi*).
fn can_run(instance: &Instance) -> bool {
    !matches!(
        instance.lifecycle(),
        InstanceLifecycle::Retained | InstanceLifecycle::Unknown
    ) && instance.data_initialized_at().is_some()
}

/// Decides the writes of a daemon start inside `current`. `latest` is the
/// newest session row, `instances` the instances that are not retained, and
/// `leases` their leases that are not released. `new_lease` gives the ID of
/// each lease it creates.
///
/// - No row: `first`, one `InsertSession`.
/// - Another session: `new`. The open row ends `unknown`, a row is
///   inserted, the manual and startup leases of other sessions are
///   released, each `at_login` instance that can run gets a startup lease
///   and desired `running`, each `manual` instance without an active
///   environment lease gets desired `stopped`, and the table is pruned.
/// - The same session: `continued`; a closed row opens again.
///
/// Every instance change is one `UpdateInstance` whose `expected` is the
/// revision the op before it wrote.
#[must_use]
pub fn plan_session_rotation(
    current: &LoginSession,
    latest: Option<&DaemonSession>,
    instances: &[Instance],
    leases: &[Lease],
    now: Timestamp,
    new_lease: &mut dyn FnMut() -> LeaseId,
) -> SessionRotation {
    let row = || {
        DaemonSession::new(
            current.id.clone(),
            current.boot_id,
            current.audit_session,
            now,
        )
        .expect("a login session has an audit session of 1 or more")
    };
    let Some(latest) = latest else {
        return SessionRotation {
            kind: RotationKind::First,
            ops: vec![WriteOp::InsertSession(row())],
            started: Vec::new(),
        };
    };
    if latest.id() == &current.id {
        let ops = if latest.is_open() {
            Vec::new()
        } else {
            vec![WriteOp::ReopenSession {
                id: latest.id().clone(),
            }]
        };
        return SessionRotation {
            kind: RotationKind::Continued,
            ops,
            started: Vec::new(),
        };
    }

    let mut ops = Vec::new();
    if latest.is_open() {
        ops.push(WriteOp::EndSession {
            id: latest.id().clone(),
            at: now,
            reason: SessionEndReason::Unknown,
        });
    }
    ops.push(WriteOp::InsertSession(row()));
    for lease in leases {
        let ours = lease.session() == Some(&current.id);
        let released = matches!(lease.state(), LeaseState::Released { .. });
        if matches!(lease.holder(), LeaseHolder::Manual | LeaseHolder::Startup)
            && !ours
            && !released
        {
            let mut lease = lease.clone();
            if lease.release(now).is_ok() {
                ops.push(WriteOp::UpdateLease(lease));
            }
        }
    }
    let mut started = Vec::new();
    for instance in instances {
        match instance.startup() {
            StartupPolicy::AtLogin if can_run(instance) => {
                let lease = Lease::new(
                    new_lease(),
                    instance.id(),
                    LeaseHolder::Startup,
                    Some(current.id.clone()),
                    now,
                )
                .expect("a startup lease with a session is valid");
                ops.push(WriteOp::InsertLease(lease));
                set_desired(&mut ops, instance, DesiredState::Running);
                started.push(instance.id());
            }
            StartupPolicy::Manual if !holds_environment_lease(instance.id(), leases) => {
                set_desired(&mut ops, instance, DesiredState::Stopped);
            }
            _ => {}
        }
    }
    ops.push(WriteOp::PruneSessions {
        keep: SESSIONS_KEPT,
    });
    SessionRotation {
        kind: RotationKind::New,
        ops,
        started,
    }
}

fn holds_environment_lease(instance: InstanceId, leases: &[Lease]) -> bool {
    leases.iter().any(|lease| {
        lease.instance_id() == instance
            && lease.is_active()
            && matches!(lease.holder(), LeaseHolder::Environment(_))
    })
}

/// Writes `desired` on `instance` when it differs, as one `UpdateInstance`.
fn set_desired(ops: &mut Vec<WriteOp>, instance: &Instance, desired: DesiredState) {
    if instance.desired() == desired {
        return;
    }
    let expected = instance.revision();
    let mut changed = instance.clone();
    changed.set_desired(desired);
    ops.push(WriteOp::UpdateInstance {
        instance: changed,
        expected,
    });
}

/// What a rotation left in the registry, for `daemon.get_info`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct RotationOutcome {
    /// How the run relates to the session rows.
    pub kind: RotationKind,
    /// The session of this run.
    pub session: SessionId,
    /// When the session row began.
    pub started_at: Timestamp,
    /// How the session before this one ended, if there was one.
    pub previous_end: Option<SessionEndReason>,
    /// The instances that got a startup lease.
    pub started: Vec<InstanceId>,
}

/// What the guard decided, handed out of the registry thread.
type Planned = (RotationKind, Vec<InstanceId>);

/// Runs the rotation for `current` in one guarded commit, then reads the
/// session rows back for the outcome. A blocking call: the registry thread
/// runs the guard.
///
/// # Errors
///
/// Returns a [`BatchError`] when the guard or the write fails, and a
/// [`RegistryError`] inside it when the rows cannot be read.
pub fn rotate_session(
    registry: &dyn Registry,
    current: &LoginSession,
    now: Timestamp,
) -> Result<RotationOutcome, BatchError> {
    let planned: Arc<Mutex<Option<Planned>>> = Arc::new(Mutex::new(None));
    let record = Arc::clone(&planned);
    let session = current.clone();
    let guard: stackly_domain::CommitGuard = Box::new(move |view: &dyn GuardView| {
        let latest = view.latest_session()?;
        let instances = view.instances(InstanceFilter {
            include_retained: false,
        })?;
        let mut leases = Vec::new();
        for instance in &instances {
            leases.extend(view.leases(instance.id(), LeaseFilter::Unreleased)?);
        }
        let rotation = plan_session_rotation(
            &session,
            latest.as_ref(),
            &instances,
            &leases,
            now,
            &mut LeaseId::generate,
        );
        *record.lock().unwrap_or_else(PoisonError::into_inner) =
            Some((rotation.kind, rotation.started));
        if rotation.ops.is_empty() {
            Ok(GuardDecision::Refuse(Box::new(())))
        } else {
            Ok(GuardDecision::Write(rotation.ops))
        }
    });
    match registry.commit_guarded(guard)? {
        Guarded::Committed(_) | Guarded::Refused(_) => {}
    }
    let (kind, started) = planned
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take()
        .ok_or_else(|| {
            BatchError::batch(RegistryError::Io {
                detail: "the session rotation guard did not run".to_owned(),
            })
        })?;
    let rows = registry
        .recent_sessions(2)
        .map_err(BatchError::batch)?
        .value;
    let this = rows
        .iter()
        .find(|row| row.id() == &current.id)
        .ok_or_else(|| {
            BatchError::batch(RegistryError::Io {
                detail: "the session row is missing after the rotation".to_owned(),
            })
        })?;
    let previous = rows.iter().find(|row| row.id() != &current.id);
    Ok(RotationOutcome {
        kind,
        session: current.id.clone(),
        started_at: this.started_at(),
        previous_end: previous.and_then(DaemonSession::end_reason),
        started,
    })
}
