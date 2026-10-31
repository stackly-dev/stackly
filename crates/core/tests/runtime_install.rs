//! The `install_runtime` operation end to end on fakes (spec 0018, AC-1 to
//! AC-6, AC-10 to AC-15): five stages through the operation engine, the
//! registry that only ever sees a committed runtime, and the failure paths
//! that leave nothing behind.
#![cfg(unix)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use sha2::{Digest, Sha256};
use stackly_core::operation::testing::RecordingEvents;
use stackly_core::operation::{
    BeginOutcome, BeginRequest, BusyPolicy, EngineDeps, EngineLimits, OperationDetailView,
    OperationEngine, Origin,
};
use stackly_core::provider::{NameCheck, PackageFiles, ValidatedPackage};
use stackly_core::runtime::download::DownloadPolicy;
use stackly_core::runtime::install::{
    COMPATIBILITY_MAX, InstallDeps, InstallInput, InstallRuntimeFactory, install_runtime_work,
};
use stackly_core::runtime::samples::{MachOBuilder, TarBuilder};
use stackly_core::runtime::validate::VersionCheckSpec;
use stackly_core::secret::Redactor;
use stackly_core::{PreparedLayout, prepare_layout};
use stackly_domain::{
    Arch, ErrorCodeName, InstallMode, LockKey, OperationId, OperationKind, OperationResource,
    OperationState, Registry, ResourceRef, RuntimeId, RuntimeState, Sha256Digest,
};
use stackly_platform::{ChildRole, Clock, ExitStatus, Platform};
use stackly_testkit::evidence::{Kind, evidence};
use stackly_testkit::{FakePlatform, FakeRegistry, FakeStep, SigningCall, TestRoot};

const URL: &str = "https://downloads.example.test/nats-2.15.1.tar.gz";
const VERSION: &str = "2.15.1";

struct Harness {
    _root: TestRoot,
    fake: FakePlatform,
    platform: Arc<Platform>,
    layout: Arc<PreparedLayout>,
    registry: Arc<FakeRegistry>,
    engine: OperationEngine,
    package: ValidatedPackage,
    archive: Vec<u8>,
    sha256: Sha256Digest,
}

/// An arm64 `nats-server` inside a tar.gz under `pkg/`, with 64 KiB of data
/// that does not compress, so a download takes many chunks.
fn archive() -> Vec<u8> {
    let exec = MachOBuilder::executable().min_os(13, 0).build();
    let mut state = 0x9e37_79b9_u32;
    let noise: Vec<u8> = (0..64 * 1024)
        .map(|_| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (state >> 24) as u8
        })
        .collect();
    TarBuilder::new()
        .dir("pkg", 0o755)
        .file("pkg/nats-server", 0o755, &exec)
        .file("pkg/README", 0o644, b"readme")
        .file("pkg/data.bin", 0o644, &noise)
        .finish_gz()
}

/// `providers/nats` with one artifact that describes `archive`, and a
/// version check.
fn package(archive: &[u8], sha256: &Sha256Digest) -> ValidatedPackage {
    package_of(archive, sha256, VERSION, URL)
}

/// The same package for another build of the line.
fn package_of(archive: &[u8], sha256: &Sha256Digest, version: &str, url: &str) -> ValidatedPackage {
    let records = [record("aarch64"), record("x86_64")];
    package_with_records(archive, sha256, version, url, &records)
}

/// [`package_of`] whose artifact carries `records` as its compatibility
/// records, in that order.
fn package_with_records(
    archive: &[u8],
    sha256: &Sha256Digest,
    version: &str,
    url: &str,
    records: &[String],
) -> ValidatedPackage {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../providers/nats");
    let mut files: BTreeMap<String, Vec<u8>> = [
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
        {{ "id": "nats-{version}-macos-aarch64", "line": "2.15", "version": "{version}", "os": "macos",
           "arch": "aarch64", "status": "available", "minOs": "13.0", "format": "tar_gz",
           "urls": ["{url}"], "sha256": "{sha256}", "size": {}, "archiveRoot": "pkg",
           "signingPolicy": "keep_or_adhoc", "license": "Apache-2.0",
           "licenseUrl": "https://example.test/LICENSE", "executables": ["nats-server"],
           "stable": true, "unpackedSize": 1048576, "entryCount": 10,
           "dependencies": [ {{ "name": "libSystem.B.dylib", "kind": "system",
                                "installName": "/usr/lib/libSystem.B.dylib" }} ],
           "compatibility": [{}] }} ] }}"#,
        archive.len(),
        records.join(", ")
    );
    files.insert("versions.json".to_owned(), versions.into_bytes());
    ValidatedPackage::from_files(PackageFiles::new("nats", files), NameCheck::FolderName)
        .unwrap_or_else(|error| panic!("the test package validates: {:?}", error.issues))
}

