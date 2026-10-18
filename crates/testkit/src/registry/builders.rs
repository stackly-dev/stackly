//! Builders for registry fixtures. Each starts from valid defaults with a
//! new ID per builder, and builds through the domain's `restore`
//! constructors, so any stored state can be set up.

use std::collections::BTreeSet;
use std::fmt::Display;
use std::net::{IpAddr, Ipv4Addr};
use std::path::PathBuf;
use std::str::FromStr;

use jiff::Timestamp;
use stackly_domain::{
    Arch, ArtifactRecord, Capability, CapabilitySet, DesiredState, DisplayName, Endpoint,
    Environment, EnvironmentId, EnvironmentMember, EnvironmentName, ExactVersion, FailureCode,
    IdempotencyKey, InstallMode, Instance, InstanceId, InstanceLifecycle, IpEndpoint, Lease,
    LeaseHolder, LeaseId, LeaseState, LineCompatibility, Operation, OperationFailure, OperationId,
    OperationKind, OperationResource, OperationState, Os, Ownership, Port, PortRequest, Progress,
    ProviderName, ProviderRevision, ProviderRevisionKey, ProviderRevisionRecord, RecoveryHint,
    ResourceRevision, RevisionLabel, Runtime, RuntimeId, RuntimeLine, RuntimeState, Sha256Digest,
    SoftwareName, StageName, StartupPolicy, StoredEnvironment, StoredInstance, StoredLease,
    StoredOperation, StoredRuntime, Target, Transport, UnixEndpoint,
};
use uuid::Uuid;

/// The time fixtures use when a test sets none: 2026-01-01T00:00:00Z,
/// where [`FakeClock`](crate::FakeClock) starts.
const FIXTURE_TIME: Timestamp = Timestamp::constant(1_767_225_600, 0);

/// Parses fixture text that the builder or its caller promises is valid.
fn parse<T>(what: &str, text: &str) -> T
where
    T: FromStr,
    T::Err: Display,
{
    text.parse()
        .unwrap_or_else(|err| panic!("fixture {what} {text:?} must be valid: {err}"))
}

/// Returns a revision number that the caller promises is at least 1.
fn revision(number: u64) -> ResourceRevision {
    ResourceRevision::new(number)
        .unwrap_or_else(|err| panic!("a fixture revision must be at least 1: {err}"))
}

/// Returns a digest no other call returns, built from a new UUID.
fn fresh_digest() -> Sha256Digest {
    let id = *Uuid::now_v7().as_bytes();
    let mut bytes = [0; 32];
    bytes[..16].copy_from_slice(&id);
    bytes[16..].copy_from_slice(&id);
    Sha256Digest::from_bytes(bytes)
}

/// The last 8 hex characters of an ID's text, unique enough for names.
fn suffix(id: impl Display) -> String {
    let text = id.to_string();
    text[text.len() - 8..].to_owned()
}

/// Builds a [`ProviderRevision`], or its registry record.
///
/// Defaults: `official/postgresql` revision `2026.10.1` with a new manifest
/// digest, software `postgresql`, capability `persistence`, no extra line
/// compatibility, `schema_version` `stackly.dev/v1`.
#[derive(Debug, Clone)]
pub struct ProviderRevisionBuilder {
    provider: ProviderName,
    revision: RevisionLabel,
    manifest_sha256: Sha256Digest,
    software: SoftwareName,
    capabilities: CapabilitySet,
    line_compatibility: LineCompatibility,
    schema_version: Option<String>,
    publisher: Option<String>,
    approval: Option<String>,
}

impl ProviderRevisionBuilder {
    /// Starts from the defaults.
    #[must_use]
    pub fn new() -> Self {
        Self {
            provider: parse("provider", "official/postgresql"),
            revision: parse("revision label", "2026.10.1"),
            manifest_sha256: fresh_digest(),
            software: parse("software", "postgresql"),
            capabilities: CapabilitySet::new([Capability::Persistence]),
            line_compatibility: LineCompatibility::default(),
            schema_version: Some("stackly.dev/v1".to_owned()),
            publisher: None,
            approval: None,
        }
    }

