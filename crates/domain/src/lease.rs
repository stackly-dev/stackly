//! Leases: who needs an instance running, and the stop and autostart rules
//! that follow (BR-014, BR-015, BR-024).

use jiff::Timestamp;

use crate::error::{InvariantDetail, InvariantError};
use crate::id::{EnvironmentId, InstanceId, LeaseId};
use crate::instance::StartupPolicy;
use crate::value::SessionId;

/// Who holds a lease.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LeaseHolder {
    /// An environment that is up.
    Environment(EnvironmentId),
    /// The user started the instance by hand.
    Manual,
    /// The instance started at login (feature 31).
    Startup,
}

/// The state of a lease.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LeaseState {
    /// It keeps the instance wanted.
    Active,
    /// An explicit stop suppressed this startup lease at `at`; autostart
    /// waits for the next session (BR-024).
    Suppressed {
        /// When it was suppressed.
        at: Timestamp,
    },
    /// Released at `at`.
    Released {
        /// When it was released.
        at: Timestamp,
    },
}

impl LeaseState {
    /// Returns the stable snake_case code: `active`, `suppressed`, or
    /// `released`.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Suppressed { .. } => "suppressed",
            Self::Released { .. } => "released",
        }
    }
}

/// A claim that an instance should run.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Lease {
    id: LeaseId,
    instance_id: InstanceId,
    holder: LeaseHolder,
    session: Option<SessionId>,
    state: LeaseState,
    acquired_at: Timestamp,
}

impl Lease {
    /// Creates an `active` lease.
    ///
    /// # Errors
    ///
    /// Returns an [`InvariantError`] (not a BR) for an environment lease
    /// with a session ([`InvariantDetail::EnvironmentLeaseWithSession`]) or
    /// a startup lease without one
    /// ([`InvariantDetail::StartupLeaseWithoutSession`]). A manual lease
    /// may have a session or not.
    pub fn new(
        id: LeaseId,
        instance_id: InstanceId,
        holder: LeaseHolder,
        session: Option<SessionId>,
        acquired_at: Timestamp,
    ) -> Result<Self, InvariantError> {
        match (holder, &session) {
            (LeaseHolder::Environment(_), Some(_)) => {
                Err(InvariantDetail::EnvironmentLeaseWithSession { lease: id }.into())
            }
            (LeaseHolder::Startup, None) => {
                Err(InvariantDetail::StartupLeaseWithoutSession { lease: id }.into())
            }
            _ => Ok(Self {
                id,
                instance_id,
                holder,
                session,
                state: LeaseState::Active,
                acquired_at,
            }),
        }
    }

    /// Returns the ID.
    #[must_use]
    pub const fn id(&self) -> LeaseId {
        self.id
    }

    /// Returns the instance it keeps wanted.
    #[must_use]
    pub const fn instance_id(&self) -> InstanceId {
        self.instance_id
    }

    /// Returns who holds it.
    #[must_use]
    pub const fn holder(&self) -> LeaseHolder {
        self.holder
    }

    /// Returns the login session, if any.
    #[must_use]
    pub const fn session(&self) -> Option<&SessionId> {
        self.session.as_ref()
    }

    /// Returns the state.
    #[must_use]
    pub const fn state(&self) -> LeaseState {
        self.state
    }

    /// Returns whether the lease is `active`.
    #[must_use]
    pub const fn is_active(&self) -> bool {
        matches!(self.state, LeaseState::Active)
    }

    /// Returns when it was acquired.
    #[must_use]
    pub const fn acquired_at(&self) -> Timestamp {
        self.acquired_at
    }

    /// Releases an `active` or `suppressed` lease at `at`.
    ///
    /// # Errors
    ///
    /// Returns an [`InvariantError`] with
    /// [`InvariantDetail::LeaseAlreadyReleased`] for a released lease.
    pub fn release(&mut self, at: Timestamp) -> Result<(), InvariantError> {
        match self.state {
            LeaseState::Active | LeaseState::Suppressed { .. } => {
                self.state = LeaseState::Released { at };
                Ok(())
            }
            LeaseState::Released { .. } => {
                Err(InvariantDetail::LeaseAlreadyReleased { lease: self.id }.into())
            }
        }
    }

    /// Suppresses an `active` startup lease at `at`, so autostart waits for
    /// the next session (BR-024).
    ///
    /// # Errors
    ///
    /// Returns an [`InvariantError`] with
    /// [`InvariantDetail::SuppressNeedsActiveStartupLease`] for any other
    /// lease.
    pub fn suppress(&mut self, at: Timestamp) -> Result<(), InvariantError> {
        if self.holder == LeaseHolder::Startup && self.is_active() {
            self.state = LeaseState::Suppressed { at };
            Ok(())
        } else {
            Err(InvariantDetail::SuppressNeedsActiveStartupLease { lease: self.id }.into())
        }
    }
}