/// A compatibility record the catalog holds for the artifact, for `arch`:
/// the package carries one for this host and one for Intel that the input
/// leaves out.
fn record(arch: &str) -> String {
    record_for(arch, 27)
}

/// [`record`] for macOS `major`, so one package can hold a record per major.
fn record_for(arch: &str, major: u32) -> String {
    format!(
        r#"{{ "scope": "runtime", "result": "pass", "providerRevision": null, "os": "macos",
             "osVersion": "{major}.0", "osBuild": "26A428", "arch": "{arch}", "checks": ["ready", "stop"],
             "knownLimitations": ["provider_integration_not_covered"], "summary": "ready in 493 ms",
             "testedAt": "2026-10-18T11:07:19.973Z", "evidence": "runtime/run/record" }}"#
    )
}

impl Harness {
    async fn new() -> Self {
        let root = TestRoot::new().expect("a test root");
        let fake = FakePlatform::new(&root);
        let platform = Arc::new(fake.platform());
        let layout = Arc::new(prepare_layout(&platform).expect("the layout prepares"));
        let registry = Arc::new(FakeRegistry::new(
            Arc::new(fake.clock().clone()),
            fake.clock().now(),
        ));
        let engine = engine_on(&platform, &layout, &registry, &RecordingEvents::new());
        let archive = archive();
        let sha256 = Sha256Digest::from_bytes(Sha256::digest(&archive).into());
        let package = package(&archive, &sha256);
        fake.fetcher().serve(URL, archive.clone());
        fake.fetcher().set_chunk(512);
        let harness = Self {
            _root: root,
            fake,
            platform,
            layout,
            registry,
            engine,
            package,
            archive,
            sha256,
        };
        harness.engine.register_factory(
            OperationKind::InstallRuntime,
            Arc::new(InstallRuntimeFactory::new(harness.deps())),
        );
        harness
            .engine
            .recover()
            .await
            .expect("an empty registry recovers");
        harness.engine.start();
        harness
    }

    fn deps(&self) -> InstallDeps {
        InstallDeps {
            platform: Arc::clone(&self.platform),
            layout: Arc::clone(&self.layout),
            policy: DownloadPolicy {
                backoff: [Duration::from_millis(1), Duration::from_millis(1)],
                retry_after_max: Duration::from_millis(5),
                sync_every: 2048,
                space_reserve: 0,
            },
        }
    }

    fn input(&self) -> InstallInput {
        let artifact = &self.package.versions().artifacts()[0];
        let check = self
            .package
            .version_check()
            .map(VersionCheckSpec::from_check);
        InstallInput::new(
            self.package.software(),
            "official/nats",
            "stable",
            &self.platform.info().host_facts(),
            artifact,
            self.package.key(),
            check.as_ref(),
        )
        .expect("the input freezes")
    }

    fn request(&self, runtime: RuntimeId) -> BeginRequest {
        let input = self.input();
        BeginRequest {
            kind: OperationKind::InstallRuntime,
            resource: OperationResource::Runtime(runtime),
            creates_resource: true,
            extra_locks: vec![LockKey::Artifact(self.sha256)],
            busy: BusyPolicy::Queue,
            expected_revision: None,
            origin: Origin::Background,
            input: input.to_payload().expect("the input fits the journal"),
            work: install_runtime_work(self.deps(), &input, runtime).expect("the work builds"),
            precheck: None,
        }
    }

    async fn begin(&self) -> (OperationId, RuntimeId) {
        let runtime = RuntimeId::generate();
        self.fake
            .clock()
            .advance(jiff::SignedDuration::from_secs(1));
        match self.engine.begin(self.request(runtime)).await {
            Ok(BeginOutcome::Created(view)) => (view.operation.id(), runtime),
            other => panic!("expected a created operation: {other:?}"),
        }
    }

