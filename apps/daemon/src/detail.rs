//! What `instance.get_detail` and `instance.get_data_usage` keep between
//! requests (spec 0021, *Ukuran data* and *Method IPC*): the pinned packages
//! the daemon has read, and the sizes of data folders it has measured.
//!
//! Neither cache is a source of truth. A pinned package is checked against
//! its `provider.yaml` on disk at every request, and a size is only an
//! estimate that expires.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::Duration;

use jiff::{SignedDuration, Timestamp};
use stackly_core::provider::{MANIFEST_MAX_LEN, NameCheck, ValidatedPackage, read_package};
use stackly_domain::{InstanceId, ProviderRevisionKey};
use stackly_platform::{
    DirectoryLayout, Platform, PlatformError, UsageLimits, UserId, check_private_dir,
};
use tokio::sync::{Semaphore, watch};

/// The most entries a data folder scan reads before it stops.
pub const DATA_USAGE_MAX_ENTRIES: u64 = 200_000;
/// How long a data folder scan may walk.
pub const DATA_USAGE_DEADLINE: Duration = Duration::from_secs(2);
/// How long a complete size is answered from memory.
pub const DATA_USAGE_CACHE_TTL: Duration = Duration::from_secs(60);
/// How long an incomplete size is answered from memory.
pub const DATA_USAGE_PARTIAL_TTL: Duration = Duration::from_secs(10);
/// The most scans that walk at the same time, for all instances.
pub const DATA_USAGE_CONCURRENCY: usize = 2;
/// The most pinned packages kept in memory.
pub const PINNED_PACKAGE_CACHE: usize = 32;
/// The size of the cache of measured folders where expired entries are
/// dropped on insert.
const USAGE_CACHE_SWEEP: usize = 64;

/// The limits of the data folder scans; tests shrink them through
/// `DaemonConfig`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct DataUsageConfig {
    /// The most entries one scan reads.
    pub max_entries: u64,
    /// How long one scan may walk.
    pub deadline: Duration,
    /// How long a complete result stays fresh.
    pub cache_ttl: Duration,
    /// How long an incomplete result stays fresh.
    pub partial_ttl: Duration,
    /// How many scans may walk at the same time.
    pub concurrency: usize,
}

impl Default for DataUsageConfig {
    fn default() -> Self {
        Self {
            max_entries: DATA_USAGE_MAX_ENTRIES,
            deadline: DATA_USAGE_DEADLINE,
            cache_ttl: DATA_USAGE_CACHE_TTL,
            partial_ttl: DATA_USAGE_PARTIAL_TTL,
            concurrency: DATA_USAGE_CONCURRENCY,
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The pinned packages read so far, most recent first. A clone shares the
/// same cache, so a blocking task can hold one.
#[derive(Clone, Default)]
pub struct PinnedPackages {
    entries: Arc<Mutex<Vec<PinnedEntry>>>,
}

struct PinnedEntry {
    key: ProviderRevisionKey,
    /// The bytes of `provider.yaml` the package was built from.
    manifest: Vec<u8>,
    package: Arc<ValidatedPackage>,
}

impl std::fmt::Debug for PinnedPackages {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PinnedPackages").finish_non_exhaustive()
    }
}

impl PinnedPackages {
    /// The package pinned as `key`, or `None` when its folder cannot be
    /// read now or holds another package.
    ///
    /// Every call checks `provider.yaml` in the pinned folder against the
    /// bytes the cached package was built from; a difference reads the
    /// folder again and checks its digest against `key`. A package that
    /// cannot be read is never cached, so a repaired folder is found at the
    /// next request.
    #[must_use]
    pub fn get(
        &self,
        platform: &Platform,
        layout: &DirectoryLayout,
        key: &ProviderRevisionKey,
    ) -> Option<Arc<ValidatedPackage>> {
        let dir = layout.provider_revision_dir(key);
        let manifest = platform
            .fs()
            .read_file_limited(&dir.join("provider.yaml"), MANIFEST_MAX_LEN)
            .ok();
        let cached = {
            let mut entries = lock(&self.entries);
            let at = entries.iter().position(|entry| entry.key == *key);
            at.map(|at| {
                let entry = entries.remove(at);
                let hit = (manifest.as_deref() == Some(entry.manifest.as_slice()))
                    .then(|| Arc::clone(&entry.package));
                if hit.is_some() {
                    entries.insert(0, entry);
                }
                hit
            })
        };
        if let Some(Some(package)) = cached {
            return Some(package);
        }
        let manifest = manifest?;
        let package = read_pinned(platform, &dir, key)?;
        let package = Arc::new(package);
        let mut entries = lock(&self.entries);
        entries.retain(|entry| entry.key != *key);
        entries.insert(
            0,
            PinnedEntry {
                key: key.clone(),
                manifest,
                package: Arc::clone(&package),
            },
        );
        entries.truncate(PINNED_PACKAGE_CACHE);
        Some(package)
    }
}

/// Reads and validates the folder `dir`, and keeps it only when it is the
/// package `key` names.
fn read_pinned(
    platform: &Platform,
    dir: &Path,
    key: &ProviderRevisionKey,
) -> Option<ValidatedPackage> {
    let files = read_package(platform.fs(), dir).ok()?;
    let package = ValidatedPackage::from_files(files, NameCheck::Skip).ok()?;
    (package.key() == key).then_some(package)
}

/// A size that was measured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MeasuredUsage {
    /// The allocated bytes counted.
    pub bytes: u64,
    /// False when a limit stopped the scan.
    pub complete: bool,
    /// When the scan finished.
    pub scanned_at: Timestamp,
}

/// Why a data folder could not be measured. The wire `reason`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsageFailure {
    /// The folder does not exist.
    Missing,
    /// The folder is not a private folder of the user, or part of it
    /// cannot be read.
    NotPrivate,
    /// Any other failure.
    Io,
}

