//! Environments: named groups of instances, each a dedicated or a shared
//! member.

use std::collections::BTreeSet;

use crate::code::code_enum;
use crate::error::{InvariantDetail, InvariantError};
use crate::id::{EnvironmentId, InstanceId};
use crate::value::{EnvironmentName, MemberAlias, Sha256Digest};

code_enum! {
    /// Whether an environment owns a member instance's data.
    pub enum Ownership: Ownership {
        /// The environment owns the instance and its data.
        Dedicated => "dedicated",
        /// The environment uses an instance it never owns.
        Shared => "shared",
    }
}

/// One instance in an environment, under an alias.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EnvironmentMember {
    alias: MemberAlias,
    instance_id: InstanceId,
    ownership: Ownership,
}

impl EnvironmentMember {
    /// Creates a member.
    #[must_use]
    pub const fn new(alias: MemberAlias, instance_id: InstanceId, ownership: Ownership) -> Self {
        Self {
            alias,
            instance_id,
            ownership,
        }
    }

    /// Returns the alias, unique in its environment.
    #[must_use]
    pub const fn alias(&self) -> &MemberAlias {
        &self.alias
    }

    /// Returns the instance.
    #[must_use]
    pub const fn instance_id(&self) -> InstanceId {
        self.instance_id
    }

    /// Returns whether the environment owns the instance.
    #[must_use]
    pub const fn ownership(&self) -> Ownership {
        self.ownership
    }
}

/// A named group of instances.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Environment {
    id: EnvironmentId,
    name: EnvironmentName,
    manifest_sha256: Option<Sha256Digest>,
    members: Vec<EnvironmentMember>,
}

impl Environment {
    /// Creates an environment.
    ///
    /// # Errors
    ///
    /// Returns an [`InvariantError`] (not a BR) when two members share an
    /// alias ([`InvariantDetail::DuplicateMemberAlias`]) or an instance
    /// joins more than once ([`InvariantDetail::DuplicateMemberInstance`]).
    pub fn new(
        id: EnvironmentId,
        name: EnvironmentName,
        manifest_sha256: Option<Sha256Digest>,
        members: Vec<EnvironmentMember>,
    ) -> Result<Self, InvariantError> {
        let mut aliases = BTreeSet::new();
        let mut instances = BTreeSet::new();
        for member in &members {
            if !aliases.insert(&member.alias) {
                return Err(InvariantDetail::DuplicateMemberAlias {
                    alias: member.alias.clone(),
                }
                .into());
            }
            if !instances.insert(member.instance_id) {
                return Err(InvariantDetail::DuplicateMemberInstance {
                    instance: member.instance_id,
                }
                .into());
            }
        }
        Ok(Self {
            id,
            name,
            manifest_sha256,
            members,
        })
    }

    /// Returns the ID.
    #[must_use]
    pub const fn id(&self) -> EnvironmentId {
        self.id
    }

    /// Returns the name.
    #[must_use]
    pub const fn name(&self) -> &EnvironmentName {
        &self.name
    }

    /// Returns the digest of the environment manifest it came from, if any.
    #[must_use]
    pub const fn manifest_sha256(&self) -> Option<&Sha256Digest> {
        self.manifest_sha256.as_ref()
    }

    /// Returns the members in their given order.
    #[must_use]
    pub fn members(&self) -> &[EnvironmentMember] {
        &self.members
    }

    /// Returns the instances this environment owns: its `dedicated`
    /// members. A `shared` member is never owned.
    pub fn owned_instances(&self) -> impl Iterator<Item = InstanceId> + '_ {
        self.members
            .iter()
            .filter(|member| member.ownership == Ownership::Dedicated)
            .map(EnvironmentMember::instance_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(alias: &str, instance: InstanceId, ownership: Ownership) -> EnvironmentMember {
        EnvironmentMember::new(alias.parse().unwrap(), instance, ownership)
    }

    fn environment(members: Vec<EnvironmentMember>) -> Result<Environment, InvariantError> {
        Environment::new(
            EnvironmentId::generate(),
            "shop".parse().unwrap(),
            None,
            members,
        )
    }

    #[test]
    fn only_dedicated_members_are_owned() {
        let (db, cache) = (InstanceId::generate(), InstanceId::generate());
        let env = environment(vec![
            member("db", db, Ownership::Dedicated),
            member("cache", cache, Ownership::Shared),
        ])
        .unwrap();

        assert_eq!(env.owned_instances().collect::<Vec<_>>(), [db]);
        assert_eq!(env.members().len(), 2);
    }

    #[test]
    fn aliases_and_instances_are_unique_per_environment() {
        let (a, b) = (InstanceId::generate(), InstanceId::generate());

        let err = environment(vec![
            member("db", a, Ownership::Dedicated),
            member("db", b, Ownership::Shared),
        ])
        .unwrap_err();
        assert!(matches!(
            err.detail(),
            InvariantDetail::DuplicateMemberAlias { .. }
        ));

        let err = environment(vec![
            member("db", a, Ownership::Dedicated),
            member("replica", a, Ownership::Shared),
        ])
        .unwrap_err();
        assert_eq!(
            err.detail(),
            &InvariantDetail::DuplicateMemberInstance { instance: a }
        );
        assert_eq!(err.rule(), None);
    }
}
