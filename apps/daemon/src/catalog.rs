//! The catalog subsystem (spec 0017, *Read model di daemon*, *Kesehatan
//! subsystem*, *Lokasi catalog saat start*): the catalog the start slot
//! loads once, its health, and the two `catalog` methods.
//!
//! The catalog never changes while the daemon runs. Loading it reads files
//! only; it never writes, spawns, or resolves a secret.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use stackly_core::catalog::{
    Catalog, CatalogLocation, CatalogMeta, CatalogOptions, CatalogState, ensure_source,
    load_catalog,
};
use stackly_core::instance::create::SourceRecorder;
use stackly_domain::{ProviderName, ProviderSourceId, Registry, RegistryError};
use stackly_platform::Clock;
use stackly_protocol::ErrorBody;
use stackly_protocol::catalog::{GetProviderParams, ListProvidersReply, ProviderDetail};
use stackly_protocol::codes::daemon;
use stackly_protocol::daemon::{SubsystemHealth, SubsystemStatus, subsystem};
use stackly_storage::SqliteRegistry;

use crate::dispatch::HandlerFuture;
use crate::map;
use crate::state::DaemonCore;

/// The health detail of a daemon without a catalog folder.
pub const NOT_CONFIGURED_DETAIL: &str =
    "no catalog: not inside an app bundle and STACKLY_CATALOG_DIR is unset";

/// The catalog an in process test gives the daemon instead of the one its
/// start configuration names (`DaemonConfig.catalog`).
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum CatalogOverride {
    /// Loads this folder with these options; `None` means no catalog.
    Location {
        /// The folder, or `None` for `NotConfigured`.
        location: Option<CatalogLocation>,
        /// The loader options, such as accepting `testing` packages.
        options: CatalogOptions,
    },
    /// Uses this catalog as if the start slot had loaded it.
    Preloaded(Catalog),
}

/// Loads the catalog for the start slot: the override when a test set one,
/// otherwise `location` (`STACKLY_CATALOG_DIR`, or the app bundle's catalog
/// folder, see `startup::catalog_location`) with the default options, and
/// `NotConfigured` without either.
pub async fn load(
    core: &DaemonCore,
    location: Option<CatalogLocation>,
    over: Option<CatalogOverride>,
) -> CatalogState {
    let (location, options) = match over {
        Some(CatalogOverride::Preloaded(catalog)) => {
            return CatalogState::Loaded(Arc::new(catalog));
        }
        Some(CatalogOverride::Location { location, options }) => (location, options),
        None => (location, CatalogOptions::default()),
    };
    let Some(location) = location else {
        return CatalogState::NotConfigured;
    };
    let platform = core.platform().clone();
    let loaded = tokio::task::spawn_blocking(move || {
        load_catalog(platform.fs(), platform.clock(), location, options)
    })
    .await;
    loaded.unwrap_or_else(|failure| {
        tracing::error!(panicked = failure.is_panic(), "the catalog load failed");
        CatalogState::Unavailable {
            reason: stackly_core::catalog::UnavailableReason::IndexInvalid,
            detail: "loading the catalog failed inside the daemon".to_owned(),
        }
    })
}

/// Logs what the start slot loaded, with the field `catalog_state`.
pub fn log_loaded(state: &CatalogState) {
    match state {
        CatalogState::NotConfigured => tracing::warn!(
            catalog_state = "not_configured",
            "no provider catalog: STACKLY_CATALOG_DIR is unset"
        ),
        CatalogState::Unavailable { reason, detail } => tracing::error!(
            catalog_state = "unavailable",
            reason = reason.as_code(),
            detail = detail.as_str(),
            "the provider catalog cannot be used",
        ),
        CatalogState::Loaded(catalog) => {
            let meta = catalog.meta();
            tracing::info!(
                catalog_state = "loaded",
                revision = meta.revision,
                content_sha256 = %meta.content_sha256,
                entries = catalog.entries().len(),
                invalid = catalog.invalid_count(),
                path = %meta.location.path().display(),
                "provider catalog loaded",
            );
        }
    }
}

/// The row of the loaded catalog in `provider_sources` (spec 0017 K-2,
/// spec 0020 AC-14): the ID the start slot recorded, or why it could not.
///
/// A catalog that is loaded gets exactly one row; a restart finds the same
/// one. A failed write shows as a `degraded` part of the `catalog`
/// subsystem and is tried once more right before the first pin.
#[derive(Debug, Default)]
pub struct CatalogSource {
    state: Mutex<SourceState>,
}

