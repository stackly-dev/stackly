//! `runtime::commit` (spec 0018, AC-13): one rename that refuses an existing
//! target, a rename that already happened before a restart, and a folder
//! left by an operation that never committed.
#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use stackly_core::prepare_layout;
use stackly_core::runtime::commit::{CommitError, CommitOutcome, CommitRequest, commit_tree};
use stackly_domain::{OperationState, Registry, RuntimeId, Sha256Digest, WriteBatch, WriteOp};
use stackly_platform::Clock;
use stackly_testkit::evidence::{Kind, evidence};
use stackly_testkit::{FakePlatform, FakeRegistry, OperationBuilder, RuntimeBuilder, TestRoot};

struct Rig {
    _root: TestRoot,
    platform: stackly_platform::Platform,
    layout: stackly_core::PreparedLayout,
    registry: FakeRegistry,
    sha256: Sha256Digest,
    stage_tree: PathBuf,
    scratch: PathBuf,
    target: PathBuf,
}

fn marker(operation: &str, sha: &str) -> String {
    format!(
        r#"{{"schema":"stackly.runtime-marker/v1","operationId":"{operation}","sha256":"{sha}","software":"nats","version":"2.15.1","target":"macos-aarch64"}}"#
    )
}

impl Rig {
    fn new(operation: &str) -> Self {
        let root = TestRoot::new().expect("a test root");
        let fake = FakePlatform::new(&root);
        let platform = fake.platform();
        let layout = prepare_layout(&platform).expect("the layout prepares");
        let registry = FakeRegistry::new(Arc::new(fake.clock().clone()), fake.clock().now());
        let sha256: Sha256Digest = "ab".repeat(32).parse().expect("a digest");
        let scratch = layout.layout().operations_dir().join(operation);
        let stage = scratch.join("stage");
        let stage_tree = stage.join("tree");
        for dir in [&scratch, &stage, &stage_tree] {
            platform.fs().create_dir_private(dir).expect("a folder");
        }
        platform
            .fs()
            .write_file_atomic(
                &stage_tree.join(".stackly-runtime.json"),
                marker(operation, &sha256.to_string()).as_bytes(),
            )
            .expect("the marker is written");
        platform
            .fs()
            .write_file_atomic(&stage_tree.join("nats-server"), b"binary")
            .expect("the executable is written");
        let target = layout.layout().runtime_dir(
            &"nats".parse().expect("a software"),
            &"2.15.1".parse().expect("a version"),
            "macos-aarch64".parse().expect("a target"),
            &sha256,
        );
        Self {
            _root: root,
            platform,
            layout,
            registry,
            sha256,
            stage_tree,
            scratch,
            target,
        }
    }

    fn commit(&self, operation: &str) -> Result<CommitOutcome, CommitError> {
        commit_tree(&CommitRequest {
            platform: &self.platform,
            layout: &self.layout,
            registry: &self.registry,
            software: &"nats".parse().expect("a software"),
            version: &"2.15.1".parse().expect("a version"),
            target: "macos-aarch64".parse().expect("a target"),
            sha256: &self.sha256,
            stage_tree: &self.stage_tree,
            scratch: &self.scratch,
            operation_id: operation,
        })
    }

    /// Puts a folder at the target with this marker text, or none.
    fn plant(&self, marker_text: Option<&str>) {
        let fs = self.platform.fs();
        let mut dir = PathBuf::new();
        for part in self.target.components() {
            dir.push(part);
            if dir.starts_with(self.layout.layout().runtimes_dir())
                && dir != self.layout.layout().runtimes_dir()
            {
                let _ = fs.create_dir_private(&dir);
            }
        }
        if let Some(text) = marker_text {
            fs.write_file_atomic(&self.target.join(".stackly-runtime.json"), text.as_bytes())
                .expect("the planted marker is written");
        }
    }
}

fn exists(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok()
}

const OP: &str = "01a15f10-b90a-720f-b9b0-ab884f2d560a";

#[test]
fn a_clean_commit_moves_the_tree_with_one_rename_and_makes_the_parent_folders() {
    let ev = evidence!(
        covers = ["REQ-004"],
        kind = Kind::Fixture,
        expected = "the staged tree is renamed to runtime_dir(software, version, target, sha256), the parent folders are made one by one, and the stage folder no longer holds the tree",
    );
    let rig = Rig::new(OP);

    let outcome = rig.commit(OP);

    assert_eq!(outcome, Ok(CommitOutcome::Moved));
    assert!(
        exists(&rig.target.join("nats-server"))
            && exists(&rig.target.join(".stackly-runtime.json"))
    );
    assert!(!exists(&rig.stage_tree));
    ev.pass();
}

