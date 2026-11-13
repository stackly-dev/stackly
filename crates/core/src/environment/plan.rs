//! `environment::plan` (spec 0044, *Aksi plan*): what Up would do for a
//! valid manifest on this Mac, read only.
//!
//! A plan matches the manifest to the registry by name, then gives every
//! service a `create`, `start`, or `keep` item: an existing instance by the
//! *Lifecycle ke aksi* table, a new one through the same `gather` and
//! `evaluate` that preflight and create use, with the runtime resolved but
//! never installed. It never starts an operation, writes, downloads,
//! spawns, or changes trust (AC-6); its only touch on the system is the
//! trial bind of create's port check.

use std::collections::BTreeSet;
use std::net::IpAddr;

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use stackly_domain::{
    DependencyEdge, EndpointName, Environment, EnvironmentId, EnvironmentName, ExactVersion,
    HealthState, InstallMode, InstanceId, InstanceLifecycle, InstanceName, ManifestLocation,
    MemberAlias, Ownership, Port, PortRequest, ProviderName, ProviderRevisionKey, ReadinessState,
    ResourceRevision, RevisionLabel, RuntimeId, RuntimeLine, RuntimeSelector, RuntimeState,
    SecretRef, Sha256Digest, StartupPolicy, StorageLocationId, Target,
};
use stackly_platform::{HostFacts, StoreState};

use super::finding::{FindingCode, FindingLocation};
use super::local::{
    LocalFile, LocalInput, LocalOverrides, LocalState, OverrideState, effective_port,
};
use super::lock::{
    ArtifactUnknownReason, LockFile, LockInput, LockStaleItem, LockStaleReason, LockState,
    LockedService, lock_file_name, locked_artifact, stale_reason,
};
use super::register::UnreadableReason;
use super::validate::{EnvironmentManifest, ServiceSpec, Validation};
use crate::catalog::{
    Catalog, CatalogEntry, NotInstallableReason, TrustVerdict, require_entry_trust,
};
use crate::instance::create::{
    Check, CheckStatus, CreateDeps, CreateDraft, CreateError, GatherError, PlannedRuntime,
    PortRequestDraft, ProbeFact, ProviderChoice, SecretSourceKind, SourceFact, evaluate, gather,
    gather_planned,
};
use crate::provider::ArtifactStatus;
use crate::runtime::resolve::{ResolveError, resolve};
use crate::secret::SecretFieldRule;

/// The runtime ID a planned create carries while its runtime is not
/// installed. It never reaches a journal: a plan starts no operation.
pub const PLANNED_RUNTIME_PLACEHOLDER_ID: &str = "00000000-0000-7000-8000-000000000001";

/// [`PLANNED_RUNTIME_PLACEHOLDER_ID`] as an ID.
///
/// # Panics
///
/// Never: the constant is a valid UUID version 7.
#[must_use]
pub fn planned_runtime_placeholder_id() -> RuntimeId {
    PLANNED_RUNTIME_PLACEHOLDER_ID
        .parse()
        .expect("the planned runtime placeholder is a valid UUID version 7")
}

/// The one place a plan takes its inputs (spec 0044, *Titik sambung*;
/// spec 0045 for the lock, the overrides, and `locked`).
#[derive(Debug, Clone, Copy)]
pub struct PlanInputs<'a> {
    /// The validated manifest.
    pub manifest: &'a Validation,
    /// Where it lives: the canonical parent folder plus the file name.
    pub location: &'a ManifestLocation,
    /// The environment the request named, for a `registered` source.
    pub registered: Option<EnvironmentId>,
    /// The lock next to the manifest; none reads as no file.
    pub lock: Option<&'a LockInput>,
    /// The local override file next to the manifest; none reads as no
    /// file.
    pub overrides: Option<&'a LocalInput>,
    /// Whether the request asked for `--locked` (AC-6): nothing is
    /// resolved from a manifest selector.
    pub locked: bool,
}

/// One instance as the plan and the status see it: the registry row with
/// what the supervisor says now. The daemon builds it from the same facts
/// as `InstanceSummary`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstanceView {
    /// The ID.
    pub id: InstanceId,
    /// The name.
    pub name: InstanceName,
    /// The provider of its pinned revision.
    pub provider: ProviderName,
    /// The exact runtime version.
    pub runtime_version: ExactVersion,
    /// The runtime line, or `None` when its runtime row could not be read.
    pub runtime_line: Option<RuntimeLine>,
    /// The lifecycle.
    pub lifecycle: InstanceLifecycle,
    /// Whether its data was initialized (`data_initialized_at` is set).
    pub initialized: bool,
    /// The readiness.
    pub readiness: ReadinessState,
    /// The health.
    pub health: HealthState,
    /// The code of `InstanceSummary.blocker`, if one is set.
    pub blocker: Option<String>,
    /// Why it is not ready, already redacted: the readiness detail or the
    /// last failure.
    pub detail: Option<String>,
    /// Every IP endpoint with its host and effective port.
    pub endpoints: Vec<(EndpointName, IpAddr, Option<Port>)>,
    /// The artifact digest of its runtime, when the runtime is managed.
    pub runtime_artifact_sha256: Option<Sha256Digest>,
}

/// What a plan reads.
pub struct PlanDeps<'a> {
    /// What create's `gather` reads, the registry included.
    pub create: CreateDeps<'a>,
    /// The loaded catalog.
    pub catalog: &'a Catalog,
    /// This Mac, for `runtime::resolve`.
    pub host: &'a HostFacts,
    /// Every instance, retained ones included: a member that is retained
    /// still gets its item and its `env.instance_blocked` blocker (AC-9).
    pub instances: &'a [InstanceView],
}

/// How the manifest's name matches the registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Registration {
    /// No environment has the name.
    New,
    /// An environment has the name at this location.
    Registered,
    /// An environment has the name at another location.
    NameTaken,
}

impl Registration {
    /// Returns the stable code.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::New => "new",
            Self::Registered => "registered",
            Self::NameTaken => "name_taken",
        }
    }
}

/// What a plan does with one service.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PlanAction {
    /// Create an instance, then start it.
    Create,
    /// Start an existing instance.
    Start,
    /// Leave an existing instance as it is.
    Keep,
}

impl PlanAction {
    /// Returns the stable code.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Start => "start",
            Self::Keep => "keep",
        }
    }
}

/// One step of an item, so install, create, and start read apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PlanStep {
    /// Install the runtime.
    InstallRuntime,
    /// Create the instance.
    CreateInstance,
    /// Start the instance.
    StartInstance,
}

impl PlanStep {
    /// Returns the stable code.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::InstallRuntime => "install_runtime",
            Self::CreateInstance => "create_instance",
            Self::StartInstance => "start_instance",
        }
    }
}

/// Whether the provider of a `create` item may run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PlanTrust {
    /// A catalog entry, or an approved local one.
    Trusted,
    /// A local entry not approved yet.
    ApprovalRequired,
    /// A revoked revision.
    Revoked,
}