#[derive(Debug, Default)]
struct SourceState {
    id: Option<ProviderSourceId>,
    error: Option<String>,
}

impl CatalogSource {
    fn lock(&self) -> MutexGuard<'_, SourceState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The ID of the recorded row, once there is one.
    #[must_use]
    pub fn id(&self) -> Option<ProviderSourceId> {
        self.lock().id
    }

    /// Why the row could not be recorded, in the words of the `degraded`
    /// part: `catalog source not recorded: <code>`.
    #[must_use]
    pub fn degraded(&self) -> Option<String> {
        self.lock().error.clone()
    }

    /// Writes the row of `meta` unless it is there, and remembers the ID or
    /// the reason it could not.
    ///
    /// # Errors
    ///
    /// Returns the registry error when the row cannot be read or written.
    pub fn record(
        &self,
        registry: &dyn Registry,
        clock: &dyn Clock,
        meta: &CatalogMeta,
    ) -> Result<ProviderSourceId, RegistryError> {
        if let Some(id) = self.id() {
            return Ok(id);
        }
        match ensure_source(registry, clock, meta) {
            Ok(id) => {
                let mut state = self.lock();
                state.id = Some(id);
                state.error = None;
                Ok(id)
            }
            Err(error) => {
                tracing::warn!(error = %error, "cannot record the catalog source");
                self.lock().error = Some(format!(
                    "catalog source not recorded: {}",
                    crate::map::registry_error(&error).code()
                ));
                Err(error)
            }
        }
    }
}

/// Records the source of the loaded catalog for the start slot: nothing for
/// a daemon without a catalog.
pub async fn record_source(core: &DaemonCore, source: &Arc<CatalogSource>) {
    let (Some(registry), Some(state)) = (core.registry(), core.catalog()) else {
        return;
    };
    let CatalogState::Loaded(catalog) = state.as_ref() else {
        return;
    };
    let catalog = Arc::clone(catalog);
    let source = Arc::clone(source);
    let clock = core.platform().shared_clock();
    let recorded = tokio::task::spawn_blocking(move || {
        source.record(&registry, &*clock, catalog.meta()).map(drop)
    })
    .await;
    if recorded.is_err() {
        tracing::error!("the catalog source task failed");
    }
}

/// The recorder the create work calls right before its first pin: the same
/// row, written again when the start slot could not write it.
pub struct SourceHandle {
    source: Arc<CatalogSource>,
    registry: SqliteRegistry,
    clock: Arc<dyn Clock>,
    catalog: CatalogState,
}

impl SourceHandle {
    /// A recorder for the catalog `catalog` of this run.
    #[must_use]
    pub fn new(
        source: Arc<CatalogSource>,
        registry: SqliteRegistry,
        clock: Arc<dyn Clock>,
        catalog: CatalogState,
    ) -> Self {
        Self {
            source,
            registry,
            clock,
            catalog,
        }
    }
}

impl SourceRecorder for SourceHandle {
    fn ensure(&self) -> Result<ProviderSourceId, RegistryError> {
        match &self.catalog {
            CatalogState::Loaded(catalog) => {
                self.source
                    .record(&self.registry, &*self.clock, catalog.meta())
            }
            _ => Err(RegistryError::Unavailable),
        }
    }
}

/// The `catalog` subsystem health: `unavailable` while loading, without a
/// catalog, or with an unusable one; `degraded` while an entry failed
/// validation or the source row could not be recorded; `ok` otherwise.
#[must_use]
pub fn health(state: Option<&CatalogState>, source_problem: Option<&str>) -> SubsystemHealth {
    let (status, detail) = match state {
        None => (
            SubsystemStatus::Unavailable,
            Some("the catalog is loading".to_owned()),
        ),
        Some(CatalogState::NotConfigured) => (
            SubsystemStatus::Unavailable,
            Some(NOT_CONFIGURED_DETAIL.to_owned()),
        ),
        Some(CatalogState::Unavailable { reason, detail }) => (
            SubsystemStatus::Unavailable,
            Some(format!("catalog {}: {detail}", reason.as_code())),
        ),
        Some(CatalogState::Loaded(catalog)) => {
            let mut parts = Vec::new();
            if catalog.invalid_count() > 0 {
                parts.push(format!(
                    "{} catalog providers failed validation",
                    catalog.invalid_count()
                ));
            }
            parts.extend(source_problem.map(str::to_owned));
            if parts.is_empty() {
                (SubsystemStatus::Ok, None)
            } else {
                (SubsystemStatus::Degraded, Some(parts.join("; ")))
            }
        }
    };
    SubsystemHealth {
        name: subsystem::CATALOG.to_owned(),
        status,
        detail,
    }
}

