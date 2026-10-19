//! Compensation of what an operation created, and only that (spec 0007
//! AC-8, BR-023, VAL-007).

use std::os::unix::fs::symlink;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use stackly_core::operation::testing::{HoldHandle, compensator_conformance};
use stackly_core::operation::{
    BoxFuture, CompensationError, CompensationResult, CompensationScope, Compensator,
    OperationContext, OperationOutcome, OperationWork, StepSpec, WorkResult,
};
use stackly_core::{daemon_info, write_instance_manifest};
use stackly_domain::{
    Arch, CreatedItem, CreatedRecord, CreatedSeq, CreatedState, InstanceId, InstanceManifest,
    ManifestRuntime, NewInstanceManifest, OperationId, OperationKind, OperationState, Os,
    ProviderRevisionKey, Registry, RuntimeId, StepSeq, Target, WriteBatch, WriteOp,
};
use stackly_platform::{Clock, Platform};
use stackly_testkit::evidence::{Kind, evidence};
use stackly_testkit::{InstanceBuilder, ProviderRevisionBuilder, RuntimeBuilder};

use crate::common::{Harness, manager_request, wait_state};

const DIGEST: &str = "da578821d15e9c0f59eb8e4672962c6f032074e802684576debc6dfc7f86815f";

fn manifest(platform: &Platform, id: InstanceId, created_by: OperationId) -> InstanceManifest {
    InstanceManifest::new(NewInstanceManifest {
        instance_id: id,
        name: "orders-db".parse().expect("test instance name is valid"),
        display_name: None,
        software: "postgresql".parse().expect("test software name is valid"),
        provider: ProviderRevisionKey::new(
            "official/postgresql"
                .parse()
                .expect("test provider name is valid"),
            "2026.10.1"
                .parse()
                .expect("test provider revision is valid"),
            DIGEST.parse().expect("test provider digest is valid"),
        ),
        runtime: ManifestRuntime::managed(
            RuntimeId::generate(),
            "17.6".parse().expect("test runtime version is valid"),
            "17".parse().expect("test runtime line is valid"),
            Target::new(Os::Macos, Arch::Aarch64),
            DIGEST.parse().expect("test runtime digest is valid"),
        ),
        created_at: platform.clock().now(),
        created_by_operation: created_by,
        stackly_version: daemon_info().version.to_owned(),
    })
}

/// What a [`CreateTree`] work does with the instance folder it records.
#[derive(Clone, Default)]
struct Tree {
    /// Plant a symlink to this folder inside the instance's data.
    plant: Option<PathBuf>,
    /// Write a manifest naming another operation.
    foreign_manifest: bool,
    /// Also create the instance's run folder.
    run_dir: bool,
    /// Decline the item as already existing, and make the folder anyway.
    already_exists: bool,
    /// Hold after the tree is made.
    hold: Option<HoldHandle>,
    /// The new instance ID, for the test.
    made: Arc<Mutex<Option<InstanceId>>>,
}

/// Creates an instance tree in a reversible step, then fails.
struct CreateTree(Tree);

impl OperationWork for CreateTree {
    fn run(self: Box<Self>, mut ctx: OperationContext) -> BoxFuture<'static, WorkResult> {
        Box::pin(async move {
            let tree = self.0;
            ctx.begin_step(StepSpec::new(
                "create".parse().expect("create step name is valid"),
            ))
            .await?;
            let (id, created) = ctx.new_instance_id().await?;
            *tree
                .made
                .lock()
                .expect("created instance ID lock is not poisoned") = Some(id);
            let layout = ctx.layout().layout().clone();
            let paths = layout.instance_paths(id);
            if tree.already_exists {
                ctx.not_created(
                    created,
                    "already_exists"
                        .parse()
                        .expect("already-exists reason is valid"),
                )
                .await?;
                std::fs::create_dir_all(&paths.data)
                    .expect("existing instance data directory can be created");
            } else {
                std::fs::create_dir_all(&paths.data)
                    .expect("instance data directory can be created");
                std::fs::create_dir_all(layout.instance_log_dir(id))
                    .expect("instance log directory can be created");
                std::fs::write(paths.data.join("PG_VERSION"), b"17")
                    .expect("PostgreSQL version marker can be written");
                if tree.run_dir {
                    std::fs::create_dir_all(layout.instance_run_dir(id.short_key()))
                        .expect("instance run directory can be created");
                }
                if let Some(target) = &tree.plant {
                    symlink(target, paths.data.join("escape"))
                        .expect("test symlink can be created");
                }
                let owner = if tree.foreign_manifest {
                    OperationId::generate()
                } else {
                    ctx.id()
                };
                write_instance_manifest(
                    ctx.platform(),
                    &paths,
                    &manifest(ctx.platform(), id, owner),
                )
                .expect("test instance manifest can be written");
                ctx.confirm_created(created).await?;
            }
            ctx.finish_step(None).await?;
            if let Some(hold) = &tree.hold {
                hold.mark_started();
                hold.released().await;
            }
            ctx.begin_step(StepSpec::new(
                "initdb".parse().expect("initdb step name is valid"),
            ))
            .await?;
            Err(stackly_core::operation::WorkError::Failed(Box::new(
                InitFailed,
            )))
        })
    }
}

