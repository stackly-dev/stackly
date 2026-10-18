//! Opening the registry: create it, inspect it read only, migrate it after
//! a verified snapshot, or report recovery without touching it
//! (spec 0005, "Open, migrasi, dan recovery").

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use jiff::Timestamp;
use rusqlite::{Connection, ErrorCode, TransactionBehavior};
use stackly_domain::RegistryId;
use stackly_platform::{DirectoryLayout, FileKind, Filesystem, Platform, PlatformError, UserId};

use crate::connection::{
    self, BUSY_TIMEOUT, WriteOpenError, open_create, open_inspect, read_int, set_foreign_keys,
};
use crate::error::StorageError;
use crate::files::{LinkOutcome, LockAttempt, RegistryFiles};
use crate::fingerprint::schema_fingerprint;
use crate::migration::{MigrationError, MigrationSet, apply_pending};
use crate::recovery::{
    Detection, RecoveryReason, RecoveryReport, ReportContext, ReportSnapshot, SHM_SUFFIX,
    WAL_SUFFIX, file_name, platform_io, same_contents, with_suffix,
};
use crate::registry::SqliteRegistry;
use crate::snapshot;
use crate::time::{format_timestamp, parse_timestamp};

/// The `application_id` in the header of every Stackly registry: `STKY`.
pub const APPLICATION_ID: i32 = 0x5354_4B59;
/// The temporary file a new registry is built in, next to the registry.
const CREATE_TEMP: &str = ".stackly.sqlite.creating";
/// Suffix of a rollback journal.
const JOURNAL_SUFFIX: &str = "-journal";
/// The 16 bytes every SQLite database starts with.
const SQLITE_MAGIC: &[u8; 16] = b"SQLite format 3\0";
/// Size of the SQLite database header.
const SQLITE_HEADER_LEN: usize = 100;
/// The one entry a standard folder may hold without counting as data.
const FINDER_METADATA: &str = ".DS_Store";
/// Most leftovers listed in a `registry_missing` report.
const EVIDENCE_MAX: usize = 100;
/// Most `quick_check` messages copied into a report.
const QUICK_CHECK_MESSAGES: usize = 5;

/// The result of opening the registry.
#[derive(Debug)]
#[expect(
    clippy::large_enum_variant,
    reason = "spec 0005 names the variant Recovery(RecoveryReport), and an open happens once per \
              daemon start"
)]
pub enum RegistryOpen {
    /// The registry is open and current.
    Ready {
        /// The handle to the registry thread.
        registry: SqliteRegistry,
        /// What the open did.
        summary: OpenSummary,
    },
    /// The registry was left untouched; the daemon runs in recovery mode.
    Recovery(RecoveryReport),
}

/// What an open that reached ready did.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct OpenSummary {
    /// Whether this open created the registry.
    pub created: bool,
    /// The schema version before migrating, when this open migrated.
    pub migrated_from: Option<u32>,
    /// The schema version now.
    pub schema_version: u32,
    /// The verified snapshot taken (or reused) before migrating.
    pub snapshot: Option<PathBuf>,
    /// The bundled SQLite version.
    pub sqlite_version: &'static str,
    /// The registry's own ID.
    pub registry_id: RegistryId,
    /// When the open finished. Readiness and health read back as observed
    /// at this time.
    pub opened_at: Timestamp,
}

/// How to open the registry. Production uses [`open_registry`]; tests swap
/// the migration set or the busy timeout.
#[derive(Debug, Clone)]
pub struct OpenOptions {
    migrations: MigrationSet,
    busy_timeout: Duration,
}

impl Default for OpenOptions {
    fn default() -> Self {
        Self::new()
    }
}

impl OpenOptions {
    /// The production options: the built in migrations and a 5 second busy
    /// timeout.
    #[must_use]
    pub fn new() -> Self {
        Self {
            migrations: MigrationSet::builtin(),
            busy_timeout: BUSY_TIMEOUT,
        }
    }