impl PlanTrust {
    /// Returns the stable code.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::Trusted => "trusted",
            Self::ApprovalRequired => "approval_required",
            Self::Revoked => "revoked",
        }
    }
}

/// The state of one secret field of a `create` item (AC-10).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PlanSecretStatus {
    /// The reference is registered and holds a value.
    Present,
    /// The reference is registered without a value.
    Missing,
    /// No registry row names the reference.
    Unregistered,
    /// The store could not be asked.
    Unknown,
    /// No reference: create generates the value.
    Generated,
}

impl PlanSecretStatus {
    /// Returns the stable code.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::Present => "present",
            Self::Missing => "missing",
            Self::Unregistered => "unregistered",
            Self::Unknown => "unknown",
            Self::Generated => "generated",
        }
    }

    /// Whether Up needs a secret from the user first.
    #[must_use]
    pub const fn needs_secret(self) -> bool {
        matches!(self, Self::Missing | Self::Unregistered | Self::Unknown)
    }
}

/// Where the runtime of a `create` item comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RuntimeSource {
    /// The artifact the lock names for this Mac.
    Lock,
    /// The manifest selector, resolved against the catalog.
    Catalog,
}

impl RuntimeSource {
    /// Returns the stable code.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::Lock => "lock",
            Self::Catalog => "catalog",
        }
    }
}

/// The runtime of a `create` item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanRuntime {
    /// The selector as the manifest wrote it, `stable` when it wrote none.
    pub selector: String,
    /// The exact version it resolved to.
    pub version: ExactVersion,
    /// The artifact ID.
    pub artifact_id: String,
    /// The artifact digest.
    pub artifact_sha256: Sha256Digest,
    /// Whether Up installs it.
    pub install: bool,
    /// The installed runtime, when there is one.
    pub runtime_id: Option<RuntimeId>,
    /// The download size, when Up installs it.
    pub download_size: Option<u64>,
    /// Where it comes from.
    pub source: RuntimeSource,
}

/// One endpoint of an item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanEndpoint {
    /// The endpoint.
    pub name: EndpointName,
    /// Its host.
    pub host: IpAddr,
    /// Its port: what create asks for, or the effective port of an
    /// existing instance.
    pub port: PortRequest,
}

/// One secret field of a `create` item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanSecret {
    /// The field.
    pub field: String,
    /// The reference the manifest names, if any.
    pub reference: Option<SecretRef>,
    /// Its state.
    pub status: PlanSecretStatus,
}

/// How an existing member differs from its service (*Aturan
/// `differences`*). Never a blocker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanDifference {
    /// `provider`, `runtime`, `endpoint.<name>.port`, `ownership`,
    /// `instanceRef`, or `lock`.
    pub field: String,
    /// What the manifest says.
    pub manifest: String,
    /// What the instance has.
    pub instance: String,
}

/// One service of the plan.
#[derive(Debug, Clone, PartialEq)]
pub struct PlanItem {
    /// The alias.
    pub alias: MemberAlias,
    /// The ownership of the service.
    pub ownership: Ownership,
    /// What Up does.
    pub action: PlanAction,
    /// The steps, in order.
    pub steps: Vec<PlanStep>,
    /// The instance: its ID when it exists, and its name.
    pub instance_id: Option<InstanceId>,
    /// The instance name.
    pub instance_name: InstanceName,
    /// The provider.
    pub provider: ProviderName,
    /// The trust of the provider, for a `create` item.
    pub trust: Option<PlanTrust>,
    /// The key of the catalog entry a `create` item uses.
    pub provider_revision: Option<ProviderRevisionKey>,
    /// The runtime, for a `create` item whose selector resolved.
    pub runtime: Option<PlanRuntime>,
    /// The endpoints.
    pub endpoints: Vec<PlanEndpoint>,
    /// The secret fields, for a `create` item, in schema order.
    pub secrets: Vec<PlanSecret>,
    /// The aliases it waits for.
    pub waits_for: Vec<MemberAlias>,
    /// How the existing member differs from the manifest.
    pub differences: Vec<PlanDifference>,
    /// The preflight rows of a `create` item.
    pub checks: Vec<Check>,
    /// The data location an override chose, for a `create` item.
    pub storage: Option<PlanStorage>,
}

/// The data location of a `create` item that an override chose (AC-11).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanStorage {
    /// The registered data location.
    pub location_id: StorageLocationId,
    /// Its stored path.
    pub path: String,
}

/// Why a plan is not applicable.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum BlockerKind {
    /// A preflight row of a `create` item is an error: the code create
    /// itself refuses with.
    Create(CreateError),
    /// The selector did not resolve to an installable artifact.
    Resolve {
        /// Why.
        error: ResolveError,
        /// The selector as the manifest wrote it, `stable` when it wrote
        /// none.
        selector: String,
    },
    /// `env.name_taken`: the name is registered at another location.
    NameTaken {
        /// The name.
        name: EnvironmentName,
        /// Where it is registered, if anywhere.
        location: Option<ManifestLocation>,
    },
    /// `env.manifest_renamed`: the file of a registered environment
    /// declares another name.
    ManifestRenamed {
        /// The name the file declares.
        declared: EnvironmentName,
        /// The name of the environment.
        registered: EnvironmentName,
    },
    /// `env.instance_ref_not_found`.
    InstanceRefNotFound {
        /// The name the service borrows.
        instance_ref: InstanceName,
    },
    /// `env.instance_ref_mismatch`.
    InstanceRefMismatch {
        /// The provider the service names.
        expected: ProviderName,
        /// The provider of the instance.
        actual: ProviderName,
    },
    /// `env.instance_blocked`: the instance can't be started.
    InstanceBlocked {
        /// The instance.
        instance: InstanceName,
        /// The lifecycle or blocker code.
        reason: String,
    },
    /// `provider.approval_required`, for `env.lock`: a local entry not
    /// approved yet.
    ProviderApprovalRequired {
        /// The provider.
        provider: ProviderName,
    },
    /// `provider.revoked`, for `env.lock`.
    ProviderRevoked {
        /// The provider.
        provider: ProviderName,
    },
    /// `env.lock_missing`: `locked` without a lock file.
    LockMissing {
        /// The lock file name.
        file: String,
    },
    /// `env.lock_unreadable`.
    LockUnreadable {
        /// Why.
        reason: UnreadableReason,
    },
    /// `env.lock_invalid`: the loader refused the lock.
    LockInvalid {
        /// The finding code.
        code: FindingCode,
        /// Where.
        location: FindingLocation,
    },
    /// `env.lock_stale`, with `locked`: one alias does not match.
    LockStale {
        /// Why.
        reason: LockStaleReason,
    },
    /// `env.lock_provider_mismatch`: the catalog has another revision.
    LockProviderMismatch {
        /// The provider.
        provider: ProviderName,
        /// The revision the lock names.
        locked_revision: RevisionLabel,
        /// The revision the catalog holds.
        available_revision: RevisionLabel,
    },
    /// `env.lock_no_artifact`: no locked artifact for this Mac.
    LockNoArtifact {
        /// This Mac's target.
        target: Target,
        /// The targets the entry has.
        locked_targets: Vec<String>,
    },
    /// `env.lock_artifact_unknown`.
    LockArtifactUnknown {
        /// The locked artifact ID.
        artifact_id: String,
        /// The locked digest.
        sha256: Sha256Digest,
        /// Why.
        reason: ArtifactUnknownReason,
    },
    /// `env.local_unreadable`.
    LocalUnreadable {
        /// Why.
        reason: UnreadableReason,
    },
    /// `env.local_invalid`: the override file has error findings.
    LocalInvalid {
        /// How many.
        errors: usize,
    },
    /// `env.override_location_unknown`: `dataLocation` is not a
    /// registered data location.
    OverrideLocationUnknown {
        /// The path as the file wrote it.
        path: String,
    },
}

