//! The environment membership of every instance, in memory (spec 0044
//! AC-15): `InstanceSummary.ownership` and `InstanceSummary.environments`.
//!
//! Like the autostart book, it is loaded once when the daemon starts, before
//! reconciliation publishes the first summary, and again after every commit
//! that changes `environments` or `environment_instances`, so
//! `instance.list`, `instance.get`, the `instance.*` events, and doctor read
//! the same value without the registry. Each entry also says whether the
//! environment's lease on the instance is active (spec 0047 AC-12), read
//! with the environments, so a commit that writes a lease of an environment
//! reloads the book too.

use std::collections::{HashMap, HashSet};
use std::sync::{PoisonError, RwLock};

use stackly_domain::{
    Environment, EnvironmentId, InstanceId, LeaseFilter, Ownership, Registry, RegistryError,
};
use stackly_protocol::env::{EnvOwnership, InstanceEnvironment};
use stackly_protocol::instance::InstanceOwnership;

use crate::state::DaemonCore;

/// What an instance's summary shows of its environments.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Membership {
    /// `dedicated` for a dedicated member, `shared` for an instance that is
    /// only borrowed, `manual` otherwise.
    pub ownership: InstanceOwnership,
    /// Its environments, the dedicated one first, then by name.
    pub environments: Vec<InstanceEnvironment>,
}

/// The wire value of a member's ownership.
#[must_use]
pub const fn env_ownership(ownership: Ownership) -> EnvOwnership {
    match ownership {
        Ownership::Dedicated => EnvOwnership::Dedicated,
        Ownership::Shared => EnvOwnership::Shared,
    }
}

/// The membership of every instance that belongs to an environment.
#[derive(Debug, Default)]
pub struct MembershipBook {
    members: RwLock<HashMap<InstanceId, Membership>>,
}

impl MembershipBook {
    /// The membership of `id`: `manual` with no environments when it
    /// belongs to none.
    #[must_use]
    pub fn get(&self, id: InstanceId) -> Membership {
        self.members
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&id)
            .cloned()
            .unwrap_or_default()
    }

    /// Replaces the book with the membership of `environments`, with the
    /// `(environment, instance)` pairs whose environment lease is active.
    pub fn load(
        &self,
        environments: &[Environment],
        leased: &HashSet<(EnvironmentId, InstanceId)>,
    ) {
        *self.members.write().unwrap_or_else(PoisonError::into_inner) =
            book_of(environments, leased);
    }
}

/// The membership of every instance in `environments`; `leased` holds the
/// pairs whose environment lease is active.
#[must_use]
pub fn book_of(
    environments: &[Environment],
    leased: &HashSet<(EnvironmentId, InstanceId)>,
) -> HashMap<InstanceId, Membership> {
    let mut book: HashMap<InstanceId, Membership> = HashMap::new();
    for environment in environments {
        for member in environment.members() {
            book.entry(member.instance_id())
                .or_default()
                .environments
                .push(InstanceEnvironment {
                    environment_id: environment.id().as_uuid(),
                    name: environment.name().to_string(),
                    alias: member.alias().to_string(),
                    ownership: env_ownership(member.ownership()),
                    lease_active: leased.contains(&(environment.id(), member.instance_id())),
                });
        }
    }
    for membership in book.values_mut() {
        membership.environments.sort_by(|a, b| {
            let dedicated =
                |entry: &InstanceEnvironment| entry.ownership != EnvOwnership::Dedicated;
            dedicated(a)
                .cmp(&dedicated(b))
                .then_with(|| a.name.cmp(&b.name))
                .then_with(|| a.alias.cmp(&b.alias))
        });
        membership.ownership = if membership
            .environments
            .iter()
            .any(|entry| entry.ownership == EnvOwnership::Dedicated)
        {
            InstanceOwnership::Dedicated
        } else {
            InstanceOwnership::Shared
        };
    }
    book
}

