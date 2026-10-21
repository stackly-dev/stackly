//! `storage.get_capacity` in process (spec 0022 AC-4): the level of every
//! boundary read through `FakeFilesystem`, the three `capacity_unavailable`
//! reasons, one read for calls that arrive together, a timeout that leaves
//! at most one thread behind, recovery mode, and no mutation.

use std::io;
use std::sync::Arc;
use std::time::{Duration, Instant};

use stackly_core::{PreparedLayout, prepare_layout};
use stackly_ipc::{Client, ConnectPolicy};
use stackly_platform::{Clock, Platform, RootKind, UserId};
use stackly_protocol::codes::storage;
use stackly_protocol::events::ShutdownReason;
use stackly_protocol::handshake::{ClientInfo, ClientKind};
use stackly_protocol::storage::{CapacityLevel, CapacityReport};
use stackly_protocol::{Call, ErrorBody, Reply, Timestamp};
use stackly_testkit::evidence::{Kind, evidence};
use stackly_testkit::{FakePlatform, FakeVolume, Fault, FsOp, TestRoot};
use stacklyd::lock::{DaemonLock, LockOwner};
use stacklyd::log_sink::{LogSink, LogSinkConfig};
use stacklyd::map::StartFacts;
use stacklyd::server::{self, DaemonConfig, DaemonDeps, DaemonHandle};
use uuid::Uuid;

use crate::common;

const GB: u64 = 1_000_000_000;

/// A daemon site whose platform is the fake one, so a test sets the volume,
/// injects faults, and reads the mutation log.
struct FakeSite {
    _root: TestRoot,
    fake: FakePlatform,
    platform: Platform,
    prepared: PreparedLayout,
    _lock: DaemonLock,
}

impl FakeSite {
    fn new() -> Self {
        let root = TestRoot::new().expect("a test root can be created");
        let fake = FakePlatform::new(&root).with_roots(root.roots());
        let platform = fake.platform();
        let prepared = prepare_layout(&platform).expect("the test root layout can be prepared");
        let owner = LockOwner::new(
            std::process::id(),
            Uuid::now_v7(),
            Timestamp::from_jiff(fake.clock().now()),
            "0.1.0",
        );
        let lock = DaemonLock::acquire(&prepared.layout().daemon_lock(), owner)
            .expect("the test root's lock is free");
        Self {
            _root: root,
            fake,
            platform,
            prepared,
            _lock: lock,
        }
    }

    fn data_root(&self) -> std::path::PathBuf {
        self.prepared.canonical_root(RootKind::Data).to_path_buf()
    }

    fn set_volume(&self, total: u64, available: u64) {
        self.fake
            .fs()
            .set_volume(self.data_root(), FakeVolume::new(total, available));
    }

    async fn start(&self) -> DaemonHandle {
        self.start_with_timeout(Duration::from_secs(5)).await
    }

    async fn start_with_timeout(&self, timeout: Duration) -> DaemonHandle {
        let mut config: DaemonConfig = common::config();
        config.capacity_read_timeout = timeout;
        let log = LogSink::start(Box::new(io::sink()), LogSinkConfig::DAEMON)
            .expect("the log writer starts");
        let facts = StartFacts::new(
            Uuid::now_v7(),
            Timestamp::from_jiff(self.fake.clock().now()),
            true,
            std::process::id(),
        );
        let deps = DaemonDeps::new(
            self.platform.clone(),
            self.prepared.clone(),
            facts,
            Arc::new(log),
        )
        .with_config(config);
        let handle = server::start(deps).await.expect("the server starts");
        handle.started().await;
        handle
    }

    async fn client(&self) -> Client {
        let UserId::Unix(uid) = self
            .platform
            .identity()
            .current_user()
            .expect("the current user can be read")
        else {
            panic!("not a unix user");
        };
        Client::connect(
            &self.prepared.layout().daemon_socket(),
            ConnectPolicy::same_user(uid),
            ClientInfo {
                kind: ClientKind::Cli,
                version: "0.1.0".to_owned(),
                pid: Some(std::process::id()),
                interactive: None,
            },
        )
        .await
        .expect("the client connects")
        .0
    }

