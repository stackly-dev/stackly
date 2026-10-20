//! `seed_fake` on `FakePlatform` and `FakeRegistry` (spec 0014, AC-1 and
//! AC-2): an empty root gets the fake runtime and package, a second run
//! writes nothing, a missing part is made alone, and a leftover folder or a
//! different digest is refused before anything is written.
#![cfg(unix)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use stackly_core::supervisor::fixture::{FAKE_EXECUTABLE, FixtureError, SeededFake, seed_fake};
use stackly_domain::{InstallMode, Registry};
use stackly_platform::{Clock, DirectoryLayout, Platform};
use stackly_testkit::{FakePlatform, FakeRegistry, TestRoot};

const SERVER_BYTES: &[u8] = b"#!/bin/sh\nexit 0\n";

struct Site {
    _root: TestRoot,
    platform: Platform,
    layout: DirectoryLayout,
    registry: FakeRegistry,
    server: PathBuf,
    package: PathBuf,
}

impl Site {
    fn new() -> Self {
        let root = TestRoot::new().expect("a test root can be made");
        let fake = FakePlatform::new(&root);
        let platform = fake.platform();
        let layout = DirectoryLayout::new(
            platform
                .directories()
                .roots()
                .expect("the fake platform resolves its roots"),
        );
        let registry = FakeRegistry::new(Arc::new(fake.clock().clone()), fake.clock().now());
        let server = root.path().join("input").join(FAKE_EXECUTABLE);
        fs::create_dir_all(server.parent().expect("the server path has a parent"))
            .expect("the input folder can be made");
        fs::write(&server, SERVER_BYTES).expect("the stand in server can be written");
        let package = root.path().join("input").join("package");
        copy_dir(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../providers/fake"),
            &package,
        );
        Self {
            _root: root,
            platform,
            layout,
            registry,
            server,
            package,
        }
    }

    fn seed(&self) -> Result<SeededFake, FixtureError> {
        seed_fake(
            &self.platform,
            &self.layout,
            &self.registry,
            &self.server,
            &self.package,
        )
    }

    fn generation(&self) -> u64 {
        self.registry
            .runtimes()
            .expect("the registry reads")
            .generation
            .get()
    }

    fn runtime_count(&self) -> usize {
        self.registry
            .runtimes()
            .expect("the registry reads")
            .value
            .len()
    }
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).expect("the target folder can be made");
    for entry in fs::read_dir(from).expect("the source folder reads") {
        let entry = entry.expect("an entry reads");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("the type reads").is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target).expect("a file copies");
        }
    }
}

/// Spec 0014 AC-1.
#[test]
fn an_empty_root_gets_the_runtime_the_artifact_and_the_provider_in_one_commit() {
    let site = Site::new();

    let seeded = site.seed().expect("an empty root seeds");

    assert!(seeded.created);
    assert_eq!(site.generation(), 1, "one batch holds all three records");
    let runtimes = site.registry.runtimes().expect("the registry reads").value;
    assert_eq!(runtimes.len(), 1);
    assert_eq!(runtimes[0].id(), seeded.runtime_id);
    let InstallMode::Managed { artifact_sha256 } = runtimes[0].install() else {
        panic!("the seeded runtime is managed");
    };
    assert!(
        site.registry
            .artifact(artifact_sha256)
            .expect("the registry reads")
            .value
            .is_some()
    );
    assert!(
        site.registry
            .provider_revision(&seeded.provider)
            .expect("the registry reads")
            .value
            .is_some()
    );
    assert!(site.layout.provider_revision_dir(&seeded.provider).is_dir());
}

/// Spec 0014 AC-1.
#[test]
fn seeding_again_writes_nothing_and_returns_the_same_ids() {
    let site = Site::new();
    let first = site.seed().expect("an empty root seeds");
    let generation = site.generation();

    let second = site.seed().expect("a seeded root seeds again");

    assert!(!second.created);
    assert_eq!(second.runtime_id, first.runtime_id);
    assert_eq!(second.provider, first.provider);
    assert_eq!(site.generation(), generation, "no commit ran");
    assert_eq!(site.runtime_count(), 1);
}

/// Spec 0014 AC-1.
#[test]
fn a_root_with_the_runtime_only_gets_the_provider_alone() {
    let site = Site::new();
    let first = site.seed().expect("an empty root seeds");
    // A fresh registry that holds the runtime rows only, and no provider
    // folder on disk.
    let fresh = FakeRegistry::new(
        Arc::new(
            FakePlatform::new(&TestRoot::new().expect("a root"))
                .clock()
                .clone(),
        ),
        site.platform.clock().now(),
    );
    let runtime = site
        .registry
        .runtimes()
        .expect("the registry reads")
        .value
        .remove(0);
    let InstallMode::Managed { artifact_sha256 } = runtime.install().clone() else {
        panic!("the seeded runtime is managed");
    };
    let artifact = site
        .registry
        .artifact(&artifact_sha256)
        .expect("the registry reads")
        .value
        .expect("the artifact is stored");
    fresh
        .commit(stackly_domain::WriteBatch::new(vec![
            stackly_domain::WriteOp::InsertArtifact(artifact),
            stackly_domain::WriteOp::InsertRuntime(runtime),
        ]))
        .expect("the runtime row commits");
    fs::remove_dir_all(site.layout.provider_revision_dir(&first.provider))
        .expect("the provider folder goes");

    let seeded = seed_fake(
        &site.platform,
        &site.layout,
        &fresh,
        &site.server,
        &site.package,
    )
    .expect("the missing provider is made alone");

    assert!(seeded.created);
    assert_eq!(seeded.runtime_id, first.runtime_id);
    assert!(
        fresh
            .provider_revision(&seeded.provider)
            .expect("the registry reads")
            .value
            .is_some()
    );
    assert_eq!(fresh.runtimes().expect("reads").value.len(), 1);
}

