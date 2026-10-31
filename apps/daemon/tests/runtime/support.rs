//! Helpers for the runtime tests: a site with a real filesystem and
//! registry but a fake download, fake code signing, and a fake process for
//! the version check, a catalog of one NATS like package whose artifact is
//! a small archive, and typed calls.

use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use sha2::{Digest, Sha256};
use stackly_core::catalog::{Catalog, CatalogEntry, ProviderEntry, TileColor, TileHint};
use stackly_core::provider::{NameCheck, PackageFiles, ValidatedPackage};
use stackly_core::runtime::samples::{MachOBuilder, TarBuilder};
use stackly_core::{PreparedLayout, prepare_layout};
use stackly_domain::Sha256Digest;
use stackly_ipc::{CallOptions, Client, ConnectPolicy};
use stackly_platform::{
    CapturedOutput, ChildRole, Clock, ExitStatus, Platform, SystemClock, UserId,
};
use stackly_platform_macos::MacosPlatform;
use stackly_protocol::handshake::{ClientInfo, ClientKind};
use stackly_protocol::operation::{OperationDetail, OperationState};
use stackly_protocol::runtime::InstallRuntimeParams;
use stackly_protocol::{Call, ErrorBody, Reply, Timestamp};
use stackly_testkit::{FakePlatform, TestRoot};
use stacklyd::catalog::CatalogOverride;
use stacklyd::lock::{DaemonLock, LockOwner};
use stacklyd::log_sink::{LogSink, LogSinkConfig};
use stacklyd::map::StartFacts;
use stacklyd::server::{self, DaemonConfig, DaemonDeps, DaemonHandle};
use uuid::Uuid;

/// How long a test waits for something that should happen.
pub const PATIENCE: Duration = Duration::from_secs(20);
/// Where the artifact of the test package is served from.
pub const URL: &str = "https://downloads.example.test/nats-2.15.1.tar.gz";

/// One test's daemon site.
pub struct Site {
    _root: TestRoot,
    pub fake: FakePlatform,
    pub platform: Platform,
    pub prepared: PreparedLayout,
    _lock: DaemonLock,
}