impl BlockerKind {
    /// Returns the wire code.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::Create(error) => error.code(),
            Self::Resolve { error, .. } => resolve_code(error),
            Self::NameTaken { .. } => "env.name_taken",
            Self::ManifestRenamed { .. } => "env.manifest_renamed",
            Self::InstanceRefNotFound { .. } => "env.instance_ref_not_found",
            Self::InstanceRefMismatch { .. } => "env.instance_ref_mismatch",
            Self::InstanceBlocked { .. } => "env.instance_blocked",
            Self::ProviderApprovalRequired { .. } => "provider.approval_required",
            Self::ProviderRevoked { .. } => "provider.revoked",
            Self::LockMissing { .. } => "env.lock_missing",
            Self::LockUnreadable { .. } => "env.lock_unreadable",
            Self::LockInvalid { .. } => "env.lock_invalid",
            Self::LockStale { .. } => "env.lock_stale",
            Self::LockProviderMismatch { .. } => "env.lock_provider_mismatch",
            Self::LockNoArtifact { .. } => "env.lock_no_artifact",
            Self::LockArtifactUnknown { .. } => "env.lock_artifact_unknown",
            Self::LocalUnreadable { .. } => "env.local_unreadable",
            Self::LocalInvalid { .. } => "env.local_invalid",
            Self::OverrideLocationUnknown { .. } => "env.override_location_unknown",
        }
    }
}

const fn resolve_code(error: &ResolveError) -> &'static str {
    match error {
        ResolveError::Unresolved { .. } => "runtime.selector_unresolved",
        ResolveError::Ambiguous { .. } => "runtime.selector_ambiguous",
        ResolveError::NotInstallable { .. } => "catalog.not_installable",
    }
}

/// One problem that makes the plan not applicable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanBlocker {
    /// The service it is about, if any.
    pub service: Option<MemberAlias>,
    /// What it is.
    pub kind: BlockerKind,
}

/// A member whose alias the manifest no longer declares. It is never
/// touched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetachedMember {
    /// The alias.
    pub alias: MemberAlias,
    /// The instance.
    pub instance_id: InstanceId,
    /// Its name, when the instance is still listed.
    pub instance_name: Option<InstanceName>,
    /// The member's ownership.
    pub ownership: Ownership,
}

/// The environment a plan matched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvironmentRef {
    /// The ID.
    pub id: EnvironmentId,
    /// The name.
    pub name: EnvironmentName,
    /// The revision.
    pub revision: ResourceRevision,
}

/// A plan.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    /// How the name matched; none for a manifest that is not valid.
    pub registration: Option<Registration>,
    /// The environment of the same name, if any.
    pub environment: Option<EnvironmentRef>,
    /// Where that environment is registered.
    pub registered_location: Option<ManifestLocation>,
    /// Whether the manifest is valid and nothing blocks.
    pub applicable: bool,
    /// The digest Up compares, only when applicable.
    pub digest: Option<Sha256Digest>,
    /// One item per service, in manifest order.
    pub items: Vec<PlanItem>,
    /// The start order: by dependency, ties in manifest order.
    pub order: Vec<MemberAlias>,
    /// What blocks the plan.
    pub blockers: Vec<PlanBlocker>,
    /// Members the manifest no longer declares.
    pub detached: Vec<DetachedMember>,
    /// How many secret fields need a value from the user.
    pub needs_secrets: usize,
    /// The lock next to the manifest.
    pub lock: PlanLock,
    /// The local override file, when there is one.
    pub local: Option<PlanLocal>,
}

/// One override as a plan shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanOverride {
    /// The alias as the file wrote it.
    pub service: String,
    /// `endpoints.<name>.port` or `storage.dataLocation`.
    pub field: String,
    /// The port, `auto`, or the path as written.
    pub value: String,
    /// What became of it.
    pub state: OverrideState,
}

/// The local override file of a plan (AC-9).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanLocal {
    /// The file name.
    pub file: String,
    /// Its state.
    pub state: LocalState,
    /// The digest of the bytes read.
    pub sha256: Option<Sha256Digest>,
    /// Every override, the ignored ones included.
    pub overrides: Vec<PlanOverride>,
    /// Every finding.
    pub findings: super::finding::Findings,
}

/// The lock of a plan (AC-5, AC-6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanLock {
    /// The lock file name.
    pub file: String,
    /// Its state.
    pub state: LockState,
    /// The digest of a valid lock (`in_sync` or `stale`).
    pub sha256: Option<Sha256Digest>,
    /// Whether the plan was made with `locked`.
    pub locked: bool,
    /// Every target of every entry, by text.
    pub targets: Vec<String>,
    /// The aliases that do not match.
    pub stale: Vec<LockStaleItem>,
}

/// Why a plan could not be read.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum PlanError {
    /// Create's `gather`, or the registry, failed.
    #[error(transparent)]
    Gather(#[from] GatherError),
}

impl From<stackly_domain::RegistryError> for PlanError {
    fn from(error: stackly_domain::RegistryError) -> Self {
        Self::Gather(GatherError::Registry(error))
    }
}

