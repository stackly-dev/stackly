//! The cases of the `create_instance` operation.

use std::sync::{Arc, Mutex, PoisonError};

use stackly_core::instance::create::{CreateHooks, SecretSourceKind};
use stackly_core::read_instance_manifest;
use stackly_domain::{
    CreatedState, InstanceId, InstanceLifecycle, OperationState, ProviderSourceKind, Registry,
    SecretPurpose, SecretRef, SecretUser,
};
use stackly_evidence::{Kind, evidence};
use stackly_platform::{ChildRole, ExitStatus, InstanceLock, ProcessSupervision, SecretValue};

use crate::common::Rig;
use crate::harness::CreateRig;

fn reference_of(id: InstanceId, field: &str) -> SecretRef {
    SecretRef::for_instance(
        id,
        &SecretPurpose::from_field_name(field).expect("a valid purpose"),
    )
}

#[tokio::test]
async fn a_create_pins_registers_and_initializes_a_new_instance() {
    let ev = evidence!(
        covers = ["REQ-008", "BR-001"],
        kind = Kind::Fixture,
        expected = "a create on the fake provider pins the catalog package, stores the generated secret, makes the folders, registers the instance with its first config revision, and initializes it, and ends succeeded with a stopped instance whose created_at is the manifest createdAt",
    );
    let cr = CreateRig::new().await;
    let rig = &cr.rig;
    let id = InstanceId::generate();

    let op = cr
        .begin(id, cr.draft("orders"), Vec::new(), CreateHooks::default())
        .await
        .expect("the engine accepts the create");
    let step = rig.helper_spawn(ChildRole::InitStep, 0).await;
    rig.processes().exit(step.pid, Some(ExitStatus::Exited(0)));
    let done = rig.wait_state(op, OperationState::Succeeded).await;

    let instance = rig.instance_row(id);
    assert_eq!(instance.lifecycle(), InstanceLifecycle::Stopped);
    assert_eq!(instance.name().as_str(), "orders");
    assert_eq!(instance.provider_revision(), cr.package.key());
    assert_eq!(instance.runtime_id(), rig.runtime.id());
    let manifest = read_instance_manifest(&rig.platform, &rig.paths(id)).expect("a manifest");
    assert_eq!(manifest.created_at(), instance.created_at());
    assert_eq!(
        manifest.data().initialized_at(),
        instance.data_initialized_at()
    );
    assert_eq!(manifest.created_by_operation(), op);
    assert!(
        rig.registry
            .provider_revision(cr.package.key())
            .expect("it reads")
            .value
            .is_some(),
        "the revision is recorded"
    );
    assert!(
        rig.layout
            .layout()
            .provider_revision_dir(cr.package.key())
            .join("provider.yaml")
            .is_file(),
        "the package is pinned"
    );
    let source = rig
        .registry
        .provider_source_by_location(ProviderSourceKind::Development, "/stackly-test-catalog")
        .expect("it reads")
        .value
        .expect("the catalog source is recorded");
    assert_eq!(
        rig.registry
            .provider_revision(cr.package.key())
            .expect("it reads")
            .value
            .and_then(|record| record.source_id),
        Some(source.id)
    );
    let config = rig
        .registry
        .active_config_revision(id)
        .expect("it reads")
        .value
        .expect("a first config revision");
    assert_eq!(config.created_by_operation, Some(op));
    let reference = reference_of(id, "authToken");
    assert!(config.values.as_str().contains(reference.as_str()));
    let uses = rig
        .registry
        .secret_uses_by(SecretUser::Instance(id))
        .expect("it reads")
        .value;
    assert_eq!(uses.len(), 1);
    assert_eq!(uses[0].reference, reference);
    assert!(rig.fake.secrets().contains(&reference));
    let steps: Vec<_> = done
        .steps
        .iter()
        .map(|step| step.name().as_str().to_owned())
        .collect();
    assert_eq!(
        steps,
        ["validate", "pin", "secrets", "layout", "register", "init"]
    );
    assert!(
        done.created
            .iter()
            .all(|c| matches!(c.state(), CreatedState::Created | CreatedState::Kept)),
        "{:?}",
        states(&done)
    );
    assert!(cr.removed.all().is_empty());
    ev.pass();
}

