//! The native harness of the runtime install (spec 0018 AC-20): a test root
//! with the real macOS adapters behind recorders, the `official/<name>`
//! package of `providers/`, the in process daemon, and the artifact copied
//! from the runtime cache of `cargo xtask` into the daemon's download cache.
//! Nothing here downloads anything on its own: an artifact that is not in
//! the cache ends a test as `BLOCKED`.
#![allow(dead_code, reason = "each scenario uses a part of these helpers")]

use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use stackly_core::catalog::{Catalog, CatalogEntry, ProviderEntry, TileColor, TileHint};
use stackly_core::provider::{NameCheck, ValidatedPackage, read_package};
use stackly_core::{PreparedLayout, prepare_layout};
use stackly_ipc::{CallOptions, Client, ConnectPolicy};
use stackly_platform::{Clock, Fetcher, Platform, SystemClock, UserId};
use stackly_platform_macos::{MacosFetcher, MacosPlatform};
use stackly_protocol::handshake::{ClientInfo, ClientKind};
use stackly_protocol::operation::{GetOperationParams, OperationDetail, OperationState};
use stackly_protocol::runtime::InstallRuntimeParams;
use stackly_protocol::{Call, Reply, Timestamp};
use stackly_testkit::{ReapGuard, RuntimeCache, TestRoot};
use stacklyd::catalog::CatalogOverride;
use stacklyd::lock::{DaemonLock, LockOwner};
use stacklyd::log_sink::{LogSink, LogSinkConfig};
use stacklyd::map::StartFacts;
use stacklyd::server::{self, DaemonConfig, DaemonDeps, DaemonHandle};
use uuid::Uuid;

use crate::recorders::{Asked, NoNetwork, RecordingCodeSigning, RecordingProcesses, Spawned};

/// The owner who can fill the runtime cache (spec 0002).
pub const OWNER: &str = "@ojiepermana";

/// How long a native install may take: a 175 MB archive is hashed, unpacked,
/// and checked with `codesign` on a busy Mac.
pub const PATIENCE: Duration = Duration::from_secs(300);

/// One artifact of a package in `providers/`.
#[derive(Debug, Clone, Copy)]
pub struct Line {
    /// The package folder and software name.
    pub software: &'static str,
    /// The artifact ID in `versions.json`.
    pub artifact: &'static str,
    /// The selector a user would type: the line.
    pub selector: &'static str,
    /// The exact version the artifact has.
    pub version: &'static str,
}

/// NATS 2.15.
pub const NATS: Line = Line {
    software: "nats",
    artifact: "nats-2.15.1-macos-aarch64",
    selector: "2.15",
    version: "2.15.1",
};

/// Traefik 3.7.
pub const TRAEFIK: Line = Line {
    software: "traefik",
    artifact: "traefik-3.7.15-macos-aarch64",
    selector: "3.7",
    version: "3.7.15",
};

/// PostgreSQL 17.
pub const POSTGRESQL_17: Line = Line {
    software: "postgresql",
    artifact: "postgresql-17.11-r2-macos-aarch64",
    selector: "17",
    version: "17.11",
};

/// PostgreSQL 18.
pub const POSTGRESQL_18: Line = Line {
    software: "postgresql",
    artifact: "postgresql-18.6-r2-macos-aarch64",
    selector: "18",
    version: "18.6",
};

/// The folder of the package of `software`.
#[must_use]
pub fn package_dir(software: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../providers")
        .join(software)
}

/// The digest and size of `line` in its `versions.json`.
#[must_use]
pub fn artifact_facts(line: Line) -> (String, u64) {
    let text = std::fs::read_to_string(package_dir(line.software).join("versions.json"))
        .expect("versions.json can be read");
    let versions: serde_json::Value = serde_json::from_str(&text).expect("versions.json is JSON");
    let artifact = versions["artifacts"]
        .as_array()
        .and_then(|all| all.iter().find(|artifact| artifact["id"] == line.artifact))
        .unwrap_or_else(|| panic!("versions.json has {}", line.artifact));
    (
        artifact["sha256"]
            .as_str()
            .expect("an artifact has a sha256")
            .to_owned(),
        artifact["size"].as_u64().expect("an artifact has a size"),
    )
}

/// The cached file of `line`, or `None` when the spike never downloaded it.
#[must_use]
pub fn cached(line: Line) -> Option<(String, u64)> {
    let (sha256, size) = artifact_facts(line);
    RuntimeCache::open()?
        .entry(&sha256, size)
        .map(|_| (sha256, size))
}

/// The package of `software` in `providers/`, read and validated.
#[must_use]
pub fn package(platform: &Platform, software: &str) -> ValidatedPackage {
    let files = read_package(platform.fs(), &package_dir(software)).expect("the package reads");
    ValidatedPackage::from_files(files, NameCheck::FolderName).expect("the package validates")
}

/// One test's root, platform, and daemon.
pub struct Site {
    /// Declared first, so it drops first: it stops what still names this
    /// root, then the root goes.
    _reap: ReapGuard,
    pub root: TestRoot,
    pub platform: Platform,
    pub prepared: PreparedLayout,
    pub spawned: Arc<Mutex<Vec<Spawned>>>,
    pub asked: Arc<Mutex<Vec<Asked>>>,
    pub fetched: Arc<Mutex<Vec<String>>>,
    _lock: DaemonLock,
}

impl Site {
    /// A root whose downloads go nowhere: an install must find its artifact
    /// in the cache.
    pub fn offline() -> Self {
        let (fetcher, urls) = NoNetwork::new();
        Self::with_fetcher(Arc::new(fetcher), urls)
    }