/// Plans the manifest of `inputs` against this Mac.
///
/// # Errors
///
/// Returns a [`PlanError`] when the registry or a platform port cannot be
/// read; everything the plan finds wrong is a blocker.
pub fn plan(deps: &PlanDeps<'_>, inputs: &PlanInputs<'_>) -> Result<Plan, PlanError> {
    let absent = LockInput {
        file: lock_file_name(&manifest_file_name(inputs.location)),
        content: LockFile::Absent,
    };
    let lock_input = inputs.lock.unwrap_or(&absent);
    let mut plan = Plan {
        registration: None,
        environment: None,
        registered_location: None,
        applicable: false,
        digest: None,
        items: Vec::new(),
        order: Vec::new(),
        blockers: Vec::new(),
        detached: Vec::new(),
        needs_secrets: 0,
        lock: PlanLock {
            file: lock_input.file.clone(),
            state: LockState::Absent,
            sha256: lock_input.content.sha256(),
            locked: inputs.locked,
            targets: lock_input
                .content
                .lock()
                .map(super::lock::EnvironmentLock::targets)
                .unwrap_or_default(),
            stale: Vec::new(),
        },
        local: None,
    };
    let Some(manifest) = &inputs.manifest.manifest else {
        // A manifest that is not valid: the lock state only as far as
        // `invalid`, without the stale check.
        plan.lock.state = match &lock_input.content {
            LockFile::Absent => LockState::Absent,
            LockFile::Unreadable(_) => LockState::Unreadable,
            LockFile::Invalid(_) => LockState::Invalid,
            LockFile::Valid { .. } => LockState::InSync,
        };
        plan.local = inputs.overrides.and_then(shape_only);
        return Ok(plan);
    };
    let (lock_state, stale) = super::lock::state(manifest, &lock_input.content);
    plan.lock.state = lock_state;
    plan.lock.stale = stale;
    lock_blockers(&mut plan, &lock_input.content, inputs.locked);
    let lock = LockUse {
        lock: lock_input.content.lock(),
        locked: inputs.locked,
    };
    let registry = deps.create.registry;
    let environments = registry.environments()?.value;

    // 1. The name against the registry.
    if let Some(id) = inputs.registered
        && let Some(registered) = environments.iter().find(|env| env.id() == id)
        && registered.name() != &manifest.name
    {
        plan.environment = Some(reference(registered));
        plan.registered_location = registered.manifest_location().cloned();
        plan.blockers.push(PlanBlocker {
            service: None,
            kind: BlockerKind::ManifestRenamed {
                declared: manifest.name.clone(),
                registered: registered.name().clone(),
            },
        });
        return Ok(plan);
    }
    let same_name = environments.iter().find(|env| env.name() == &manifest.name);
    let registration = match same_name {
        None => Registration::New,
        Some(env) if env.manifest_location() == Some(inputs.location) => Registration::Registered,
        Some(env) => {
            plan.blockers.push(PlanBlocker {
                service: None,
                kind: BlockerKind::NameTaken {
                    name: env.name().clone(),
                    location: env.manifest_location().cloned(),
                },
            });
            Registration::NameTaken
        }
    };
    plan.registration = Some(registration);
    plan.environment = same_name.map(reference);
    plan.registered_location = same_name.and_then(|env| env.manifest_location().cloned());
    let members = match (registration, same_name) {
        (Registration::Registered, Some(env)) => env.members().to_vec(),
        _ => Vec::new(),
    };
    let member_aliases: BTreeSet<MemberAlias> = members
        .iter()
        .map(|member| member.alias().clone())
        .collect();
    let overrides = local_overrides(
        &mut plan,
        inputs.overrides,
        manifest,
        deps.catalog,
        &member_aliases,
    );

    // 2. One item per service.
    for service in &manifest.services {
        let member = members
            .iter()
            .find(|member| member.alias() == &service.alias);
        let item = if let Some(member) = member {
            existing_item(
                deps,
                service,
                member.instance_id(),
                Some(member.ownership()),
                &Context {
                    lock: &lock,
                    overrides: &overrides,
                },
                &mut plan,
            )
        } else if let Some(instance_ref) = &service.instance_ref {
            borrowed_item(
                deps,
                service,
                instance_ref,
                &Context {
                    lock: &lock,
                    overrides: &overrides,
                },
                &mut plan,
            )
        } else {
            Some(create_item(
                deps,
                service,
                &Context {
                    lock: &lock,
                    overrides: &overrides,
                },
                &mut plan,
            )?)
        };
        if let Some(item) = item {
            plan.items.push(item);
        }
    }
    plan.detached = members
        .iter()
        .filter(|member| manifest.service(member.alias()).is_none())
        .map(|member| DetachedMember {
            alias: member.alias().clone(),
            instance_id: member.instance_id(),
            instance_name: deps
                .instances
                .iter()
                .find(|view| view.id == member.instance_id())
                .map(|view| view.name.clone()),
            ownership: member.ownership(),
        })
        .collect();

    // 3. Order, 4. applicable, 5. digest.
    plan.order = start_order(&manifest.services);
    plan.needs_secrets = plan
        .items
        .iter()
        .flat_map(|item| &item.secrets)
        .filter(|secret| secret.status.needs_secret())
        .count();
    plan.applicable = plan.blockers.is_empty();
    if plan.applicable {
        plan.digest = Some(digest(
            inputs.manifest.sha256,
            registration,
            &plan.items,
            &plan.lock,
            plan.local.as_ref().and_then(|local| local.sha256),
        ));
    }
    Ok(plan)
}

/// The file name of a manifest location.
#[must_use]
pub fn manifest_file_name(location: &ManifestLocation) -> String {
    location
        .as_path()
        .file_name()
        .map_or_else(String::new, |name| name.to_string_lossy().into_owned())
}

/// The lock a plan uses: the valid lock, if any, and the mode.
#[derive(Clone, Copy)]
struct LockUse<'a> {
    lock: Option<&'a super::lock::EnvironmentLock>,
    locked: bool,
}

impl<'a> LockUse<'a> {
    /// The entry of a dedicated `service` when it is *cocok*.
    fn matching(&self, service: &ServiceSpec) -> Option<&'a LockedService> {
        let lock = self.lock?;
        let entry = lock.service(&service.alias);
        if service.instance_ref.is_some() || stale_reason(service, entry).is_some() {
            return None;
        }
        entry
    }
}

/// What every item reads past the service: the lock and the overrides.
struct Context<'a> {
    lock: &'a LockUse<'a>,
    overrides: &'a LocalOverrides,
}

/// The local file of a plan whose manifest is not valid: only the
/// findings of its shape.
fn shape_only(input: &LocalInput) -> Option<PlanLocal> {
    let (state, findings) = match &input.content {
        LocalFile::Absent => return None,
        LocalFile::Unreadable(_) => (LocalState::Unreadable, super::finding::Findings::default()),
        LocalFile::Invalid { findings, .. } => (LocalState::Invalid, findings.clone()),
        LocalFile::Loaded { .. } => (LocalState::Present, super::finding::Findings::default()),
    };
    Some(PlanLocal {
        file: input.file.clone(),
        state,
        sha256: input.content.sha256(),
        overrides: Vec::new(),
        findings,
    })
}