    /// Sets the provider, such as `official/mysql`.
    #[must_use]
    pub fn provider(mut self, provider: &str) -> Self {
        self.provider = parse("provider", provider);
        self
    }

    /// Sets the revision label.
    #[must_use]
    pub fn revision(mut self, label: &str) -> Self {
        self.revision = parse("revision label", label);
        self
    }

    /// Sets the software.
    #[must_use]
    pub fn software(mut self, software: &str) -> Self {
        self.software = parse("software", software);
        self
    }

    /// Sets the capabilities.
    #[must_use]
    pub fn capabilities(mut self, capabilities: impl IntoIterator<Item = Capability>) -> Self {
        self.capabilities = CapabilitySet::new(capabilities);
        self
    }

    /// Sets the line compatibility.
    #[must_use]
    pub fn line_compatibility(mut self, compat: LineCompatibility) -> Self {
        self.line_compatibility = compat;
        self
    }

    /// Sets the registry columns of the record.
    #[must_use]
    pub fn columns(
        mut self,
        schema_version: Option<&str>,
        publisher: Option<&str>,
        approval: Option<&str>,
    ) -> Self {
        self.schema_version = schema_version.map(str::to_owned);
        self.publisher = publisher.map(str::to_owned);
        self.approval = approval.map(str::to_owned);
        self
    }

    /// Returns the key the revision will have.
    #[must_use]
    pub fn key(&self) -> ProviderRevisionKey {
        ProviderRevisionKey::new(
            self.provider.clone(),
            self.revision.clone(),
            self.manifest_sha256,
        )
    }

    /// Builds the provider revision.
    #[must_use]
    pub fn build(self) -> ProviderRevision {
        ProviderRevision::restore(
            self.key(),
            self.software,
            self.capabilities,
            self.line_compatibility,
        )
    }

    /// Builds the registry record of the provider revision.
    #[must_use]
    pub fn record(self) -> ProviderRevisionRecord {
        let schema_version = self.schema_version.clone();
        let publisher = self.publisher.clone();
        let approval = self.approval.clone();
        ProviderRevisionRecord::new(self.build(), schema_version, publisher, approval)
    }
}

impl Default for ProviderRevisionBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Builds a [`Runtime`].
///
/// Defaults: a new ID, `postgresql` `17.6` line `17` for `macos-aarch64`,
/// managed from a new artifact digest, `resolving`, no dependencies,
/// revision 1. A state that needs `installed_at` gets the fixture time
/// unless one was set.
#[derive(Debug, Clone)]
pub struct RuntimeBuilder {
    id: RuntimeId,
    software: SoftwareName,
    version: ExactVersion,
    line: RuntimeLine,
    target: Target,
    install: InstallMode,
    state: RuntimeState,
    installed_at: Option<Timestamp>,
    last_verified_at: Option<Timestamp>,
    depends_on: BTreeSet<RuntimeId>,
    revision: ResourceRevision,
}

impl RuntimeBuilder {
    /// Starts from the defaults.
    #[must_use]
    pub fn new() -> Self {
        Self {
            id: RuntimeId::generate(),
            software: parse("software", "postgresql"),
            version: parse("version", "17.6"),
            line: parse("line", "17"),
            target: Target::new(Os::Macos, Arch::Aarch64),
            install: InstallMode::Managed {
                artifact_sha256: fresh_digest(),
            },
            state: RuntimeState::Resolving,
            installed_at: None,
            last_verified_at: None,
            depends_on: BTreeSet::new(),
            revision: ResourceRevision::INITIAL,
        }
    }

    /// Returns the ID the runtime will have.
    #[must_use]
    pub const fn id(&self) -> RuntimeId {
        self.id
    }

    /// Sets the ID, to build another revision of a stored runtime.
    #[must_use]
    pub const fn with_id(mut self, id: RuntimeId) -> Self {
        self.id = id;
        self
    }

    /// Sets the software.
    #[must_use]
    pub fn software(mut self, software: &str) -> Self {
        self.software = parse("software", software);
        self
    }