/// Nothing a create made is left, except the pinned package.
fn assert_clean(cr: &CreateRig, id: InstanceId) {
    let rig = &cr.rig;
    assert!(
        rig.registry.instance(id).expect("it reads").value.is_none(),
        "no instance row"
    );
    let paths = rig.paths(id);
    assert!(!paths.root.exists(), "no instance folder");
    assert!(
        !rig.layout.layout().instance_log_dir(id).exists(),
        "no log folder"
    );
    let reference = reference_of(id, "authToken");
    assert!(
        rig.registry
            .secret_reference(&reference)
            .expect("it reads")
            .value
            .is_none(),
        "no reference row"
    );
    assert!(!rig.fake.secrets().contains(&reference), "no stored value");
    assert!(
        rig.registry
            .secret_uses_by(SecretUser::Instance(id))
            .expect("it reads")
            .value
            .is_empty(),
        "no secret use"
    );
    assert!(
        rig.layout
            .layout()
            .provider_revision_dir(cr.package.key())
            .join("provider.yaml")
            .is_file(),
        "the pinned package stays"
    );
}

fn states(detail: &stackly_core::operation::OperationDetailView) -> Vec<(String, CreatedState)> {
    detail
        .created
        .iter()
        .map(|c| (c.item().kind_code().to_owned(), c.state()))
        .collect()
}

#[tokio::test]
async fn a_failed_init_is_compensated_in_full_and_the_pinned_package_stays() {
    let ev = evidence!(
        covers = ["BR-023", "BR-008"],
        kind = Kind::Fixture,
        expected = "an init step that exits with an error ends the operation failed with supervisor.init_failed, and compensation removes the registry row (with an instance.removed event), the instance folders, and the secret reference, while the pinned package stays",
    );
    let cr = CreateRig::new().await;
    let rig = &cr.rig;
    let id = InstanceId::generate();

    let op = cr
        .begin(id, cr.draft("orders"), Vec::new(), CreateHooks::default())
        .await
        .expect("accepted");
    let step = rig.helper_spawn(ChildRole::InitStep, 0).await;
    rig.processes().exit(step.pid, Some(ExitStatus::Exited(1)));
    let failed = rig.wait_state(op, OperationState::Failed).await;

    assert_eq!(Rig::failure_code(&failed), "supervisor.init_failed");
    assert_clean(&cr, id);
    assert_eq!(
        states(&failed),
        [
            ("secret_ref".to_owned(), CreatedState::Compensated),
            ("instance_tree".to_owned(), CreatedState::Compensated),
            ("custom".to_owned(), CreatedState::Compensated),
        ]
    );
    let removed = cr.removed.all();
    assert_eq!(removed.len(), 1);
    assert_eq!(removed[0].0, id);
    ev.pass();
}