    /// A root with the real network fetcher, for the run that was approved.
    pub fn online() -> Self {
        Self::with_fetcher(
            Arc::new(MacosFetcher::new()),
            Arc::new(Mutex::new(Vec::new())),
        )
    }

    fn with_fetcher(fetcher: Arc<dyn Fetcher>, fetched: Arc<Mutex<Vec<String>>>) -> Self {
        let root = TestRoot::new().expect("a test root can be created");
        let (processes, spawned) = RecordingProcesses::new();
        let (signing, asked) = RecordingCodeSigning::new();
        let platform = MacosPlatform::with_roots(root.roots())
            .expect("this Mac reports its OS version")
            .with_fetcher(fetcher)
            .with_code_signing(Arc::new(signing))
            .with_processes(Arc::new(processes));
        let prepared = prepare_layout(&platform).expect("the test root layout can be prepared");
        let reap = ReapGuard::new(
            root.path(),
            platform
                .directories()
                .roots()
                .expect("the macOS platform resolved its roots")
                .runtime(),
        );
        let owner = LockOwner::new(
            std::process::id(),
            Uuid::now_v7(),
            Timestamp::from_jiff(SystemClock.now()),
            "0.1.0",
        );
        let lock = DaemonLock::acquire(&prepared.layout().daemon_lock(), owner)
            .expect("the test root's lock is free");
        Self {
            _reap: reap,
            root,
            platform,
            prepared,
            spawned,
            asked,
            fetched,
            _lock: lock,
        }
    }

    /// Copies the cached file of `line` into the download cache of the
    /// daemon, where an install finds it as a finished download.
    pub fn fill_download_cache(&self, line: Line) {
        let (sha256, size) = artifact_facts(line);
        RuntimeCache::open()
            .expect("HOME is set")
            .copy_to(&sha256, size, &self.prepared.layout().downloads_dir())
            .expect("the cached artifact can be copied")
            .expect("the artifact is in the runtime cache");
    }

    /// The socket of the daemon.
    pub fn socket(&self) -> PathBuf {
        self.prepared.layout().daemon_socket()
    }

    fn uid(&self) -> u32 {
        match self
            .platform
            .identity()
            .current_user()
            .expect("the current user can be read")
        {
            UserId::Unix(uid) => uid,
            other => panic!("not a unix user: {other:?}"),
        }
    }

    /// Starts a daemon whose catalog holds the package of `software`.
    pub async fn start(&self, software: &str) -> DaemonHandle {
        let log = LogSink::start(Box::new(io::sink()), LogSinkConfig::DAEMON)
            .expect("the log writer starts");
        let facts = StartFacts::new(
            Uuid::now_v7(),
            Timestamp::from_jiff(SystemClock.now()),
            true,
            std::process::id(),
        );
        let entry = CatalogEntry::Provider(ProviderEntry {
            package: Arc::new(package(&self.platform, software)),
            tile: TileHint {
                mark: "Nt".to_owned(),
                tile: TileColor::Blue,
            },
            notes: Vec::new(),
        });
        let mut config = DaemonConfig::default();
        config.catalog = Some(CatalogOverride::Preloaded(Catalog::for_tests(&[entry])));
        let handle = server::start(
            DaemonDeps::new(
                self.platform.clone(),
                self.prepared.clone(),
                facts,
                Arc::new(log),
            )
            .with_config(config),
        )
        .await
        .expect("the server starts");
        handle.started().await;
        handle
    }

    /// A typed client of the test's uid, as an interactive CLI.
    pub async fn client(&self) -> Client {
        Client::connect(
            &self.socket(),
            ConnectPolicy::same_user(self.uid()),
            ClientInfo {
                kind: ClientKind::Cli,
                version: "0.1.0".to_owned(),
                pid: Some(std::process::id()),
                interactive: Some(true),
            },
        )
        .await
        .expect("the client connects")
        .0
    }
}

/// The params of an install of `selector` of `official/<software>`.
#[must_use]
pub fn install_params(software: &str, selector: &str) -> InstallRuntimeParams {
    InstallRuntimeParams {
        provider: format!("official/{software}"),
        selector: selector.to_owned(),
        expected_sha256: None,
    }
}

/// `call` with an idempotency key.
pub async fn install(
    client: &Client,
    params: InstallRuntimeParams,
    key: &str,
) -> Box<stackly_protocol::runtime::InstallRuntimeReply> {
    let mut options = CallOptions::default();
    options.idempotency_key = Some(key.to_owned());
    match client
        .call_detailed(Call::RuntimeInstall(params), options)
        .await
    {
        Ok(Reply::RuntimeInstall(reply)) => reply,
        other => panic!("expected a runtime.install reply, got {other:?}"),
    }
}

/// Waits until the operation ends and returns it, whatever the state.
pub async fn wait_end(client: &Client, id: Uuid) -> Box<OperationDetail> {
    let deadline = tokio::time::Instant::now() + PATIENCE;
    loop {
        let Reply::OperationGet(detail) = client
            .call(Call::OperationGet(GetOperationParams { operation_id: id }))
            .await
            .expect("the operation can be read")
        else {
            panic!("expected an operation.get reply");
        };
        if matches!(
            detail.operation.state,
            OperationState::Succeeded
                | OperationState::Failed
                | OperationState::Cancelled
                | OperationState::NeedsRecovery
        ) {
            return detail;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "operation {id} did not end: {:?}",
            detail.operation
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}
