//! The secret service's place in the daemon (spec 0011 build step 5): the
//! start slot builds it once the registry is ready, every `secret.*`
//! handler reaches it through [`Secrets::service`], and each committed
//! change of a reference is published as `secret.changed`.
//!
//! The one [`Redactor`] of the run lives in [`DaemonCore`](crate::state::DaemonCore)
//! and is handed in here, so a value the service learns is hidden by the
//! log writer, the error bodies, and the events at once.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use stackly_core::PreparedLayout;
use stackly_core::provider::{NameCheck, ValidatedPackage, read_package};
use stackly_core::secret::{
    ChangeStatus, ProbeReport, Redactor, SecretChange, SecretDeps, SecretEvents, SecretFieldRule,
    SecretFieldRules, SecretService, StoreStatus,
};
use stackly_domain::{Instance, ProviderRevisionKey, Registry, SecretFieldName};
use stackly_platform::{Platform, SecretBackend, StoreState};
use stackly_protocol::codes::registry;
use stackly_protocol::events::EventKind;
use stackly_protocol::secret::{
    SecretChangeStatus, SecretChanged, SecretStoreBackend, SecretStoreState, SecretStoreStatus,
    SecretStoreTarget,
};
use stackly_protocol::{ErrorBody, ResourceKind, ResourceRef, Timestamp};
use stackly_storage::SqliteRegistry;

use crate::events::EventBus;
use crate::ipc::operation::{dto, starting};

/// Where the service stands in this daemon run.
#[derive(Clone)]
enum Slot {
    /// The registry is not open yet.
    Waiting,
    /// The service exists.
    Ready(Arc<SecretService>),
    /// No service for this run: the registry is in recovery mode or broke.
    Unavailable,
}

/// The daemon's secret service and the facts `daemon.get_info` reports
/// about its store.
pub struct Secrets {
    slot: Mutex<Slot>,
    target: Option<SecretStoreTarget>,
}

impl std::fmt::Debug for Secrets {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Secrets")
            .field("target", &self.target)
            .finish_non_exhaustive()
    }
}

impl Secrets {
    /// A holder for a run whose store uses `target`, or no keychain.
    #[must_use]
    pub fn new(target: Option<SecretStoreTarget>) -> Self {
        Self {
            slot: Mutex::new(Slot::Waiting),
            target,
        }
    }

    /// Builds the service on the ready registry. Changes are published on
    /// `bus`, and every value it sees is learned by `redactor`.
    #[must_use]
    pub fn build(
        &self,
        registry: SqliteRegistry,
        platform: &Platform,
        prepared: &PreparedLayout,
        redactor: Arc<Redactor>,
        bus: &Arc<EventBus>,
    ) -> Arc<SecretService> {
        let service = Arc::new(SecretService::new(SecretDeps {
            registry: Arc::new(registry) as Arc<dyn Registry>,
            platform: Arc::new(platform.clone()),
            redactor,
            fields: Arc::new(PackageFieldRules::new(platform.clone(), prepared.clone())),
            events: Arc::new(BusSecretEvents::new(Arc::clone(bus))),
        }));
        *self.lock() = Slot::Ready(Arc::clone(&service));
        service
    }

    /// There is no service in this run: the registry is in recovery mode
    /// or stopped working.
    pub fn unavailable(&self) {
        *self.lock() = Slot::Unavailable;
    }

    /// Lets go of the service at the end of the stop, so the registry
    /// handle it holds closes.
    pub fn close(&self) {
        *self.lock() = Slot::Unavailable;
    }

    /// The service, once it exists.
    ///
    /// # Errors
    ///
    /// Returns `daemon.starting` (subsystem `registry`) while the registry
    /// opens, and `registry.unavailable` when this run has no service.
    pub fn service(&self) -> Result<Arc<SecretService>, Box<ErrorBody>> {
        match &*self.lock() {
            Slot::Ready(service) => Ok(Arc::clone(service)),
            Slot::Waiting => Err(Box::new(starting(
                stackly_protocol::daemon::subsystem::REGISTRY,
            ))),
            Slot::Unavailable => Err(Box::new(ErrorBody::new(
                &registry::UNAVAILABLE,
                "the registry is not available",
            ))),
        }
    }

    /// The store as `daemon.get_info` reports it: its backend and cached
    /// state, and the keychain chosen at start. It never calls the store.
    #[must_use]
    pub fn store(&self, platform: &Platform) -> SecretStoreStatus {
        let store = platform.secrets();
        status_of(store.backend(), store.state(), self.target)
    }

