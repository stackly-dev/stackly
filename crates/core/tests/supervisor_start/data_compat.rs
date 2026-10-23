//! A start refuses data its runtime may not run, with the code and details
//! of `check_data_compatibility` (spec 0025 AC-8, spec 0010 AC-8, spec 0019
//! AC-11).

use stackly_core::instance::data_compat::{DataOutcome, check_data_compatibility};
use stackly_core::instance::rebind;
use stackly_core::operation::OperationError;
use stackly_core::supervisor::{SupervisorError, codes};
use stackly_domain::{
    ArtifactRecord, InstallMode, InstanceId, InstanceLifecycle, OperationState, Registry,
    RuntimeId, RuntimeState, WriteBatch, WriteOp,
};
use stackly_testkit::RuntimeBuilder;

use crate::common::{Rig, Spec};

/// The four refusals a recorded line can meet.
#[derive(Clone, Copy, Debug)]
enum Case {
    Incompatible,
    MarkerMismatch,
    MarkerMissing,
    MarkerUnreadable,
}

impl Case {
    const ALL: [Self; 4] = [
        Self::Incompatible,
        Self::MarkerMismatch,
        Self::MarkerMissing,
        Self::MarkerUnreadable,
    ];

    const fn outcome(self) -> DataOutcome {
        match self {
            Self::Incompatible => DataOutcome::Incompatible,
            Self::MarkerMismatch => DataOutcome::MarkerMismatch,
            Self::MarkerMissing => DataOutcome::MarkerMissing,
            Self::MarkerUnreadable => DataOutcome::MarkerUnreadable,
        }
    }
}

/// Builds a rig and a stopped instance whose data meets `case`.
async fn rig_for(case: Case) -> (Rig, InstanceId) {
    let rig = Rig::new().await;
    let spec = match case {
        Case::MarkerMismatch => Spec {
            marker: "9.9\n",
            ..Spec::default()
        },
        _ => Spec::default(),
    };
    let id = rig.instance(&spec);
    let marker = rig.paths(id).data.join("FAKE_VERSION");
    match case {
        Case::Incompatible | Case::MarkerMismatch => {}
        Case::MarkerMissing => std::fs::remove_file(&marker).expect("the marker is removed"),
        Case::MarkerUnreadable => {
            // A folder where the marker should be: not a regular file.
            std::fs::remove_file(&marker).expect("the marker is removed");
            std::fs::create_dir(&marker).expect("the marker folder is made");
        }
    }
    if matches!(case, Case::Incompatible) {
        // The runtime row now belongs to another line than the data.
        let other = RuntimeBuilder::new()
            .with_id(rig.runtime.id())
            .software("fake")
            // The same version keeps the runtime folder; only the line differs.
            .version("1.0.0", "2.0")
            .state(RuntimeState::Installed)
            .revision(2)
            .install(rig.runtime.install().clone())
            .installed_at(
                "2026-01-01T00:00:00Z"
                    .parse()
                    .expect("a valid test timestamp"),
            )
            .build();
        rig.registry
            .commit(WriteBatch::new(vec![WriteOp::UpdateRuntime {
                runtime: other,
                expected: rig.runtime.revision(),
            }]))
            .expect("the runtime row is replaced");
    }
    (rig, id)
}

/// Stores a second installed runtime of the rig's software and of `line`,
/// as the target of a rebind check.
fn another_runtime(rig: &Rig, line: &str) -> RuntimeId {
    let runtime = RuntimeBuilder::new()
        .software("fake")
        .version("9.0.0", line)
        .state(RuntimeState::Installed)
        .installed_at(
            "2026-01-01T00:00:00Z"
                .parse()
                .expect("a valid test timestamp"),
        )
        .build();
    let InstallMode::Managed { artifact_sha256 } = runtime.install() else {
        unreachable!("the builder makes a managed runtime");
    };
    let artifact = ArtifactRecord::new(
        *artifact_sha256,
        runtime.software().clone(),
        runtime.version().clone(),
        runtime.target(),
        None,
        None,
    );
    let id = runtime.id();
    rig.registry
        .commit(WriteBatch::new(vec![
            WriteOp::InsertArtifact(artifact),
            WriteOp::InsertRuntime(runtime),
        ]))
        .expect("the second runtime is stored");
    id
}