    /// Uses another migration set, for tests only.
    #[doc(hidden)]
    #[must_use]
    pub fn with_migrations(mut self, migrations: MigrationSet) -> Self {
        self.migrations = migrations;
        self
    }

    /// Uses another busy timeout, for tests only.
    #[doc(hidden)]
    #[must_use]
    pub const fn with_busy_timeout(mut self, busy_timeout: Duration) -> Self {
        self.busy_timeout = busy_timeout;
        self
    }

    /// Opens the registry of `layout`, creating it in a clean data root.
    ///
    /// # Errors
    ///
    /// Returns a [`StorageError`] when the environment or a storage owned
    /// file is unusable. Any problem with the registry file itself returns
    /// [`RegistryOpen::Recovery`] instead.
    pub fn open(
        &self,
        platform: &Platform,
        layout: &DirectoryLayout,
        app_version: &str,
    ) -> Result<RegistryOpen, StorageError> {
        let opener = Opener {
            platform,
            fs: platform.fs(),
            layout,
            app_version,
            set: &self.migrations,
            busy_timeout: self.busy_timeout,
            files: RegistryFiles::new(layout.registry_dir()),
            db: layout.registry_db(),
        };
        match opener.open() {
            Ok(open) => Ok(open),
            Err(Stop::Fatal(err)) => {
                tracing::error!(error = %err, "cannot open the registry");
                Err(err)
            }
            Err(Stop::Recovery(detection)) => opener.recovery(detection),
        }
    }
}

/// Opens the registry of `layout` with the production options.
///
/// # Errors
///
/// Returns a [`StorageError`] when the environment or a storage owned file
/// is unusable; see [`OpenOptions::open`].
pub fn open_registry(
    platform: &Platform,
    layout: &DirectoryLayout,
    app_version: &str,
) -> Result<RegistryOpen, StorageError> {
    OpenOptions::new().open(platform, layout, app_version)
}

/// Why an open stopped before ready.
enum Stop {
    /// The environment failed; the daemon exits.
    Fatal(StorageError),
    /// The registry needs recovery; the open reports it.
    Recovery(Detection),
}

impl From<StorageError> for Stop {
    fn from(err: StorageError) -> Self {
        Self::Fatal(err)
    }
}

impl From<Detection> for Stop {
    fn from(detection: Detection) -> Self {
        Self::Recovery(detection)
    }
}

/// What the read only inspection learned.
struct Inspection {
    version: u32,
    application_id: i64,
    registry_id: RegistryId,
}

struct Opener<'a> {
    platform: &'a Platform,
    fs: &'a dyn Filesystem,
    layout: &'a DirectoryLayout,
    app_version: &'a str,
    set: &'a MigrationSet,
    busy_timeout: Duration,
    files: RegistryFiles,
    db: PathBuf,
}

