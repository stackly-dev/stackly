//! Seeding the registry tables that no `WriteOp` covers yet, for tests.

use jiff::Timestamp;
use stackly_domain::{BackupId, InstanceId, RuntimeId};
use uuid::Uuid;

/// Writes rows into registry tables that have no
/// [`WriteOp`](stackly_domain::WriteOp) yet, so tests can set up what reads
/// and prune see. Seeding never raises the generation.
///
/// [`FakeRegistry`](crate::FakeRegistry) implements it in memory.
/// `stackly-storage` tests implement it for a wrapper that writes through a
/// second rusqlite connection to the same registry file.
///
/// Only tests use it, so each method panics when the row cannot be written,
/// for example when it refers to a runtime or instance that does not
/// exist.
pub trait RegistrySeed {
    /// Writes one `backups` row.
    fn seed_backup(&self, backup: BackupSeed);

    /// Writes `count` `audit_events` rows, each with a new ID and
    /// `occurred_at` set to `at`.
    fn seed_audit_events(&self, count: u32, at: Timestamp);

    /// Writes `count` `process_observations` rows for `instance`, each
    /// observed at `at`.
    fn seed_observations(&self, instance: InstanceId, count: u32, at: Timestamp);

    /// Writes one `secret_references` row. It holds a reference, never a
    /// secret value (BR-018).
    fn seed_secret_reference(&self, reference: SecretReferenceSeed);
}

/// One `backups` row to seed (the table belongs to feature 49).
///
/// A seeding implementation fills the remaining required columns
/// (`runtime_identity`, `method`, `scope`, `consistency`, `path`) with
/// fixed valid values.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct BackupSeed {
    /// The backup's ID.
    pub id: BackupId,
    /// The instance it backs up, if it still exists.
    pub instance_id: Option<InstanceId>,
    /// The runtime a restore needs, which keeps that runtime referenced
    /// (BR-007).
    pub runtime_id: Option<RuntimeId>,
    /// When it was made.
    pub created_at: Timestamp,
}

impl BackupSeed {
    /// A backup with a new ID made at `created_at`, tied to no instance or
    /// runtime yet.
    #[must_use]
    pub fn new(created_at: Timestamp) -> Self {
        Self {
            id: BackupId::generate(),
            instance_id: None,
            runtime_id: None,
            created_at,
        }
    }

    /// Returns the backup tied to `instance`.
    #[must_use]
    pub const fn with_instance(mut self, instance: InstanceId) -> Self {
        self.instance_id = Some(instance);
        self
    }

    /// Returns the backup that needs `runtime` to restore.
    #[must_use]
    pub const fn with_runtime(mut self, runtime: RuntimeId) -> Self {
        self.runtime_id = Some(runtime);
        self
    }
}

/// One `secret_references` row to seed: a reference into the platform
/// secret store, never the secret itself (spec 0011 owns the format).
///
/// A seeding implementation fills the remaining required columns with
/// fixed valid values.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct SecretReferenceSeed {
    /// The reference's ID, a UUID version 7.
    pub id: Uuid,
    /// The opaque key of the entry in the platform secret store.
    pub store_key: String,
    /// What the secret is for, a snake_case code.
    pub purpose: String,
    /// When the reference was created.
    pub created_at: Timestamp,
}

impl SecretReferenceSeed {
    /// A reference with a new ID to the store entry `store_key`, purpose
    /// `password`, created at `created_at`.
    #[must_use]
    pub fn new(store_key: impl Into<String>, created_at: Timestamp) -> Self {
        Self {
            id: Uuid::now_v7(),
            store_key: store_key.into(),
            purpose: "password".to_owned(),
            created_at,
        }
    }
}
