//! [`SecretService`]: the one use case that stores a value behind a
//! reference, and the reads, probes, reveal, delete, and resolve built on
//! the registry and the secret store (spec 0011).
//!
//! The service is sync, like the ports it calls: the daemon reaches it
//! through `spawn_blocking`. Every store call goes through the
//! [`SecretWorker`](super::SecretWorker) the daemon installs in its
//! `Platform`, and every value the service sees is learned by the
//! [`Redactor`] before it goes anywhere else.

mod access;
mod audit;
mod locks;
mod probe;
mod put;
mod resolve;

use std::fmt;
use std::sync::Arc;

use jiff::Timestamp;
use stackly_domain::{
    AuditActor, ClientCode, OperationId, PlatformStoreKey, Registry, RegistryError,
    ResourceRevision, SecretRef, SecretReferenceView, SecretStatus, SecretUser, Versioned,
};
use stackly_platform::{Interaction, Platform, SecretBackend, SecretStore, StoreState};

use super::error::SecretError;
use super::redact::Redactor;
use super::rules::{SecretFieldRules, UseRules};
use locks::ReferenceLocks;

pub(crate) use access::Removed;
pub use access::RevealPurpose;
pub use probe::{ProbeReport, SecretListing};
pub use put::{FieldContext, PutOutcome, PutRequest, PutTarget, PutWarning, SecretInput};
pub use resolve::LearnReport;

/// What the secret service needs.
#[derive(Clone)]
pub struct SecretDeps {
    /// The registry the reference rows, their uses, and the audit rows are
    /// in.
    pub registry: Arc<dyn Registry>,
    /// The platform ports: the secret store (the worker, in the daemon)
    /// and the clock.
    pub platform: Arc<Platform>,
    /// The daemon's shared redactor, which learns every value the service
    /// sees.
    pub redactor: Arc<Redactor>,
    /// The rules of the secret fields instances use.
    pub fields: Arc<dyn SecretFieldRules>,
    /// Where every committed change of a reference is reported.
    pub events: Arc<dyn SecretEvents>,
}

impl fmt::Debug for SecretDeps {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SecretDeps").finish_non_exhaustive()
    }
}

/// Receives every committed change of a reference, after the service
/// released the reference: the daemon publishes each as `secret.changed`.
pub trait SecretEvents: Send + Sync + 'static {
    /// A reference was stored, observed with another status, restored, or
    /// deleted.
    fn changed(&self, change: &SecretChange);
}

/// The status a [`SecretChange`] reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ChangeStatus {
    /// The store holds a value.
    Present,
    /// The store holds no value.
    Missing,
    /// The store could not be checked.
    Unknown,
    /// The reference row is gone.
    Deleted,
}

impl ChangeStatus {
    /// Returns the stable code, for example `deleted`.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::Present => "present",
            Self::Missing => "missing",
            Self::Unknown => "unknown",
            Self::Deleted => "deleted",
        }
    }
}

impl From<SecretStatus> for ChangeStatus {
    fn from(status: SecretStatus) -> Self {
        match status {
            SecretStatus::Present => Self::Present,
            SecretStatus::Missing => Self::Missing,
            SecretStatus::Unknown => Self::Unknown,
        }
    }
}

/// One committed change of a reference. A change from a probe keeps the
/// revision.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct SecretChange {
    /// The reference.
    pub reference: SecretRef,
    /// Its status now.
    pub status: ChangeStatus,
    /// When the status was observed, or `None` when it never was.
    pub status_observed_at: Option<Timestamp>,
    /// The row's revision; for a delete, the last one it had.
    pub revision: ResourceRevision,
}

/// Who asks the service for something, for the audit rows, and whether
/// the store may show them a dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct SecretCaller {
    /// Whether a store call may show a dialog: only for a user action
    /// from an interactive client (AC-4).
    pub interaction: Interaction,
    /// Who acts: the user for a client request, the daemon for a
    /// background trigger.
    pub actor: AuditActor,
    /// The kind of client, for a client request.
    pub client: Option<ClientCode>,
    /// The user ID of the client's peer, for a client request.
    pub peer_uid: Option<u32>,
    /// The operation the call belongs to, if any.
    pub operation: Option<OperationId>,
}