/// The code and the details a failed start stored.
fn stored_refusal(
    detail: &stackly_core::operation::OperationDetailView,
) -> (String, Vec<(String, String)>) {
    let error = detail
        .view
        .journal
        .failure_error()
        .expect("the failed operation stored its error");
    (
        error.code().to_string(),
        error
            .details()
            .iter()
            .map(|(key, value)| (key.to_string(), value.clone()))
            .collect(),
    )
}

#[tokio::test]
async fn a_start_refuses_data_its_runtime_may_not_run_before_any_spawn() {
    // covers: spec 0025 AC-8 (new start cases: incompatible, marker_missing, marker_unreadable)
    for case in Case::ALL {
        let (rig, id) = rig_for(case).await;
        let op = rig.start(id).await;
        let detail = rig.wait_state(op, OperationState::Failed).await;
        let (code, details) = stored_refusal(&detail);

        assert_eq!(code, codes::DATA_INCOMPATIBLE.to_string(), "{case:?}");
        assert!(
            details.contains(&("outcome".to_owned(), case.outcome().as_code().to_owned())),
            "{case:?}: {details:?}"
        );
        assert_eq!(
            details.iter().any(|(key, _)| key == "data"),
            matches!(case, Case::Incompatible),
            "only an incompatible line names the data line: {case:?}: {details:?}"
        );
        assert_eq!(
            rig.instance_row(id).lifecycle(),
            InstanceLifecycle::Stopped,
            "{case:?}"
        );
        assert!(rig.processes().spawns().is_empty(), "{case:?}");
    }
}

#[tokio::test]
async fn an_incompatible_start_names_both_lines() {
    // covers: spec 0025 AC-8
    let (rig, id) = rig_for(Case::Incompatible).await;
    let op = rig.start(id).await;
    let detail = rig.wait_state(op, OperationState::Failed).await;
    let (_, details) = stored_refusal(&detail);

    assert!(
        details.contains(&("data".to_owned(), "1.0".to_owned())),
        "{details:?}"
    );
    assert!(
        details.contains(&("runtime".to_owned(), "2.0".to_owned())),
        "{details:?}"
    );
}

#[tokio::test]
async fn data_compatibility_is_the_same_for_start_and_rebind() {
    // covers: spec 0025 AC-8 (one answer for start and rebind)
    for case in Case::ALL {
        let (rig, id) = rig_for(case).await;
        let op = rig.start(id).await;
        let detail = rig.wait_state(op, OperationState::Failed).await;
        let started = stored_refusal(&detail);

        let instance = rig.instance_row(id);
        let runtime = rig
            .registry
            .runtime(instance.runtime_id())
            .expect("the registry reads")
            .value
            .expect("the runtime row exists");
        let refused = check_data_compatibility(
            rig.platform.fs(),
            &rig.package,
            &rig.paths(id),
            instance.data_line(),
            runtime.line(),
        )
        .expect_err("the shared check refuses the same data");
        let error = SupervisorError::from(refused);
        let direct = (
            error.code().to_string(),
            error
                .details()
                .iter()
                .map(|(key, value)| (key.to_string(), value.clone()))
                .collect::<Vec<_>>(),
        );

        assert_eq!(started, direct, "{case:?}");

        // The rebind check asks the same question of the same data: the
        // target has the line the start refused.
        let target = another_runtime(&rig, &runtime.line().to_string());
        let refused = rebind::check(&*rig.registry, &rig.platform, &rig.layout, id, target)
            .expect_err("the rebind check refuses the same data");
        let rebound = (
            refused.code().to_string(),
            refused
                .details()
                .iter()
                .map(|(key, value)| (key.to_string(), value.clone()))
                .collect::<Vec<_>>(),
        );
        assert_eq!(started, rebound, "{case:?}");
    }
}