impl UsageFailure {
    /// The value of the `reason` detail.
    #[must_use]
    pub const fn reason(self) -> &'static str {
        match self {
            Self::Missing => "missing",
            Self::NotPrivate => "not_private",
            Self::Io => "io",
        }
    }
}

/// What a client asked of `instance.get_data_usage`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UsageRequest {
    /// Measure again even when a fresh result is held.
    pub refresh: bool,
    /// Never start a scan.
    pub cached_only: bool,
}

type Outcome = Result<MeasuredUsage, UsageFailure>;

#[derive(Default)]
struct UsageState {
    cache: HashMap<InstanceId, MeasuredUsage>,
    inflight: HashMap<InstanceId, watch::Receiver<Option<Outcome>>>,
}

struct UsageShared {
    config: DataUsageConfig,
    permits: Arc<Semaphore>,
    state: Mutex<UsageState>,
}

/// The sizes of data folders: a cache per instance, one scan per instance
/// at a time, and a semaphore over all scans (spec 0021 AC-5).
#[derive(Default)]
pub struct DataUsageService {
    shared: OnceLock<Arc<UsageShared>>,
}

impl std::fmt::Debug for DataUsageService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DataUsageService").finish_non_exhaustive()
    }
}

impl DataUsageService {
    /// Records the limits of this run. The first call wins; a service that
    /// was never configured uses the defaults.
    pub fn configure(&self, config: DataUsageConfig) {
        let _ = self.shared.set(Arc::new(UsageShared::new(config)));
    }

    fn shared(&self) -> &Arc<UsageShared> {
        self.shared
            .get_or_init(|| Arc::new(UsageShared::new(DataUsageConfig::default())))
    }