    /// Sets the exact version and its line.
    #[must_use]
    pub fn version(mut self, version: &str, line: &str) -> Self {
        self.version = parse("version", version);
        self.line = parse("line", line);
        self
    }

    /// Sets the target.
    #[must_use]
    pub const fn target(mut self, target: Target) -> Self {
        self.target = target;
        self
    }

    /// Makes it a managed runtime of the artifact with `sha256`.
    #[must_use]
    pub fn managed(mut self, sha256: Sha256Digest) -> Self {
        self.install = InstallMode::Managed {
            artifact_sha256: sha256,
        };
        self
    }

    /// Makes it an external runtime in `root`.
    #[must_use]
    pub fn external(mut self, root: impl Into<PathBuf>) -> Self {
        self.install = InstallMode::External {
            root: root.into(),
            observed_sha256: None,
        };
        self
    }

    /// Sets how it is installed.
    #[must_use]
    pub fn install(mut self, install: InstallMode) -> Self {
        self.install = install;
        self
    }

    /// Adds a runtime it depends on.
    #[must_use]
    pub fn depends_on(mut self, dependency: RuntimeId) -> Self {
        self.depends_on.insert(dependency);
        self
    }

    /// Sets the state.
    #[must_use]
    pub const fn state(mut self, state: RuntimeState) -> Self {
        self.state = state;
        self
    }

    /// Sets when the install was committed.
    #[must_use]
    pub const fn installed_at(mut self, at: Timestamp) -> Self {
        self.installed_at = Some(at);
        self
    }

    /// Sets when verification last passed.
    #[must_use]
    pub const fn last_verified_at(mut self, at: Timestamp) -> Self {
        self.last_verified_at = Some(at);
        self
    }

    /// Sets the revision, at least 1.
    #[must_use]
    pub fn revision(mut self, number: u64) -> Self {
        self.revision = revision(number);
        self
    }

    /// Returns the artifact record a managed runtime needs in the registry
    /// first.
    ///
    /// # Panics
    ///
    /// Panics for an external runtime, which has no artifact.
    #[must_use]
    pub fn artifact(&self) -> ArtifactRecord {
        let InstallMode::Managed { artifact_sha256 } = &self.install else {
            panic!("only a managed runtime has an artifact");
        };
        ArtifactRecord::new(
            *artifact_sha256,
            self.software.clone(),
            self.version.clone(),
            self.target,
            Some("15.0".to_owned()),
            Some("https://example.invalid/postgresql-17.6.tar.gz".to_owned()),
        )
    }

    /// Builds the runtime.
    ///
    /// # Panics
    ///
    /// Panics when the runtime depends on itself.
    #[must_use]
    pub fn build(self) -> Runtime {
        let needs_installed_at = matches!(
            self.state,
            RuntimeState::Installed | RuntimeState::Missing | RuntimeState::Drifted
        );
        let installed_at = self
            .installed_at
            .or(needs_installed_at.then_some(FIXTURE_TIME));
        Runtime::restore(StoredRuntime {
            id: self.id,
            software: self.software,
            version: self.version,
            line: self.line,
            target: self.target,
            install: self.install,
            state: self.state,
            installed_at,
            last_verified_at: self.last_verified_at,
            depends_on: self.depends_on,
            revision: self.revision,
        })
        .unwrap_or_else(|err| panic!("a fixture runtime must be consistent: {err}"))
    }
}

impl Default for RuntimeBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Builds an [`Instance`].
///
/// Defaults: a new ID, the name `instance-<short key>`, the software, line,
/// and version of the runtime, the provider revision given, an IP endpoint
/// `main` (TCP `127.0.0.1`, fixed port 5432) then a Unix endpoint `local`,
/// desired `stopped`, startup `manual`, `uninitialized`, revision 1,
/// created at the fixture time by a new operation ID.
///
/// A lifecycle that needs initialized data gets `data_initialized_at` at
/// the fixture time, and `retained` gets `retained_at` and `retained_by`,
/// unless they were set. The instance is built with readiness and health
/// observed at its `created_at`; the registry replaces both when it reads
/// one back.
#[derive(Debug, Clone)]
pub struct InstanceBuilder {
    stored: StoredInstance,
    name: Option<String>,
}