#[tokio::test]
async fn a_folder_that_exists_is_never_initialized_or_removed() {
    let ev = evidence!(
        covers = ["BR-008", "BR-023"],
        kind = Kind::Fixture,
        expected = "a create whose new ID already has an instance folder with an orphan PG_VERSION fails instance.data_not_empty before init: nothing is spawned, the orphan file is identical, the tree record is not_owned already_exists, and the secret of the operation is compensated",
    );
    let cr = CreateRig::new().await;
    let rig = &cr.rig;
    let id = InstanceId::generate();
    let data = rig.paths(id).data;
    std::fs::create_dir_all(&data).expect("the orphan folder can be made");
    std::fs::write(data.join("PG_VERSION"), b"17\n").expect("the orphan file can be written");

    let op = cr
        .begin(id, cr.draft("orders"), Vec::new(), CreateHooks::default())
        .await
        .expect("accepted");
    let failed = rig.wait_state(op, OperationState::Failed).await;

    assert_eq!(Rig::failure_code(&failed), "instance.data_not_empty");
    assert_eq!(
        std::fs::read(data.join("PG_VERSION")).expect("it reads"),
        b"17\n"
    );
    assert!(rig.processes().spawns().is_empty(), "nothing is spawned");
    let tree = failed
        .created
        .iter()
        .find(|c| c.item().kind_code() == "instance_tree")
        .expect("a tree record");
    assert_eq!(tree.state(), CreatedState::NotOwned);
    assert_eq!(
        tree.detail().map(ToString::to_string).as_deref(),
        Some("already_exists")
    );
    assert!(
        failed
            .created
            .iter()
            .any(|c| c.item().kind_code() == "secret_ref" && c.state() == CreatedState::Compensated)
    );
    assert!(rig.registry.instance(id).expect("it reads").value.is_none());
    ev.pass();
}

#[tokio::test]
async fn a_file_that_appears_in_the_data_folder_after_the_layout_stops_the_create() {
    let ev = evidence!(
        covers = ["BR-008", "BR-023"],
        kind = Kind::Fixture,
        expected = "a file planted between the creation of the layout and the check of the data folder fails the create with instance.data_not_empty before init, the file is left identical, and the tree record is not_owned data_not_empty so compensation does not delete it",
    );
    let cr = CreateRig::new().await;
    let rig = &cr.rig;
    let id = InstanceId::generate();
    let hooks = CreateHooks {
        after_layout: Some(Arc::new(|paths| {
            std::fs::write(paths.data.join("planted"), b"mine").expect("it plants a file");
        })),
        before_init: None,
    };

    let op = cr
        .begin(id, cr.draft("orders"), Vec::new(), hooks)
        .await
        .expect("accepted");
    let failed = rig.wait_state(op, OperationState::Failed).await;

    assert_eq!(Rig::failure_code(&failed), "instance.data_not_empty");
    assert_eq!(
        std::fs::read(rig.paths(id).data.join("planted")).expect("it reads"),
        b"mine"
    );
    let tree = failed
        .created
        .iter()
        .find(|c| c.item().kind_code() == "instance_tree")
        .expect("a tree record");
    assert_eq!(tree.state(), CreatedState::NotOwned);
    assert_eq!(
        tree.detail().map(ToString::to_string).as_deref(),
        Some("data_not_empty")
    );
    assert!(rig.processes().spawns().is_empty());
    ev.pass();
}

#[tokio::test]
async fn a_cancel_during_init_waits_for_the_step_then_compensates() {
    let ev = evidence!(
        covers = ["BR-023"],
        kind = Kind::Fixture,
        expected = "a cancel while init runs lets the init step finish, then compensates everything and ends the operation cancelled with no row, folder, or reference left",
    );
    let cr = CreateRig::new().await;
    let rig = &cr.rig;
    let id = InstanceId::generate();
    let op = cr
        .begin(id, cr.draft("orders"), Vec::new(), CreateHooks::default())
        .await
        .expect("accepted");
    let step = rig.helper_spawn(ChildRole::InitStep, 0).await;

    rig.engine.cancel(op).await.expect("the cancel is accepted");
    rig.processes().exit(step.pid, Some(ExitStatus::Exited(0)));
    let cancelled = rig.wait_state(op, OperationState::Cancelled).await;

    assert_clean(&cr, id);
    assert!(
        states(&cancelled)
            .iter()
            .all(|(_, state)| *state == CreatedState::Compensated),
        "{:?}",
        states(&cancelled)
    );
    ev.pass();
}