impl Opener<'_> {
    fn open(&self) -> Result<RegistryOpen, Stop> {
        self.prepare_dirs()?;
        self.clean_leftovers()?;
        if let Some(reason) = self.preflight()? {
            return Err(Detection::new(reason).into());
        }
        let created = if self.exists(&self.db)? {
            false
        } else {
            let evidence = self.leftovers()?;
            if !evidence.is_empty() {
                return Err(Detection::new(RecoveryReason::RegistryMissing { evidence }).into());
            }
            self.create()?;
            true
        };
        let inspection = self.inspect()?;
        let conn = self.open_write(&inspection)?;
        let (conn, migrated_from, snapshot) = if inspection.version < self.set.latest() {
            let (conn, snapshot) = self.migrate(conn, inspection.version)?;
            (conn, Some(inspection.version), Some(snapshot))
        } else {
            (conn, None, None)
        };
        let opened_at = self.platform.clock().now();
        let registry = SqliteRegistry::start(conn, self.platform.shared_clock(), opened_at)?;
        let summary = OpenSummary {
            created,
            migrated_from,
            schema_version: self.set.latest(),
            snapshot,
            sqlite_version: rusqlite::version(),
            registry_id: inspection.registry_id,
            opened_at,
        };
        tracing::info!(
            created,
            schema_version = summary.schema_version,
            migrated_from,
            sqlite_version = summary.sqlite_version,
            "registry open"
        );
        Ok(RegistryOpen::Ready { registry, summary })
    }

    fn recovery(&self, detection: Detection) -> Result<RegistryOpen, StorageError> {
        let report = self
            .report_context()
            .report(detection, self.platform.clock().now())?;
        tracing::error!(
            reason = report.reason.code(),
            report = %report.path.display(),
            "the registry needs recovery; it was left untouched"
        );
        Ok(RegistryOpen::Recovery(report))
    }

    fn report_context(&self) -> ReportContext<'_> {
        ReportContext {
            fs: self.fs,
            files: &self.files,
            layout: self.layout,
            app_version: self.app_version,
            app_schema_version: self.set.latest(),
        }
    }

    // Step 1: the folders and what storage itself left behind.

    fn prepare_dirs(&self) -> Result<(), StorageError> {
        let user = self
            .platform
            .identity()
            .current_user()
            .map_err(platform_io(&self.layout.registry_dir()))?;
        self.check_dir(&self.layout.registry_dir(), &user, false)?;
        self.check_dir(&self.layout.registry_snapshots_dir(), &user, true)?;
        self.check_dir(&self.layout.registry_recovery_dir(), &user, true)
    }

    fn check_dir(&self, path: &Path, user: &UserId, create: bool) -> Result<(), StorageError> {
        let unusable = |reason| StorageError::RegistryDirUnusable {
            path: path.to_path_buf(),
            reason,
        };
        let info = match self.fs.symlink_metadata(path).map_err(platform_io(path))? {
            Some(info) => info,
            None if create => {
                match self.fs.create_dir_private(path) {
                    Ok(()) | Err(PlatformError::AlreadyExists { .. }) => {}
                    Err(err) => return Err(platform_io(path)(err)),
                }
                self.fs
                    .symlink_metadata(path)
                    .map_err(platform_io(path))?
                    .ok_or_else(|| unusable("does not exist"))?
            }
            None => return Err(unusable("does not exist; prepare the layout first")),
        };
        let reason = match info.kind {
            FileKind::Symlink => "a symlink",
            FileKind::File | FileKind::Other => "not a directory",
            FileKind::Dir if info.owner.as_ref() != Some(user) => "owned by another user",
            FileKind::Dir if info.mode.is_none_or(|mode| mode & 0o077 != 0) => {
                "group or others have access"
            }
            FileKind::Dir => return Ok(()),
        };
        Err(unusable(reason))
    }

    fn clean_leftovers(&self) -> Result<(), StorageError> {
        let temp = self.layout.registry_dir().join(CREATE_TEMP);
        let journal = with_suffix(&temp, JOURNAL_SUFFIX);
        match self.files.try_lock(&temp)? {
            LockAttempt::Busy => return Err(StorageError::CreateInProgress { path: temp }),
            // The lock may be on a file that was replaced since it was
            // opened; then the name belongs to a creator that is running.
            LockAttempt::Acquired(lock) if !self.files.is_same_file(&lock, &temp)? => {
                return Err(StorageError::CreateInProgress { path: temp });
            }
            // The journal goes first: while the temp file still exists under
            // our lock, nobody can start a new create and its journal.
            LockAttempt::Acquired(lock) => {
                self.files.remove(&journal)?;
                self.files.remove(&temp)?;
                drop(lock);
                tracing::info!("removed the leftover of an interrupted registry create");
            }
            // A journal is only ever removed under the temp file's lock: a
            // creator that is running holds it while its journal lives.
            LockAttempt::Missing => {}
        }
        let snapshots = self.layout.registry_snapshots_dir();
        for entry in self
            .fs
            .read_dir(&snapshots)
            .map_err(platform_io(&snapshots))?
        {
            let name = entry.name.to_string_lossy();
            if entry.info.kind == FileKind::File
                && name.starts_with('.')
                && name.ends_with(".sqlite.tmp")
            {
                self.files.remove(&snapshots.join(&entry.name))?;
            }
        }
        Ok(())
    }

    // Step 2: every registry file is a regular file or absent.

    fn preflight(&self) -> Result<Option<RecoveryReason>, StorageError> {
        for suffix in ["", WAL_SUFFIX, SHM_SUFFIX, JOURNAL_SUFFIX] {
            let path = with_suffix(&self.db, suffix);
            let Some(info) = self
                .fs
                .symlink_metadata(&path)
                .map_err(platform_io(&path))?
            else {
                continue;
            };
            let detail = match info.kind {
                FileKind::File => continue,
                FileKind::Symlink => "a symlink",
                FileKind::Dir => "a directory",
                FileKind::Other => "a FIFO, socket, or device",
            };
            return Ok(Some(RecoveryReason::UnsafeRegistryFile {
                file: file_name(&path),
                detail: detail.to_owned(),
            }));
        }
        Ok(None)
    }

    fn exists(&self, path: &Path) -> Result<bool, StorageError> {
        Ok(self
            .fs
            .symlink_metadata(path)
            .map_err(platform_io(path))?
            .is_some())
    }

    // Step 3: a missing registry is created only in a clean data root.

    fn leftovers(&self) -> Result<Vec<String>, StorageError> {
        let mut evidence = Vec::new();
        for suffix in [WAL_SUFFIX, SHM_SUFFIX, JOURNAL_SUFFIX] {
            let path = with_suffix(&self.db, suffix);
            if self.exists(&path)? {
                evidence.push(path.display().to_string());
            }
        }
        let dirs = [
            self.layout.registry_snapshots_dir(),
            self.layout.registry_recovery_dir(),
            self.layout.providers_dir(),
            self.layout.runtimes_dir(),
            self.layout.instances_dir(),
            self.layout.environments_dir(),
            self.layout.operations_dir(),
            self.layout.backups_dir(),
            self.layout.retained_dir(),
        ];
        for dir in dirs {
            if evidence.len() >= EVIDENCE_MAX {
                break;
            }
            let entries = match self.fs.read_dir(&dir) {
                Ok(entries) => entries,
                Err(PlatformError::NotFound { .. }) => continue,
                Err(err) => return Err(platform_io(&dir)(err)),
            };
            let mut names: Vec<_> = entries
                .into_iter()
                .map(|entry| entry.name)
                .filter(|name| name != FINDER_METADATA)
                .collect();
            names.sort();
            evidence.extend(
                names
                    .into_iter()
                    .map(|name| dir.join(name).display().to_string()),
            );
        }
        evidence.truncate(EVIDENCE_MAX);
        Ok(evidence)
    }

    fn create(&self) -> Result<(), StorageError> {
        let temp = self.layout.registry_dir().join(CREATE_TEMP);
        let journal = with_suffix(&temp, JOURNAL_SUFFIX);
        let Some(lock) = self.files.create_locked(&temp)? else {
            return Err(StorageError::CreateInProgress { path: temp });
        };
        // Holding a new temp file's lock means any journal next to it is
        // left over from a creator that finished or died.
        self.files.remove(&journal)?;
        if let Err(err) = self.build(&temp) {
            // Both are ours while the lock is held; the journal goes first so
            // a create that starts once the name is free keeps its own.
            let _ = self.files.remove(&journal);
            let _ = self.files.remove(&temp);
            return Err(err);
        }
        lock.sync_all().map_err(|err| StorageError::Io {
            op: "sync",
            path: temp.clone(),
            detail: err.to_string(),
        })?;
        if !self.files.is_same_file(&lock, &temp)? {
            return Err(StorageError::CreateInProgress { path: temp });
        }
        match self.files.hard_link(&temp, &self.db)? {
            LinkOutcome::Linked => {}
            LinkOutcome::TargetExists => {
                self.files.remove(&temp)?;
                return Err(StorageError::CreateInProgress {
                    path: self.db.clone(),
                });
            }
            LinkOutcome::SourceMissing => {
                return Err(StorageError::CreateInProgress { path: temp });
            }
        }
        self.files.remove(&temp)?;
        self.files.sync_dir(&self.layout.registry_dir())?;
        drop(lock);
        tracing::info!("created a new registry");
        Ok(())
    }

    /// Builds the whole schema and `registry_info` in the temporary file, in
    /// one transaction.
    fn build(&self, temp: &Path) -> Result<(), StorageError> {
        let failed = |step: &str, err: &dyn std::fmt::Display| StorageError::CreateFailed {
            detail: format!("{step}: {err}"),
        };
        let mut conn = open_create(temp)?;
        let now = self.platform.clock().now();
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|err| failed("begin", &sqlite_detail(&err)))?;
        tx.pragma_update(None, "application_id", APPLICATION_ID)
            .map_err(|err| failed("application_id", &sqlite_detail(&err)))?;
        apply_pending(&tx, self.set, 0, now, self.app_version)
            .map_err(|err| failed("migrate", &err))?;
        tx.execute(
            "INSERT INTO registry_info (singleton, registry_id, created_at, created_by_version, \
             generation) VALUES (1, ?1, ?2, ?3, 0)",
            (
                RegistryId::generate().to_string(),
                format_timestamp(now),
                self.app_version,
            ),
        )
        .map_err(|err| failed("registry_info", &sqlite_detail(&err)))?;
        tx.commit()
            .map_err(|err| failed("commit", &sqlite_detail(&err)))?;
        set_foreign_keys(&conn, true).map_err(|err| match err {
            WriteOpenError::Sqlite(err) => failed("foreign_keys", &sqlite_detail(&err)),
            WriteOpenError::Pragma(err) => err,
        })?;
        conn.close()
            .map_err(|(_, err)| failed("close", &sqlite_detail(&err)))
    }

    // Step 4: decide between ready and recovery without writing anything.

    fn inspect(&self) -> Result<Inspection, Detection> {
        let header = self
            .files
            .read_prefix(&self.db, SQLITE_HEADER_LEN)
            .map_err(|err| {
                Detection::new(RecoveryReason::OpenFailed {
                    detail: err.to_string(),
                })
            })?;
        if header.len() < SQLITE_HEADER_LEN || !header.starts_with(SQLITE_MAGIC) {
            return Err(Detection::new(RecoveryReason::NotARegistry {
                detail: "the file does not start with a SQLite header".to_owned(),
            }));
        }
        let conn = open_inspect(&self.db, self.busy_timeout).map_err(classify)?;
        let application_id = read_int(&conn, "application_id").map_err(classify)?;
        if application_id != i64::from(APPLICATION_ID) {
            return Err(Detection::new(RecoveryReason::NotARegistry {
                detail: format!("application_id is {application_id:#x}"),
            }));
        }
        quick_check(&conn)?;
        let registry_id = registry_info(&conn)?;
        let version = read_int(&conn, "user_version").map_err(classify)?;
        let version = u32::try_from(version).map_err(|_| {
            Detection::new(RecoveryReason::IntegrityFailed {
                detail: format!("user_version {version} is out of range"),
            })
        })?;
        if version > self.set.latest() {
            return Err(Detection::new(RecoveryReason::SchemaNewerThanApp).with_version(version));
        }
        self.check_history(&conn, version).map_err(|detail| {
            Detection::new(RecoveryReason::MigrationHistoryMismatch { detail })
                .with_version(version)
        })?;
        let actual =
            schema_fingerprint(&conn).map_err(|err| classify(err).with_version(version))?;
        let expected = self.set.fingerprint(version).unwrap_or_default();
        if actual != expected {
            return Err(Detection::new(RecoveryReason::SchemaFingerprintMismatch {
                expected: expected.to_owned(),
                actual,
            })
            .with_version(version));
        }
        Ok(Inspection {
            version,
            application_id,
            registry_id,
        })
    }

    fn check_history(&self, conn: &Connection, version: u32) -> Result<(), String> {
        if version == 0 {
            return Err("no migration is recorded".to_owned());
        }
        let rows = conn
            .prepare("SELECT version, sha256 FROM schema_migrations ORDER BY version")
            .and_then(|mut statement| {
                statement
                    .query_map([], |row| {
                        Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .map_err(|err| format!("schema_migrations cannot be read: {err}"))?;
        let recorded: Vec<i64> = rows.iter().map(|(version, _)| *version).collect();
        let expected: Vec<i64> = (1..=i64::from(version)).collect();
        if recorded != expected {
            return Err(format!(
                "recorded versions are {recorded:?}, expected 1 to {version}"
            ));
        }
        for (recorded, sha256) in rows {
            let known = u32::try_from(recorded)
                .ok()
                .and_then(|version| self.set.get(version))
                .ok_or_else(|| format!("version {recorded} is unknown to this app"))?;
            if known.sha256() != sha256 {
                return Err(format!(
                    "the checksum of version {recorded} differs from this app's migration"
                ));
            }
        }
        Ok(())
    }

    // Step 5: the write connection.

    fn open_write(&self, inspection: &Inspection) -> Result<Connection, Stop> {
        let version = inspection.version;
        let conn = match connection::open_write(&self.db, self.busy_timeout) {
            Ok(conn) => conn,
            Err(WriteOpenError::Pragma(err)) => return Err(Stop::Fatal(err)),
            Err(WriteOpenError::Sqlite(err)) => {
                return Err(classify(err).with_version(version).into());
            }
        };
        let reread = read_int(&conn, "application_id").and_then(|application_id| {
            read_int(&conn, "user_version").map(|user_version| (application_id, user_version))
        });
        match reread {
            Ok((application_id, user_version))
                if application_id == inspection.application_id
                    && user_version == i64::from(version) => {}
            Ok(_) => {
                return Err(Detection::new(RecoveryReason::OpenFailed {
                    detail: "the registry file changed between inspection and opening it for \
                             writing"
                        .to_owned(),
                })
                .with_version(version)
                .into());
            }
            Err(err) => return Err(classify(err).with_version(version).into()),
        }
        if let Err(err) = conn.execute_batch("BEGIN IMMEDIATE; ROLLBACK;") {
            return Err(classify(err).with_version(version).into());
        }
        Ok(conn)
    }

    // Steps 6 and 7: snapshot, migrate in one transaction, prune snapshots.

    fn migrate(&self, mut conn: Connection, from: u32) -> Result<(Connection, PathBuf), Stop> {
        let to = self.set.latest();
        let snapshot = match self.snapshot(&conn, from, to) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                close(conn);
                return Err(Detection::new(RecoveryReason::BackupFailed { error })
                    .with_version(from)
                    .into());
            }
        };
        let started = Instant::now();
        if let Err(err) = self.apply(&mut conn, from) {
            close(conn);
            let mut detection = Detection::new(RecoveryReason::MigrationFailed {
                from,
                to,
                failed_version: err.version,
                error: err.detail,
            })
            .with_version(from);
            detection.snapshot = Some(Box::new(snapshot));
            return Err(detection.into());
        }
        set_foreign_keys(&conn, true).map_err(|err| match err {
            WriteOpenError::Pragma(err) => Stop::Fatal(err),
            WriteOpenError::Sqlite(err) => Stop::Fatal(StorageError::PragmaNotApplied {
                name: "foreign_keys",
                expected: "1".to_owned(),
                actual: err.to_string(),
            }),
        })?;
        tracing::info!(
            from,
            to,
            duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            snapshot = %snapshot.path.display(),
            "registry migrated"
        );
        self.prune_snapshots();
        Ok((conn, snapshot.path))
    }

    fn apply(&self, conn: &mut Connection, from: u32) -> Result<(), MigrationError> {
        let failed = |err: &dyn std::fmt::Display| MigrationError {
            version: from + 1,
            detail: err.to_string(),
        };
        set_foreign_keys(conn, false).map_err(|err| match err {
            WriteOpenError::Sqlite(err) => failed(&err),
            WriteOpenError::Pragma(err) => failed(&err),
        })?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|err| failed(&err))?;
        apply_pending(
            &tx,
            self.set,
            from,
            self.platform.clock().now(),
            self.app_version,
        )?;
        tx.commit().map_err(|err| MigrationError {
            version: self.set.latest(),
            detail: err.to_string(),
        })
    }

    /// Reuses the snapshot of the latest recovery report when it is for the
    /// same version and the registry files are unchanged since, otherwise
    /// takes a new one.
    fn snapshot(&self, conn: &Connection, from: u32, to: u32) -> Result<ReportSnapshot, String> {
        if let Some(snapshot) = self.reusable_snapshot(from) {
            tracing::info!(
                snapshot = %snapshot.path.display(),
                "reusing the snapshot named by the last recovery report"
            );
            return Ok(snapshot);
        }
        let snapshot = snapshot::take(
            conn,
            &self.files,
            self.layout,
            from,
            to,
            self.platform.clock().now(),
        )?;
        tracing::info!(snapshot = %snapshot.path.display(), "took a registry snapshot");
        Ok(snapshot)
    }

    fn reusable_snapshot(&self, from: u32) -> Option<ReportSnapshot> {
        let context = self.report_context();
        let (_, latest) = context.reports().ok()?.pop()?;
        let latest = latest?;
        let snapshot = latest.snapshot.clone()?;
        if snapshot.schema_version != from {
            return None;
        }
        let current = context.registry_files().ok()?;
        if !same_contents(&latest.files, &current) {
            return None;
        }
        let sha256 = snapshot::verify(&self.files, &snapshot.path, from).ok()?;
        (sha256 == snapshot.sha256).then_some(snapshot)
    }

    fn prune_snapshots(&self) {
        let reports = match self.report_context().reports() {
            Ok(reports) => reports,
            Err(err) => {
                tracing::warn!(error = %err, "cannot list recovery reports; keeping every snapshot");
                return;
            }
        };
        let mut protected = HashSet::new();
        for (path, report) in reports {
            let Some(report) = report else {
                tracing::warn!(
                    report = %path.display(),
                    "cannot read a recovery report; keeping every snapshot"
                );
                return;
            };
            if let Some(snapshot) = report.snapshot {
                protected.insert(snapshot.path);
            }
        }
        match snapshot::prune(self.fs, &self.files, self.layout, &protected) {
            Ok(removed) if !removed.is_empty() => {
                tracing::info!(count = removed.len(), "removed old registry snapshots");
            }
            Ok(_) => {}
            Err(err) => tracing::warn!(error = %err, "cannot remove old registry snapshots"),
        }
    }
}