impl InstanceBuilder {
    /// Starts from the defaults for an instance of `provider` on `runtime`.
    #[must_use]
    pub fn new(runtime: &Runtime, provider: &ProviderRevisionKey) -> Self {
        let main = IpEndpoint::new(
            parse("endpoint name", "main"),
            Transport::Tcp,
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            PortRequest::Fixed(
                Port::new(5432).unwrap_or_else(|err| panic!("5432 is a valid port: {err}")),
            ),
            None,
        );
        let local = UnixEndpoint::new(
            parse("endpoint name", "local"),
            parse("socket name", ".s.PGSQL.5432"),
        );
        Self {
            stored: StoredInstance {
                id: InstanceId::generate(),
                name: parse("instance name", "placeholder"),
                display_name: None,
                provider_revision: provider.clone(),
                software: runtime.software().clone(),
                runtime_id: runtime.id(),
                data_line: runtime.line().clone(),
                data_version: runtime.version().clone(),
                data_initialized_at: None,
                endpoints: vec![Endpoint::Ip(main), Endpoint::Unix(local)],
                desired: DesiredState::Stopped,
                startup: StartupPolicy::Manual,
                lifecycle: InstanceLifecycle::Uninitialized,
                last_failure: None,
                revision: ResourceRevision::INITIAL,
                created_at: FIXTURE_TIME,
                created_by: OperationId::generate(),
                retained_at: None,
                retained_by: None,
            },
            name: None,
        }
    }

    /// Returns the ID the instance will have.
    #[must_use]
    pub const fn id(&self) -> InstanceId {
        self.stored.id
    }

    /// Sets the ID, for example one that shares a short key with another.
    #[must_use]
    pub const fn with_id(mut self, id: InstanceId) -> Self {
        self.stored.id = id;
        self
    }

    /// Sets the name.
    #[must_use]
    pub fn name(mut self, name: &str) -> Self {
        self.name = Some(name.to_owned());
        self
    }

    /// Sets the display name.
    #[must_use]
    pub fn display_name(mut self, name: &str) -> Self {
        self.stored.display_name = Some(parse::<DisplayName>("display name", name));
        self
    }

    /// Sets the software, for example one that differs from the runtime's.
    #[must_use]
    pub fn software(mut self, software: &str) -> Self {
        self.stored.software = parse("software", software);
        self
    }

    /// Sets the runtime.
    #[must_use]
    pub const fn runtime_id(mut self, runtime: RuntimeId) -> Self {
        self.stored.runtime_id = runtime;
        self
    }

    /// Sets the provider revision.
    #[must_use]
    pub fn provider_revision(mut self, key: ProviderRevisionKey) -> Self {
        self.stored.provider_revision = key;
        self
    }

    /// Sets the endpoints, in order.
    #[must_use]
    pub fn endpoints(mut self, endpoints: Vec<Endpoint>) -> Self {
        self.stored.endpoints = endpoints;
        self
    }

    /// Sets the desired state.
    #[must_use]
    pub const fn desired(mut self, desired: DesiredState) -> Self {
        self.stored.desired = desired;
        self
    }

    /// Sets the startup policy.
    #[must_use]
    pub const fn startup(mut self, startup: StartupPolicy) -> Self {
        self.stored.startup = startup;
        self
    }

    /// Sets the lifecycle.
    #[must_use]
    pub const fn lifecycle(mut self, lifecycle: InstanceLifecycle) -> Self {
        self.stored.lifecycle = lifecycle;
        self
    }

    /// Makes the instance retained by `operation` at `at`.
    #[must_use]
    pub const fn retained(mut self, operation: OperationId, at: Timestamp) -> Self {
        self.stored.lifecycle = InstanceLifecycle::Retained;
        self.stored.retained_at = Some(at);
        self.stored.retained_by = Some(operation);
        self
    }

    /// Sets when the data was initialized.
    #[must_use]
    pub const fn data_initialized_at(mut self, at: Timestamp) -> Self {
        self.stored.data_initialized_at = Some(at);
        self
    }