#[tokio::test]
async fn a_lock_a_helper_still_holds_keeps_the_row_and_the_tree_until_abandon() {
    let ev = evidence!(
        covers = ["BR-023", "VAL-007"],
        kind = Kind::Fixture,
        expected = "when a process holds instance.lock at compensation, instance_registration and instance_tree refuse with instance_locked and delete nothing, so the operation needs recovery with the registry row and the folder intact, and abandon cleans them once the lock is free",
    );
    let cr = CreateRig::new().await;
    let rig = &cr.rig;
    let id = InstanceId::generate();
    let held: Arc<Mutex<Option<InstanceLock>>> = Arc::new(Mutex::new(None));
    let hooks = CreateHooks {
        after_layout: None,
        before_init: Some({
            let held = Arc::clone(&held);
            let processes = rig.processes().clone();
            Arc::new(move |paths| {
                let outcome = processes
                    .lock_instance(&paths.lock)
                    .expect("the fake locks");
                if let stackly_platform::LockOutcome::Acquired(lock) = outcome {
                    *held.lock().unwrap_or_else(PoisonError::into_inner) = Some(lock);
                }
            })
        }),
    };

    let op = cr
        .begin(id, cr.draft("orders"), Vec::new(), hooks)
        .await
        .expect("accepted");
    let stuck = rig.wait_state(op, OperationState::NeedsRecovery).await;

    assert!(
        rig.registry.instance(id).expect("it reads").value.is_some(),
        "the row stays"
    );
    assert!(rig.paths(id).root.is_dir(), "the folder stays");
    assert!(
        stuck
            .created
            .iter()
            .any(|c| c.state() == CreatedState::CompensationFailed
                && c.detail().map(ToString::to_string).as_deref() == Some("instance_locked")),
        "{:?}",
        states(&stuck)
    );

    held.lock().unwrap_or_else(PoisonError::into_inner).take();
    let abandoned = rig
        .engine
        .abandon(op, stuck.view.operation.revision())
        .await
        .expect("abandon is accepted");

    assert_eq!(abandoned.view.operation.state(), OperationState::Failed);
    assert_clean(&cr, id);
    ev.pass();
}

#[tokio::test]
async fn a_locked_keychain_fails_the_secrets_step_before_any_folder_exists() {
    let ev = evidence!(
        covers = ["AC-007"],
        kind = Kind::Fixture,
        expected = "a locked secret store fails a generated secret with secret.store_locked, a copy from another reference with secret.needs_secret and one store_locked item, both before the instance folder is made",
    );
    let cr = CreateRig::new().await;
    let rig = &cr.rig;
    rig.fake.secrets().lock();
    let id = InstanceId::generate();

    let op = cr
        .begin(id, cr.draft("orders"), Vec::new(), CreateHooks::default())
        .await
        .expect("accepted");
    let failed = rig.wait_state(op, OperationState::Failed).await;

    assert_eq!(Rig::failure_code(&failed), "secret.store_locked");
    assert!(!rig.paths(id).root.exists(), "no folder before the secret");
    assert!(rig.registry.instance(id).expect("it reads").value.is_none());
    ev.pass();
}

/// Finishes the init of the create that runs next: the `n`th init helper.
async fn finish_init(rig: &Rig, n: usize) {
    let step = rig.helper_spawn(ChildRole::InitStep, n).await;
    rig.processes().exit(step.pid, Some(ExitStatus::Exited(0)));
}

fn register_reference(rig: &Rig, text: &str, value: &str) -> SecretRef {
    use stackly_domain::{
        PlatformStoreKey, SecretOwner, SecretReferenceId, SecretReferenceRecord, SecretStatus,
        WriteBatch, WriteOp,
    };
    let reference = SecretRef::parse(text).expect("a valid reference");
    let mut record = SecretReferenceRecord::new(
        SecretReferenceId::generate(),
        reference.clone(),
        PlatformStoreKey::new(format!("fake:{text}")).expect("a valid key"),
        SecretOwner::User,
        SecretPurpose::user(),
        rig.fake_now(),
    );
    record.status = SecretStatus::Present;
    rig.registry
        .commit(WriteBatch::new(vec![WriteOp::InsertSecretReference(
            record,
        )]))
        .expect("the reference commits");
    rig.fake.secrets().insert(&reference, value);
    reference
}