/// *Aturan override*: the file checked against the manifest, its
/// `PlanLocal`, and the overrides the items apply, which are none when
/// the file can't be read or has an error (AC-9).
fn local_overrides(
    plan: &mut Plan,
    input: Option<&LocalInput>,
    manifest: &EnvironmentManifest,
    catalog: &Catalog,
    members: &BTreeSet<MemberAlias>,
) -> LocalOverrides {
    let Some(input) = input else {
        return LocalOverrides::default();
    };
    let mut local = PlanLocal {
        file: input.file.clone(),
        state: LocalState::Present,
        sha256: input.content.sha256(),
        overrides: Vec::new(),
        findings: super::finding::Findings::default(),
    };
    let block = |plan: &mut Plan, kind: BlockerKind| {
        plan.blockers.push(PlanBlocker {
            service: None,
            kind,
        });
    };
    let checked = match &input.content {
        LocalFile::Absent => return LocalOverrides::default(),
        LocalFile::Unreadable(reason) => {
            local.state = LocalState::Unreadable;
            block(plan, BlockerKind::LocalUnreadable { reason: *reason });
            None
        }
        LocalFile::Invalid { findings, .. } => {
            local.state = LocalState::Invalid;
            local.findings = findings.clone();
            block(
                plan,
                BlockerKind::LocalInvalid {
                    errors: findings.count_of(super::finding::Severity::Error).max(1),
                },
            );
            None
        }
        LocalFile::Loaded { document, .. } => {
            let checked = super::local::validate(document, manifest, catalog, &|alias| {
                members.contains(alias)
            });
            local.findings = checked.findings.clone();
            local.overrides = checked
                .overrides
                .iter()
                .map(|found| PlanOverride {
                    service: found.service.clone(),
                    field: found.field(),
                    value: found.value_text(),
                    state: found.state(
                        found
                            .alias
                            .as_ref()
                            .is_some_and(|alias| members.contains(alias)),
                    ),
                })
                .collect();
            if checked.has_errors() {
                local.state = LocalState::Invalid;
                block(
                    plan,
                    BlockerKind::LocalInvalid {
                        errors: checked.findings.count_of(super::finding::Severity::Error),
                    },
                );
                None
            } else {
                Some(checked)
            }
        }
    };
    plan.local = Some(local);
    checked.unwrap_or_default()
}

/// The registered data location `path` names (*Penerapan*): the canonical
/// path is the stored path of a `data` location, or its `Stackly`
/// subfolder is.
fn data_location(deps: &PlanDeps<'_>, path: &str) -> Result<Option<PlanStorage>, PlanError> {
    let Ok(canonical) = deps
        .create
        .platform
        .fs()
        .canonicalize(std::path::Path::new(path))
    else {
        return Ok(None);
    };
    let rows = deps.create.registry.storage_locations()?.value;
    Ok(rows
        .iter()
        .filter(|row| row.kind() == stackly_domain::LocationKind::Data)
        .find_map(|row| {
            let stored = std::path::Path::new(row.path()?);
            (stored == canonical || stored == canonical.join("Stackly")).then(|| PlanStorage {
                location_id: row.id(),
                path: row.path().unwrap_or_default().to_owned(),
            })
        }))
}

/// The blockers of the lock file itself (*Mode*): one that can't be read
/// or is not valid always blocks; with `locked`, a missing lock and every
/// stale alias block too.
fn lock_blockers(plan: &mut Plan, file: &LockFile, locked: bool) {
    let mut push = |service: Option<MemberAlias>, kind: BlockerKind| {
        plan.blockers.push(PlanBlocker { service, kind });
    };
    match file {
        LockFile::Unreadable(reason) => push(None, BlockerKind::LockUnreadable { reason: *reason }),
        LockFile::Invalid(problem) => push(
            None,
            BlockerKind::LockInvalid {
                code: problem.code,
                location: problem.location.clone(),
            },
        ),
        LockFile::Absent if locked => push(
            None,
            BlockerKind::LockMissing {
                file: plan.lock.file.clone(),
            },
        ),
        LockFile::Valid { .. } if locked => {
            for item in plan.lock.stale.clone() {
                push(
                    Some(item.service),
                    BlockerKind::LockStale {
                        reason: item.reason,
                    },
                );
            }
        }
        _ => {}
    }
}

fn reference(env: &Environment) -> EnvironmentRef {
    EnvironmentRef {
        id: env.id(),
        name: env.name().clone(),
        revision: env.revision(),
    }
}

/// The action of an existing instance by its lifecycle (*Lifecycle ke
/// aksi*), with the reason of the one `env.instance_blocked` blocker when
/// Up could not start it: the lifecycle `unknown` or `retained` first, then
/// `not_initialized`, then the instance's own blocker.
fn lifecycle_action(view: &InstanceView) -> (PlanAction, Option<String>) {
    use InstanceLifecycle as L;
    match view.lifecycle {
        L::Unknown | L::Retained => (PlanAction::Keep, Some(view.lifecycle.as_code().to_owned())),
        L::Uninitialized => (PlanAction::Keep, Some(NOT_INITIALIZED.to_owned())),
        L::Stopped | L::Failed if !view.initialized => {
            (PlanAction::Keep, Some(NOT_INITIALIZED.to_owned()))
        }
        L::Running | L::Starting | L::Initializing | L::Recovering => (PlanAction::Keep, None),
        L::Stopped | L::Failed | L::Stopping => (PlanAction::Start, view.blocker.clone()),
    }
}

/// The `reason` of `env.instance_blocked` for an instance with no data.
pub const NOT_INITIALIZED: &str = "not_initialized";

fn steps_of(action: PlanAction) -> Vec<PlanStep> {
    match action {
        PlanAction::Start => vec![PlanStep::StartInstance],
        PlanAction::Keep | PlanAction::Create => Vec::new(),
    }
}

fn view_endpoints(view: &InstanceView) -> Vec<PlanEndpoint> {
    view.endpoints
        .iter()
        .map(|(name, host, port)| PlanEndpoint {
            name: name.clone(),
            host: *host,
            port: port.map_or(PortRequest::Auto, PortRequest::Fixed),
        })
        .collect()
}

fn waits_for(service: &ServiceSpec) -> Vec<MemberAlias> {
    service
        .depends_on
        .iter()
        .map(|edge| edge.service().clone())
        .collect()
}

/// An item for an instance that exists: a member, or a borrowed one.
/// The registry never deletes a member's instance (`ON DELETE RESTRICT`)
/// and `instances` holds every instance, retained ones included, so the
/// view is always there for a member.
fn existing_item(
    deps: &PlanDeps<'_>,
    service: &ServiceSpec,
    instance: InstanceId,
    member_ownership: Option<Ownership>,
    context: &Context<'_>,
    plan: &mut Plan,
) -> Option<PlanItem> {
    let view = deps.instances.iter().find(|view| view.id == instance)?;
    let (action, blocked) = lifecycle_action(view);
    if let Some(reason) = blocked {
        plan.blockers.push(PlanBlocker {
            service: Some(service.alias.clone()),
            kind: BlockerKind::InstanceBlocked {
                instance: view.name.clone(),
                reason,
            },
        });
    }
    let differences = member_ownership
        .map(|ownership| {
            let mut found = differences(service, view, ownership, context.overrides);
            found.extend(lock_difference(
                context.lock.matching(service),
                deps.host,
                view,
            ));
            found
        })
        .unwrap_or_default();
    Some(PlanItem {
        alias: service.alias.clone(),
        ownership: service.ownership,
        action,
        steps: steps_of(action),
        instance_id: Some(view.id),
        instance_name: view.name.clone(),
        provider: service.provider.clone(),
        trust: None,
        provider_revision: None,
        runtime: None,
        endpoints: view_endpoints(view),
        secrets: Vec::new(),
        waits_for: waits_for(service),
        differences,
        checks: Vec::new(),
        storage: None,
    })
}