    async fn capacity(&self) -> Result<CapacityReport, Box<ErrorBody>> {
        let client = self.client().await;
        match client.call(Call::StorageGetCapacity).await? {
            Reply::StorageGetCapacity(report) => Ok(report),
            other => panic!("not a capacity reply: {other:?}"),
        }
    }
}

#[tokio::test]
async fn the_level_follows_every_boundary_of_the_volume_the_data_root_lies_on() {
    let ev = evidence!(
        covers = ["REQ-033"],
        kind = Kind::Fixture,
        expected = "storage.get_capacity answers the level stackly_domain computes at each boundary, with the total and available bytes and the percent used, stamped with the platform clock",
    );
    let site = FakeSite::new();
    let daemon = site.start().await;
    let cases = [
        (20 * GB, 1_999_999_999, CapacityLevel::Critical),
        (20 * GB, 2 * GB, CapacityLevel::Low),
        (20 * GB, 4_999_999_999, CapacityLevel::Low),
        (20 * GB, 5 * GB, CapacityLevel::Ok),
        (100 * GB, 21 * GB, CapacityLevel::Ok),
        (100 * GB, 20 * GB, CapacityLevel::Filling),
        (100 * GB, 11 * GB, CapacityLevel::Filling),
        (100 * GB, 10 * GB, CapacityLevel::Low),
    ];

    for (total, available, level) in cases {
        site.set_volume(total, available);

        let report = site.capacity().await.expect("the volume reads");

        assert_eq!(
            (
                report.total_bytes.get(),
                report.available_bytes.get(),
                report.level
            ),
            (total, available, level),
            "{available} of {total}"
        );
        assert_eq!(
            report.used_percent,
            u8::try_from((total - available) * 100 / total).expect("a percent"),
        );
        assert_eq!(
            report.checked_at,
            Timestamp::from_jiff(site.fake.clock().now())
        );
    }
    drop(daemon);
    ev.pass();
}

#[tokio::test]
async fn checked_at_is_the_clock_when_the_read_finishes_not_when_it_starts() {
    let ev = evidence!(
        covers = ["REQ-033"],
        kind = Kind::Fixture,
        expected = "a clock moved while a delayed volume read is in flight is the time storage.get_capacity stamps, because checkedAt is read when the read finishes",
    );
    let site = FakeSite::new();
    let _daemon = site.start().await;
    site.set_volume(20 * GB, 10 * GB);
    site.fake
        .fs()
        .set_delay(FsOp::VolumeSpace, Duration::from_millis(600));
    let started = site.fake.clock().now();

    let call = tokio::spawn({
        let client = site.client().await;
        async move { client.call(Call::StorageGetCapacity).await }
    });
    // Well inside the 600 ms the fake volume read sleeps.
    tokio::time::sleep(Duration::from_millis(150)).await;
    let step = jiff::SignedDuration::from_secs(60);
    site.fake.clock().advance(step);
    let Reply::StorageGetCapacity(report) = call
        .await
        .expect("the call task ends")
        .expect("the volume reads")
    else {
        panic!("not a capacity reply");
    };

    assert_eq!(
        report.checked_at,
        Timestamp::from_jiff(
            started
                .checked_add(step)
                .expect("a minute later is in range")
        ),
    );
    ev.pass();
}

#[tokio::test]
async fn available_above_total_is_held_to_total() {
    let site = FakeSite::new();
    let _daemon = site.start().await;
    site.set_volume(10 * GB, 50 * GB);

    let report = site.capacity().await.expect("the volume reads");

    assert_eq!(report.available_bytes.get(), 10 * GB);
    assert_eq!(report.used_percent, 0);
    assert_eq!(report.level, CapacityLevel::Ok);
}

