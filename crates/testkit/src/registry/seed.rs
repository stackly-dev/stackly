//! Seeding the registry tables that no `WriteOp` covers yet, for tests.

use jiff::Timestamp;
use stackly_domain::{
    BackupId, InstanceId, Pid, ProcessIdentity, ProcessObservation, ProcessStart, RuntimeId,
};

/// The process ID and session ID of a seeded observation.
const SEEDED_PID: u32 = 4242;
/// When the process of a seeded observation started, in Unix seconds.
const SEEDED_START_SECONDS: u64 = 1_700_000_000;
/// The executable of a seeded observation.
pub const SEEDED_EXECUTABLE: &str = "/opt/stackly/bin/postgres";
/// The boot of a seeded observation.
const SEEDED_BOOT: &str = "00000000-0000-4000-8000-000000004242";

/// Returns the observation [`RegistrySeed::seed_observations`] writes for
/// `instance` at `at`: process 4242, started at `1700000000.000000`, in
/// session 4242, running [`SEEDED_EXECUTABLE`], in a fixed boot. Every
/// seeding implementation writes these values, so
/// [`latest_process_observation`](stackly_domain::Registry::latest_process_observation)
/// reads the same record from the fake and from SQLite.
///
/// # Panics
///
/// Never in practice: the fixed values are valid.
#[must_use]
pub fn seeded_observation(instance: InstanceId, at: Timestamp) -> ProcessObservation {
    let pid = Pid::new(SEEDED_PID).expect("the seeded process ID is between 1 and i32::MAX");
    ProcessObservation {
        instance,
        identity: ProcessIdentity {
            pid,
            start: ProcessStart::new(SEEDED_START_SECONDS, 0)
                .expect("the seeded start has no microseconds past a second"),
            executable: SEEDED_EXECUTABLE.into(),
            session: pid,
            boot: SEEDED_BOOT
                .parse()
                .expect("the seeded boot ID is a hyphenated UUID"),
        },
        observed_at: at,
    }
}

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
    /// `occurred_at` set to `at`: actor `daemon`, action `test.seed`, result
    /// `succeeded`, and details `{}`, so the audit read gives the same rows
    /// on every registry.
    fn seed_audit_events(&self, count: u32, at: Timestamp);

    /// Writes `count` `process_observations` rows for `instance`, each the
    /// record [`seeded_observation`] returns for `at`.
    fn seed_observations(&self, instance: InstanceId, count: u32, at: Timestamp);
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