/// The selector as the manifest wrote it, `stable` when it wrote none.
fn written_selector(service: &ServiceSpec) -> String {
    service
        .selector_text
        .clone()
        .unwrap_or_else(|| "stable".to_owned())
}

/// An item for a shared service that is not a member yet.
fn borrowed_item(
    deps: &PlanDeps<'_>,
    service: &ServiceSpec,
    instance_ref: &InstanceName,
    context: &Context<'_>,
    plan: &mut Plan,
) -> Option<PlanItem> {
    // A retained instance gives its name up, so a name never finds one.
    let Some(view) = deps
        .instances
        .iter()
        .find(|view| &view.name == instance_ref && view.lifecycle != InstanceLifecycle::Retained)
    else {
        plan.blockers.push(PlanBlocker {
            service: Some(service.alias.clone()),
            kind: BlockerKind::InstanceRefNotFound {
                instance_ref: instance_ref.clone(),
            },
        });
        return None;
    };
    if view.provider != service.provider {
        plan.blockers.push(PlanBlocker {
            service: Some(service.alias.clone()),
            kind: BlockerKind::InstanceRefMismatch {
                expected: service.provider.clone(),
                actual: view.provider.clone(),
            },
        });
    }
    existing_item(deps, service, view.id, None, context, plan)
}

/// The `lock` difference of a member (AC-8): its managed runtime has
/// another digest than the artifact the matching entry locks for this
/// Mac. Never a blocker, and the instance is never changed to match.
fn lock_difference(
    entry: Option<&LockedService>,
    host: &HostFacts,
    view: &InstanceView,
) -> Option<PlanDifference> {
    let entry = entry?;
    let locked = entry.artifact_for(host.target)?;
    let own = view.runtime_artifact_sha256?;
    (own != locked.sha256).then(|| PlanDifference {
        field: "lock".into(),
        manifest: entry.version.to_string(),
        instance: view.runtime_version.to_string(),
    })
}

/// *Aturan `differences`*, for a member.
fn differences(
    service: &ServiceSpec,
    view: &InstanceView,
    member: Ownership,
    overrides: &LocalOverrides,
) -> Vec<PlanDifference> {
    let mut found = Vec::new();
    let mut differ = |field: String, manifest: String, instance: String| {
        found.push(PlanDifference {
            field,
            manifest,
            instance,
        });
    };
    if view.provider != service.provider {
        differ(
            "provider".into(),
            service.provider.to_string(),
            view.provider.to_string(),
        );
    }
    if let Some(RuntimeSelector::Text {
        text,
        version,
        line,
        ..
    }) = &service.selector
    {
        let differs = match (version, line) {
            (Some(version), _) => version != &view.runtime_version,
            (None, Some(line)) => view.runtime_line.as_ref().is_some_and(|own| own != line),
            (None, None) => false,
        };
        if differs {
            differ(
                "runtime".into(),
                text.clone(),
                view.runtime_version.to_string(),
            );
        }
    }
    // The port the service asks for wins over the manifest when a local
    // override names it (AC-10).
    let mut names: Vec<EndpointName> = service
        .endpoints
        .iter()
        .map(|endpoint| endpoint.name.clone())
        .collect();
    for (name, _) in overrides.ports(&service.alias) {
        if !names.contains(&name) {
            names.push(name);
        }
    }
    for name in names {
        let PortRequest::Fixed(port) = effective_port(overrides, service, &name) else {
            continue;
        };
        let effective = view
            .endpoints
            .iter()
            .find(|(own, _, _)| own == &name)
            .and_then(|(_, _, port)| *port);
        if effective != Some(port) {
            differ(
                format!("endpoint.{name}.port"),
                port.to_string(),
                effective.map_or_else(|| "auto".to_owned(), |port| port.to_string()),
            );
        }
    }
    if member != service.ownership {
        differ(
            "ownership".into(),
            service.ownership.as_code().to_owned(),
            member.as_code().to_owned(),
        );
    }
    if let Some(instance_ref) = &service.instance_ref
        && instance_ref != &view.name
    {
        differ(
            "instanceRef".into(),
            instance_ref.to_string(),
            view.name.to_string(),
        );
    }
    found
}

