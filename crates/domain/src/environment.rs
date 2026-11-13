//! Environments: named groups of instances, each a dedicated or a shared
//! member, and the services their manifest declares (spec 0044).

use std::collections::BTreeSet;
use std::fmt;
use std::path::{Component, Path, PathBuf};
use std::str::FromStr;

use crate::code::code_enum;
use crate::error::{InvariantDetail, InvariantError, ValueError, ValueKind};
use crate::id::{EnvironmentId, InstanceId};
use crate::runtime::RuntimeSelector;
use crate::value::{
    EnvironmentName, InstanceName, MemberAlias, ProviderName, ResourceRevision, Sha256Digest,
};

code_enum! {
    /// Whether an environment owns a member instance's data.
    pub enum Ownership: Ownership {
        /// The environment owns the instance and its data.
        Dedicated => "dedicated",
        /// The environment uses an instance it never owns.
        Shared => "shared",
    }
}

code_enum! {
    /// What a service waits for before a service that depends on it starts.
    pub enum DependencyCondition: DependencyCondition {
        /// The dependency is running and ready.
        Ready => "ready",
    }
}

/// Where an environment manifest lives: an absolute UTF-8 path of 2 to
/// 4096 bytes with no `.` or `..` segment, the shape of the
/// `environments.manifest_location` column.
///
/// The daemon builds it from the canonical parent folder plus the file
/// name, so two spellings of one file compare equal (spec 0044 AC-11).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ManifestLocation(PathBuf);

impl ManifestLocation {
    /// Longest location, in bytes.
    pub const MAX: usize = 4096;

    /// Checks `path` and wraps it.
    ///
    /// # Errors
    ///
    /// Returns a [`ValueError`] when the path is not absolute, not UTF-8,
    /// shorter than 2 or longer than [`ManifestLocation::MAX`] bytes, or
    /// holds a `.` or `..` segment.
    pub fn new(path: impl Into<PathBuf>) -> Result<Self, ValueError> {
        let path = path.into();
        let valid = path.is_absolute()
            && path.to_str().is_some_and(|text| {
                (2..=Self::MAX).contains(&text.len())
                    && !text
                        .split('/')
                        .any(|segment| segment == "." || segment == "..")
            })
            && path
                .components()
                .all(|component| matches!(component, Component::RootDir | Component::Normal(_)));
        if valid {
            Ok(Self(path))
        } else {
            Err(ValueError::new(
                ValueKind::ManifestLocation,
                "must be an absolute UTF-8 path of 2 to 4096 bytes without . or ..",
            ))
        }
    }

    /// Returns the path.
    #[must_use]
    pub fn as_path(&self) -> &Path {
        &self.0
    }

    /// Returns the path as text; [`ManifestLocation::new`] checked it is
    /// UTF-8.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.to_str().unwrap_or_default()
    }
}

impl fmt::Display for ManifestLocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ManifestLocation {
    type Err = ValueError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

/// One `dependsOn` entry of a declared service.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DependencyEdge {
    service: MemberAlias,
    condition: DependencyCondition,
    timeout_seconds: Option<u32>,
}

impl DependencyEdge {
    /// Creates an edge to `service`.
    #[must_use]
    pub const fn new(
        service: MemberAlias,
        condition: DependencyCondition,
        timeout_seconds: Option<u32>,
    ) -> Self {
        Self {
            service,
            condition,
            timeout_seconds,
        }
    }

    /// Returns the alias depended on.
    #[must_use]
    pub const fn service(&self) -> &MemberAlias {
        &self.service
    }

    /// Returns what the dependent waits for.
    #[must_use]
    pub const fn condition(&self) -> DependencyCondition {
        self.condition
    }

    /// Returns the wait limit the manifest wrote, if any; none means the
    /// default of Up (spec 0046).
    #[must_use]
    pub const fn timeout_seconds(&self) -> Option<u32> {
        self.timeout_seconds
    }
}

/// One service an environment manifest declares, as the registry keeps it
/// in its snapshot: no settings, endpoints, or secret references.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DeclaredService {
    alias: MemberAlias,
    provider: ProviderName,
    ownership: Ownership,
    selector: Option<RuntimeSelector>,
    instance_ref: Option<InstanceName>,
    instance_name: Option<InstanceName>,
    depends_on: Vec<DependencyEdge>,
}

impl DeclaredService {
    /// Creates a declared service.
    #[must_use]
    pub const fn new(
        alias: MemberAlias,
        provider: ProviderName,
        ownership: Ownership,
        selector: Option<RuntimeSelector>,
        instance_ref: Option<InstanceName>,
        instance_name: Option<InstanceName>,
        depends_on: Vec<DependencyEdge>,
    ) -> Self {
        Self {
            alias,
            provider,
            ownership,
            selector,
            instance_ref,
            instance_name,
            depends_on,
        }
    }