/// Closes a connection before a report records the files' digests.
fn close(conn: Connection) {
    if let Err((_, err)) = conn.close() {
        tracing::warn!(error = %err, "closing the registry write connection failed");
    }
}

fn quick_check(conn: &Connection) -> Result<(), Detection> {
    let messages = conn
        .prepare("PRAGMA quick_check")
        .and_then(|mut statement| {
            statement
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()
        })
        .map_err(classify)?;
    if messages == ["ok"] {
        return Ok(());
    }
    let shown: Vec<_> = messages
        .iter()
        .take(QUICK_CHECK_MESSAGES)
        .cloned()
        .collect();
    Err(Detection::new(RecoveryReason::IntegrityFailed {
        detail: format!("quick_check: {}", shown.join("; ")),
    }))
}

fn registry_info(conn: &Connection) -> Result<RegistryId, Detection> {
    let invalid = |detail: String| Detection::new(RecoveryReason::IntegrityFailed { detail });
    let rows = conn
        .prepare("SELECT registry_id, created_at, generation FROM registry_info")
        .and_then(|mut statement| {
            statement
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()
        })
        .map_err(|err| invalid(format!("registry_info cannot be read: {err}")))?;
    let [(id, created_at, generation)] = rows.as_slice() else {
        return Err(invalid(format!(
            "registry_info holds {} rows, expected 1",
            rows.len()
        )));
    };
    let id: RegistryId = id
        .parse()
        .map_err(|err| invalid(format!("registry_info.registry_id: {err}")))?;
    if parse_timestamp(created_at).is_none() || *generation < 0 {
        return Err(invalid("registry_info holds an invalid row".to_owned()));
    }
    Ok(id)
}

