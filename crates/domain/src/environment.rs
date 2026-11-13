//! Environments: named groups of instances, each a dedicated or a shared
//! member, and the services their manifest declares (spec 0044).

use std::collections::BTreeSet;
use std::fmt;
use std::path::{Component, Path, PathBuf};
use std::str::FromStr;

use jiff::Timestamp;

use crate::code::code_enum;
use crate::error::{InvariantDetail, InvariantError, ValueError, ValueKind};
use crate::id::{EnvironmentId, InstanceId, OperationId};
use crate::runtime::RuntimeSelector;
use crate::up::UpReport;
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

/// One edge an Up used for a member, with its effective timeout (spec
/// 0046): an element of `environment_instances.dependency_conditions`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MemberDependency {
    service: MemberAlias,
    condition: DependencyCondition,
    timeout_seconds: u32,
}

impl MemberDependency {
    /// Creates the edge to `service`.
    #[must_use]
    pub const fn new(
        service: MemberAlias,
        condition: DependencyCondition,
        timeout_seconds: u32,
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

    /// Returns what the member waits for.
    #[must_use]
    pub const fn condition(&self) -> DependencyCondition {
        self.condition
    }

    /// Returns the effective wait limit, in seconds.
    #[must_use]
    pub const fn timeout_seconds(&self) -> u32 {
        self.timeout_seconds
    }
}

/// One instance in an environment, under an alias.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EnvironmentMember {
    alias: MemberAlias,
    instance_id: InstanceId,
    ownership: Ownership,
    dependency_conditions: Vec<MemberDependency>,
}

impl EnvironmentMember {
    /// Creates a member with no dependency conditions.
    #[must_use]
    pub const fn new(alias: MemberAlias, instance_id: InstanceId, ownership: Ownership) -> Self {
        Self {
            alias,
            instance_id,
            ownership,
            dependency_conditions: Vec::new(),
        }
    }

    /// Returns the member with the edges the last Up used for it.
    #[must_use]
    pub fn with_conditions(mut self, conditions: Vec<MemberDependency>) -> Self {
        self.dependency_conditions = conditions;
        self
    }