    /// Returns the alias, unique in its environment.
    #[must_use]
    pub const fn alias(&self) -> &MemberAlias {
        &self.alias
    }

    /// Returns the provider.
    #[must_use]
    pub const fn provider(&self) -> &ProviderName {
        &self.provider
    }

    /// Returns whether the environment owns the service's instance.
    #[must_use]
    pub const fn ownership(&self) -> Ownership {
        self.ownership
    }

    /// Returns the runtime selector the manifest wrote, if any.
    #[must_use]
    pub const fn selector(&self) -> Option<&RuntimeSelector> {
        self.selector.as_ref()
    }

    /// Returns the existing instance a shared service borrows, if any.
    #[must_use]
    pub const fn instance_ref(&self) -> Option<&InstanceName> {
        self.instance_ref.as_ref()
    }

    /// Returns the instance name the manifest wrote, if any.
    #[must_use]
    pub const fn instance_name(&self) -> Option<&InstanceName> {
        self.instance_name.as_ref()
    }

    /// Returns the services this one waits for, in manifest order.
    #[must_use]
    pub fn depends_on(&self) -> &[DependencyEdge] {
        &self.depends_on
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

/// Every field of an [`Environment`] as the registry stores it, for
/// [`Environment::restore`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredEnvironment {
    /// The ID.
    pub id: EnvironmentId,
    /// The name.
    pub name: EnvironmentName,
    /// Where its manifest lives, if anywhere.
    pub manifest_location: Option<ManifestLocation>,
    /// The digest of the environment manifest it came from, if any.
    pub manifest_sha256: Option<Sha256Digest>,
    /// The services its manifest declared when it was last accepted.
    pub declared: Vec<DeclaredService>,
    /// The members, in order.
    pub members: Vec<EnvironmentMember>,
    /// The revision.
    pub revision: ResourceRevision,
}

/// A named group of instances.
///
/// Every mutation raises [`Environment::revision`] by exactly 1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Environment {
    id: EnvironmentId,
    name: EnvironmentName,
    manifest_location: Option<ManifestLocation>,
    manifest_sha256: Option<Sha256Digest>,
    declared: Vec<DeclaredService>,
    members: Vec<EnvironmentMember>,
    revision: ResourceRevision,
}