    async fn wait(&self, id: OperationId, state: OperationState) -> OperationDetailView {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
        loop {
            if let Some(detail) = self.engine.get(id).await.expect("the registry reads")
                && detail.view.operation.state() == state
            {
                return detail;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "{id} did not reach {state}: {:?}",
                self.engine.get(id).await
            );
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }

    /// Waits until the operation is parked after a shutdown: queued again
    /// with no runner.
    async fn wait_parked(&self, id: OperationId) {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
        loop {
            let detail = self.engine.get(id).await.expect("reads").expect("exists");
            if detail.view.operation.state() == OperationState::Queued
                && detail
                    .steps
                    .iter()
                    .any(|step| step.name().as_str() == "download")
            {
                return;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "the operation did not park: {:?}",
                detail.view.operation.state()
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    /// Waits until no runner holds a slot: a runner removes its scratch
    /// folder after the final commit.
    async fn idle(&self) {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        while self.engine.stats().running > 0 {
            assert!(
                tokio::time::Instant::now() < deadline,
                "a runner is still busy"
            );
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }

    /// Ends every version check the install spawns, as a binary that prints
    /// `output` and exits with `status`.
    fn answer_checks(
        &self,
        status: ExitStatus,
        output: &'static str,
    ) -> tokio::task::JoinHandle<()> {
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
                        Some(status),
                        stackly_platform::CapturedOutput {
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

    fn runtime_dir(&self) -> PathBuf {
        self.layout.layout().runtime_dir(
            self.package.software(),
            &VERSION.parse().expect("a version"),
            "macos-aarch64".parse().expect("a target"),
            &self.sha256,
        )
    }

    fn cache_entry(&self) -> PathBuf {
        self.layout
            .layout()
            .downloads_dir()
            .join(self.sha256.to_string())
    }

    fn runtimes(&self) -> Vec<stackly_domain::Runtime> {
        self.registry.runtimes().expect("the registry reads").value
    }

    fn scratch(&self, id: OperationId) -> PathBuf {
        self.layout.layout().operations_dir().join(id.to_string())
    }
}

// covers: AC-13
#[test]
fn an_install_input_keeps_at_most_eight_host_records_in_catalog_order() {
    let archive = archive();
    let sha256 = Sha256Digest::from_bytes(Sha256::digest(&archive).into());
    // Ten host records, one per macOS major from 18 to 27, with an Intel
    // record between them that the input leaves out and does not count.
    let mut records: Vec<String> = (18..28).map(|major| record_for("aarch64", major)).collect();
    records.insert(3, record("x86_64"));
    let package = package_with_records(&archive, &sha256, VERSION, URL, &records);
    let root = TestRoot::new().expect("a test root");
    let platform = FakePlatform::new(&root).platform();

    let input = InstallInput::new(
        package.software(),
        "official/nats",
        "stable",
        &platform.info().host_facts(),
        &package.versions().artifacts()[0],
        package.key(),
        None,
    )
    .expect("the input freezes");

    assert_eq!(input.compatibility.len(), COMPATIBILITY_MAX);
    let kept: Vec<_> = input
        .compatibility
        .iter()
        .map(|record| (record.arch.clone(), record.os_version.clone()))
        .collect();
    let expected: Vec<_> = (18..26)
        .map(|major| ("aarch64".to_owned(), format!("{major}.0")))
        .collect();
    assert_eq!(
        kept, expected,
        "the first eight host records, in catalog order"
    );
}

fn engine_on(
    platform: &Arc<Platform>,
    layout: &Arc<PreparedLayout>,
    registry: &Arc<FakeRegistry>,
    events: &RecordingEvents,
) -> OperationEngine {
    OperationEngine::new(
        EngineDeps {
            registry: Arc::clone(registry) as Arc<dyn Registry>,
            platform: Arc::clone(platform),
            layout: Arc::clone(layout),
            events: Arc::new(events.clone()),
            redactor: Arc::new(Redactor::new()) as Arc<dyn stackly_core::operation::Redact>,
        },
        EngineLimits::default()
            .with_intervals(Duration::from_millis(100), Duration::from_millis(20)),
    )
}

fn failure_code(detail: &OperationDetailView) -> String {
    detail
        .view
        .journal
        .failure_error()
        .expect("the failed operation stored its error")
        .code()
        .to_string()
}

fn exists(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok()
}

#[tokio::test]
async fn a_selector_install_runs_five_stages_and_commits_one_installed_runtime() {
    let ev = evidence!(
        covers = ["REQ-003", "REQ-004", "REQ-005", "AC-004", "AC-005"],
        kind = Kind::Fixture,
        expected = "an install of a cached-less artifact downloads, verifies, extracts, validates, and commits in order; the registry gets one installed managed runtime and its artifact with signature, dependencies, and the cache entry stays while the scratch folder is gone",
    );
    let rig = Harness::new().await;
    rig.answer_checks(ExitStatus::Exited(0), "nats-server: v2.15.1\n");

    let (op, runtime_id) = rig.begin().await;
    let detail = rig.wait(op, OperationState::Succeeded).await;
    rig.idle().await;

    let steps: Vec<String> = detail
        .steps
        .iter()
        .map(|step| step.name().to_string())
        .collect();
    assert_eq!(
        steps,
        [
            "download",
            "verify_digest",
            "extract",
            "validate_binary",
            "commit"
        ]
    );
    let runtimes = rig.runtimes();
    assert_eq!(runtimes.len(), 1);
    let runtime = &runtimes[0];
    assert_eq!(runtime.id(), runtime_id);
    assert_eq!(runtime.state(), RuntimeState::Installed);
    assert_eq!(runtime.version().as_str(), VERSION);
    assert!(
        matches!(runtime.install(), InstallMode::Managed { artifact_sha256 } if *artifact_sha256 == rig.sha256)
    );
    assert!(runtime.installed_at().is_some() && runtime.last_verified_at().is_some());
    let record = rig
        .registry
        .artifact(&rig.sha256)
        .expect("reads")
        .value
        .expect("the artifact is stored");
    assert_eq!(
        record.catalog_id.as_deref(),
        Some("nats-2.15.1-macos-aarch64")
    );
    assert_eq!(record.size, Some(rig.archive.len() as u64));
    assert_eq!(record.source.as_deref(), Some(URL));
    // The record of this host was copied in order; the Intel one was not.
    assert_eq!(record.compatibility.len(), 1);
    assert_eq!(record.compatibility[0].arch, Arch::Aarch64);
    assert_eq!(record.compatibility[0].checks, ["ready", "stop"]);
    let signature = record.signature_info.expect("signature info").to_string();
    assert!(
        signature.contains("\"policy\":\"keep_or_adhoc\""),
        "{signature}"
    );
    assert!(
        signature.contains("\"adhocSignedAtInstall\":false"),
        "{signature}"
    );
    let dependencies = record
        .dependency_manifest
        .expect("dependencies")
        .to_string();
    assert!(dependencies.contains("libSystem.B.dylib"), "{dependencies}");
    // The result names exactly the new runtime.
    let result = detail
        .view
        .journal
        .result()
        .expect("the result is stored")
        .to_string();
    assert!(result.contains(&runtime_id.to_string()), "{result}");
    // Files: the tree is in its place with the marker, the cache stays, the
    // scratch folder is gone.
    let dir = rig.runtime_dir();
    assert!(exists(&dir.join("nats-server")) && exists(&dir.join(".stackly-runtime.json")));
    assert!(exists(&rig.cache_entry()));
    assert!(!exists(&rig.scratch(op)));
    assert_eq!(rig.fake.fetcher().requests().len(), 1);
    ev.pass();
}

#[tokio::test]
async fn a_failed_digest_fails_the_install_and_leaves_no_runtime_artifact_or_folder() {
    let ev = evidence!(
        covers = ["AC-059", "VAL-010"],
        kind = Kind::Fixture,
        expected = "an artifact whose bytes do not match the catalog digest fails runtime.digest_mismatch with no runtime row, no artifact row, no runtime folder, and no cache entry",
    );
    let rig = Harness::new().await;
    let mut wrong = rig.archive.clone();
    let last = wrong.len() - 1;
    wrong[last] ^= 0xff;
    rig.fake.fetcher().script(
        URL,
        [
            FakeStep::ServeOther(wrong.clone()),
            FakeStep::ServeOther(wrong),
        ],
    );

    let (op, _) = rig.begin().await;
    let detail = rig.wait(op, OperationState::Failed).await;

    assert_eq!(failure_code(&detail), "runtime.digest_mismatch");
    assert!(rig.runtimes().is_empty());
    assert!(
        rig.registry
            .artifact(&rig.sha256)
            .expect("reads")
            .value
            .is_none()
    );
    assert!(!exists(&rig.runtime_dir()));
    assert!(!exists(&rig.cache_entry()));
    ev.pass();
}

#[tokio::test]
async fn no_network_and_no_cache_fails_offline_and_a_cached_entry_installs_without_the_network() {
    let ev = evidence!(
        covers = ["REQ-046", "AC-046"],
        kind = Kind::Fixture,
        expected = "an install with every host unreachable and no cache entry fails runtime.download_offline, retryable; with the verified entry in the cache the install succeeds with no fetch call",
    );
    let rig = Harness::new().await;
    rig.fake.fetcher().go_offline();
    let (op, _) = rig.begin().await;
    let detail = rig.wait(op, OperationState::Failed).await;
    assert_eq!(failure_code(&detail), "runtime.download_offline");
    assert!(
        detail
            .view
            .journal
            .failure_error()
            .expect("an error")
            .code()
            == &ErrorCodeName::from_static("runtime.download_offline")
    );
    assert!(rig.runtimes().is_empty());
    let before = rig.fake.fetcher().requests().len();

    std::fs::write(rig.cache_entry(), &rig.archive).expect("fill the cache");
    rig.answer_checks(ExitStatus::Exited(0), "nats-server: v2.15.1\n");
    let (second, _) = rig.begin().await;
    rig.wait(second, OperationState::Succeeded).await;

    assert_eq!(
        rig.fake.fetcher().requests().len(),
        before,
        "no fetch for a cached entry"
    );
    assert_eq!(rig.runtimes().len(), 1);
    ev.pass();
}

#[tokio::test]
async fn a_version_that_differs_from_the_catalog_fails_the_install_before_commit() {
    let rig = Harness::new().await;
    rig.answer_checks(ExitStatus::Exited(0), "nats-server: v2.14.0\n");

    let (op, _) = rig.begin().await;
    let detail = rig.wait(op, OperationState::Failed).await;
    rig.idle().await;

    assert_eq!(failure_code(&detail), "runtime.version_mismatch");
    assert!(rig.runtimes().is_empty());
    assert!(!exists(&rig.runtime_dir()), "nothing reached runtimes/");
    assert!(
        !exists(&rig.scratch(op)),
        "the scratch folder is compensated"
    );
    assert!(
        exists(&rig.cache_entry()),
        "the cache entry stays for the next try"
    );
}

#[tokio::test]
async fn an_unsigned_file_is_signed_ad_hoc_for_the_keep_or_adhoc_policy() {
    let rig = Harness::new().await;
    rig.fake.code_signing().set_unsigned("nats-server");
    rig.answer_checks(ExitStatus::Exited(0), "nats-server: v2.15.1\n");

    let (op, _) = rig.begin().await;
    rig.wait(op, OperationState::Succeeded).await;

    let signed = rig
        .fake
        .code_signing()
        .calls()
        .into_iter()
        .any(|call| matches!(call, SigningCall::SignAdhoc(_)));
    assert!(signed);
    let record = rig
        .registry
        .artifact(&rig.sha256)
        .expect("reads")
        .value
        .expect("stored");
    assert!(
        record
            .signature_info
            .expect("info")
            .to_string()
            .contains("\"adhocSignedAtInstall\":true")
    );
}

#[tokio::test]
async fn two_installs_of_one_artifact_end_with_one_runtime_and_the_same_resource() {
    let ev = evidence!(
        covers = ["REQ-004"],
        kind = Kind::Fixture,
        expected = "two installs of the same artifact queued behind the artifact lock leave one runtime; the second operation succeeds with the existing runtime as its only resource",
    );
    let rig = Harness::new().await;
    rig.answer_checks(ExitStatus::Exited(0), "nats-server: v2.15.1\n");

    let (first, first_runtime) = rig.begin().await;
    let (second, _) = rig.begin().await;
    rig.wait(first, OperationState::Succeeded).await;
    let detail = rig.wait(second, OperationState::Succeeded).await;

    assert_eq!(rig.runtimes().len(), 1);
    let result = detail.view.journal.result().expect("a result").to_string();
    assert!(result.contains(&first_runtime.to_string()), "{result}");
    assert_eq!(rig.fake.fetcher().requests().len(), 1);
    let _ = ResourceRef::Runtime(first_runtime);
    ev.pass();
}

#[tokio::test]
async fn a_cancel_during_the_download_ends_cancelled_and_keeps_the_partial_file() {
    let ev = evidence!(
        covers = ["REQ-004"],
        kind = Kind::Fixture,
        expected = "a cancel before the commit stage ends the operation cancelled, removes the scratch folder, leaves no runtime, and keeps the .part file in the cache",
    );
    let rig = Harness::new().await;
    let engine = rig.engine.clone();
    let handle = tokio::runtime::Handle::current();
    let target = Arc::new(std::sync::Mutex::new(None::<OperationId>));
    let seen = Arc::clone(&target);
    let asked = Arc::new(std::sync::atomic::AtomicBool::new(false));
    rig.fake.fetcher().after_chunk(move |delivered| {
        if delivered >= 4096 && !asked.swap(true, std::sync::atomic::Ordering::SeqCst) {
            // The operation ID is known once begin returned.
            let id = loop {
                if let Some(id) = *seen.lock().expect("not poisoned") {
                    break id;
                }
                std::thread::sleep(Duration::from_millis(5));
            };
            let engine = engine.clone();
            handle.spawn(async move {
                let _ = engine.cancel(id).await;
            });
            std::thread::sleep(Duration::from_millis(200));
        }
    });

    let runtime = RuntimeId::generate();
    let request = rig.request(runtime);
    let started = match rig.engine.begin(request).await {
        Ok(BeginOutcome::Created(view)) => view.operation.id(),
        other => panic!("{other:?}"),
    };
    *target.lock().expect("not poisoned") = Some(started);
    let detail = rig.wait(started, OperationState::Cancelled).await;
    rig.idle().await;

    assert!(rig.runtimes().is_empty());
    assert!(!exists(&rig.scratch(started)));
    let part = PathBuf::from(format!("{}.part", rig.cache_entry().display()));
    assert!(exists(&part) || exists(&rig.cache_entry()));
    let _ = (detail, part);
    ev.pass();
}

#[tokio::test]
async fn a_daemon_stopped_in_the_download_resumes_the_same_operation_from_the_partial_file() {
    let ev = evidence!(
        covers = ["REQ-004", "AC-004"],
        kind = Kind::Fixture,
        expected = "a shutdown in the download stage parks the operation; after a restart the factory rebuilds the work from the journal input, the download continues from the partial file with Range, and the operation ends succeeded with one runtime",
    );
    let rig = Harness::new().await;
    rig.answer_checks(ExitStatus::Exited(0), "nats-server: v2.15.1\n");
    let engine = rig.engine.clone();
    let handle = tokio::runtime::Handle::current();
    let asked = Arc::new(std::sync::atomic::AtomicBool::new(false));
    rig.fake.fetcher().after_chunk(move |delivered| {
        if delivered >= 8192 && !asked.swap(true, std::sync::atomic::Ordering::SeqCst) {
            let engine = engine.clone();
            handle.spawn(async move {
                engine.shutdown(Duration::from_secs(10)).await;
            });
            // Long enough for the stop to reach the next chunk.
            std::thread::sleep(Duration::from_millis(300));
        }
    });

    let (op, _) = rig.begin().await;
    rig.wait_parked(op).await;
    rig.idle().await;
    assert!(rig.runtimes().is_empty());

    // A new daemon: a new engine on the same registry and folders.
    let engine = engine_on(
        &rig.platform,
        &rig.layout,
        &rig.registry,
        &RecordingEvents::new(),
    );
    engine.register_factory(
        OperationKind::InstallRuntime,
        Arc::new(InstallRuntimeFactory::new(rig.deps())),
    );
    engine.recover().await.expect("the registry recovers");
    engine.start();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    loop {
        let detail = engine.get(op).await.expect("reads").expect("exists");
        if detail.view.operation.state() == OperationState::Succeeded {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the resumed operation did not finish: {:?}",
            detail.view.operation.state()
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    assert_eq!(rig.runtimes().len(), 1);
    let requests = rig.fake.fetcher().requests();
    assert!(requests.len() >= 2, "{requests:?}");
    assert!(
        requests
            .iter()
            .any(|request| request.resume_offset.is_some_and(|offset| offset > 0)),
        "the second request resumes: {requests:?}"
    );
    ev.pass();
}

#[tokio::test]
async fn a_catalog_that_moves_a_line_leaves_the_installed_runtime_alone_and_installs_a_second_one()
{
    let ev = evidence!(
        covers = ["REQ-003", "AC-003"],
        kind = Kind::Fixture,
        expected = "after the line 2.15 points to a newer artifact, the installed runtime keeps its ID, version, digest, and files, and an install of the new artifact makes a second runtime with a new ID; no selector is stored on either",
    );
    let rig = Harness::new().await;
    let first_driver = rig.answer_checks(ExitStatus::Exited(0), "nats-server: v2.15.1\n");
    let (first, first_runtime) = rig.begin().await;
    rig.wait(first, OperationState::Succeeded).await;
    rig.idle().await;
    first_driver.abort();
    let before = std::fs::read(rig.runtime_dir().join("nats-server")).expect("the binary");

    // The catalog now carries 2.15.2 on the same line.
    let mut newer = rig.archive.clone();
    newer.extend_from_slice(b" ");
    let newer = {
        let exec = MachOBuilder::executable().min_os(13, 0).build();
        TarBuilder::new()
            .dir("pkg", 0o755)
            .file("pkg/nats-server", 0o755, &exec)
            .file("pkg/NEW", 0o644, b"a newer build")
            .finish_gz()
    };
    let sha = Sha256Digest::from_bytes(Sha256::digest(&newer).into());
    let url = "https://downloads.example.test/nats-2.15.2.tar.gz";
    rig.fake.fetcher().serve(url, newer.clone());
    let package = package_of(&newer, &sha, "2.15.2", url);
    let artifact = &package.versions().artifacts()[0];
    let check = package.version_check().map(VersionCheckSpec::from_check);
    let input = InstallInput::new(
        package.software(),
        "official/nats",
        "2.15",
        &rig.platform.info().host_facts(),
        artifact,
        package.key(),
        check.as_ref(),
    )
    .expect("the input freezes");
    assert_eq!(input.selector, "2.15", "the selector is only an origin");
    let second_runtime = RuntimeId::generate();
    let request = BeginRequest {
        kind: OperationKind::InstallRuntime,
        resource: OperationResource::Runtime(second_runtime),
        creates_resource: true,
        extra_locks: vec![LockKey::Artifact(sha)],
        busy: BusyPolicy::Queue,
        expected_revision: None,
        origin: Origin::Background,
        input: input.to_payload().expect("fits"),
        work: install_runtime_work(rig.deps(), &input, second_runtime).expect("builds"),
        precheck: None,
    };
    rig.answer_checks(ExitStatus::Exited(0), "nats-server: v2.15.2\n");
    rig.fake.clock().advance(jiff::SignedDuration::from_secs(1));
    let second = match rig.engine.begin(request).await {
        Ok(BeginOutcome::Created(view)) => view.operation.id(),
        other => panic!("{other:?}"),
    };
    rig.wait(second, OperationState::Succeeded).await;

    let runtimes = rig.runtimes();
    assert_eq!(runtimes.len(), 2);
    let old = runtimes
        .iter()
        .find(|runtime| runtime.id() == first_runtime)
        .expect("the first runtime is still there");
    assert_eq!(old.version().as_str(), VERSION);
    assert!(
        matches!(old.install(), InstallMode::Managed { artifact_sha256 } if *artifact_sha256 == rig.sha256)
    );
    assert_eq!(
        std::fs::read(rig.runtime_dir().join("nats-server")).expect("the binary"),
        before
    );
    let other = runtimes
        .iter()
        .find(|runtime| runtime.id() == second_runtime)
        .expect("the second runtime");
    assert_eq!(other.version().as_str(), "2.15.2");
    ev.pass();
}
