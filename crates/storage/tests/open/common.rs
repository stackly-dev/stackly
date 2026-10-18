//! Helpers shared by the registry open tests.

use std::fs::{self, DirBuilder};
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use sha2::{Digest, Sha256};
use stackly_platform::{DirectoryLayout, Platform};
use stackly_storage::{
    MigrationSet, OpenOptions, OpenSummary, RecoveryReport, RegistryOpen, SqliteRegistry,
    StorageError,
};
use stackly_testkit::{FakePlatform, TestRoot};

/// The app version the tests open with.
pub const APP_VERSION: &str = "0.1.0-test";

/// One test root with its fake platform and a layout whose data root and
/// standard data folders exist, the way `prepare_layout` leaves them.
pub struct Harness {
    pub root: TestRoot,
    pub fake: FakePlatform,
    pub layout: DirectoryLayout,
}

impl Harness {
    pub fn new() -> Self {
        let root = TestRoot::new().expect("a test root can be created");
        let fake = FakePlatform::new(&root);
        let layout = DirectoryLayout::new(root.roots());
        let mut builder = DirBuilder::new();
        builder.mode(0o700);
        builder
            .create(layout.roots().data())
            .expect("the data root can be created");
        for dir in [
            layout.registry_dir(),
            layout.providers_dir(),
            layout.runtimes_dir(),
            layout.instances_dir(),
            layout.environments_dir(),
            layout.operations_dir(),
            layout.backups_dir(),
            layout.retained_dir(),
        ] {
            builder
                .create(dir)
                .expect("a standard folder can be created");
        }
        Self { root, fake, layout }
    }

    pub fn platform(&self) -> Platform {
        self.fake.platform()
    }

    pub fn db(&self) -> PathBuf {
        self.layout.registry_db()
    }

    pub fn wal(&self) -> PathBuf {
        suffixed(&self.db(), "-wal")
    }

    pub fn try_open_with(&self, options: &OpenOptions) -> Result<RegistryOpen, StorageError> {
        options.open(&self.platform(), &self.layout, APP_VERSION)
    }

    pub fn open_with(&self, options: &OpenOptions) -> RegistryOpen {
        self.try_open_with(options)
            .expect("opening the registry succeeds or reports recovery")
    }

    pub fn open(&self) -> RegistryOpen {
        self.open_with(&OpenOptions::new())
    }

    /// Creates the registry with the built in migrations and closes it.
    pub fn create(&self) -> OpenSummary {
        let (registry, summary) = ready(self.open());
        drop(registry);
        summary
    }

    /// A plain read write connection to the registry, as another process
    /// would open it.
    pub fn connect(&self) -> Connection {
        Connection::open(self.db()).expect("the registry file opens")
    }

    /// The SHA-256 of the registry file and of its WAL, where an absent WAL
    /// hashes like an empty one.
    pub fn digests(&self) -> (String, String) {
        (sha256_of(&self.db()), sha256_of(&self.wal()))
    }

    pub fn snapshots(&self) -> Vec<String> {
        names_in(&self.layout.registry_snapshots_dir())
    }

    pub fn reports(&self) -> Vec<String> {
        names_in(&self.layout.registry_recovery_dir())
    }

    /// Writes `sql`, a frozen fixture, into a new registry file.
    pub fn load_fixture(&self, sql: &str) {
        let conn = Connection::open(self.db()).expect("a new registry file can be created");
        // A dump lists tables by name, not in foreign key order.
        conn.pragma_update(None, "foreign_keys", false)
            .expect("foreign keys can be turned off");
        conn.execute_batch(sql).expect("the fixture loads");
        conn.close().expect("the fixture connection closes");
        fs::set_permissions(self.db(), fs::Permissions::from_mode(0o600))
            .expect("the fixture file can be made private");
    }
}

pub fn ready(open: RegistryOpen) -> (SqliteRegistry, OpenSummary) {
    match open {
        RegistryOpen::Ready { registry, summary } => (registry, summary),
        RegistryOpen::Recovery(report) => panic!("expected a ready registry, got {report:#?}"),
    }
}

pub fn recovery(open: RegistryOpen) -> RecoveryReport {
    match open {
        RegistryOpen::Recovery(report) => report,
        RegistryOpen::Ready { summary, .. } => panic!("expected recovery, got {summary:#?}"),
    }
}

/// The built in set plus the given test migrations, with fingerprints
/// computed from scratch.
pub fn extended_set(extra: &[stackly_storage::Migration]) -> MigrationSet {
    let mut migrations = MigrationSet::builtin().migrations().to_vec();
    migrations.extend_from_slice(extra);
    let fingerprints = MigrationSet::fingerprints_from_scratch(&migrations)
        .expect("every test migration applies from scratch");
    MigrationSet::new(migrations, fingerprints)
}

pub fn sha256_of(path: &Path) -> String {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(err) => panic!("cannot read {}: {err}", path.display()),
    };
    let digest = Sha256::digest(&bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn mode_of(path: &Path) -> u32 {
    fs::symlink_metadata(path)
        .expect("the path exists")
        .permissions()
        .mode()
        & 0o777
}

pub fn suffixed(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

fn names_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = match fs::read_dir(dir) {
        Ok(entries) => entries
            .map(|entry| {
                entry
                    .expect("a folder entry can be read")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect(),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(err) => panic!("cannot list {}: {err}", dir.display()),
    };
    names.sort();
    names
}

/// The schema as stored, `(type, name, sql)` per row, sorted, for comparing
/// a registry before and after an open.
pub fn schema_rows(conn: &Connection) -> Vec<(String, String, Option<String>)> {
    let mut statement = conn
        .prepare("SELECT type, name, sql FROM sqlite_schema ORDER BY type, name")
        .expect("sqlite_schema can be read");
    statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .expect("sqlite_schema can be queried")
        .collect::<rusqlite::Result<_>>()
        .expect("every schema row reads")
}

/// The row count of every table.
pub fn row_counts(conn: &Connection) -> Vec<(String, i64)> {
    let tables: Vec<String> = conn
        .prepare("SELECT name FROM sqlite_schema WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
        .expect("tables can be listed")
        .query_map([], |row| row.get(0))
        .expect("tables can be queried")
        .collect::<rusqlite::Result<_>>()
        .expect("every table name reads");
    tables
        .into_iter()
        .map(|table| {
            let count = conn
                .query_row(&format!("SELECT count(*) FROM \"{table}\""), [], |row| {
                    row.get(0)
                })
                .expect("a table can be counted");
            (table, count)
        })
        .collect()
}

pub fn user_version(conn: &Connection) -> i64 {
    conn.pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("user_version reads")
}
