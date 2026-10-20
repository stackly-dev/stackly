//! The bundled provider catalog (spec 0017): the `catalog.json` index that
//! `cargo xtask catalog build` writes, the rules the build and the daemon
//! share, and the read model the daemon loads once at start.
//!
//! Reading a catalog never writes a file, runs a process, touches the
//! network, or resolves a secret (REQ-033).

mod assemble;
mod index;
mod install;
mod load;
mod model;
mod rules;
mod status;
#[cfg(feature = "test-support")]
pub mod testing;

pub use assemble::{
    AssembleOptions, AssembledCatalog, SourceNote, SourcePlanned, SourceProvider, SourceSpec,
    assemble, expected_content_sha256,
};

pub use index::{
    CATALOG_INDEX_FILE, CATALOG_PACKAGES_DIR, CATALOG_SCHEMA, CatalogIndex, IndexError,
    IndexPlanned, IndexPlatformNote, IndexProvider, content_sha256, package_dir, provider_entry,
    versions_sha256,
};
pub use install::{Installability, NotInstallableReason, VersionNotFound, installability};
pub use load::load_catalog;
pub use model::{
    CATALOG_OUTDATED_AFTER, Catalog, CatalogEntry, CatalogLocation, CatalogMeta, CatalogOptions,
    CatalogState, InvalidEntry, PlannedEntry, PlatformNote, ProviderEntry, TileHint,
    UnavailableReason,
};
pub use rules::{
    ARTIFACT_ID_MAX_LEN, ARTIFACTS_MAX, BLOCKED_REASON_MAX_LEN, DEPENDENCIES_MAX, INDEX_MAX_LEN,
    LICENSE_MAX_LEN, LICENSE_URL_MAX_LEN, LimitKind, LimitViolation, NOTE_MAX_LEN,
    OFFICIAL_NAMESPACE, PACKAGE_BYTES_MAX, PLANNED_MAX, PLATFORM_NOTES_MAX, PROVIDERS_MAX,
    SUMMARY_VERSIONS_MAX, TESTING_NAMESPACE, TileColor, category_code, category_from_code,
    check_note, default_mark, index_problems, is_mark, namespace_allowed, package_limits,
    platform_note_problems,
};
pub use status::{
    CellStatus, EntryState, EntryStatus, MATRIX_OSES, SupportReason, TargetSupport, entry_status,
    host_cell_status, version_support,
};