    /// Answers `instance.get_data_usage` for the data folder at `path`, in
    /// the order of the spec: a fresh cached size, `None` for `cached_only`,
    /// the scan already running, else a new scan that no client can cancel.
    ///
    /// # Errors
    ///
    /// Returns the [`UsageFailure`] of the scan, which is shared by every
    /// request that waited for it and never cached.
    pub async fn usage(
        &self,
        platform: &Platform,
        id: InstanceId,
        path: PathBuf,
        request: UsageRequest,
    ) -> Result<Option<MeasuredUsage>, UsageFailure> {
        let shared = Arc::clone(self.shared());
        let mut waiting = {
            let mut state = lock(&shared.state);
            if !request.refresh
                && let Some(cached) = state.cache.get(&id)
                && shared.fresh(cached, platform.clock().now())
            {
                return Ok(Some(*cached));
            }
            if request.cached_only {
                return Ok(None);
            }
            // A scan whose task died without an answer leaves a closed
            // channel behind; it is not running.
            match state.inflight.get(&id) {
                Some(running) if running.has_changed().is_ok() => running.clone(),
                _ => {
                    let (sender, receiver) = watch::channel(None);
                    state.inflight.insert(id, receiver.clone());
                    let platform = platform.clone();
                    let scanning = Arc::clone(&shared);
                    tokio::spawn(async move {
                        let outcome = scanning.scan(&platform, &path).await;
                        {
                            let mut state = lock(&scanning.state);
                            state.inflight.remove(&id);
                            if let Ok(usage) = outcome {
                                scanning.remember(&mut state, id, usage, platform.clock().now());
                            }
                        }
                        let _ = sender.send(Some(outcome));
                    });
                    receiver
                }
            }
        };
        let outcome = waiting.wait_for(Option::is_some).await;
        match outcome {
            Ok(done) => (*done)
                .ok_or(UsageFailure::Io)
                .and_then(|outcome| outcome.map(Some)),
            // The scan task ended without an answer, which only a panic
            // does.
            Err(_) => Err(UsageFailure::Io),
        }
    }
}

impl UsageShared {
    fn new(config: DataUsageConfig) -> Self {
        Self {
            permits: Arc::new(Semaphore::new(config.concurrency.max(1))),
            config,
            state: Mutex::new(UsageState::default()),
        }
    }

    /// Whether `usage` is still fresh at `now`: `scanned_at <= now <
    /// scanned_at + ttl`.
    fn fresh(&self, usage: &MeasuredUsage, now: Timestamp) -> bool {
        let ttl = if usage.complete {
            self.config.cache_ttl
        } else {
            self.config.partial_ttl
        };
        let Ok(ttl) = SignedDuration::try_from(ttl) else {
            return false;
        };
        usage
            .scanned_at
            .checked_add(ttl)
            .is_ok_and(|until| usage.scanned_at <= now && now < until)
    }

    fn remember(
        &self,
        state: &mut UsageState,
        id: InstanceId,
        usage: MeasuredUsage,
        now: Timestamp,
    ) {
        if state.cache.len() >= USAGE_CACHE_SWEEP {
            state.cache.retain(|_, held| self.fresh(held, now));
        }
        state.cache.insert(id, usage);
    }

    /// Checks the folder, waits for a permit, and walks it.
    async fn scan(&self, platform: &Platform, path: &Path) -> Outcome {
        let user = platform
            .identity()
            .current_user()
            .map_err(|_| UsageFailure::Io)?;
        check_folder(platform, path, &user)?;
        let permit = Arc::clone(&self.permits)
            .acquire_owned()
            .await
            .map_err(|_| UsageFailure::Io)?;
        let limits = UsageLimits::new(self.config.max_entries, self.config.deadline);
        let walker = platform.clone();
        let root = path.to_path_buf();
        let walked = tokio::task::spawn_blocking(move || walker.fs().tree_usage(&root, limits))
            .await
            .map_err(|_| UsageFailure::Io)?;
        drop(permit);
        let walked = walked.map_err(|error| failure_of(&error))?;
        Ok(MeasuredUsage {
            bytes: walked.bytes,
            complete: walked.complete,
            scanned_at: platform.clock().now(),
        })
    }
}

/// The folder must exist and be a private folder of `user`.
fn check_folder(platform: &Platform, path: &Path, user: &UserId) -> Result<(), UsageFailure> {
    match platform.fs().symlink_metadata(path) {
        Ok(None) => Err(UsageFailure::Missing),
        Ok(Some(info)) => check_private_dir(&info, user).map_err(|_| UsageFailure::NotPrivate),
        Err(_) => Err(UsageFailure::Io),
    }
}