    /// Returns the edges the last Up used for it, with their effective
    /// timeouts (spec 0046); empty before the first Up.
    #[must_use]
    pub fn dependency_conditions(&self) -> &[MemberDependency] {
        &self.dependency_conditions
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
    /// The digest of the lock the last Up used, if any (spec 0045).
    pub lock_sha256: Option<Sha256Digest>,
    /// The services its manifest declared when it was last accepted.
    pub declared: Vec<DeclaredService>,
    /// The members, in order.
    pub members: Vec<EnvironmentMember>,
    /// The last `environment_up` operation, if any (spec 0046).
    pub last_operation: Option<OperationId>,
    /// The report of the last Up, if any (spec 0046).
    pub last_up: Option<UpReport>,
    /// When the registry last wrote the row; `None` before the first
    /// write.
    pub updated_at: Option<Timestamp>,
    /// The revision.
    pub revision: ResourceRevision,
}

/// A named group of instances.
///
/// Every mutation raises [`Environment::revision`] by exactly 1. Equality
/// leaves out [`Environment::updated_at`], the registry's write stamp.
#[derive(Debug, Clone, Eq)]
pub struct Environment {
    id: EnvironmentId,
    name: EnvironmentName,
    manifest_location: Option<ManifestLocation>,
    manifest_sha256: Option<Sha256Digest>,
    lock_sha256: Option<Sha256Digest>,
    declared: Vec<DeclaredService>,
    members: Vec<EnvironmentMember>,
    last_operation: Option<OperationId>,
    last_up: Option<UpReport>,
    updated_at: Option<Timestamp>,
    revision: ResourceRevision,
}

impl PartialEq for Environment {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
            && self.name == other.name
            && self.manifest_location == other.manifest_location
            && self.manifest_sha256 == other.manifest_sha256
            && self.lock_sha256 == other.lock_sha256
            && self.declared == other.declared
            && self.members == other.members
            && self.last_operation == other.last_operation
            && self.last_up == other.last_up
            && self.revision == other.revision
    }
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
            lock_sha256: None,
            declared: Vec::new(),
            members,
            last_operation: None,
            last_up: None,
            updated_at: None,
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
        check_members(&stored.members)?;
        Ok(Self {
            id: stored.id,
            name: stored.name,
            manifest_location: stored.manifest_location,
            manifest_sha256: stored.manifest_sha256,
            lock_sha256: stored.lock_sha256,
            declared: stored.declared,
            members: stored.members,
            last_operation: stored.last_operation,
            last_up: stored.last_up,
            updated_at: stored.updated_at,
            revision: stored.revision,
        })
    }

    /// Returns it with the registry's write stamp. Only the registry and
    /// test fakes call it; it does not change the revision.
    #[must_use]
    pub const fn stamped(mut self, at: Timestamp) -> Self {
        self.updated_at = Some(at);
        self
    }

    /// Adds `member` after the others (spec 0046: a shared `instanceRef`
    /// at accept, or a dedicated instance a create made). Raises the
    /// revision by 1.
    ///
    /// # Errors
    ///
    /// Returns an [`InvariantError`] when its alias or instance is already
    /// a member; the environment does not change.
    pub fn bind_member(&mut self, member: EnvironmentMember) -> Result<(), InvariantError> {
        let mut members = self.members.clone();
        members.push(member);
        check_members(&members)?;
        self.members = members;
        self.revision = self.revision.next();
        Ok(())
    }

    /// Replaces the dependency conditions of the member `alias`. Raises the
    /// revision by 1.
    ///
    /// # Errors
    ///
    /// Returns an [`InvariantError`] with [`InvariantDetail::UnknownMember`]
    /// when no member has that alias.
    pub fn set_member_conditions(
        &mut self,
        alias: &MemberAlias,
        conditions: Vec<MemberDependency>,
    ) -> Result<(), InvariantError> {
        let Some(member) = self
            .members
            .iter_mut()
            .find(|member| &member.alias == alias)
        else {
            return Err(InvariantDetail::UnknownMember {
                alias: alias.clone(),
            }
            .into());
        };
        member.dependency_conditions = conditions;
        self.revision = self.revision.next();
        Ok(())
    }

    /// Removes the member that holds `instance`, as deleting the instance
    /// a create made does. Raises the revision by 1 and returns true when
    /// there was one; otherwise nothing changes.
    pub fn remove_member(&mut self, instance: InstanceId) -> bool {
        let before = self.members.len();
        self.members.retain(|member| member.instance_id != instance);
        let removed = self.members.len() != before;
        if removed {
            self.revision = self.revision.next();
        }
        removed
    }

    /// Records that the Up `operation` started at `started_at`: it is the
    /// last operation, and the last Up report is running with no service
    /// yet. Raises the revision by 1.
    pub fn begin_up(&mut self, operation: OperationId, started_at: Timestamp) {
        self.last_operation = Some(operation);
        self.last_up = Some(UpReport::running(operation, started_at));
        self.revision = self.revision.next();
    }

    /// Records the report of the Up that ends now. Raises the revision by
    /// 1.
    pub fn finish_up(&mut self, report: UpReport) {
        self.last_operation = Some(report.operation());
        self.last_up = Some(report);
        self.revision = self.revision.next();
    }

    /// Records that the Down `operation` released this environment's
    /// leases (spec 0047 AC-4): it is the last operation and the last Up
    /// report is cleared, because nothing it reported holds anymore.
    /// Raises the revision by 1.
    pub fn begin_down(&mut self, operation: OperationId) {
        self.last_operation = Some(operation);
        self.last_up = None;
        self.revision = self.revision.next();
    }

    /// Records the manifest this environment was accepted from: where it
    /// lives, its digest, the services it declares, and the digest of the
    /// lock the plan used, if any (spec 0045 AC-12). Raises the revision
    /// by 1.
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
        lock_sha256: Option<Sha256Digest>,
    ) -> Result<(), InvariantError> {
        check_declared(&declared)?;
        self.manifest_location = Some(location);
        self.manifest_sha256 = Some(sha256);
        self.lock_sha256 = lock_sha256;
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

    /// Returns the digest of the lock the last Up used, if any.
    #[must_use]
    pub const fn lock_sha256(&self) -> Option<&Sha256Digest> {
        self.lock_sha256.as_ref()
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

    /// Returns the member with `alias`, if any.
    #[must_use]
    pub fn member(&self, alias: &MemberAlias) -> Option<&EnvironmentMember> {
        self.members.iter().find(|member| &member.alias == alias)
    }

    /// Returns the last `environment_up` operation, if any (spec 0046).
    #[must_use]
    pub const fn last_operation(&self) -> Option<OperationId> {
        self.last_operation
    }

    /// Returns the report of the last Up, if any (spec 0046).
    #[must_use]
    pub const fn last_up(&self) -> Option<&UpReport> {
        self.last_up.as_ref()
    }

    /// Returns when the registry last wrote the row (spec 0044 AC-14),
    /// `None` for an environment that was never written.
    #[must_use]
    pub const fn updated_at(&self) -> Option<Timestamp> {
        self.updated_at
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

fn check_members(members: &[EnvironmentMember]) -> Result<(), InvariantError> {
    let mut aliases = BTreeSet::new();
    let mut instances = BTreeSet::new();
    for member in members {
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
    Ok(())
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
            None,
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
                None,
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
                None,
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
    fn bind_member_adds_a_unique_member_and_raises_the_revision_by_one() {
        let (db, cache) = (InstanceId::generate(), InstanceId::generate());
        let mut env = environment(vec![member("db", db, Ownership::Dedicated)]).unwrap();

        env.bind_member(member("cache", cache, Ownership::Shared))
            .unwrap();
        assert_eq!(env.revision().get(), 2);
        assert_eq!(env.members().len(), 2);

        let err = env
            .bind_member(member("cache", InstanceId::generate(), Ownership::Shared))
            .unwrap_err();
        assert!(matches!(
            err.detail(),
            InvariantDetail::DuplicateMemberAlias { .. }
        ));
        let err = env
            .bind_member(member("replica", db, Ownership::Shared))
            .unwrap_err();
        assert_eq!(
            err.detail(),
            &InvariantDetail::DuplicateMemberInstance { instance: db }
        );
        assert_eq!(env.revision().get(), 2, "a refused bind keeps the revision");
    }

    #[test]
    fn member_conditions_change_only_for_a_known_alias() {
        let db = InstanceId::generate();
        let mut env = environment(vec![member("db", db, Ownership::Dedicated)]).unwrap();
        let conditions = vec![MemberDependency::new(
            "cache".parse().unwrap(),
            DependencyCondition::Ready,
            600,
        )];

        env.set_member_conditions(&"db".parse().unwrap(), conditions.clone())
            .unwrap();
        assert_eq!(env.revision().get(), 2);
        assert_eq!(
            env.member(&"db".parse().unwrap())
                .map(EnvironmentMember::dependency_conditions),
            Some(conditions.as_slice())
        );

        let err = env
            .set_member_conditions(&"api".parse().unwrap(), Vec::new())
            .unwrap_err();
        assert!(matches!(
            err.detail(),
            InvariantDetail::UnknownMember { .. }
        ));
        assert_eq!(env.revision().get(), 2);
    }

    #[test]
    fn remove_member_drops_the_instance_and_raises_the_revision_only_when_present() {
        let db = InstanceId::generate();
        let mut env = environment(vec![member("db", db, Ownership::Dedicated)]).unwrap();

        assert!(!env.remove_member(InstanceId::generate()));
        assert_eq!(env.revision().get(), 1);
        assert!(env.remove_member(db));
        assert!(env.members().is_empty());
        assert_eq!(env.revision().get(), 2);
    }

    #[test]
    fn begin_and_finish_up_record_the_last_operation_and_report() {
        use crate::up::{ServiceReport, UpPhase, UpState};

        let mut env = environment(Vec::new()).unwrap();
        let operation = OperationId::generate();
        let started = Timestamp::UNIX_EPOCH;

        env.begin_up(operation, started);
        assert_eq!(env.revision().get(), 2);
        assert_eq!(env.last_operation(), Some(operation));
        assert_eq!(env.last_up().map(UpReport::state), Some(UpState::Running));

        let report = UpReport::new(
            operation,
            UpState::Failed,
            started,
            Some(started),
            vec![ServiceReport::new("db".parse().unwrap(), UpPhase::Failed)],
        );
        env.finish_up(report.clone());
        assert_eq!(env.revision().get(), 3);
        assert_eq!(env.last_up(), Some(&report));
    }

    // covers: AC-4 (spec 0047)
    #[test]
    fn begin_down_clears_the_last_up_and_records_the_down() {
        let mut env = environment(Vec::new()).unwrap();
        let up = OperationId::generate();
        env.begin_up(up, Timestamp::UNIX_EPOCH);
        let members = env.members().to_vec();
        let down = OperationId::generate();

        env.begin_down(down);

        assert_eq!(env.revision().get(), 3);
        assert_eq!(env.last_operation(), Some(down));
        assert_eq!(env.last_up(), None);
        assert_eq!(env.members(), members.as_slice());
    }

    #[test]
    fn equality_leaves_out_the_write_stamp() {
        let env = environment(Vec::new()).unwrap();
        let stamped = env.clone().stamped(Timestamp::UNIX_EPOCH);

        assert_eq!(stamped, env);
        assert_eq!(stamped.updated_at(), Some(Timestamp::UNIX_EPOCH));
        assert_eq!(env.updated_at(), None);
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