impl Environment {
    /// Creates an environment at revision 1, with no manifest location
    /// and no declared services; [`Environment::set_manifest`] adds them.
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
        Self::restore(StoredEnvironment {
            id,
            name,
            manifest_location: None,
            manifest_sha256,
            declared: Vec::new(),
            members,
            revision: ResourceRevision::INITIAL,
        })
    }

    /// Rebuilds an environment the registry stored, with the checks of
    /// [`Environment::new`]. Only the registry and test fakes call it.
    ///
    /// # Errors
    ///
    /// Returns the [`InvariantError`]s of [`Environment::new`], and those
    /// of [`Environment::set_manifest`] for the declared services.
    pub fn restore(stored: StoredEnvironment) -> Result<Self, InvariantError> {
        check_declared(&stored.declared)?;
        let mut aliases = BTreeSet::new();
        let mut instances = BTreeSet::new();
        for member in &stored.members {
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
            id: stored.id,
            name: stored.name,
            manifest_location: stored.manifest_location,
            manifest_sha256: stored.manifest_sha256,
            declared: stored.declared,
            members: stored.members,
            revision: stored.revision,
        })
    }

    /// Records the manifest this environment was accepted from: where it
    /// lives, its digest, and the services it declares. Raises the
    /// revision by 1.
    ///
    /// # Errors
    ///
    /// Returns an [`InvariantError`] when two declared services share an
    /// alias ([`InvariantDetail::DuplicateDeclaredService`]) or a service
    /// depends on an alias not declared
    /// ([`InvariantDetail::UnknownDeclaredDependency`]); the environment
    /// does not change.
    pub fn set_manifest(
        &mut self,
        location: ManifestLocation,
        sha256: Sha256Digest,
        declared: Vec<DeclaredService>,
    ) -> Result<(), InvariantError> {
        check_declared(&declared)?;
        self.manifest_location = Some(location);
        self.manifest_sha256 = Some(sha256);
        self.declared = declared;
        self.revision = self.revision.next();
        Ok(())
    }

    /// Forgets where the manifest lives, because another environment now
    /// holds that location (spec 0044 AC-11). Raises the revision by 1.
    pub fn clear_manifest_location(&mut self) {
        self.manifest_location = None;
        self.revision = self.revision.next();
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

    /// Returns where its manifest lives, if anywhere.
    #[must_use]
    pub const fn manifest_location(&self) -> Option<&ManifestLocation> {
        self.manifest_location.as_ref()
    }

    /// Returns the digest of the environment manifest it came from, if any.
    #[must_use]
    pub const fn manifest_sha256(&self) -> Option<&Sha256Digest> {
        self.manifest_sha256.as_ref()
    }

    /// Returns the services its manifest declared, in manifest order.
    #[must_use]
    pub fn declared(&self) -> &[DeclaredService] {
        &self.declared
    }

    /// Returns the members in their given order.
    #[must_use]
    pub fn members(&self) -> &[EnvironmentMember] {
        &self.members
    }

    /// Returns the revision: 1 when created. The registry compares it
    /// before every update.
    #[must_use]
    pub const fn revision(&self) -> ResourceRevision {
        self.revision
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

fn check_declared(declared: &[DeclaredService]) -> Result<(), InvariantError> {
    let mut aliases = BTreeSet::new();
    for service in declared {
        if !aliases.insert(&service.alias) {
            return Err(InvariantDetail::DuplicateDeclaredService {
                alias: service.alias.clone(),
            }
            .into());
        }
    }
    for service in declared {
        if let Some(edge) = service
            .depends_on
            .iter()
            .find(|edge| !aliases.contains(&edge.service))
        {
            return Err(InvariantDetail::UnknownDeclaredDependency {
                service: service.alias.clone(),
                dependency: edge.service.clone(),
            }
            .into());
        }
    }
    Ok(())
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

    fn declared(alias: &str, depends_on: &[&str]) -> DeclaredService {
        DeclaredService::new(
            alias.parse().unwrap(),
            "official/postgresql".parse().unwrap(),
            Ownership::Dedicated,
            Some(RuntimeSelector::Stable),
            None,
            None,
            depends_on
                .iter()
                .map(|dep| {
                    DependencyEdge::new(dep.parse().unwrap(), DependencyCondition::Ready, None)
                })
                .collect(),
        )
    }

    fn location() -> ManifestLocation {
        "/Users/me/shop/stackly.yaml".parse().unwrap()
    }

    #[test]
    fn set_manifest_records_the_snapshot_and_raises_the_revision_by_one() {
        let mut env = environment(Vec::new()).unwrap();
        assert!(env.manifest_location().is_none());
        assert!(env.declared().is_empty());

        env.set_manifest(
            location(),
            Sha256Digest::from_bytes([1; 32]),
            vec![declared("api", &["db"]), declared("db", &[])],
        )
        .unwrap();

        assert_eq!(env.revision().get(), 2);
        assert_eq!(env.manifest_location(), Some(&location()));
        assert_eq!(env.declared().len(), 2);
        assert_eq!(env.declared()[0].depends_on()[0].service().as_str(), "db");

        env.clear_manifest_location();
        assert_eq!(env.revision().get(), 3);
        assert!(env.manifest_location().is_none());
        assert!(env.manifest_sha256().is_some());
    }

    #[test]
    fn declared_aliases_are_unique_and_dependencies_must_be_declared() {
        let mut env = environment(Vec::new()).unwrap();
        let err = env
            .set_manifest(
                location(),
                Sha256Digest::from_bytes([2; 32]),
                vec![declared("db", &[]), declared("db", &[])],
            )
            .unwrap_err();
        assert!(matches!(
            err.detail(),
            InvariantDetail::DuplicateDeclaredService { .. }
        ));
        let err = env
            .set_manifest(
                location(),
                Sha256Digest::from_bytes([2; 32]),
                vec![declared("api", &["cache"])],
            )
            .unwrap_err();
        assert!(matches!(
            err.detail(),
            InvariantDetail::UnknownDeclaredDependency { .. }
        ));
        assert_eq!(
            env.revision().get(),
            1,
            "a refused change keeps the revision"
        );
    }

    #[test]
    fn a_manifest_location_is_absolute_without_dot_segments() {
        assert!("/a".parse::<ManifestLocation>().is_ok());
        for bad in ["relative/stackly.yaml", "/", "/a/../b", "/a/./b", ""] {
            assert!(bad.parse::<ManifestLocation>().is_err(), "{bad}");
        }
        let long = format!("/{}", "a".repeat(ManifestLocation::MAX));
        assert!(long.parse::<ManifestLocation>().is_err());
    }
}