#[tokio::test]
async fn a_failed_read_and_a_volume_of_size_zero_answer_capacity_unavailable_with_the_reason() {
    let site = FakeSite::new();
    let _daemon = site.start().await;

    site.fake.fs().fail_next(FsOp::VolumeSpace, Fault::Io);
    let io = site.capacity().await.expect_err("the read fails");
    site.fake
        .fs()
        .fail_next(FsOp::VolumeSpace, Fault::PermissionDenied);
    let denied = site.capacity().await.expect_err("the read fails");
    site.set_volume(0, 0);
    let no_size = site.capacity().await.expect_err("the volume has no size");

    for error in [&io, &denied, &no_size] {
        assert_eq!(error.code(), storage::CAPACITY_UNAVAILABLE.code);
        assert!(error.retryable());
    }
    assert_eq!(io.detail("reason"), Some("io"));
    assert_eq!(io.detail("kind"), Some("other"));
    assert_eq!(denied.detail("kind"), Some("permission_denied"));
    assert_eq!(no_size.detail("reason"), Some("no_size"));
    assert_eq!(no_size.detail("kind"), None);
    assert_eq!(no_size.hint(), None);
}

#[tokio::test]
async fn calls_that_arrive_together_wait_for_one_read() {
    let site = FakeSite::new();
    let _daemon = site.start().await;
    site.fake
        .fs()
        .set_delay(FsOp::VolumeSpace, Duration::from_millis(300));
    // Only the first read fails: a second read would succeed.
    site.fake.fs().fail_next(FsOp::VolumeSpace, Fault::Io);

    let (first, second, third) = tokio::join!(site.capacity(), site.capacity(), site.capacity());

    for result in [first, second, third] {
        let error = result.expect_err("every call got the one failing read");
        assert_eq!(error.detail("reason"), Some("io"));
    }
}

#[tokio::test]
async fn a_read_over_the_timeout_answers_timeout_and_never_holds_a_second_thread() {
    let ev = evidence!(
        covers = ["REQ-033"],
        kind = Kind::Fixture,
        expected = "a statfs that outlasts the read timeout answers storage.capacity_unavailable reason timeout with its static hint, calls during the stuck read get the same answer at once, and the next call after the thread ended reads again",
    );
    let site = FakeSite::new();
    let _daemon = site.start_with_timeout(Duration::from_millis(100)).await;
    site.fake
        .fs()
        .set_delay(FsOp::VolumeSpace, Duration::from_millis(700));

    let first = site.capacity().await.expect_err("the read times out");
    let started = Instant::now();
    let second = site
        .capacity()
        .await
        .expect_err("the stuck read answers again");

    assert_eq!(first.code(), storage::CAPACITY_UNAVAILABLE.code);
    assert_eq!(first.detail("reason"), Some("timeout"));
    assert_eq!(
        first.hint(),
        Some("The disk that holds Stackly data didn’t answer. Check that it is still connected.")
    );
    assert_eq!(second.detail("reason"), Some("timeout"));
    assert!(
        started.elapsed() < Duration::from_millis(300),
        "the second call did not wait for a new read: {:?}",
        started.elapsed()
    );

    tokio::time::sleep(Duration::from_millis(800)).await;
    site.fake.fs().set_delay(FsOp::VolumeSpace, Duration::ZERO);
    let report = site.capacity().await.expect("the volume reads again");
    assert_eq!(report.level, CapacityLevel::Ok);
    ev.pass();
}

#[tokio::test]
async fn the_method_is_served_in_recovery_mode_and_changes_nothing() {
    let site = FakeSite::new();
    let first = site.start().await;
    first
        .shutdown(ShutdownReason::Requested, std::future::pending())
        .await;
    let db = site.prepared.layout().registry_db();
    let mut bytes = std::fs::read(&db).expect("the registry file exists");
    bytes[60..64].copy_from_slice(&999_u32.to_be_bytes());
    std::fs::write(&db, &bytes).expect("the registry file is written");
    let daemon = site.start().await;
    assert!(daemon.health().recovery.is_some());
    let mutations = site.fake.fs().mutations();
    let registry_before = std::fs::read(&db).expect("the registry file exists");

    let report = site
        .capacity()
        .await
        .expect("storage.get_capacity is served in recovery mode");

    assert_eq!(report.level, CapacityLevel::Ok);
    assert_eq!(site.fake.fs().mutations(), mutations);
    assert_eq!(
        std::fs::read(&db).expect("the registry file exists"),
        registry_before
    );
}