impl Site {
    pub fn new() -> Self {
        let root = TestRoot::new().expect("a test root can be created");
        let fake = FakePlatform::new(&root);
        let platform = MacosPlatform::with_roots(root.roots())
            .expect("this Mac reports its OS version")
            .with_fetcher(Arc::new(fake.fetcher().clone()))
            .with_code_signing(Arc::new(fake.code_signing().clone()))
            .with_processes(Arc::new(fake.processes().clone()));
        let prepared = prepare_layout(&platform).expect("the test root layout can be prepared");
        let owner = LockOwner::new(
            std::process::id(),
            Uuid::now_v7(),
            Timestamp::from_jiff(SystemClock.now()),
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

    /// Starts a daemon with `catalog` and waits for its start slot.
    pub async fn start(&self, catalog: Catalog) -> DaemonHandle {
        let log = LogSink::start(Box::new(io::sink()), LogSinkConfig::DAEMON)
            .expect("the log writer starts");
        let facts = StartFacts::new(
            Uuid::now_v7(),
            Timestamp::from_jiff(SystemClock.now()),
            true,
            std::process::id(),
        );
        let mut config = DaemonConfig::default();
        config.catalog = Some(CatalogOverride::Preloaded(catalog));
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

    /// Ends every version check the install spawns, as a binary that prints
    /// `output`.
    pub fn answer_checks(&self, output: &'static str) -> tokio::task::JoinHandle<()> {
        let fake = self.fake.clone();
        tokio::spawn(async move {
            let mut answered = 0;
            loop {
                let spawns: Vec<_> = fake
                    .processes()
                    .spawns()
                    .into_iter()
                    .filter(|spawn| spawn.role == ChildRole::RuntimeCheck)
                    .collect();
                for spawn in spawns.iter().skip(answered) {
                    fake.processes().exit_with_output(
                        spawn.pid,
                        Some(ExitStatus::Exited(0)),
                        CapturedOutput {
                            stdout: output.as_bytes().to_vec(),
                            stderr: Vec::new(),
                        },
                    );
                }
                answered = spawns.len();
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
    }
}

/// A typed client of the test's uid, as an interactive CLI.
pub async fn client(site: &Site) -> Client {
    Client::connect(
        &site.socket(),
        ConnectPolicy::same_user(site.uid()),
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

/// `call` with an idempotency key.
pub async fn call_with_key(
    client: &Client,
    call: Call,
    key: &str,
) -> Result<Reply, Box<ErrorBody>> {
    let mut options = CallOptions::default();
    options.idempotency_key = Some(key.to_owned());
    client.call_detailed(call, options).await
}

/// The params of an install of `selector` of `official/nats`.
pub fn nats(selector: &str) -> InstallRuntimeParams {
    InstallRuntimeParams {
        provider: "official/nats".to_owned(),
        selector: selector.to_owned(),
        expected_sha256: None,
    }
}

/// An arm64 `nats-server` inside a tar.gz under `pkg/`.
pub fn archive() -> Vec<u8> {
    let exec = MachOBuilder::executable().min_os(13, 0).build();
    TarBuilder::new()
        .dir("pkg", 0o755)
        .file("pkg/nats-server", 0o755, &exec)
        .file("pkg/README", 0o644, b"readme")
        .finish_gz()
}

/// The digest of `bytes`.
pub fn digest(bytes: &[u8]) -> Sha256Digest {
    Sha256Digest::from_bytes(Sha256::digest(bytes).into())
}

/// `providers/nats` with one artifact that describes `archive`.
pub fn package(archive: &[u8]) -> ValidatedPackage {
    package_with(archive, "")
}

/// [`package`] whose artifact also carries `extra`, JSON fields written
/// after `dependencies` with a leading comma, such as `compatibility`.
pub fn package_with(archive: &[u8], extra: &str) -> ValidatedPackage {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../providers/nats");
    let mut files: std::collections::BTreeMap<String, Vec<u8>> = [
        "provider.yaml",
        "schemas/settings.schema.json",
        "templates/nats.conf.tmpl",
    ]
    .into_iter()
    .map(|path| {
        (
            path.to_owned(),
            std::fs::read(dir.join(path)).expect("package file"),
        )
    })
    .collect();
    let versions = format!(
        r#"{{ "schema": "stackly.provider-versions/v1", "software": "nats", "artifacts": [
        {{ "id": "nats-2.15.1-macos-aarch64", "line": "2.15", "version": "2.15.1", "os": "macos",
           "arch": "aarch64", "status": "available", "minOs": "13.0", "format": "tar_gz",
           "urls": ["{URL}"], "sha256": "{}", "size": {}, "archiveRoot": "pkg",
           "signingPolicy": "keep_or_adhoc", "license": "Apache-2.0",
           "licenseUrl": "https://example.test/LICENSE", "executables": ["nats-server"],
           "stable": true, "unpackedSize": 1048576, "entryCount": 10,
           "dependencies": [ {{ "name": "libSystem.B.dylib", "kind": "system",
                                "installName": "/usr/lib/libSystem.B.dylib" }} ]{extra} }} ] }}"#,
        digest(archive),
        archive.len()
    );
    files.insert("versions.json".to_owned(), versions.into_bytes());
    ValidatedPackage::from_files(PackageFiles::new("nats", files), NameCheck::FolderName)
        .unwrap_or_else(|error| panic!("the test package validates: {:?}", error.issues))
}

/// A catalog of the packages, each as a provider entry.
pub fn catalog(packages: Vec<ValidatedPackage>) -> Catalog {
    let entries: Vec<CatalogEntry> = packages
        .into_iter()
        .map(|package| {
            CatalogEntry::Provider(ProviderEntry {
                package: Arc::new(package),
                tile: TileHint {
                    mark: "Tt".to_owned(),
                    tile: TileColor::Blue,
                },
                notes: Vec::new(),
            })
        })
        .collect();
    Catalog::for_tests(&entries)
}

/// Waits until the operation is in `state`.
pub async fn wait_state(client: &Client, id: Uuid, state: OperationState) -> Box<OperationDetail> {
    let deadline = tokio::time::Instant::now() + PATIENCE;
    loop {
        let Reply::OperationGet(detail) = client
            .call(Call::OperationGet(
                stackly_protocol::operation::GetOperationParams { operation_id: id },
            ))
            .await
            .expect("the operation can be read")
        else {
            panic!("expected an operation.get reply");
        };
        if detail.operation.state == state {
            return detail;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "operation {id} did not reach {state:?}: {:?}",
            detail.operation
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// Makes every folder under `providers/` private (0700), as `pin` writes
/// them. The fixture seed writes the pinned fake package with the
/// permissions of a test, which a create from the catalog refuses to pin
/// into (spec 0020, K-1).
pub fn make_pinned_folders_private(providers: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    let mut pending = vec![providers.to_path_buf()];
    while let Some(dir) = pending.pop() {
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))
            .expect("a provider folder can be made private");
        for entry in std::fs::read_dir(&dir).expect("a provider folder can be read") {
            let path = entry.expect("an entry can be read").path();
            if path.is_dir() {
                pending.push(path);
            }
        }
    }
}