/// A SQLite error with its extended result code, for messages.
fn sqlite_detail(err: &rusqlite::Error) -> String {
    match err {
        rusqlite::Error::SqliteFailure(failure, _) => {
            format!("{err} (extended code {})", failure.extended_code)
        }
        _ => err.to_string(),
    }
}

/// Maps a SQLite error during inspection or the write open to a recovery
/// reason.
fn classify(err: rusqlite::Error) -> Detection {
    let detail = err.to_string();
    Detection::new(match err.sqlite_error_code() {
        Some(ErrorCode::NotADatabase) => RecoveryReason::NotARegistry { detail },
        Some(ErrorCode::DatabaseCorrupt) => RecoveryReason::IntegrityFailed { detail },
        Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked) => RecoveryReason::RegistryLocked,
        _ => RecoveryReason::OpenFailed { detail },
    })
}

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use std::fs::DirBuilder;
    use std::os::unix::fs::DirBuilderExt;

    use stackly_testkit::{FakePlatform, TestRoot};

    use stackly_domain::RegistryError;

    use super::*;
    use crate::registry::sql_error;

    fn open_ready(root: &TestRoot) -> SqliteRegistry {
        let fake = FakePlatform::new(root);
        let layout = DirectoryLayout::new(root.roots());
        let mut builder = DirBuilder::new();
        builder.mode(0o700).recursive(true);
        builder.create(layout.registry_dir()).unwrap();
        match open_registry(&fake.platform(), &layout, "0.1.0").unwrap() {
            RegistryOpen::Ready { registry, .. } => registry,
            RegistryOpen::Recovery(report) => panic!("unexpected recovery {report:#?}"),
        }
    }

    #[test]
    fn the_write_connection_runs_with_every_required_pragma() {
        let root = TestRoot::new().unwrap();
        let registry = open_ready(&root);

        let pragmas = registry
            .run(|conn| {
                let text = |name: &str| -> rusqlite::Result<String> {
                    conn.pragma_query_value(None, name, |row| row.get(0))
                };
                let int = |name: &str| read_int(conn, name);
                (|| {
                    Ok((
                        text("journal_mode")?,
                        [
                            int("synchronous")?,
                            int("fullfsync")?,
                            int("checkpoint_fullfsync")?,
                            int("foreign_keys")?,
                            int("busy_timeout")?,
                            int("trusted_schema")?,
                            int("application_id")?,
                        ],
                    ))
                })()
                .map_err(sql_error)
            })
            .unwrap();

        assert_eq!(pragmas.0, "wal");
        assert_eq!(
            pragmas.1,
            [2, 1, 1, 1, 5000, 0, i64::from(APPLICATION_ID)],
            "synchronous, fullfsync, checkpoint_fullfsync, foreign_keys, busy_timeout, \
             trusted_schema, application_id"
        );
    }

    #[test]
    fn a_write_that_breaks_a_foreign_key_is_refused_as_integrity() {
        let root = TestRoot::new().unwrap();
        let registry = open_ready(&root);

        let err = registry
            .run(|conn| {
                conn.execute(
                    "INSERT INTO runtime_dependencies (runtime_id, dependency_runtime_id) VALUES \
                     ('0199f000-0000-7000-8000-000000000020', \
                     '0199f000-0000-7000-8000-000000000021')",
                    [],
                )
                .map_err(sql_error)
            })
            .unwrap_err();

        assert!(
            matches!(&err, RegistryError::Integrity { detail } if detail.contains("FOREIGN KEY")),
            "{err:?}"
        );
    }
}