#[tokio::test]
async fn a_value_a_copy_and_a_bind_each_reach_the_instance_without_a_value_in_the_journal() {
    let ev = evidence!(
        covers = ["AC-007"],
        kind = Kind::Fixture,
        expected = "a value secret is stored under the derived reference and never appears in the journal input, a copy gets its own reference with the source value, and a bind records only the use of the bound reference and creates no reference",
    );
    let cr = CreateRig::new().await;
    let rig = &cr.rig;

    // value
    let id = InstanceId::generate();
    let mut draft = cr.draft("with-value");
    draft.secrets = vec![("authToken".to_owned(), SecretSourceKind::Value)];
    let value = SecretValue::new("fixture-secret-value-1".to_owned()).expect("a value");
    let op = cr
        .begin(
            id,
            draft,
            vec![("authToken".to_owned(), value)],
            CreateHooks::default(),
        )
        .await
        .expect("accepted");
    finish_init(rig, 0).await;
    let done = rig.wait_state(op, OperationState::Succeeded).await;
    assert_eq!(
        rig.fake
            .secrets()
            .stored_value(&reference_of(id, "authToken")),
        Some("fixture-secret-value-1".to_owned())
    );
    assert!(
        !done
            .view
            .journal
            .input()
            .as_str()
            .contains("fixture-secret-value-1"),
        "the journal input holds no value"
    );

    // copy
    let source = register_reference(rig, "local:shared/fake/admin", "copied-secret-value-2");
    let id = InstanceId::generate();
    let mut draft = cr.draft("with-copy");
    draft.secrets = vec![(
        "authToken".to_owned(),
        SecretSourceKind::CopyFrom(source.to_string()),
    )];
    let op = cr
        .begin(id, draft, Vec::new(), CreateHooks::default())
        .await
        .expect("accepted");
    finish_init(rig, 1).await;
    rig.wait_state(op, OperationState::Succeeded).await;
    assert_eq!(
        rig.fake
            .secrets()
            .stored_value(&reference_of(id, "authToken")),
        Some("copied-secret-value-2".to_owned())
    );
    assert!(
        rig.fake.secrets().contains(&source),
        "the source is untouched"
    );

    // bind
    let bound = register_reference(rig, "local:shared/fake/bound", "bound-secret-value-3");
    let id = InstanceId::generate();
    let mut draft = cr.draft("with-bind");
    draft.secrets = vec![(
        "authToken".to_owned(),
        SecretSourceKind::Bind(bound.to_string()),
    )];
    let op = cr
        .begin(id, draft, Vec::new(), CreateHooks::default())
        .await
        .expect("accepted");
    finish_init(rig, 2).await;
    rig.wait_state(op, OperationState::Succeeded).await;
    assert!(
        !rig.fake.secrets().contains(&reference_of(id, "authToken")),
        "a bind creates no reference"
    );
    let uses = rig
        .registry
        .secret_uses_by(SecretUser::Instance(id))
        .expect("it reads")
        .value;
    assert_eq!(uses.len(), 1);
    assert_eq!(uses[0].reference, bound);
    ev.pass();
}

#[tokio::test]
async fn a_copy_from_a_locked_store_needs_the_source_and_nothing_else_is_made() {
    let cr = CreateRig::new().await;
    let rig = &cr.rig;
    let source = register_reference(rig, "local:shared/fake/locked", "locked-secret-value-4");
    rig.fake.secrets().lock();
    let id = InstanceId::generate();
    let mut draft = cr.draft("orders");
    draft.secrets = vec![(
        "authToken".to_owned(),
        SecretSourceKind::CopyFrom(source.to_string()),
    )];

    let op = cr
        .begin(id, draft, Vec::new(), CreateHooks::default())
        .await
        .expect("accepted");
    let failed = rig.wait_state(op, OperationState::Failed).await;

    let error = failed
        .view
        .journal
        .failure_error()
        .expect("the failure is stored");
    assert_eq!(error.code().as_str(), "secret.needs_secret");
    assert_eq!(error.items().len(), 1);
    let fields = error.items()[0].fields();
    assert!(
        fields
            .iter()
            .any(|(key, value)| key.as_str() == "reference" && value == source.as_str())
    );
    assert!(
        fields
            .iter()
            .any(|(key, value)| key.as_str() == "reason" && value == "store_locked")
    );
    assert!(!rig.paths(id).root.exists(), "no folder");
}