    /// Sets the last failure code.
    #[must_use]
    pub fn last_failure(mut self, code: &str) -> Self {
        self.stored.last_failure = Some(parse::<FailureCode>("failure code", code));
        self
    }

    /// Sets the revision, at least 1.
    #[must_use]
    pub fn revision(mut self, number: u64) -> Self {
        self.stored.revision = revision(number);
        self
    }

    /// Sets when it was created.
    #[must_use]
    pub const fn created_at(mut self, at: Timestamp) -> Self {
        self.stored.created_at = at;
        self
    }

    /// Builds the instance.
    ///
    /// # Panics
    ///
    /// Panics when the fields are inconsistent, which the defaults never
    /// are.
    #[must_use]
    pub fn build(self) -> Instance {
        let mut stored = self.stored;
        stored.name = match &self.name {
            Some(name) => parse("instance name", name),
            None => parse(
                "instance name",
                &format!("instance-{}", stored.id.short_key()),
            ),
        };
        let needs_data = matches!(
            stored.lifecycle,
            InstanceLifecycle::Stopped
                | InstanceLifecycle::Starting
                | InstanceLifecycle::Running
                | InstanceLifecycle::Stopping
                | InstanceLifecycle::Recovering
                | InstanceLifecycle::Unknown
        );
        if needs_data && stored.data_initialized_at.is_none() {
            stored.data_initialized_at = Some(FIXTURE_TIME);
        }
        if stored.lifecycle == InstanceLifecycle::Retained && stored.retained_at.is_none() {
            stored.retained_at = Some(FIXTURE_TIME);
            stored.retained_by = Some(OperationId::generate());
        }
        let loaded_at = stored.created_at;
        Instance::restore(stored, loaded_at)
            .unwrap_or_else(|err| panic!("a fixture instance must be consistent: {err}"))
    }
}

/// Builds an [`Environment`].
///
/// Defaults: a new ID, the name `env-<last 8 hex of the ID>`, no manifest,
/// no members, revision 1.
#[derive(Debug, Clone)]
pub struct EnvironmentBuilder {
    stored: StoredEnvironment,
}

impl EnvironmentBuilder {
    /// Starts from the defaults.
    #[must_use]
    pub fn new() -> Self {
        let id = EnvironmentId::generate();
        Self {
            stored: StoredEnvironment {
                id,
                name: parse::<EnvironmentName>("environment name", &format!("env-{}", suffix(id))),
                manifest_sha256: None,
                members: Vec::new(),
                revision: ResourceRevision::INITIAL,
            },
        }
    }

    /// Returns the ID the environment will have.
    #[must_use]
    pub const fn id(&self) -> EnvironmentId {
        self.stored.id
    }

    /// Sets the ID, to build another revision of a stored environment.
    #[must_use]
    pub const fn with_id(mut self, id: EnvironmentId) -> Self {
        self.stored.id = id;
        self
    }

    /// Sets the name.
    #[must_use]
    pub fn name(mut self, name: &str) -> Self {
        self.stored.name = parse("environment name", name);
        self
    }

    /// Sets the digest of the manifest it came from.
    #[must_use]
    pub const fn manifest_sha256(mut self, sha256: Sha256Digest) -> Self {
        self.stored.manifest_sha256 = Some(sha256);
        self
    }

    /// Adds a member under `alias`.
    #[must_use]
    pub fn member(mut self, alias: &str, instance: InstanceId, ownership: Ownership) -> Self {
        self.stored.members.push(EnvironmentMember::new(
            parse("member alias", alias),
            instance,
            ownership,
        ));
        self
    }

    /// Adds a dedicated member under the alias `member-<n>`.
    #[must_use]
    pub fn dedicated(self, instance: InstanceId) -> Self {
        let alias = format!("member-{}", self.stored.members.len());
        self.member(&alias, instance, Ownership::Dedicated)
    }

    /// Adds a shared member under the alias `member-<n>`.
    #[must_use]
    pub fn shared(self, instance: InstanceId) -> Self {
        let alias = format!("member-{}", self.stored.members.len());
        self.member(&alias, instance, Ownership::Shared)
    }