/// Spec 0014 AC-2.
#[test]
fn folders_without_their_rows_are_a_leftover_and_nothing_is_written() {
    let site = Site::new();
    let first = site.seed().expect("an empty root seeds");
    let fresh = FakeRegistry::new(
        Arc::new(
            FakePlatform::new(&TestRoot::new().expect("a root"))
                .clock()
                .clone(),
        ),
        site.platform.clock().now(),
    );

    let error = seed_fake(
        &site.platform,
        &site.layout,
        &fresh,
        &site.server,
        &site.package,
    )
    .expect_err("the folders exist without rows");

    assert!(
        matches!(error, FixtureError::Leftover { .. }),
        "got {error:?}"
    );
    assert_eq!(fresh.runtimes().expect("reads").generation.get(), 0);
    assert!(site.layout.provider_revision_dir(&first.provider).is_dir());
}

/// Spec 0014 AC-2.
#[test]
fn a_provider_folder_without_its_row_is_a_leftover() {
    let site = Site::new();
    let first = site.seed().expect("an empty root seeds");
    let runtime = site
        .registry
        .runtimes()
        .expect("the registry reads")
        .value
        .remove(0);
    let InstallMode::Managed { artifact_sha256 } = runtime.install().clone() else {
        panic!("the seeded runtime is managed");
    };
    let artifact = site
        .registry
        .artifact(&artifact_sha256)
        .expect("the registry reads")
        .value
        .expect("the artifact is stored");
    let fresh = FakeRegistry::new(
        Arc::new(
            FakePlatform::new(&TestRoot::new().expect("a root"))
                .clock()
                .clone(),
        ),
        site.platform.clock().now(),
    );
    fresh
        .commit(stackly_domain::WriteBatch::new(vec![
            stackly_domain::WriteOp::InsertArtifact(artifact),
            stackly_domain::WriteOp::InsertRuntime(runtime),
        ]))
        .expect("the runtime row commits");

    let error = seed_fake(
        &site.platform,
        &site.layout,
        &fresh,
        &site.server,
        &site.package,
    )
    .expect_err("the provider folder exists without its row");

    assert!(
        matches!(&error, FixtureError::Leftover { path } if *path == site.layout.provider_revision_dir(&first.provider)),
        "got {error:?}"
    );
    assert_eq!(fresh.runtimes().expect("reads").generation.get(), 1);
}

/// Spec 0014 AC-2.
#[test]
fn a_runtime_of_the_same_version_with_another_binary_is_a_conflict() {
    let site = Site::new();
    site.seed().expect("an empty root seeds");
    let generation = site.generation();
    fs::write(&site.server, b"another binary").expect("the stand in server changes");

    let error = site.seed().expect_err("the binary differs");

    assert!(
        matches!(error, FixtureError::Conflict { .. }),
        "got {error:?}"
    );
    assert_eq!(site.generation(), generation);
    assert_eq!(site.runtime_count(), 1);
}

/// Spec 0014 AC-2.
#[test]
fn a_provider_of_the_same_revision_with_another_digest_is_a_conflict() {
    let site = Site::new();
    site.seed().expect("an empty root seeds");
    let generation = site.generation();
    let manifest = site.package.join("provider.yaml");
    let mut text = fs::read_to_string(&manifest).expect("the manifest reads");
    text.push_str("\n# a comment changes the digest\n");
    fs::write(&manifest, text).expect("the manifest changes");

    let error = site.seed().expect_err("the digest differs");

    assert!(
        matches!(&error, FixtureError::Conflict { what } if what.contains("provider")),
        "got {error:?}"
    );
    assert_eq!(site.generation(), generation);
}

/// Spec 0014 AC-1.
#[test]
fn a_root_with_the_provider_only_gets_the_runtime_alone() {
    let site = Site::new();
    let first = site.seed().expect("an empty root seeds");
    let provider = site
        .registry
        .provider_revision(&first.provider)
        .expect("the registry reads")
        .value
        .expect("the provider is stored");
    let fresh = FakeRegistry::new(
        Arc::new(
            FakePlatform::new(&TestRoot::new().expect("a root"))
                .clock()
                .clone(),
        ),
        site.platform.clock().now(),
    );
    fresh
        .commit(stackly_domain::WriteBatch::new(vec![
            stackly_domain::WriteOp::InsertProviderRevision(provider),
        ]))
        .expect("the provider row commits");
    let runtime_dir = site.layout.runtimes_dir().join("fake");
    fs::remove_dir_all(runtime_dir).expect("the runtime folder goes");

    let seeded = seed_fake(
        &site.platform,
        &site.layout,
        &fresh,
        &site.server,
        &site.package,
    )
    .expect("the missing runtime is made alone");

    assert!(seeded.created);
    assert_ne!(seeded.runtime_id, first.runtime_id, "a new runtime row");
    assert_eq!(fresh.runtimes().expect("reads").value.len(), 1);
    assert_eq!(fresh.runtimes().expect("reads").generation.get(), 2);
}