#[tokio::test]
async fn a_generation_conflict_at_register_is_retried_up_to_five_times() {
    let ev = evidence!(
        covers = ["REQ-008"],
        kind = Kind::Fixture,
        expected = "a register commit that meets a generation conflict four times still succeeds on the fifth try, and five conflicts end the operation failed with registry.generation_conflict and a full compensation",
    );
    let cr = CreateRig::new().await;
    let rig = &cr.rig;

    rig.registry.conflict_instance_inserts(4);
    let id = InstanceId::generate();
    let op = cr
        .begin(id, cr.draft("retried"), Vec::new(), CreateHooks::default())
        .await
        .expect("accepted");
    finish_init(rig, 0).await;
    rig.wait_state(op, OperationState::Succeeded).await;
    assert!(rig.registry.instance(id).expect("it reads").value.is_some());

    rig.registry.conflict_instance_inserts(5);
    let id = InstanceId::generate();
    let op = cr
        .begin(
            id,
            cr.draft("conflicted"),
            Vec::new(),
            CreateHooks::default(),
        )
        .await
        .expect("accepted");
    let failed = rig.wait_state(op, OperationState::Failed).await;
    assert_eq!(Rig::failure_code(&failed), "registry.generation_conflict");
    assert_clean(&cr, id);
    ev.pass();
}

#[tokio::test]
async fn ten_creates_with_one_name_make_one_instance() {
    let ev = evidence!(
        covers = ["REQ-008"],
        kind = Kind::Fixture,
        expected = "ten creates begun at once with the same name make exactly one operation, the other nine are refused at once as busy, and a later create of that name fails instance.name_taken",
    );
    let cr = Arc::new(CreateRig::new().await);
    let mut tasks = Vec::new();
    for _ in 0..10 {
        let cr = Arc::clone(&cr);
        tasks.push(tokio::spawn(async move {
            cr.begin(
                InstanceId::generate(),
                cr.draft("shared"),
                Vec::new(),
                CreateHooks::default(),
            )
            .await
        }));
    }
    let mut accepted = Vec::new();
    let mut busy = 0;
    for task in tasks {
        match task.await.expect("the task ends") {
            Ok(op) => accepted.push(op),
            Err(stackly_core::operation::BeginError::Busy { .. }) => busy += 1,
            Err(other) => panic!("unexpected refusal: {other:?}"),
        }
    }
    assert_eq!((accepted.len(), busy), (1, 9));

    finish_init(&cr.rig, 0).await;
    cr.rig
        .wait_state(accepted[0], OperationState::Succeeded)
        .await;
    let again = cr
        .begin(
            InstanceId::generate(),
            cr.draft("shared"),
            Vec::new(),
            CreateHooks::default(),
        )
        .await
        .expect("accepted");
    let failed = cr.rig.wait_state(again, OperationState::Failed).await;
    assert_eq!(Rig::failure_code(&failed), "instance.name_taken");
    ev.pass();
}