impl SecretCaller {
    /// A background trigger of the daemon: no dialog ever.
    #[must_use]
    pub const fn background() -> Self {
        Self {
            interaction: Interaction::Never,
            actor: AuditActor::Daemon,
            client: None,
            peer_uid: None,
            operation: None,
        }
    }

    /// A user request from a client. Only an interactive client may see a
    /// dialog.
    #[must_use]
    pub const fn client(client: ClientCode, peer_uid: u32, interactive: bool) -> Self {
        Self {
            interaction: if interactive {
                Interaction::AllowPrompt
            } else {
                Interaction::Never
            },
            actor: AuditActor::User,
            client: Some(client),
            peer_uid: Some(peer_uid),
            operation: None,
        }
    }

    /// Returns the caller as part of `operation`.
    #[must_use]
    pub fn in_operation(mut self, operation: OperationId) -> Self {
        self.operation = Some(operation);
        self
    }
}

/// The store, as the secret commands report it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct StoreStatus {
    /// Which store it is.
    pub backend: SecretBackend,
    /// Its state, from the cache; never a call to the store.
    pub state: StoreState,
}

/// The secret service (spec 0011). See the module documentation.
///
/// `put`, `delete`, and `probe_one` for the same reference never overlap:
/// each holds a lock on the reference in memory while it runs.
pub struct SecretService {
    deps: SecretDeps,
    locks: ReferenceLocks,
}

impl fmt::Debug for SecretService {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SecretService").finish_non_exhaustive()
    }
}

impl SecretService {
    /// Builds the service.
    #[must_use]
    pub fn new(deps: SecretDeps) -> Self {
        Self {
            deps,
            locks: ReferenceLocks::default(),
        }
    }

    /// Returns the store's backend and its cached state.
    #[must_use]
    pub fn store_status(&self) -> StoreStatus {
        StoreStatus {
            backend: self.store().backend(),
            state: self.store().state(),
        }
    }

    fn store(&self) -> &dyn SecretStore {
        self.deps.platform.secrets()
    }

    fn now(&self) -> Timestamp {
        self.deps.platform.clock().now()
    }

    fn registry(&self) -> &dyn Registry {
        self.deps.registry.as_ref()
    }

    /// Unwraps a registry read.
    fn read<T>(result: Result<Versioned<T>, RegistryError>) -> Result<T, SecretError> {
        result.map(|read| read.value).map_err(SecretError::from)
    }

    /// The key the store names the reference by, as the registry stores it.
    fn store_key(&self, reference: &SecretRef) -> Result<PlatformStoreKey, SecretError> {
        PlatformStoreKey::new(self.store().store_key(reference)).map_err(|err| {
            SecretError::Internal {
                reason: format!("the secret store key does not fit the registry: {err}"),
            }
        })
    }

    /// The rules of one use, from its instance and the field rules.
    fn use_rules(
        &self,
        user: SecretUser,
        field: &stackly_domain::SecretFieldName,
    ) -> Result<UseRules, SecretError> {
        match user {
            SecretUser::Environment(_) => Ok(UseRules::ENVIRONMENT),
            SecretUser::Instance(id) => {
                let instance = Self::read(self.registry().instance(id))?;
                Ok(instance.map_or(UseRules::ENVIRONMENT, |instance| {
                    UseRules::of_instance(&instance, self.deps.fields.rules(&instance, field))
                }))
            }
        }
    }

    /// Reports a change, after the reference is released.
    fn report(&self, change: &SecretChange) {
        self.deps.events.changed(change);
    }

    /// The change a stored row reports.
    fn change_of(view: &SecretReferenceView) -> SecretChange {
        SecretChange {
            reference: view.record.reference.clone(),
            status: view.record.status.into(),
            status_observed_at: view.record.status_observed_at,
            revision: view.record.revision,
        }
    }
}