    /// Removes every member.
    #[must_use]
    pub fn no_members(mut self) -> Self {
        self.stored.members.clear();
        self
    }

    /// Sets the revision, at least 1.
    #[must_use]
    pub fn revision(mut self, number: u64) -> Self {
        self.stored.revision = revision(number);
        self
    }

    /// Builds the environment.
    ///
    /// # Panics
    ///
    /// Panics when two members share an alias or an instance.
    #[must_use]
    pub fn build(self) -> Environment {
        Environment::restore(self.stored)
            .unwrap_or_else(|err| panic!("a fixture environment must be consistent: {err}"))
    }
}

impl Default for EnvironmentBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Builds a [`Lease`].
///
/// Defaults: a new ID, a manual lease without a session, `active`,
/// acquired at the fixture time.
#[derive(Debug, Clone)]
pub struct LeaseBuilder {
    stored: StoredLease,
}

impl LeaseBuilder {
    /// Starts from the defaults for a lease on `instance`.
    #[must_use]
    pub fn new(instance: InstanceId) -> Self {
        Self {
            stored: StoredLease {
                id: LeaseId::generate(),
                instance_id: instance,
                holder: LeaseHolder::Manual,
                session: None,
                state: LeaseState::Active,
                acquired_at: FIXTURE_TIME,
            },
        }
    }

    /// Returns the ID the lease will have.
    #[must_use]
    pub const fn id(&self) -> LeaseId {
        self.stored.id
    }

    /// Sets the ID, to build another state of a stored lease.
    #[must_use]
    pub const fn with_id(mut self, id: LeaseId) -> Self {
        self.stored.id = id;
        self
    }

    /// Makes it a lease of `environment`, without a session.
    #[must_use]
    pub fn environment(mut self, environment: EnvironmentId) -> Self {
        self.stored.holder = LeaseHolder::Environment(environment);
        self.stored.session = None;
        self
    }

    /// Makes it a manual lease, with a session or not.
    #[must_use]
    pub fn manual(mut self, session: Option<&str>) -> Self {
        self.stored.holder = LeaseHolder::Manual;
        self.stored.session = session.map(|text| parse("session ID", text));
        self
    }

    /// Makes it a startup lease of `session`.
    #[must_use]
    pub fn startup(mut self, session: &str) -> Self {
        self.stored.holder = LeaseHolder::Startup;
        self.stored.session = Some(parse("session ID", session));
        self
    }

    /// Makes it released at `at`.
    #[must_use]
    pub const fn released(mut self, at: Timestamp) -> Self {
        self.stored.state = LeaseState::Released { at };
        self
    }

    /// Makes it suppressed at `at`; only a startup lease can be.
    #[must_use]
    pub const fn suppressed(mut self, at: Timestamp) -> Self {
        self.stored.state = LeaseState::Suppressed { at };
        self
    }

    /// Makes it active again.
    #[must_use]
    pub const fn active(mut self) -> Self {
        self.stored.state = LeaseState::Active;
        self
    }

    /// Sets when it was acquired.
    #[must_use]
    pub const fn acquired_at(mut self, at: Timestamp) -> Self {
        self.stored.acquired_at = at;
        self
    }

    /// Builds the lease.
    ///
    /// # Panics
    ///
    /// Panics for a holder and session that do not fit, or a suppressed
    /// lease that is not a startup lease.
    #[must_use]
    pub fn build(self) -> Lease {
        Lease::restore(self.stored)
            .unwrap_or_else(|err| panic!("a fixture lease must be consistent: {err}"))
    }
}

/// Builds an [`Operation`].
///
/// Defaults: a new ID, `create_instance` on the manager, no idempotency
/// key, `queued`, indeterminate progress, created and updated at the
/// fixture time, revision 1. A state past `queued` other than `cancelled`
/// gets `started_at` at `created_at`, and `failed` gets the failure
/// `fixture_failure` unless one was set.
#[derive(Debug, Clone)]
pub struct OperationBuilder {
    stored: StoredOperation,
    updated_at: Option<Timestamp>,
}