#[tokio::test]
async fn two_creates_with_one_fixed_port_make_one_instance_with_that_port() {
    let ev = evidence!(
        covers = ["BR-004", "BR-005"],
        kind = Kind::Fixture,
        expected = "two creates with different names and the same fixed port, held apart at register, leave exactly one instance with the port: the other fails supervisor.endpoint_conflict and leaves no folder, reference, or row",
    );
    let cr = CreateRig::new().await;
    let rig = &cr.rig;
    let port = Rig::free_port();
    let draft = |name: &str| {
        let mut draft = cr.draft(name);
        draft.endpoints = vec![(
            "http".to_owned(),
            stackly_core::instance::create::PortRequestDraft::Fixed(port),
        )];
        draft
    };
    let hold = rig
        .registry
        .hold_stage(stackly_domain::OperationKind::CreateInstance, "register");
    let first_id = InstanceId::generate();
    let first = cr
        .begin(first_id, draft("first"), Vec::new(), CreateHooks::default())
        .await
        .expect("accepted");
    while !hold.entered() {
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }

    let second_id = InstanceId::generate();
    let second = cr
        .begin(
            second_id,
            draft("second"),
            Vec::new(),
            CreateHooks::default(),
        )
        .await
        .expect("accepted");
    finish_init(rig, 0).await;
    rig.wait_state(second, OperationState::Succeeded).await;
    hold.release();
    let failed = rig.wait_state(first, OperationState::Failed).await;

    assert_eq!(Rig::failure_code(&failed), "supervisor.endpoint_conflict");
    assert_clean(&cr, first_id);
    assert!(
        rig.registry
            .instance(second_id)
            .expect("it reads")
            .value
            .is_some()
    );
    ev.pass();
}

#[tokio::test]
async fn a_catalog_source_that_cannot_be_recorded_stops_the_pin_before_any_file() {
    let ev = evidence!(
        covers = ["REQ-008"],
        kind = Kind::Fixture,
        expected = "when the source row cannot be written the pin step fails catalog.pin_failed with reason source_unrecorded and no file of the package is written",
    );
    let cr = CreateRig::new().await;
    let rig = &cr.rig;
    cr.fail_source();
    let id = InstanceId::generate();

    let op = cr
        .begin(id, cr.draft("orders"), Vec::new(), CreateHooks::default())
        .await
        .expect("accepted");
    let failed = rig.wait_state(op, OperationState::Failed).await;

    assert_eq!(Rig::failure_code(&failed), "catalog.pin_failed");
    assert!(
        failed
            .view
            .journal
            .failure_error()
            .expect("stored")
            .details()
            .iter()
            .any(|(k, v)| k.as_str() == "reason" && v == "source_unrecorded")
    );
    assert!(
        !rig.layout
            .layout()
            .provider_revision_dir(cr.package.key())
            .exists(),
        "nothing is pinned"
    );
    ev.pass();
}

#[tokio::test]
async fn a_second_create_reuses_the_pinned_revision() {
    let cr = CreateRig::new().await;
    let rig = &cr.rig;
    for (n, name) in ["one", "two"].into_iter().enumerate() {
        let op = cr
            .begin(
                InstanceId::generate(),
                cr.draft(name),
                Vec::new(),
                CreateHooks::default(),
            )
            .await
            .expect("accepted");
        finish_init(rig, n).await;
        rig.wait_state(op, OperationState::Succeeded).await;
    }
    let sources = rig
        .registry
        .provider_source_by_location(ProviderSourceKind::Development, "/stackly-test-catalog")
        .expect("it reads")
        .value;
    assert!(sources.is_some());
}

#[test]
fn the_input_of_a_huge_settings_document_keeps_only_its_digest() {
    use stackly_core::instance::create::{
        CreateDraft, PortRequestDraft, ProviderChoice, encode_input,
    };
    let draft = CreateDraft {
        name: "orders".to_owned(),
        display_name: None,
        provider: ProviderChoice {
            name: "testing/fake".to_owned(),
            revision: None,
        },
        runtime_id: stackly_domain::RuntimeId::generate(),
        endpoints: vec![("http".to_owned(), PortRequestDraft::Fixed(1234))],
        settings: serde_json::json!({ "label": "x".repeat(70_000) }),
        secrets: vec![("authToken".to_owned(), SecretSourceKind::Value)],
        secret_values: Vec::new(),
        start: false,
    };

    let payload = encode_input(InstanceId::generate(), &draft).expect("it fits once shortened");

    let text = payload.as_str();
    assert!(text.contains("\"settingsOmitted\":true"));
    assert!(text.contains("settingsSha256"));
    assert!(!text.contains(&"x".repeat(100)));
    assert!(text.contains("\"type\":\"value\""));
}