/// The `reason` a failed walk is reported with.
fn failure_of(error: &PlatformError) -> UsageFailure {
    match error {
        PlatformError::NotFound { .. } => UsageFailure::Missing,
        // A folder swapped for a symlink or a file between the check and
        // the scan, or one that cannot be read.
        PlatformError::SymlinkInPath { .. }
        | PlatformError::NotADirectory { .. }
        | PlatformError::PermissionDenied { .. } => UsageFailure::NotPrivate,
        _ => UsageFailure::Io,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usage(complete: bool, scanned_at: &str) -> MeasuredUsage {
        MeasuredUsage {
            bytes: 1,
            complete,
            scanned_at: scanned_at.parse().expect("a timestamp"),
        }
    }

    fn at(text: &str) -> Timestamp {
        text.parse().expect("a timestamp")
    }

    #[test]
    fn a_result_is_fresh_from_its_scan_until_its_ttl_and_not_after() {
        let shared = UsageShared::new(DataUsageConfig::default());
        let complete = usage(true, "2026-10-22T10:00:00Z");

        assert!(shared.fresh(&complete, at("2026-10-22T10:00:00Z")));
        assert!(shared.fresh(&complete, at("2026-10-22T10:00:59Z")));
        assert!(!shared.fresh(&complete, at("2026-10-22T10:01:00Z")));
        assert!(
            !shared.fresh(&complete, at("2026-10-22T09:59:59Z")),
            "a clock that went back never makes a result fresh"
        );
    }

    #[test]
    fn an_incomplete_result_uses_the_short_ttl() {
        let shared = UsageShared::new(DataUsageConfig::default());
        let partial = usage(false, "2026-10-22T10:00:00Z");

        assert!(shared.fresh(&partial, at("2026-10-22T10:00:09Z")));
        assert!(!shared.fresh(&partial, at("2026-10-22T10:00:10Z")));
    }

    #[test]
    fn a_walk_error_maps_to_the_reason_of_the_spec() {
        let path = PathBuf::from("/data");
        let io = |kind| PlatformError::Io {
            op: "tree_usage",
            path: path.clone(),
            kind,
            message: String::new(),
        };

        assert_eq!(
            failure_of(&PlatformError::NotFound { path: path.clone() }),
            UsageFailure::Missing
        );
        for error in [
            PlatformError::SymlinkInPath { path: path.clone() },
            PlatformError::NotADirectory { path: path.clone() },
            PlatformError::PermissionDenied { path: path.clone() },
        ] {
            assert_eq!(failure_of(&error), UsageFailure::NotPrivate, "{error}");
        }
        assert_eq!(
            failure_of(&io(std::io::ErrorKind::TimedOut)),
            UsageFailure::Io
        );
    }
}

#[cfg(all(test, unix))]
mod service {
    use std::time::Duration;

    use jiff::SignedDuration;
    use stackly_testkit::{FakePlatform, FsOp, TestRoot};

    use super::*;

    /// A fake platform and a folder of `files` small files under its root.
    struct Fixture {
        _root: TestRoot,
        fake: FakePlatform,
        service: DataUsageService,
    }

    impl Fixture {
        fn new(config: DataUsageConfig) -> Self {
            let root = TestRoot::new().expect("a test root");
            let fake = FakePlatform::new(&root);
            let service = DataUsageService::default();
            service.configure(config);
            Self {
                _root: root,
                fake,
                service,
            }
        }

        /// A private folder with `files` files, named `name`.
        fn folder(&self, name: &str, files: usize) -> PathBuf {
            let platform = self.fake.platform();
            let path = self._root.path().join(name);
            platform.fs().create_dir_private(&path).expect("the folder");
            for index in 0..files {
                platform
                    .fs()
                    .write_file_atomic(&path.join(format!("f{index}")), b"data")
                    .expect("a file");
            }
            path
        }

        async fn ask(
            &self,
            id: InstanceId,
            path: &Path,
            refresh: bool,
            cached_only: bool,
        ) -> Result<Option<MeasuredUsage>, UsageFailure> {
            self.service
                .usage(
                    &self.fake.platform(),
                    id,
                    path.to_path_buf(),
                    UsageRequest {
                        refresh,
                        cached_only,
                    },
                )
                .await
        }

        fn scans(&self) -> usize {
            self.fake.fs().tree_usage_calls()
        }
    }

    fn id(n: u128) -> InstanceId {
        InstanceId::from_uuid(uuid::Uuid::from_u128(
            0x0192_f3a1_0000_7000_8000_0000_0000_0000 + n,
        ))
        .expect("a version 7 id")
    }

    // covers: AC-5
    #[tokio::test(flavor = "multi_thread")]
    async fn a_fresh_result_is_answered_from_memory_until_it_expires() {
        let fixture = Fixture::new(DataUsageConfig::default());
        let path = fixture.folder("data", 3);

        let first = fixture
            .ask(id(1), &path, false, false)
            .await
            .expect("scans");
        let again = fixture
            .ask(id(1), &path, false, false)
            .await
            .expect("cached");

        assert_eq!(first, again, "the stored scannedAt comes back as it was");
        assert_eq!(fixture.scans(), 1);
        fixture.fake.clock().advance(SignedDuration::from_secs(59));
        fixture
            .ask(id(1), &path, false, false)
            .await
            .expect("cached");
        assert_eq!(fixture.scans(), 1);
        fixture.fake.clock().advance(SignedDuration::from_secs(1));
        let later = fixture
            .ask(id(1), &path, false, false)
            .await
            .expect("scans");
        assert_eq!(fixture.scans(), 2, "sixty seconds is expired");
        assert!(later.expect("a size").scanned_at > first.expect("a size").scanned_at);
    }

    // covers: AC-5
    #[tokio::test(flavor = "multi_thread")]
    async fn an_incomplete_result_expires_after_ten_seconds() {
        let fixture = Fixture::new(DataUsageConfig {
            max_entries: 2,
            ..DataUsageConfig::default()
        });
        let path = fixture.folder("data", 5);

        let first = fixture
            .ask(id(1), &path, false, false)
            .await
            .expect("scans");
        assert!(!first.expect("a size").complete);
        fixture.fake.clock().advance(SignedDuration::from_secs(9));
        fixture
            .ask(id(1), &path, false, false)
            .await
            .expect("cached");
        assert_eq!(fixture.scans(), 1);
        fixture.fake.clock().advance(SignedDuration::from_secs(1));
        fixture
            .ask(id(1), &path, false, false)
            .await
            .expect("scans");
        assert_eq!(fixture.scans(), 2);
    }

    // covers: AC-5
    #[tokio::test(flavor = "multi_thread")]
    async fn cached_only_never_scans_and_an_expired_entry_counts_as_none() {
        let fixture = Fixture::new(DataUsageConfig::default());
        let path = fixture.folder("data", 1);

        assert_eq!(fixture.ask(id(1), &path, false, true).await, Ok(None));
        assert_eq!(fixture.scans(), 0);
        fixture
            .ask(id(1), &path, false, false)
            .await
            .expect("scans");
        assert!(
            fixture
                .ask(id(1), &path, false, true)
                .await
                .expect("cached")
                .is_some()
        );
        fixture.fake.clock().advance(SignedDuration::from_secs(61));
        assert_eq!(fixture.ask(id(1), &path, false, true).await, Ok(None));
        assert_eq!(fixture.scans(), 1);
    }

    // covers: AC-5
    #[tokio::test(flavor = "multi_thread")]
    async fn requests_for_one_instance_at_once_share_one_scan_even_a_refresh() {
        let fixture = Fixture::new(DataUsageConfig::default());
        let path = fixture.folder("data", 2);
        fixture
            .fake
            .fs()
            .set_delay(FsOp::TreeUsage, Duration::from_millis(300));

        let (a, b, c) = tokio::join!(
            fixture.ask(id(1), &path, false, false),
            fixture.ask(id(1), &path, true, false),
            fixture.ask(id(1), &path, false, false),
        );

        assert_eq!(fixture.scans(), 1);
        assert_eq!(a, b);
        assert_eq!(b, c);
        assert!(a.expect("a size").is_some());
    }

    // covers: AC-5
    #[tokio::test(flavor = "multi_thread")]
    async fn refresh_scans_again_when_nothing_is_running() {
        let fixture = Fixture::new(DataUsageConfig::default());
        let path = fixture.folder("data", 1);

        fixture
            .ask(id(1), &path, false, false)
            .await
            .expect("scans");
        fixture.ask(id(1), &path, true, false).await.expect("scans");

        assert_eq!(fixture.scans(), 2);
    }

    // covers: AC-5
    #[tokio::test(flavor = "multi_thread")]
    async fn at_most_two_scans_walk_at_once_across_instances() {
        let fixture = Fixture::new(DataUsageConfig::default());
        let paths: Vec<_> = (0..3)
            .map(|n| fixture.folder(&format!("data{n}"), 1))
            .collect();
        fixture
            .fake
            .fs()
            .set_delay(FsOp::TreeUsage, Duration::from_millis(250));

        let (a, b, c) = tokio::join!(
            fixture.ask(id(1), &paths[0], false, false),
            fixture.ask(id(2), &paths[1], false, false),
            fixture.ask(id(3), &paths[2], false, false),
        );

        assert!([a, b, c].iter().all(|result| matches!(result, Ok(Some(_)))));
        assert_eq!(fixture.scans(), 3);
        assert_eq!(fixture.fake.fs().tree_usage_peak(), 2);
    }

    // covers: AC-5
    #[tokio::test(flavor = "multi_thread")]
    async fn a_client_that_goes_away_does_not_cancel_the_scan() {
        let fixture = Arc::new(Fixture::new(DataUsageConfig::default()));
        let path = fixture.folder("data", 2);
        fixture
            .fake
            .fs()
            .set_delay(FsOp::TreeUsage, Duration::from_millis(200));
        let waiting = {
            let fixture = Arc::clone(&fixture);
            let path = path.clone();
            tokio::spawn(async move { fixture.ask(id(1), &path, false, false).await })
        };
        tokio::time::sleep(Duration::from_millis(50)).await;
        waiting.abort();

        let mut cached = None;
        for _ in 0..100 {
            cached = fixture.ask(id(1), &path, false, true).await.expect("cache");
            if cached.is_some() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }

        assert!(cached.is_some(), "the scan finished without its client");
        assert_eq!(fixture.scans(), 1);
    }

    // covers: AC-5
    #[tokio::test(flavor = "multi_thread")]
    async fn a_folder_that_is_missing_or_not_private_fails_before_any_walk() {
        let fixture = Fixture::new(DataUsageConfig::default());
        let missing = fixture._root.path().join("nowhere");
        let open = fixture.folder("open", 0);
        std::fs::set_permissions(&open, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .expect("the mode changes");

        assert_eq!(
            fixture.ask(id(1), &missing, false, false).await,
            Err(UsageFailure::Missing)
        );
        assert_eq!(
            fixture.ask(id(2), &open, false, false).await,
            Err(UsageFailure::NotPrivate)
        );
        assert_eq!(fixture.scans(), 0);
    }

    // covers: AC-5
    #[tokio::test(flavor = "multi_thread")]
    async fn a_failed_walk_reports_its_reason_and_is_never_cached() {
        let fixture = Fixture::new(DataUsageConfig::default());
        let path = fixture.folder("data", 1);
        fixture
            .fake
            .fs()
            .fail_next(FsOp::TreeUsage, stackly_testkit::Fault::PermissionDenied);

        assert_eq!(
            fixture.ask(id(1), &path, false, false).await,
            Err(UsageFailure::NotPrivate)
        );
        fixture
            .fake
            .fs()
            .fail_next(FsOp::TreeUsage, stackly_testkit::Fault::Io);
        assert_eq!(
            fixture.ask(id(1), &path, false, false).await,
            Err(UsageFailure::Io)
        );
        assert!(
            fixture
                .ask(id(1), &path, false, false)
                .await
                .expect("scans")
                .is_some(),
            "the failures were not remembered"
        );
        assert_eq!(fixture.scans(), 3);
    }
    // covers: AC-5
    #[tokio::test(flavor = "multi_thread")]
    async fn a_folder_swapped_for_a_file_between_the_check_and_the_walk_is_not_private() {
        let fixture = Fixture::new(DataUsageConfig::default());
        let path = fixture.folder("data", 1);
        let swap = path.clone();
        fixture.fake.fs().run_on_next(FsOp::TreeUsage, move || {
            std::fs::remove_dir_all(&swap).expect("the folder goes");
            std::fs::write(&swap, b"now a file").expect("a file takes its place");
        });

        assert_eq!(
            fixture.ask(id(1), &path, false, false).await,
            Err(UsageFailure::NotPrivate)
        );
    }
}
