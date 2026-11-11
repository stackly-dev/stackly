//! The bundled provider catalog (spec 0017): the `catalog.json` index that
//! `cargo xtask catalog build` writes, the rules the build and the daemon
//! share, and the read model the daemon loads once at start.
//!
//! Reading a catalog never writes a file, runs a process, touches the
//! network, or resolves a secret (REQ-033).

mod assemble;
pub mod import;
mod index;
mod install;
mod load;
pub mod local;
mod model;
mod pin;
mod rules;
mod source;
mod status;
mod store;
#[cfg(feature = "test-support")]
pub mod testing;
mod trust;

pub use assemble::{
    AssembleOptions, AssembledCatalog, SourceNote, SourcePlanned, SourceProvider, SourceSpec,
    assemble, expected_content_sha256,
};

pub use index::{
    CATALOG_INDEX_FILE, CATALOG_PACKAGES_DIR, CATALOG_SCHEMA, CatalogIndex, IndexError,
    IndexPlanned, IndexPlatformNote, IndexProvider, IndexRevoked, content_sha256, package_dir,
    provider_entry, versions_sha256,
};
pub use install::{
    Installability, NotInstallableReason, VersionNotFound, installability, provider_installability,
};
pub use load::load_catalog;
pub use model::{
    CATALOG_OUTDATED_AFTER, Catalog, CatalogEntry, CatalogLocation, CatalogMeta, CatalogOptions,
    CatalogState, EntryOrigin, InvalidEntry, LocalInfo, PlannedEntry, PlatformNote, ProviderEntry,
    Revocation, TileHint, UnavailableReason,
};
pub use pin::{PinError, PinMeta, PinnedRevision, pin_revision, pin_revision_with};
pub use rules::{
    ARTIFACT_ID_MAX_LEN, ARTIFACTS_MAX, BLOCKED_REASON_MAX_LEN, DEPENDENCIES_MAX, INDEX_MAX_LEN,
    LICENSE_MAX_LEN, LICENSE_URL_MAX_LEN, LimitKind, LimitViolation, NOTE_MAX_LEN,
    OFFICIAL_NAMESPACE, PACKAGE_BYTES_MAX, PLANNED_MAX, PLATFORM_NOTES_MAX, PROVIDERS_MAX,
    REVOKED_MAX, REVOKED_REASON_MAX_LEN, SUMMARY_VERSIONS_MAX, TESTING_NAMESPACE, TileColor,
    category_code, category_from_code, check_note, default_mark, index_problems, is_mark,
    namespace_allowed, package_limits, platform_note_problems, revoked_covers, revoked_problems,
    revoked_shipped_problems,
};
pub use source::{BUNDLED_LOCATION, BUNDLED_TRUST_ROOT, ensure_source};
pub use status::{
    CellStatus, EntryState, EntryStatus, MATRIX_OSES, SupportReason, TargetSupport, entry_status,
    version_support,
};
pub use store::{StoreError, StoreMode, Stored, write_package};
pub use trust::{
    RevokedBy, RevokedInfo, TrustVerdict, catalog_revocation, entry_trust, guard_pinned_trust,
    guard_trust, pinned_trust, record_trust, require_entry_trust, stored_trust,
};