#[tokio::test]
async fn the_registration_compensator_passes_the_compensator_conformance_check() {
    use stackly_core::instance::create::{INSTANCE_REGISTRATION, registration_key};
    use stackly_core::operation::CompensationScope;
    use stackly_core::operation::testing::compensator_conformance;
    use stackly_domain::{CreatedItem, CreatedRecord, CreatedSeq, StepSeq};

    let cr = CreateRig::new().await;
    let rig = &cr.rig;
    let owned_id = rig.instance(&crate::common::Spec::default());
    let foreign_id = rig.instance(&crate::common::Spec::default());
    let operation = rig.instance_row(owned_id).created_by();
    let record = |id: InstanceId| {
        CreatedRecord::intend(
            CreatedSeq::FIRST,
            StepSeq::FIRST,
            CreatedItem::Custom {
                compensator: INSTANCE_REGISTRATION,
                key: registration_key(id),
            },
            None,
            rig.fake_now(),
        )
        .expect("a custom record")
    };
    let scope = CompensationScope {
        operation,
        registry: Arc::clone(&rig.registry) as Arc<dyn Registry>,
        platform: Arc::clone(&rig.platform),
        layout: Arc::clone(&rig.layout),
    };
    let compensator = stackly_core::instance::create::InstanceRegistrationCompensator::new(
        Arc::clone(&cr.removed) as Arc<dyn stackly_core::instance::create::InstanceRemoved>,
    );

    compensator_conformance(&compensator, scope, &record(owned_id), &record(foreign_id)).await;

    assert!(
        rig.registry
            .instance(owned_id)
            .expect("it reads")
            .value
            .is_none()
    );
    assert!(
        rig.registry
            .instance(foreign_id)
            .expect("it reads")
            .value
            .is_some(),
        "an instance of another operation stays"
    );
    assert_eq!(cr.removed.all().len(), 1);
}

#[tokio::test]
async fn a_queued_create_is_rebuilt_from_its_input_unless_it_held_a_value() {
    use stackly_core::instance::create::CreateWorkFactory;
    use stackly_core::operation::{RebuildError, WorkFactory};

    let cr = CreateRig::new().await;
    let rig = &cr.rig;
    let factory = CreateWorkFactory::new(cr.deps(CreateHooks::default()));
    let journal_of = |op| {
        let operation = rig
            .registry
            .operation(op)
            .expect("it reads")
            .value
            .expect("the operation");
        let journal = rig
            .registry
            .operation_journal(op)
            .expect("it reads")
            .value
            .expect("the journal");
        (operation, journal)
    };

    let generated = cr
        .begin(
            InstanceId::generate(),
            cr.draft("generated"),
            Vec::new(),
            CreateHooks::default(),
        )
        .await
        .expect("accepted");
    let (operation, journal) = journal_of(generated);
    assert!(factory.rebuild(&operation, &journal).is_ok());
    finish_init(rig, 0).await;
    rig.wait_state(generated, OperationState::Succeeded).await;

    let mut draft = cr.draft("entered");
    draft.secrets = vec![("authToken".to_owned(), SecretSourceKind::Value)];
    let value = SecretValue::new("fixture-secret-value-5".to_owned()).expect("a value");
    let entered = cr
        .begin(
            InstanceId::generate(),
            draft,
            vec![("authToken".to_owned(), value)],
            CreateHooks::default(),
        )
        .await
        .expect("accepted");
    let (operation, journal) = journal_of(entered);
    let refusal = factory.rebuild(&operation, &journal).err();
    assert!(
        matches!(refusal, Some(RebuildError::NotRebuildable { .. })),
        "{refusal:?}"
    );
    finish_init(rig, 1).await;
    rig.wait_state(entered, OperationState::Succeeded).await;
}