/// A `create` item: trust, resolve, the installed runtime, then create's
/// own evaluation.
#[allow(clippy::too_many_lines)]
fn create_item(
    deps: &PlanDeps<'_>,
    service: &ServiceSpec,
    context: &Context<'_>,
    plan: &mut Plan,
) -> Result<PlanItem, PlanError> {
    let lock = context.lock;
    let overrides = context.overrides;
    let registry = deps.create.registry;
    let block = |plan: &mut Plan, kind: BlockerKind| {
        let blocker = PlanBlocker {
            service: Some(service.alias.clone()),
            kind,
        };
        if !plan.blockers.contains(&blocker) {
            plan.blockers.push(blocker);
        }
    };
    let instance_name = service
        .instance_name
        .clone()
        .expect("a valid dedicated service has an instance name");
    let entry = match deps.catalog.entry(&service.provider) {
        Some(CatalogEntry::Provider(entry)) => Some(entry),
        _ => None,
    };

    // Trust and the runtime.
    let mut trust = None;
    let mut runtime = None;
    let mut planned = None;
    let mut runtime_id = planned_runtime_placeholder_id();
    if let Some(entry) = entry {
        let verdict = require_entry_trust(deps.catalog, registry, entry)?;
        trust = Some(match &verdict {
            TrustVerdict::Trusted => PlanTrust::Trusted,
            TrustVerdict::ApprovalRequired => PlanTrust::ApprovalRequired,
            TrustVerdict::Revoked(_) => PlanTrust::Revoked,
        });
        let matching = lock.matching(service);
        let source = if matching.is_some() {
            RuntimeSource::Lock
        } else {
            RuntimeSource::Catalog
        };
        let resolved = if let Some(locked) = matching {
            // *Runtime dari lock*: the trust gate first (create's own row
            // blocks), then the provider revision, then the exact artifact.
            if !verdict.is_trusted() {
                None
            } else if entry.package.key().revision() != &locked.provider_revision
                || entry.package.key().manifest_sha256() != &locked.provider_manifest_sha256
            {
                block(
                    plan,
                    BlockerKind::LockProviderMismatch {
                        provider: service.provider.clone(),
                        locked_revision: locked.provider_revision.clone(),
                        available_revision: entry.package.key().revision().clone(),
                    },
                );
                None
            } else {
                match locked_artifact(entry, locked, deps.host, &verdict) {
                    Ok(artifact) => Some(Ok(artifact)),
                    Err(error) => {
                        block(plan, error.blocker(written_selector(service)));
                        None
                    }
                }
            }
        } else if lock.locked {
            // With `locked`, nothing resolves from the manifest selector;
            // the lock blockers already say why.
            None
        } else {
            let selector = service.selector.clone().unwrap_or(RuntimeSelector::Stable);
            Some(match resolve(entry, &selector, deps.host, &verdict) {
                Ok(resolution) => Ok(resolution.artifact),
                Err(error) => {
                    installed_fixture(entry, &selector, deps.host, registry)?.ok_or(error)
                }
            })
        };
        match resolved {
            None => {}
            // A provider that is `pending` or `revoked` can't resolve
            // either; create's own trust row is its one blocker (AC-7).
            Some(Err(ResolveError::NotInstallable {
                reason: NotInstallableReason::ApprovalRequired | NotInstallableReason::TrustRevoked,
                ..
            })) if !matches!(verdict, TrustVerdict::Trusted) => {}
            Some(Err(error)) => block(
                plan,
                BlockerKind::Resolve {
                    error,
                    selector: written_selector(service),
                },
            ),
            Some(Ok(artifact)) => {
                let runtimes = registry.runtimes()?.value;
                let managed = |row: &stackly_domain::Runtime| match row.install() {
                    InstallMode::Managed { artifact_sha256 } => Some(*artifact_sha256),
                    InstallMode::External { .. } => None,
                };
                let (sha256, size, installed) = match &artifact.status {
                    ArtifactStatus::Available { sha256, size, .. } => (
                        Some(*sha256),
                        Some(*size),
                        runtimes.into_iter().find(|row| {
                            row.state() == RuntimeState::Installed && managed(row) == Some(*sha256)
                        }),
                    ),
                    // A test fixture has no catalog digest: the installed
                    // fixture runtime of the same version and target is it.
                    _ => {
                        let installed = runtimes.into_iter().find(|row| {
                            row.state() == RuntimeState::Installed
                                && row.software() == entry.package.software()
                                && row.version() == &artifact.version
                                && row.target() == artifact.target
                                && managed(row).is_some()
                        });
                        (installed.as_ref().and_then(managed), None, installed)
                    }
                };
                if let Some(sha256) = sha256 {
                    if let Some(row) = &installed {
                        runtime_id = row.id();
                    } else {
                        planned = Some(PlannedRuntime {
                            version: artifact.version.clone(),
                            line: artifact.line.clone(),
                            software: entry.package.software().clone(),
                        });
                    }
                    runtime = Some(PlanRuntime {
                        selector: written_selector(service),
                        version: artifact.version.clone(),
                        artifact_id: artifact.id.clone(),
                        artifact_sha256: sha256,
                        install: installed.is_none(),
                        runtime_id: installed.as_ref().map(stackly_domain::Runtime::id),
                        download_size: if installed.is_none() { size } else { None },
                        source,
                    });
                }
            }
        }
    }

    // The overrides, then create's own evaluation.
    let mut draft = draft_for(service, &instance_name, runtime_id);
    for (endpoint, port) in overrides.ports(&service.alias) {
        let request = match port {
            PortRequest::Auto => PortRequestDraft::Auto,
            PortRequest::Fixed(port) => PortRequestDraft::Fixed(port.get()),
        };
        match draft
            .endpoints
            .iter_mut()
            .find(|(name, _)| name == endpoint.as_str())
        {
            Some((_, own)) => *own = request,
            None => draft.endpoints.push((endpoint.to_string(), request)),
        }
    }
    let mut storage = None;
    if let Some(path) = overrides.data_location(&service.alias) {
        match data_location(deps, path)? {
            Some(found) => {
                draft.data_location = Some(found.location_id);
                storage = Some(found);
            }
            None => block(
                plan,
                BlockerKind::OverrideLocationUnknown {
                    path: path.to_owned(),
                },
            ),
        }
    }
    let resolved = runtime.is_some();
    let facts = match planned {
        Some(planned) => gather_planned(&deps.create, &draft, planned)?,
        None => gather(&deps.create, &draft)?,
    };
    let evaluation = evaluate(&draft, &facts);
    let mut checks = Vec::new();
    for mut check in evaluation.checks {
        if check.id == "runtime" && !resolved {
            // The resolve blocker already says why there is no runtime.
            continue;
        }
        if matches!(
            check.error,
            Some(CreateError::SecretNotFound { .. } | CreateError::SecretNeedsSecret { .. })
        ) {
            // A bound reference without a value is a status, not a blocker
            // (AC-10).
            check.status = CheckStatus::Caution;
            check.error = None;
        }
        if check.status == CheckStatus::Error
            && let Some(error) = &check.error
        {
            block(plan, BlockerKind::Create(error.clone()));
        }
        checks.push(check);
    }

    // Secrets, endpoints, steps.
    let secrets = match entry {
        None => Vec::new(),
        Some(entry) => entry
            .package
            .settings_schema()
            .fields()
            .filter(|field| SecretFieldRule::from_setting(field).is_some())
            .map(|field| {
                let reference = service
                    .secret_refs
                    .iter()
                    .find(|(name, _)| name.as_str() == field.name().as_str())
                    .map(|(_, reference)| reference.clone());
                let status = reference
                    .as_ref()
                    .map_or(PlanSecretStatus::Generated, |reference| {
                        secret_status(
                            facts.secrets.state,
                            facts.secrets.sources.get(&reference.to_string()),
                        )
                    });
                PlanSecret {
                    field: field.name().to_string(),
                    reference,
                    status,
                }
            })
            .collect(),
    };
    let endpoints = entry
        .map(|entry| {
            entry
                .package
                .endpoints()
                .filter(|spec| spec.transport().is_ip())
                .map(|spec| PlanEndpoint {
                    name: spec.name().clone(),
                    host: spec
                        .default_host()
                        .unwrap_or(IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)),
                    port: effective_port(overrides, service, spec.name()),
                })
                .collect()
        })
        .unwrap_or_default();
    let mut steps = Vec::new();
    if runtime.as_ref().is_some_and(|runtime| runtime.install) {
        steps.push(PlanStep::InstallRuntime);
    }
    steps.push(PlanStep::CreateInstance);
    steps.push(PlanStep::StartInstance);
    Ok(PlanItem {
        alias: service.alias.clone(),
        ownership: service.ownership,
        action: PlanAction::Create,
        steps,
        instance_id: None,
        instance_name,
        provider: service.provider.clone(),
        trust,
        provider_revision: entry.map(|entry| entry.package.key().clone()),
        runtime,
        endpoints,
        secrets,
        waits_for: waits_for(service),
        differences: Vec::new(),
        checks,
        storage,
    })
}