/// The loaded catalog, or the error a catalog method answers without one.
pub(crate) fn loaded(core: &DaemonCore) -> Result<Arc<Catalog>, Box<ErrorBody>> {
    match core.catalog().as_deref() {
        None => Err(Box::new(
            ErrorBody::new(&daemon::STARTING, "the provider catalog is still loading")
                .with_detail("subsystem", subsystem::CATALOG),
        )),
        Some(CatalogState::NotConfigured) => Err(Box::new(map::catalog::catalog_not_configured())),
        Some(CatalogState::Unavailable { reason, .. }) => {
            Err(Box::new(map::catalog::catalog_unavailable(*reason)))
        }
        Some(CatalogState::Loaded(catalog)) => Ok(Arc::clone(catalog)),
    }
}

/// The real `catalog.list_providers`.
pub fn list_providers(core: &DaemonCore) -> HandlerFuture<'_, ListProvidersReply> {
    Box::pin(async move {
        let catalog = loaded(core)?;
        let platform = core.platform();
        let outdated = catalog.outdated(platform.clock().now());
        Ok(map::catalog::list_providers(
            &catalog,
            &platform.info().host_facts(),
            outdated,
        ))
    })
}

/// The real `catalog.get_provider`. A name without its namespace never
/// matches an entry; when it names exactly one entry, the error suggests
/// that entry's full ID.
pub fn get_provider(
    core: &DaemonCore,
    params: GetProviderParams,
) -> HandlerFuture<'_, Box<ProviderDetail>> {
    Box::pin(async move {
        let catalog = loaded(core)?;
        let asked = params.provider.as_str();
        let entry = asked
            .parse::<ProviderName>()
            .ok()
            .and_then(|id| catalog.entry(&id));
        match entry {
            Some(entry) => Ok(Box::new(map::catalog::provider_detail(
                &catalog,
                entry,
                &core.platform().info().host_facts(),
            ))),
            None => {
                let matches = if asked.contains('/') {
                    Vec::new()
                } else {
                    catalog.find_by_name(asked)
                };
                let suggestion = match matches.as_slice() {
                    [only] => Some(*only),
                    _ => None,
                };
                Err(Box::new(map::catalog::provider_not_found(
                    asked, suggestion,
                )))
            }
        }
    })
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use stackly_core::prepare_layout;
    use stackly_platform_macos::MacosPlatform;
    use stackly_storage::{OpenOptions, RegistryOpen};
    use stackly_testkit::TestRoot;

    use super::*;

    fn loaded() -> CatalogState {
        CatalogState::Loaded(Arc::new(Catalog::for_tests(&[])))
    }

    #[test]
    fn a_source_that_could_not_be_recorded_degrades_the_catalog_subsystem() {
        let clean = health(Some(&loaded()), None);
        assert_eq!(clean.status, SubsystemStatus::Ok);
        let problem = "catalog source not recorded: registry.busy";

        let degraded = health(Some(&loaded()), Some(problem));

        assert_eq!(degraded.status, SubsystemStatus::Degraded);
        assert_eq!(degraded.detail.as_deref(), Some(problem));
        // A catalog that is not usable stays unavailable whatever the source says.
        let missing = health(Some(&CatalogState::NotConfigured), Some(problem));
        assert_eq!(missing.status, SubsystemStatus::Unavailable);
    }

    #[test]
    fn the_source_row_is_written_once_and_found_again() {
        let root = TestRoot::new().expect("a test root");
        let platform = MacosPlatform::with_roots(root.roots()).expect("this Mac reports its OS");
        let prepared = prepare_layout(&platform).expect("the layout prepares");
        let RegistryOpen::Ready { registry, .. } = OpenOptions::new()
            .open(&platform, prepared.layout(), "0.1.0")
            .expect("a fresh registry opens")
        else {
            panic!("a fresh registry opens ready");
        };
        let catalog = Catalog::for_tests(&[]);
        let source = CatalogSource::default();
        assert_eq!(source.id(), None);

        let first = source
            .record(&registry, platform.clock(), catalog.meta())
            .expect("the row is written");
        let again = CatalogSource::default()
            .record(&registry, platform.clock(), catalog.meta())
            .expect("the row is found");

        assert_eq!(first, again, "a second holder finds the same row");
        assert_eq!(source.id(), Some(first));
        assert_eq!(source.degraded(), None);
    }
}