/// Reads every environment and replaces the book. A failed read keeps the
/// book as it was and is logged; the next commit loads it again.
pub async fn reload(core: &DaemonCore) {
    let Some(registry) = core.registry() else {
        return;
    };
    let read = tokio::task::spawn_blocking(move || read_book(&registry)).await;
    match read {
        Ok(Ok((environments, leased))) => {
            core.supervision().membership().load(&environments, &leased)
        }
        Ok(Err(error)) => {
            tracing::warn!(%error, "the environment membership can't be read; it stays as it was");
        }
        Err(error) => tracing::warn!(%error, "the environment membership read did not run"),
    }
}

/// The `(environment, instance)` pairs whose environment lease is active.
pub type Leased = HashSet<(EnvironmentId, InstanceId)>;

/// Every environment, and the pairs whose environment lease is active.
fn read_book(registry: &dyn Registry) -> Result<(Vec<Environment>, Leased), RegistryError> {
    let environments = registry.environments()?.value;
    let mut leased = HashSet::new();
    for environment in &environments {
        for lease in registry
            .leases_of_environment(environment.id(), LeaseFilter::Unreleased)?
            .value
        {
            if lease.is_active() {
                leased.insert((environment.id(), lease.instance_id()));
            }
        }
    }
    Ok((environments, leased))
}

#[cfg(test)]
mod tests {
    use stackly_domain::{EnvironmentMember, ManifestLocation, StoredEnvironment};

    use super::*;

    fn environment(name: &str, members: &[(&str, InstanceId, Ownership)]) -> Environment {
        Environment::restore(StoredEnvironment {
            id: stackly_domain::EnvironmentId::generate(),
            name: name.parse().unwrap(),
            manifest_location: Some(ManifestLocation::new(format!("/p/{name}.yaml")).unwrap()),
            manifest_sha256: None,
            lock_sha256: None,
            declared: Vec::new(),
            members: members
                .iter()
                .map(|(alias, id, ownership)| {
                    EnvironmentMember::new(alias.parse().unwrap(), *id, *ownership)
                })
                .collect(),
            last_operation: None,
            last_up: None,
            updated_at: None,
            revision: stackly_domain::ResourceRevision::INITIAL,
        })
        .unwrap()
    }

    #[test]
    fn a_dedicated_member_comes_first_and_a_borrowed_only_instance_is_shared() {
        let owned = InstanceId::generate();
        let borrowed = InstanceId::generate();
        let omega = environment("omega", &[("db", owned, Ownership::Dedicated)]);
        let leased = HashSet::from([(omega.id(), owned)]);
        let book = book_of(
            &[
                environment("zeta", &[("db", owned, Ownership::Shared)]),
                environment("beta", &[("cache", borrowed, Ownership::Shared)]),
                omega,
                environment("alpha", &[("cache", borrowed, Ownership::Shared)]),
            ],
            &leased,
        );

        let owned = &book[&owned];
        assert_eq!(owned.ownership, InstanceOwnership::Dedicated);
        let names: Vec<_> = owned.environments.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["omega", "zeta"]);
        let active: Vec<_> = owned.environments.iter().map(|e| e.lease_active).collect();
        assert_eq!(active, [true, false], "only omega's lease is active");
        let borrowed = &book[&borrowed];
        assert_eq!(borrowed.ownership, InstanceOwnership::Shared);
        let names: Vec<_> = borrowed
            .environments
            .iter()
            .map(|e| e.name.as_str())
            .collect();
        assert_eq!(names, ["alpha", "beta"]);
    }

    #[test]
    fn an_instance_of_no_environment_is_manual() {
        let book = MembershipBook::default();
        book.load(&[environment("solo", &[])], &HashSet::new());
        let membership = book.get(InstanceId::generate());
        assert_eq!(membership.ownership, InstanceOwnership::Manual);
        assert!(membership.environments.is_empty());
    }
}