    /// The target the daemon chose at start.
    #[must_use]
    pub const fn target(&self) -> Option<SecretStoreTarget> {
        self.target
    }

    fn lock(&self) -> MutexGuard<'_, Slot> {
        self.slot.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// A store status on the wire.
#[must_use]
pub fn store_status(status: StoreStatus, target: Option<SecretStoreTarget>) -> SecretStoreStatus {
    status_of(status.backend, status.state, target)
}

fn status_of(
    backend: SecretBackend,
    state: StoreState,
    target: Option<SecretStoreTarget>,
) -> SecretStoreStatus {
    SecretStoreStatus {
        backend: match backend {
            SecretBackend::MacosKeychain => SecretStoreBackend::MacosKeychain,
            SecretBackend::Fake => SecretStoreBackend::Fake,
            SecretBackend::Unsupported => SecretStoreBackend::Unsupported,
        },
        state: match state {
            StoreState::Available => SecretStoreState::Available,
            StoreState::Locked => SecretStoreState::Locked,
            StoreState::Unsupported => SecretStoreState::Unsupported,
            StoreState::Unknown => SecretStoreState::Unknown,
        },
        target,
    }
}

/// Probes every registered reference once, as the daemon does when it
/// starts, and logs the count per status. A failure is logged and changes
/// nothing: the cached status stays as the last run left it.
pub async fn probe_all(service: Arc<SecretService>) -> Option<ProbeReport> {
    match tokio::task::spawn_blocking(move || service.probe_all()).await {
        Ok(Ok(report)) => Some(report),
        Ok(Err(error)) => {
            tracing::warn!(error = %error, "cannot probe the secret references at start");
            None
        }
        Err(error) => {
            tracing::error!(error = %error, "the secret probe task failed");
            None
        }
    }
}

/// Publishes each committed change of a reference as `secret.changed`:
/// the resource is the reference and the revision the row's, or its last
/// one plus 1 for a delete.
#[derive(Debug, Clone)]
pub struct BusSecretEvents {
    bus: Arc<EventBus>,
}

impl BusSecretEvents {
    /// Publishes on `bus`.
    #[must_use]
    pub const fn new(bus: Arc<EventBus>) -> Self {
        Self { bus }
    }
}

impl SecretEvents for BusSecretEvents {
    fn changed(&self, change: &SecretChange) {
        let status = match change.status {
            ChangeStatus::Present => SecretChangeStatus::Present,
            ChangeStatus::Missing => SecretChangeStatus::Missing,
            ChangeStatus::Deleted => SecretChangeStatus::Deleted,
            _ => SecretChangeStatus::Unknown,
        };
        let revision = if status == SecretChangeStatus::Deleted {
            change.revision.next()
        } else {
            change.revision
        };
        self.bus.publish_resource(
            EventKind::SecretChanged(SecretChanged {
                status,
                status_observed_at: change.status_observed_at.map(Timestamp::from_jiff),
            }),
            ResourceRef {
                kind: ResourceKind::Secret,
                id: change.reference.to_string(),
            },
            dto::revision(revision),
        );
    }
}

/// The rules of a secret field of an instance, read from the pinned
/// package of its provider revision (spec 0009, `x-stackly` rules of the
/// settings schema). A package is read once; one that cannot be read or
/// does not name the field gives no rules, so `put` then checks no bounds.
struct PackageFieldRules {
    platform: Platform,
    prepared: PreparedLayout,
    packages: Mutex<HashMap<ProviderRevisionKey, Option<Arc<ValidatedPackage>>>>,
}

impl PackageFieldRules {
    fn new(platform: Platform, prepared: PreparedLayout) -> Self {
        Self {
            platform,
            prepared,
            packages: Mutex::new(HashMap::new()),
        }
    }

    fn package(&self, key: &ProviderRevisionKey) -> Option<Arc<ValidatedPackage>> {
        let mut packages = self.packages.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(found) = packages.get(key) {
            return found.clone();
        }
        let dir = self.prepared.layout().provider_revision_dir(key);
        let package = read_package(self.platform.fs(), &dir)
            .ok()
            .and_then(|files| ValidatedPackage::from_files(files, NameCheck::Skip).ok())
            .filter(|package| package.key() == key)
            .map(Arc::new);
        packages.insert(key.clone(), package.clone());
        package
    }
}

impl SecretFieldRules for PackageFieldRules {
    fn rules(&self, instance: &Instance, field: &SecretFieldName) -> Option<SecretFieldRule> {
        let package = self.package(instance.provider_revision())?;
        package
            .settings_schema()
            .field(field.as_str())
            .and_then(SecretFieldRule::from_setting)
    }
}