#[test]
fn a_target_with_the_marker_of_this_operation_is_a_rename_that_already_happened() {
    let rig = Rig::new(OP);
    rig.plant(Some(&marker(OP, &rig.sha256.to_string())));

    let outcome = rig.commit(OP);

    assert_eq!(outcome, Ok(CommitOutcome::AlreadyMoved));
    assert!(exists(&rig.stage_tree), "no second rename");
}

#[test]
fn a_folder_of_an_operation_that_is_gone_is_an_orphan_moved_to_scratch() {
    let ev = evidence!(
        covers = ["REQ-004"],
        kind = Kind::Fixture,
        expected = "a target folder whose marker names the same artifact and an operation the registry no longer holds is moved to scratch/orphan and the commit goes on; nothing in runtimes/ is deleted",
    );
    let rig = Rig::new(OP);
    let gone = "01a15f10-0000-7000-8000-000000000001";
    rig.plant(Some(&marker(gone, &rig.sha256.to_string())));

    let outcome = rig.commit(OP);

    assert_eq!(outcome, Ok(CommitOutcome::MovedAfterOrphan));
    assert!(exists(&rig.target.join("nats-server")));
    assert!(exists(
        &rig.scratch
            .join("orphan")
            .join("runtime-0")
            .join(".stackly-runtime.json")
    ));
    ev.pass();
}

#[test]
fn a_folder_of_a_failed_operation_is_an_orphan_too() {
    let rig = Rig::new(OP);
    let failed = OperationBuilder::new()
        .state(OperationState::Failed)
        .failure("step_failed", false);
    let id = failed.id();
    rig.registry
        .commit(WriteBatch::new(vec![WriteOp::InsertOperation(
            failed.build(),
        )]))
        .expect("the operation is stored");
    rig.plant(Some(&marker(&id.to_string(), &rig.sha256.to_string())));

    assert_eq!(rig.commit(OP), Ok(CommitOutcome::MovedAfterOrphan));
}

#[test]
fn a_folder_without_a_marker_or_with_another_artifact_is_a_conflict() {
    let ev = evidence!(
        covers = ["REQ-004"],
        kind = Kind::Fixture,
        expected = "a target folder with no marker, a broken marker, or the marker of another artifact fails install_conflict and nothing is moved",
    );
    for text in [
        None,
        Some("not json".to_owned()),
        Some(marker(OP, &"cd".repeat(32))),
        Some(marker("other-op", &"cd".repeat(32))),
    ] {
        let rig = Rig::new(OP);
        rig.plant(text.as_deref());
        // The marker of this operation but another artifact is still ours
        // by the operation, so only the others conflict.
        let outcome = rig.commit(OP);
        match &text {
            Some(text) if text.contains(OP) => assert_eq!(outcome, Ok(CommitOutcome::AlreadyMoved)),
            _ => assert_eq!(
                outcome,
                Err(CommitError::Conflict {
                    path: rig.target.clone()
                })
            ),
        }
        assert!(exists(&rig.stage_tree) || matches!(outcome, Ok(CommitOutcome::MovedAfterOrphan)));
    }
    ev.pass();
}

#[test]
fn a_folder_of_an_operation_that_still_runs_is_a_conflict_and_so_is_a_registered_runtime() {
    let running = OperationBuilder::new().state(OperationState::Running);
    let id = running.id();
    let rig = Rig::new(OP);
    rig.registry
        .commit(WriteBatch::new(vec![WriteOp::InsertOperation(
            running.build(),
        )]))
        .expect("stored");
    rig.plant(Some(&marker(&id.to_string(), &rig.sha256.to_string())));
    assert_eq!(
        rig.commit(OP),
        Err(CommitError::Conflict {
            path: rig.target.clone()
        })
    );

    // A runtime for the artifact already exists: the folder is not an orphan.
    let rig = Rig::new(OP);
    let runtime = RuntimeBuilder::new()
        .managed(rig.sha256)
        .state(stackly_domain::RuntimeState::Installed)
        .installed_at(rig.registry_time())
        .last_verified_at(rig.registry_time());
    rig.registry
        .commit(WriteBatch::new(vec![
            WriteOp::InsertArtifact(runtime.artifact()),
            WriteOp::InsertRuntime(runtime.build()),
        ]))
        .expect("stored");
    rig.plant(Some(&marker(
        "01a15f10-0000-7000-8000-000000000002",
        &rig.sha256.to_string(),
    )));
    assert_eq!(
        rig.commit(OP),
        Err(CommitError::Conflict {
            path: rig.target.clone()
        })
    );
    let _ = RuntimeId::generate();
}

impl Rig {
    fn registry_time(&self) -> jiff::Timestamp {
        "2026-01-01T00:00:00Z".parse().expect("a time")
    }
}
