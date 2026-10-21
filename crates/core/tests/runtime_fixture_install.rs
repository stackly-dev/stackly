//! `runtime::fixture::install_cached_artifact` (spec 0018, *API surface*):
//! the install stages run directly against an artifact file that is already
//! on disk, with no operation engine and no network.
#![cfg(unix)]

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use sha2::{Digest, Sha256};
use stackly_core::prepare_layout;
use stackly_core::provider::{NameCheck, PackageFiles, ValidatedPackage};
use stackly_core::runtime::fixture::install_cached_artifact;
use stackly_core::runtime::install::InstallError;
use stackly_core::runtime::samples::{MachOBuilder, TarBuilder};
use stackly_domain::{InstallMode, Registry, RuntimeState, Sha256Digest};
use stackly_platform::{CapturedOutput, ChildRole, Clock, ExitStatus};
use stackly_testkit::{FakePlatform, FakeRegistry, TestRoot};

fn archive() -> Vec<u8> {
    let exec = MachOBuilder::executable().min_os(13, 0).build();
    TarBuilder::new()
        .dir("pkg", 0o755)
        .file("pkg/nats-server", 0o755, &exec)
        .finish_gz()
}

fn package(archive: &[u8], sha256: &Sha256Digest) -> ValidatedPackage {
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
        {{ "id": "nats-2.15.1-macos-aarch64", "line": "2.15", "version": "2.15.1", "os": "macos",
           "arch": "aarch64", "status": "available", "minOs": "13.0", "format": "tar_gz",
           "urls": ["https://downloads.example.test/n.tar.gz"], "sha256": "{sha256}", "size": {},
           "archiveRoot": "pkg", "signingPolicy": "keep_or_adhoc", "license": "Apache-2.0",
           "licenseUrl": "https://example.test/LICENSE", "executables": ["nats-server"],
           "stable": true }} ] }}"#,
        archive.len()
    );
    files.insert("versions.json".to_owned(), versions.into_bytes());
    ValidatedPackage::from_files(PackageFiles::new("nats", files), NameCheck::FolderName)
        .unwrap_or_else(|error| panic!("the test package validates: {:?}", error.issues))
}

#[test]
fn a_cached_artifact_installs_without_an_engine_or_a_network_and_is_committed_once() {
    let root = TestRoot::new().expect("a test root");
    let fake = FakePlatform::new(&root);
    let platform = fake.platform();
    let layout = prepare_layout(&platform).expect("the layout prepares");
    let registry = FakeRegistry::new(Arc::new(fake.clock().clone()), fake.clock().now());
    let archive = archive();
    let sha256 = Sha256Digest::from_bytes(Sha256::digest(&archive).into());
    let package = package(&archive, &sha256);
    let cache_file = root.path().join("input").join("nats.tar.gz");
    std::fs::create_dir_all(cache_file.parent().expect("a parent")).expect("a folder");
    std::fs::write(&cache_file, &archive).expect("the cache file is written");
    // The exec check is a fake process: end it as a binary that prints its
    // version.
    let done = Arc::new(AtomicBool::new(false));
    let answering = {
        let (fake, done) = (fake.clone(), Arc::clone(&done));
        std::thread::spawn(move || {
            while !done.load(Ordering::SeqCst) {
                for spawn in fake.processes().spawns() {
                    if spawn.role == ChildRole::RuntimeCheck {
                        fake.processes().exit_with_output(
                            spawn.pid,
                            Some(ExitStatus::Exited(0)),
                            CapturedOutput {
                                stdout: b"nats-server: v2.15.1\n".to_vec(),
                                stderr: Vec::new(),
                            },
                        );
                    }
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        })
    };

    let (runtime, record) = install_cached_artifact(
        &platform,
        &layout,
        &registry,
        &package,
        "nats-2.15.1-macos-aarch64",
        &cache_file,
    )
    .expect("the cached artifact installs");
    done.store(true, Ordering::SeqCst);
    answering.join().expect("the answering thread ends");

    assert_eq!(runtime.state(), RuntimeState::Installed);
    assert!(
        matches!(runtime.install(), InstallMode::Managed { artifact_sha256 } if *artifact_sha256 == sha256)
    );
    assert_eq!(record.sha256, sha256);
    assert_eq!(
        record.catalog_id.as_deref(),
        Some("nats-2.15.1-macos-aarch64")
    );
    // Both records are already in the registry: the helper wrote them.
    assert_eq!(registry.runtimes().expect("reads").value.len(), 1);
    assert!(registry.artifact(&sha256).expect("reads").value.is_some());
    assert!(
        layout
            .layout()
            .runtime_dir(
                package.software(),
                &"2.15.1".parse().expect("a version"),
                "macos-aarch64".parse().expect("a target"),
                &sha256
            )
            .join("nats-server")
            .exists()
    );
    // The scratch folder is gone.
    assert!(
        std::fs::read_dir(layout.layout().operations_dir())
            .expect("the operations folder reads")
            .next()
            .is_none()
    );
}

#[test]
fn an_artifact_the_package_lacks_and_a_file_with_another_digest_fail_with_pipeline_errors() {
    let root = TestRoot::new().expect("a test root");
    let fake = FakePlatform::new(&root);
    let platform = fake.platform();
    let layout = prepare_layout(&platform).expect("the layout prepares");
    let registry = FakeRegistry::new(Arc::new(fake.clock().clone()), fake.clock().now());
    let archive = archive();
    let sha256 = Sha256Digest::from_bytes(Sha256::digest(&archive).into());
    let package = package(&archive, &sha256);
    let wrong = root.path().join("wrong.tar.gz");
    std::fs::write(&wrong, b"not the artifact at all").expect("a file");

    let missing = install_cached_artifact(&platform, &layout, &registry, &package, "nope", &wrong);
    let digest = install_cached_artifact(
        &platform,
        &layout,
        &registry,
        &package,
        "nats-2.15.1-macos-aarch64",
        &wrong,
    );

    assert!(matches!(missing, Err(InstallError::Internal(_))));
    assert!(
        matches!(
            digest,
            Err(InstallError::SizeMismatch { .. } | InstallError::DigestMismatch { .. })
        ),
        "{digest:?}"
    );
    assert!(registry.runtimes().expect("reads").value.is_empty());
}