/// A request to stop an instance.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum StopRequest {
    /// An environment goes down.
    EnvironmentDown(EnvironmentId),
    /// The user stops the instance by hand.
    Manual {
        /// The user's login session, if known.
        session: Option<SessionId>,
        /// Whether the user chose to stop it although environments hold it.
        override_leases: bool,
    },
}

/// What a stop request does to an instance and its leases.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum StopDecision {
    /// Stop the instance.
    Stop {
        /// Leases to release.
        release: Vec<LeaseId>,
        /// Startup leases to suppress.
        suppress: Vec<LeaseId>,
        /// Environment leases the user overrode. They stay `active`; the
        /// desired state `stopped` keeps the reconciler from starting the
        /// instance again.
        overridden: Vec<LeaseId>,
    },
    /// Keep the instance running because other leases still hold it
    /// (BR-014).
    KeepRunning {
        /// Leases to release.
        release: Vec<LeaseId>,
        /// Active leases that keep it running.
        remaining: Vec<LeaseId>,
    },
    /// A manual stop needs the user to override environment leases first
    /// (BR-015).
    RequiresOverride {
        /// The environment leases that hold the instance.
        blocking: Vec<LeaseId>,
    },
}

/// Decides what a stop request does to `instance`, given its leases.
/// Leases of other instances in `leases` are ignored.
///
/// - `EnvironmentDown(env)` releases the active leases of `env`. When any
///   other active lease remains, of any holder, the instance keeps running
///   (BR-014); otherwise it stops.
/// - `Manual` needs `override_leases` while an environment holds an active
///   lease (BR-015). Otherwise the instance stops: every active manual lease
///   is released, whatever its session; the active startup leases of the
///   request's session are suppressed; and active environment leases are
///   listed as overridden.
#[must_use]
pub fn evaluate_stop(
    instance: InstanceId,
    leases: &[Lease],
    request: &StopRequest,
) -> StopDecision {
    let active: Vec<&Lease> = leases
        .iter()
        .filter(|lease| lease.instance_id == instance && lease.is_active())
        .collect();
    let ids = |keep: &dyn Fn(&Lease) -> bool| -> Vec<LeaseId> {
        active
            .iter()
            .filter(|lease| keep(lease))
            .map(|lease| lease.id)
            .collect()
    };
    match request {
        StopRequest::EnvironmentDown(env) => {
            let of_env = |lease: &Lease| lease.holder == LeaseHolder::Environment(*env);
            let release = ids(&of_env);
            let remaining = ids(&|lease: &Lease| !of_env(lease));
            if remaining.is_empty() {
                StopDecision::Stop {
                    release,
                    suppress: Vec::new(),
                    overridden: Vec::new(),
                }
            } else {
                StopDecision::KeepRunning { release, remaining }
            }
        }
        StopRequest::Manual {
            session,
            override_leases,
        } => {
            let environments =
                ids(&|lease: &Lease| matches!(lease.holder, LeaseHolder::Environment(_)));
            if !environments.is_empty() && !override_leases {
                return StopDecision::RequiresOverride {
                    blocking: environments,
                };
            }
            StopDecision::Stop {
                release: ids(&|lease: &Lease| lease.holder == LeaseHolder::Manual),
                suppress: ids(&|lease: &Lease| {
                    lease.holder == LeaseHolder::Startup
                        && session.is_some()
                        && lease.session.as_ref() == session.as_ref()
                }),
                overridden: environments,
            }
        }
    }
}

