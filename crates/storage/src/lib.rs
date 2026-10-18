//! The Stackly registry: one SQLite file at `<data>/registry/stackly.sqlite`
//! that only this crate opens, and only inside the daemon (BR-011).
//!
//! [`open_registry`] decides between two outcomes. When the registry is
//! current, or can be created in a clean data root, or migrated after a
//! verified snapshot, it returns [`RegistryOpen::Ready`] with a
//! [`SqliteRegistry`] handle. When the file is not a registry, is damaged,
//! is newer than this app, or a migration fails, it leaves every byte of
//! the registry as it was, writes a [`RecoveryReport`] to
//! `registry/recovery/`, and returns [`RegistryOpen::Recovery`]. A problem
//! with the environment instead (an unusable folder, a pragma that does not
//! apply) is a [`StorageError`], and the daemon exits.
//!
//! Migrations only move forward, all pending ones in one transaction, and
//! only after a `VACUUM INTO` snapshot was verified. Rollback means an older
//! app plus the snapshot that matches its schema version.
//!
//! Every write connection runs with `foreign_keys=ON`, `journal_mode=WAL`,
//! `synchronous=FULL`, and `fullfsync=ON`; nothing may weaken that, not even
//! for a demo or a benchmark. All times come from the platform `Clock`, as
//! 30 character UTC text, never from SQLite.

mod connection;
mod error;
mod files;
mod fingerprint;
mod migration;
mod open;
mod recovery;
mod registry;
mod snapshot;
mod time;

pub use error::StorageError;
pub use migration::{Migration, MigrationError, MigrationSet};
pub use open::{APPLICATION_ID, OpenOptions, OpenSummary, RegistryOpen, open_registry};
pub use recovery::{
    RECOVERY_REPORT_SCHEMA, RecoveryReason, RecoveryReport, ReportFile, ReportSnapshot,
};
pub use registry::{REGISTRY_QUEUE_CAPACITY, SqliteRegistry};
