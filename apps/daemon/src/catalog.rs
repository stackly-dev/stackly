//! The catalog subsystem (spec 0017, *Read model di daemon*, *Kesehatan
//! subsystem*, *Lokasi catalog saat start*): the catalog the start slot
//! loads once, its health, and the two `catalog` methods.
//!
//! The catalog never changes while the daemon runs. Loading it reads files
//! only; it never writes, spawns, or resolves a secret.

use std::sync::Arc;

use stackly_core::catalog::{Catalog, CatalogLocation, CatalogOptions, CatalogState, load_catalog};
use stackly_domain::ProviderName;
use stackly_protocol::ErrorBody;
use stackly_protocol::catalog::{GetProviderParams, ListProvidersReply, ProviderDetail};
use stackly_protocol::codes::daemon;
use stackly_protocol::daemon::{SubsystemHealth, SubsystemStatus, subsystem};

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
/// otherwise `location` (from `STACKLY_CATALOG_DIR`) with the default
/// options, and `NotConfigured` without either.
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
        // provisional: F17 step 11 (the bundled catalog of `DaemonDeps.bundle`)
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

/// The `catalog` subsystem health: `unavailable` while loading, without a
/// catalog, or with an unusable one; `degraded` while an entry failed
/// validation; `ok` otherwise.
#[must_use]
pub fn health(state: Option<&CatalogState>) -> SubsystemHealth {
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
        Some(CatalogState::Loaded(catalog)) => match catalog.invalid_count() {
            0 => (SubsystemStatus::Ok, None),
            invalid => (
                SubsystemStatus::Degraded,
                Some(format!("{invalid} catalog providers failed validation")),
            ),
        },
    };
    SubsystemHealth {
        name: subsystem::CATALOG.to_owned(),
        status,
        detail,
    }
}

/// The loaded catalog, or the error a catalog method answers without one.
fn loaded(core: &DaemonCore) -> Result<Arc<Catalog>, Box<ErrorBody>> {
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