/// A test fixture artifact is never installable, but one already
/// installed is what a plan uses: the host artifact `selector` names (an
/// exact version or a line) whose fixture runtime is installed.
fn installed_fixture<'e>(
    entry: &'e crate::catalog::ProviderEntry,
    selector: &RuntimeSelector,
    host: &HostFacts,
    registry: &dyn stackly_domain::Registry,
) -> Result<Option<&'e crate::provider::VersionArtifact>, PlanError> {
    let RuntimeSelector::Text { version, line, .. } = selector else {
        return Ok(None);
    };
    let runtimes = registry.runtimes()?.value;
    Ok(entry.artifacts().iter().find(|artifact| {
        matches!(artifact.status, ArtifactStatus::Fixture { .. })
            && artifact.target == host.target
            && (version.as_ref() == Some(&artifact.version)
                || line.as_ref() == Some(&artifact.line))
            && runtimes.iter().any(|row| {
                row.state() == RuntimeState::Installed
                    && row.software() == entry.package.software()
                    && row.version() == &artifact.version
                    && row.target() == artifact.target
            })
    }))
}

/// The create draft of a dedicated service: the one plan evaluates and Up
/// (spec 0046) creates. `runtime_id` is the installed runtime, or
/// [`planned_runtime_placeholder_id`] while it is not installed. Every
/// `secretRefs` entry binds its reference; every other secret field is
/// generated. The instance starts after create and has `startup` manual.
#[must_use]
pub fn draft_for(
    service: &ServiceSpec,
    instance_name: &InstanceName,
    runtime_id: RuntimeId,
) -> CreateDraft {
    CreateDraft {
        name: instance_name.to_string(),
        display_name: None,
        provider: ProviderChoice {
            name: service.provider.to_string(),
            revision: None,
        },
        runtime_id,
        endpoints: service
            .endpoints
            .iter()
            .map(|endpoint| {
                (
                    endpoint.name.to_string(),
                    match endpoint.port {
                        PortRequest::Auto => PortRequestDraft::Auto,
                        PortRequest::Fixed(port) => PortRequestDraft::Fixed(port.get()),
                    },
                )
            })
            .collect(),
        settings: Value::Object(service.settings.clone()),
        secrets: service
            .secret_refs
            .iter()
            .map(|(field, reference)| {
                (
                    field.to_string(),
                    SecretSourceKind::Bind(reference.to_string()),
                )
            })
            .collect(),
        secret_values: Vec::new(),
        start: true,
        startup: Some(StartupPolicy::Manual),
        duplicate_of: None,
        data_location: None,
    }
}

/// The status of a bound reference from create's secret facts.
fn secret_status(state: StoreState, fact: Option<&SourceFact>) -> PlanSecretStatus {
    if state == StoreState::Unsupported {
        return PlanSecretStatus::Unknown;
    }
    match fact {
        Some(SourceFact::Unregistered) => PlanSecretStatus::Unregistered,
        Some(SourceFact::Registered(ProbeFact::Present)) => PlanSecretStatus::Present,
        Some(SourceFact::Registered(ProbeFact::Missing)) => PlanSecretStatus::Missing,
        Some(
            SourceFact::Registered(ProbeFact::Locked | ProbeFact::Busy | ProbeFact::Failed)
            | SourceFact::Invalid,
        )
        | None => PlanSecretStatus::Unknown,
    }
}

/// The start order: a topological order of `dependsOn` where, among the
/// services whose dependencies are placed, the earliest in the manifest
/// goes first. Validation refused cycles, so every service is placed.
#[must_use]
pub fn start_order(services: &[ServiceSpec]) -> Vec<MemberAlias> {
    let mut placed: BTreeSet<&MemberAlias> = BTreeSet::new();
    let mut order = Vec::new();
    while order.len() < services.len() {
        let next = services.iter().find(|service| {
            !placed.contains(&service.alias)
                && service
                    .depends_on
                    .iter()
                    .map(DependencyEdge::service)
                    .all(|dependency| placed.contains(dependency))
        });
        let Some(next) = next else {
            // Only a cycle leaves services unplaced; keep manifest order.
            order.extend(
                services
                    .iter()
                    .filter(|service| !placed.contains(&service.alias))
                    .map(|service| service.alias.clone()),
            );
            break;
        };
        placed.insert(&next.alias);
        order.push(next.alias.clone());
    }
    order
}

/// `planDigest`: SHA-256 of the sorted key JSON (the encoding of the
/// protocol fingerprint) over the manifest digest, the registration, and
/// one entry per item in manifest order: for `create`, what Up creates;
/// for an existing instance, only which instance. No check text and no
/// secret status.
fn digest(
    manifest: Sha256Digest,
    registration: Registration,
    items: &[PlanItem],
    lock: &PlanLock,
    local: Option<Sha256Digest>,
) -> Sha256Digest {
    let items: Vec<Value> = items
        .iter()
        .map(|item| {
            if item.action != PlanAction::Create {
                // An existing instance: its action, steps, and ports follow
                // its lifecycle, and Up derives them again in its turn.
                return json!({
                    "alias": item.alias.as_str(),
                    "kind": "existing",
                    "instance": {
                        "id": item.instance_id.map(|id| id.to_string()),
                        "name": item.instance_name.as_str(),
                    },
                });
            }
            json!({
                "alias": item.alias.as_str(),
                "kind": "create",
                "steps": item.steps.iter().map(|step| step.as_code()).collect::<Vec<_>>(),
                "instance": item.instance_name.as_str(),
                "runtime": item.runtime.as_ref().map(|runtime| json!({
                    "artifactSha256": runtime.artifact_sha256.to_string(),
                    "install": runtime.install,
                    "source": runtime.source.as_code(),
                })),
                "storage": item.storage.as_ref().map(|storage| storage.location_id.to_string()),
                "providerRevision": item.provider_revision.as_ref().map(|key| json!({
                    "revision": key.revision().to_string(),
                    "manifestSha256": key.manifest_sha256().to_string(),
                })),
                "endpoints": item.endpoints.iter().map(|endpoint| json!({
                    "name": endpoint.name.as_str(),
                    "host": endpoint.host.to_string(),
                    "port": match endpoint.port {
                        PortRequest::Auto => Value::from("auto"),
                        PortRequest::Fixed(port) => Value::from(port.get()),
                    },
                })).collect::<Vec<_>>(),
            })
        })
        .collect();
    let value = json!({
        "manifestSha256": manifest.to_string(),
        "registration": registration.as_code(),
        "items": items,
        "locked": lock.locked,
        "lockSha256": lock.sha256.map(|sha| sha.to_string()),
        "localSha256": local.map(|sha| sha.to_string()),
    });
    let canonical = serde_json::to_vec(&sorted(value))
        .expect("a json value always encodes: its map keys are strings");
    Sha256Digest::from_bytes(Sha256::digest(&canonical).into())
}

/// `value` with the keys of every object sorted, at every depth.
fn sorted(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut entries: Vec<(String, Value)> = map.into_iter().collect();
            entries.sort_by(|(a, _), (b, _)| a.cmp(b));
            Value::Object(
                entries
                    .into_iter()
                    .map(|(key, value)| (key, sorted(value)))
                    .collect(),
            )
        }
        Value::Array(items) => Value::Array(items.into_iter().map(sorted).collect()),
        other => other,
    }
}