impl OperationBuilder {
    /// Starts from the defaults.
    #[must_use]
    pub fn new() -> Self {
        Self {
            stored: StoredOperation {
                id: OperationId::generate(),
                kind: OperationKind::CreateInstance,
                resource: OperationResource::Manager,
                idempotency_key: None,
                state: OperationState::Queued,
                started_at: None,
                stage: None,
                progress: Progress::Indeterminate,
                failure: None,
                recovery_hint: None,
                created_at: FIXTURE_TIME,
                updated_at: FIXTURE_TIME,
                revision: ResourceRevision::INITIAL,
            },
            updated_at: None,
        }
    }

    /// Returns the ID the operation will have.
    #[must_use]
    pub const fn id(&self) -> OperationId {
        self.stored.id
    }

    /// Sets the ID, to build another revision of a stored operation.
    #[must_use]
    pub const fn with_id(mut self, id: OperationId) -> Self {
        self.stored.id = id;
        self
    }

    /// Sets the kind.
    #[must_use]
    pub const fn kind(mut self, kind: OperationKind) -> Self {
        self.stored.kind = kind;
        self
    }

    /// Sets what it works on.
    #[must_use]
    pub const fn resource(mut self, resource: OperationResource) -> Self {
        self.stored.resource = resource;
        self
    }

    /// Sets the idempotency key.
    #[must_use]
    pub fn idempotency_key(mut self, key: &str) -> Self {
        self.stored.idempotency_key = Some(parse::<IdempotencyKey>("idempotency key", key));
        self
    }

    /// Sets the state.
    #[must_use]
    pub const fn state(mut self, state: OperationState) -> Self {
        self.stored.state = state;
        self
    }

    /// Sets the stage.
    #[must_use]
    pub fn stage(mut self, stage: &str) -> Self {
        self.stored.stage = Some(parse::<StageName>("stage name", stage));
        self
    }

    /// Sets the progress.
    #[must_use]
    pub const fn progress(mut self, progress: Progress) -> Self {
        self.stored.progress = progress;
        self
    }

    /// Sets the failure.
    #[must_use]
    pub fn failure(mut self, code: &str, retryable: bool) -> Self {
        self.stored.failure = Some(OperationFailure::new(
            parse("failure code", code),
            retryable,
        ));
        self
    }

    /// Sets the recovery hint, which needs the state `needs_recovery`.
    #[must_use]
    pub const fn recovery_hint(mut self, hint: RecoveryHint) -> Self {
        self.stored.recovery_hint = Some(hint);
        self
    }

    /// Sets when it was created, and when it was updated unless that was
    /// set.
    #[must_use]
    pub const fn created_at(mut self, at: Timestamp) -> Self {
        self.stored.created_at = at;
        self
    }

    /// Sets when it last changed.
    #[must_use]
    pub const fn updated_at(mut self, at: Timestamp) -> Self {
        self.updated_at = Some(at);
        self
    }

    /// Sets the revision, at least 1.
    #[must_use]
    pub fn revision(mut self, number: u64) -> Self {
        self.stored.revision = revision(number);
        self
    }

    /// Builds the operation.
    ///
    /// # Panics
    ///
    /// Panics for counted progress past its total, or a recovery hint
    /// outside `needs_recovery`.
    #[must_use]
    pub fn build(self) -> Operation {
        let mut stored = self.stored;
        stored.updated_at = self.updated_at.unwrap_or(stored.created_at);
        let started = !matches!(
            stored.state,
            OperationState::Queued | OperationState::Cancelled
        );
        if started && stored.started_at.is_none() {
            stored.started_at = Some(stored.created_at);
        }
        if stored.state == OperationState::Failed && stored.failure.is_none() {
            stored.failure = Some(OperationFailure::new(
                parse("failure code", "fixture_failure"),
                false,
            ));
        }
        Operation::restore(stored)
            .unwrap_or_else(|err| panic!("a fixture operation must be consistent: {err}"))
    }
}

impl Default for OperationBuilder {
    fn default() -> Self {
        Self::new()
    }
}