/// Returns whether an instance starts on its own in `session`: only with
/// policy `at_login` and an `active` startup lease of that session. A
/// suppressed lease keeps it false until the next session creates a new
/// startup lease (BR-024, feature 31). `leases` are the instance's leases.
#[must_use]
pub fn should_autostart(policy: StartupPolicy, leases: &[Lease], session: &SessionId) -> bool {
    policy == StartupPolicy::AtLogin
        && leases.iter().any(|lease| {
            lease.holder == LeaseHolder::Startup
                && lease.is_active()
                && lease.session.as_ref() == Some(session)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str) -> Timestamp {
        text.parse().unwrap()
    }

    fn lease(instance: InstanceId, holder: LeaseHolder, session: Option<&str>) -> Lease {
        Lease::new(
            LeaseId::generate(),
            instance,
            holder,
            session.map(|s| s.parse().unwrap()),
            at("2026-01-01T00:00:00Z"),
        )
        .unwrap()
    }

    #[test]
    fn holders_and_sessions_must_match() {
        let instance = InstanceId::generate();
        let id = LeaseId::generate();
        let now = at("2026-01-01T00:00:00Z");
        let session: SessionId = "s1".parse().unwrap();

        let env = Lease::new(
            id,
            instance,
            LeaseHolder::Environment(EnvironmentId::generate()),
            Some(session.clone()),
            now,
        );
        assert_eq!(
            env.unwrap_err().detail(),
            &InvariantDetail::EnvironmentLeaseWithSession { lease: id }
        );
        let startup = Lease::new(id, instance, LeaseHolder::Startup, None, now);
        assert_eq!(
            startup.unwrap_err().detail(),
            &InvariantDetail::StartupLeaseWithoutSession { lease: id }
        );
        assert!(Lease::new(id, instance, LeaseHolder::Manual, None, now).is_ok());
        assert!(Lease::new(id, instance, LeaseHolder::Manual, Some(session), now).is_ok());
    }

    #[test]
    fn release_ends_active_and_suppressed_leases_once() {
        let instance = InstanceId::generate();
        let mut manual = lease(instance, LeaseHolder::Manual, None);
        let mut startup = lease(instance, LeaseHolder::Startup, Some("s1"));

        startup.suppress(at("2026-01-01T00:00:01Z")).unwrap();
        startup.release(at("2026-01-01T00:00:02Z")).unwrap();
        manual.release(at("2026-01-01T00:00:03Z")).unwrap();

        assert_eq!(startup.state().as_code(), "released");
        assert!(manual.release(at("2026-01-01T00:00:04Z")).is_err());
        assert!(manual.suppress(at("2026-01-01T00:00:04Z")).is_err());
    }

    #[test]
    fn only_active_startup_leases_can_be_suppressed() {
        let instance = InstanceId::generate();
        let mut manual = lease(instance, LeaseHolder::Manual, Some("s1"));
        let mut startup = lease(instance, LeaseHolder::Startup, Some("s1"));

        assert!(manual.suppress(at("2026-01-01T00:00:01Z")).is_err());
        startup.suppress(at("2026-01-01T00:00:01Z")).unwrap();
        assert!(startup.suppress(at("2026-01-01T00:00:02Z")).is_err());
        assert_eq!(
            startup.state(),
            LeaseState::Suppressed {
                at: at("2026-01-01T00:00:01Z")
            }
        );
    }

    #[test]
    fn the_last_environment_going_down_stops_the_instance() {
        let instance = InstanceId::generate();
        let env = EnvironmentId::generate();
        let held = lease(instance, LeaseHolder::Environment(env), None);
        let other_instance = lease(InstanceId::generate(), LeaseHolder::Manual, None);

        let decision = evaluate_stop(
            instance,
            &[held.clone(), other_instance],
            &StopRequest::EnvironmentDown(env),
        );

        assert_eq!(
            decision,
            StopDecision::Stop {
                release: vec![held.id()],
                suppress: Vec::new(),
                overridden: Vec::new(),
            }
        );
    }

    #[test]
    fn a_manual_stop_releases_every_manual_lease_and_lists_overridden_environments() {
        let instance = InstanceId::generate();
        let env = lease(
            instance,
            LeaseHolder::Environment(EnvironmentId::generate()),
            None,
        );
        let mine = lease(instance, LeaseHolder::Manual, Some("s1"));
        let theirs = lease(instance, LeaseHolder::Manual, Some("s2"));
        let other_startup = lease(instance, LeaseHolder::Startup, Some("s2"));
        let leases = [env.clone(), mine.clone(), theirs.clone(), other_startup];

        let decision = evaluate_stop(
            instance,
            &leases,
            &StopRequest::Manual {
                session: Some("s1".parse().unwrap()),
                override_leases: true,
            },
        );

        assert_eq!(
            decision,
            StopDecision::Stop {
                release: vec![mine.id(), theirs.id()],
                suppress: Vec::new(),
                overridden: vec![env.id()],
            }
        );
    }

    #[test]
    fn manual_policy_never_autostarts() {
        let instance = InstanceId::generate();
        let startup = lease(instance, LeaseHolder::Startup, Some("s1"));
        let session: SessionId = "s1".parse().unwrap();

        assert!(!should_autostart(
            StartupPolicy::Manual,
            std::slice::from_ref(&startup),
            &session
        ));
        assert!(should_autostart(
            StartupPolicy::AtLogin,
            &[startup],
            &session
        ));
        assert!(!should_autostart(StartupPolicy::AtLogin, &[], &session));
    }
}