#[derive(Debug, thiserror::Error)]
#[error("initdb failed")]
struct InitFailed;

impl stackly_core::operation::OperationError for InitFailed {
    fn code(&self) -> stackly_domain::ErrorCodeName {
        "instance.init_failed"
            .parse()
            .expect("instance.init_failed is a known error code")
    }

    fn retryable(&self) -> bool {
        false
    }

    fn details(&self) -> Vec<(stackly_domain::DetailKey, String)> {
        Vec::new()
    }
}

fn sentinel(harness: &Harness) -> PathBuf {
    let sentinel = harness.root.path().join("sentinel");
    std::fs::create_dir_all(&sentinel).expect("sentinel directory can be created");
    std::fs::write(sentinel.join("keep"), b"sentinel").expect("sentinel marker can be written");
    sentinel
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_failed_create_removes_its_own_tree_and_a_planted_symlink_only_as_a_link() {
    let ev = evidence!(
        covers = ["BR-023", "VAL-007"],
        kind = Kind::Fixture,
        expected = "a create that fails after making its instance tree compensates exactly that tree (root, logs, run folder) through remove_owned_tree, removes a planted symlink to a sentinel outside the owned roots as a link, and leaves the sentinel identical",
    );
    let harness = Harness::new().await;
    let outside = sentinel(&harness);
    let tree = Tree {
        plant: Some(outside.clone()),
        run_dir: true,
        ..Tree::default()
    };
    let made = Arc::clone(&tree.made);

    let id = harness
        .begin(manager_request(
            OperationKind::CreateInstance,
            Box::new(CreateTree(tree)),
        ))
        .await;
    let failed = wait_state(&harness.engine, id, OperationState::Failed).await;

    let instance = made.lock().unwrap().unwrap();
    let layout = harness.layout.layout();
    assert!(!layout.instance_paths(instance).root.exists());
    assert!(!layout.instance_log_dir(instance).exists());
    assert!(!layout.instance_run_dir(instance.short_key()).exists());
    assert_eq!(std::fs::read(outside.join("keep")).unwrap(), b"sentinel");
    assert_eq!(failed.created.len(), 1);
    assert_eq!(failed.created[0].state(), CreatedState::Compensated);
    assert_eq!(
        failed.view.operation.failure().map(|f| f.code().as_str()),
        Some("step_failed")
    );
    assert_eq!(
        failed
            .view
            .journal
            .failure_error()
            .map(|e| e.code().as_str()),
        Some("instance.init_failed")
    );
    assert!(failed.view.journal.compensation_started_at().is_some());
    assert_eq!(harness.fake.fs().violations(), 0);
    ev.pass();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_tree_whose_manifest_names_another_operation_is_left_and_reported_not_owned() {
    let harness = Harness::new().await;
    let tree = Tree {
        foreign_manifest: true,
        ..Tree::default()
    };
    let made = Arc::clone(&tree.made);

    let id = harness
        .begin(manager_request(
            OperationKind::CreateInstance,
            Box::new(CreateTree(tree)),
        ))
        .await;
    let failed = wait_state(&harness.engine, id, OperationState::Failed).await;

    let instance = made.lock().unwrap().unwrap();
    assert!(
        harness
            .layout
            .layout()
            .instance_paths(instance)
            .data
            .is_dir()
    );
    assert_eq!(failed.created[0].state(), CreatedState::NotOwned);
    assert_eq!(
        failed.created[0].detail().map(|detail| detail.as_str()),
        Some("created_by_other_operation")
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_item_that_already_existed_is_never_compensated() {
    let harness = Harness::new().await;
    let tree = Tree {
        already_exists: true,
        ..Tree::default()
    };
    let made = Arc::clone(&tree.made);

    let id = harness
        .begin(manager_request(
            OperationKind::CreateInstance,
            Box::new(CreateTree(tree)),
        ))
        .await;
    let failed = wait_state(&harness.engine, id, OperationState::Failed).await;

    let instance = made.lock().unwrap().unwrap();
    assert!(
        harness
            .layout
            .layout()
            .instance_paths(instance)
            .data
            .is_dir()
    );
    assert_eq!(failed.created[0].state(), CreatedState::NotOwned);
    assert_eq!(
        failed.created[0].detail().map(|detail| detail.as_str()),
        Some("already_exists")
    );
}

/// Another valid ID with the same short key as `id`.
fn same_short_key(id: InstanceId) -> InstanceId {
    let text = id.to_string();
    let first = if text.starts_with('0') { "1" } else { "0" };
    format!("{first}{}", &text[1..])
        .parse()
        .expect("constructed instance ID is valid")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_run_folder_another_known_instance_shares_is_kept() {
    let harness = Harness::new().await;
    let hold = HoldHandle::new();
    let tree = Tree {
        run_dir: true,
        hold: Some(hold.clone()),
        ..Tree::default()
    };
    let made = Arc::clone(&tree.made);
    let id = harness
        .begin(manager_request(
            OperationKind::CreateInstance,
            Box::new(CreateTree(tree)),
        ))
        .await;
    hold.started().await;
    let instance = made.lock().unwrap().unwrap();
    let provider = ProviderRevisionBuilder::new();
    let runtime = RuntimeBuilder::new();
    let built = runtime.clone().build();
    let twin = InstanceBuilder::new(&built, &provider.key())
        .with_id(same_short_key(instance))
        .build();
    harness
        .registry
        .commit(WriteBatch::new(vec![
            WriteOp::InsertProviderRevision(provider.record()),
            WriteOp::InsertArtifact(runtime.artifact()),
            WriteOp::InsertRuntime(built),
            WriteOp::InsertInstance(twin),
        ]))
        .unwrap();

    hold.release();
    wait_state(&harness.engine, id, OperationState::Failed).await;

    let layout = harness.layout.layout();
    assert!(!layout.instance_paths(instance).root.exists());
    assert!(
        layout.instance_run_dir(instance.short_key()).is_dir(),
        "the shared run folder stays"
    );
}

/// Undoes a marker file under the test root when the payload names its
/// operation; fails while `broken` is set.
struct MarkerCompensator {
    dir: PathBuf,
    broken: AtomicBool,
}

impl Compensator for MarkerCompensator {
    fn compensate(
        &self,
        scope: CompensationScope,
        record: &CreatedRecord,
    ) -> BoxFuture<'static, Result<CompensationResult, CompensationError>> {
        let broken = self.broken.load(Ordering::SeqCst);
        let owner = record
            .payload()
            .map(|payload| payload.as_str().to_owned())
            .unwrap_or_default();
        let key = match record.item() {
            CreatedItem::Custom { key, .. } => key.to_string(),
            _ => String::new(),
        };
        let path = self.dir.join(key);
        Box::pin(async move {
            if broken {
                return Err(CompensationError::new(
                    "marker_busy"
                        .parse()
                        .expect("marker_busy is a known compensation code"),
                    "the marker is busy",
                ));
            }
            if !owner.contains(&scope.operation.to_string()) {
                return Ok(CompensationResult::NotOwned {
                    reason: "owned_by_other"
                        .parse()
                        .expect("owned_by_other is a known ownership reason"),
                });
            }
            match std::fs::remove_file(&path) {
                Ok(()) => Ok(CompensationResult::Compensated),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    Ok(CompensationResult::Compensated)
                }
                Err(error) => Err(CompensationError::new(
                    "remove_failed"
                        .parse()
                        .expect("remove_failed is a known compensation code"),
                    error.to_string(),
                )),
            }
        })
    }
}

/// Makes a marker file in a reversible step, then fails.
struct Marker {
    dir: PathBuf,
}

impl OperationWork for Marker {
    fn run(self: Box<Self>, mut ctx: OperationContext) -> BoxFuture<'static, WorkResult> {
        Box::pin(async move {
            ctx.begin_step(StepSpec::new(
                "mark".parse().expect("mark step name is valid"),
            ))
            .await?;
            let payload =
                stackly_domain::JournalPayload::new(format!(r#"{{"owner":"{}"}}"#, ctx.id()), 4096)
                    .expect("operation owner payload fits the journal bound");
            let created = ctx
                .record_custom_intent(
                    "marker_file"
                        .parse()
                        .expect("marker_file is a valid item key"),
                    "marker".parse().expect("marker is a valid item key"),
                    Some(payload),
                )
                .await?;
            std::fs::write(self.dir.join("marker"), b"x").expect("marker file can be written");
            ctx.confirm_created(created).await?;
            ctx.finish_step(None).await?;
            let _ = ctx.scratch_dir().await?;
            ctx.begin_step(StepSpec::new(
                "use".parse().expect("use step name is valid"),
            ))
            .await?;
            Err(stackly_core::operation::WorkError::Failed(Box::new(
                InitFailed,
            )))
        })
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_failed_compensation_needs_recovery_and_abandon_runs_it_again() {
    let harness = Harness::new().await;
    let compensator = Arc::new(MarkerCompensator {
        dir: harness.root.path().to_path_buf(),
        broken: AtomicBool::new(true),
    });
    harness
        .engine
        .register_compensator("marker_file".parse().unwrap(), compensator.clone());

    let id = harness
        .begin(manager_request(
            OperationKind::UpdateManager,
            Box::new(Marker {
                dir: harness.root.path().to_path_buf(),
            }),
        ))
        .await;
    let stuck = wait_state(&harness.engine, id, OperationState::NeedsRecovery).await;

    assert_eq!(
        stuck.view.operation.recovery_hint(),
        Some(stackly_domain::RecoveryHint::Abandon)
    );
    assert_eq!(
        stuck.view.journal.recovery_cause(),
        Some(stackly_domain::RecoveryCause::CompensationFailed)
    );
    assert_eq!(stuck.created[0].state(), CreatedState::CompensationFailed);
    let scratch = harness
        .layout
        .layout()
        .operations_dir()
        .join(id.to_string());
    assert!(
        scratch.is_dir(),
        "the scratch folder stays while an item failed"
    );
    assert!(harness.root.path().join("marker").exists());

    compensator.broken.store(false, Ordering::SeqCst);
    let abandoned = harness
        .engine
        .abandon(id, stuck.view.operation.revision())
        .await
        .unwrap();

    assert_eq!(abandoned.view.operation.state(), OperationState::Failed);
    assert_eq!(
        abandoned
            .view
            .operation
            .failure()
            .map(|f| f.code().as_str()),
        Some("abandoned")
    );
    assert!(abandoned.leftovers.is_empty());
    assert!(!harness.root.path().join("marker").exists());
    assert!(!scratch.exists());
    let outcome = OperationOutcome::from_payload(abandoned.view.journal.result().unwrap()).unwrap();
    assert_eq!(outcome.partial.len(), 1);
    assert_eq!(
        outcome.partial[0].outcome,
        stackly_core::operation::PartialOutcome::Done
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_custom_item_without_its_compensator_fails_compensation() {
    let harness = Harness::new().await;

    let id = harness
        .begin(manager_request(
            OperationKind::UpdateManager,
            Box::new(Marker {
                dir: harness.root.path().to_path_buf(),
            }),
        ))
        .await;
    let stuck = wait_state(&harness.engine, id, OperationState::NeedsRecovery).await;

    assert_eq!(stuck.created[0].state(), CreatedState::CompensationFailed);
    assert_eq!(
        stuck.created[0].detail().map(|detail| detail.as_str()),
        Some("compensator_missing")
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_marker_compensator_passes_the_compensator_conformance_check() {
    let harness = Harness::new().await;
    let compensator = MarkerCompensator {
        dir: harness.root.path().to_path_buf(),
        broken: AtomicBool::new(false),
    };
    let operation = OperationId::generate();
    std::fs::write(harness.root.path().join("owned"), b"x").unwrap();
    let record = |key: &str, owner: OperationId| {
        CreatedRecord::intend(
            CreatedSeq::FIRST,
            StepSeq::FIRST,
            CreatedItem::Custom {
                compensator: "marker_file".parse().unwrap(),
                key: key.parse().unwrap(),
            },
            Some(
                stackly_domain::JournalPayload::new(format!(r#"{{"owner":"{owner}"}}"#), 4096)
                    .unwrap(),
            ),
            harness.fake.clock().now(),
        )
        .unwrap()
    };
    let scope = CompensationScope {
        operation,
        registry: Arc::clone(&harness.registry) as Arc<dyn Registry>,
        platform: Arc::clone(&harness.platform),
        layout: Arc::clone(&harness.layout),
    };

    compensator_conformance(
        &compensator,
        scope,
        &record("owned", operation),
        &record("foreign", OperationId::generate()),
    )
    .await;

    assert!(!harness.root.path().join("owned").exists());
}
