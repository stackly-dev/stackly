//! Behavior of `FakeSecretStore`: the write modes, lock, deny,
//! unsupported, `fail_next`, `hold_next`, the call log, the state cache,
//! and how `FakePlatform` installs it (spec 0011).

use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use stackly_domain::{Os, SecretRef};
use stackly_platform::{
    Interaction, SecretBackend, SecretPresence, SecretStore, SecretStoreError, SecretValue,
    StoreState, WriteMode,
};
use stackly_testkit::{FakeSecretStore, SecretOp};

const VALUE: &str = "not-a-real-secret";

fn reference(text: &str) -> SecretRef {
    SecretRef::parse(text).expect("a valid test reference")
}

fn value(text: &str) -> SecretValue {
    SecretValue::new(text.to_owned()).expect("a valid test value")
}

#[test]
fn write_modes_follow_the_keychain_rules() {
    let store = FakeSecretStore::new();
    let admin = reference("local:erp/admin");

    assert_eq!(
        store.write(&admin, &value("a"), WriteMode::Replace, Interaction::Never),
        Err(SecretStoreError::NotFound)
    );
    store
        .write(
            &admin,
            &value("first"),
            WriteMode::Create,
            Interaction::Never,
        )
        .unwrap();
    assert_eq!(
        store.write(&admin, &value("b"), WriteMode::Create, Interaction::Never),
        Err(SecretStoreError::AlreadyExists)
    );
    store
        .write(
            &admin,
            &value("second"),
            WriteMode::Replace,
            Interaction::Never,
        )
        .unwrap();
    assert_eq!(store.stored_value(&admin).as_deref(), Some("second"));
    store
        .write(
            &admin,
            &value("third"),
            WriteMode::Upsert,
            Interaction::Never,
        )
        .unwrap();
    let other = reference("local:erp/other");
    store
        .write(&other, &value("new"), WriteMode::Upsert, Interaction::Never)
        .unwrap();

    assert_eq!(
        store.read(&admin, Interaction::Never).unwrap().expose(),
        "third"
    );
    assert_eq!(store.probe(&other), Ok(SecretPresence::Present));
    store.delete(&other, Interaction::Never).unwrap();
    assert_eq!(store.probe(&other), Ok(SecretPresence::Missing));
    assert_eq!(
        store.delete(&other, Interaction::Never),
        Err(SecretStoreError::NotFound)
    );
    assert_eq!(
        store.read(&other, Interaction::Never).unwrap_err(),
        SecretStoreError::NotFound
    );
}

#[test]
fn a_locked_store_refuses_reads_and_writes_but_answers_probes_and_deletes() {
    let store = FakeSecretStore::new();
    let admin = reference("local:erp/admin");
    let missing = reference("local:erp/missing");
    store.insert(&admin, VALUE);
    assert_eq!(store.state(), StoreState::Unknown);

    store.lock();
    assert_eq!(
        store.read(&admin, Interaction::AllowPrompt).unwrap_err(),
        SecretStoreError::Locked
    );
    assert_eq!(
        store.write(&admin, &value("x"), WriteMode::Upsert, Interaction::Never),
        Err(SecretStoreError::Locked)
    );
    assert_eq!(
        store.write(&missing, &value("x"), WriteMode::Create, Interaction::Never),
        Err(SecretStoreError::Locked)
    );
    assert_eq!(store.state(), StoreState::Locked);
    assert_eq!(store.probe(&admin), Ok(SecretPresence::Present));
    assert_eq!(store.probe(&missing), Ok(SecretPresence::Missing));
    assert_eq!(
        store.read(&missing, Interaction::Never).unwrap_err(),
        SecretStoreError::NotFound
    );
    assert_eq!(store.state(), StoreState::Locked, "a probe keeps it locked");
    assert_eq!(store.stored_value(&admin).as_deref(), Some(VALUE));

    store.delete(&admin, Interaction::Never).unwrap();
    assert!(!store.contains(&admin));

    store.unlock();
    assert_eq!(store.probe(&admin), Ok(SecretPresence::Missing));
    assert_eq!(store.state(), StoreState::Available);
}

#[test]
fn a_denied_item_refuses_reads_by_user_only_when_a_dialog_was_allowed() {
    let store = FakeSecretStore::new();
    let admin = reference("local:erp/admin");
    store.insert(&admin, VALUE);
    store.deny(&admin);

    assert_eq!(
        store.read(&admin, Interaction::Never).unwrap_err(),
        SecretStoreError::AccessDenied { by_user: false }
    );
    assert_eq!(
        store.read(&admin, Interaction::AllowPrompt).unwrap_err(),
        SecretStoreError::AccessDenied { by_user: true }
    );
    assert_eq!(store.probe(&admin), Ok(SecretPresence::Present));
    store
        .write(
            &admin,
            &value("replaced"),
            WriteMode::Replace,
            Interaction::Never,
        )
        .unwrap();

    store.allow(&admin);
    assert_eq!(
        store.read(&admin, Interaction::Never).unwrap().expose(),
        "replaced"
    );
    store.deny(&admin);
    store.delete(&admin, Interaction::Never).unwrap();
    assert!(!store.contains(&admin));
}

#[test]
fn an_unsupported_store_refuses_every_call() {
    let store = FakeSecretStore::new();
    let admin = reference("local:erp/admin");
    store.unsupported();

    assert_eq!(store.state(), StoreState::Unsupported);
    assert_eq!(
        store.probe(&admin),
        Err(SecretStoreError::NotSupported { os: Os::Macos })
    );
    assert_eq!(
        store.read(&admin, Interaction::Never).unwrap_err(),
        SecretStoreError::NotSupported { os: Os::Macos }
    );
    assert_eq!(store.backend(), SecretBackend::Fake);
}

#[test]
fn fail_next_fails_exactly_one_call_and_changes_nothing() {
    let store = FakeSecretStore::new();
    let admin = reference("local:erp/admin");
    store.insert(&admin, VALUE);

    store.fail_next(SecretStoreError::Backend { code: -25_293 });
    assert_eq!(
        store.write(&admin, &value("x"), WriteMode::Upsert, Interaction::Never),
        Err(SecretStoreError::Backend { code: -25_293 })
    );
    assert_eq!(store.stored_value(&admin).as_deref(), Some(VALUE));
    assert_eq!(
        store.read(&admin, Interaction::Never).unwrap().expose(),
        VALUE
    );
    assert_eq!(store.count(SecretOp::Write), 1);
}

#[test]
fn the_call_log_counts_each_method_with_its_reference_and_interaction() {
    let store = FakeSecretStore::new();
    let admin = reference("local:erp/admin");
    store.insert(&admin, VALUE);

    store.probe(&admin).unwrap();
    store.read(&admin, Interaction::AllowPrompt).unwrap();
    store.read(&admin, Interaction::Never).unwrap();
    store.delete(&admin, Interaction::Never).unwrap();

    assert_eq!(store.count(SecretOp::Probe), 1);
    assert_eq!(store.count(SecretOp::Read), 2);
    assert_eq!(store.count(SecretOp::Write), 0);
    assert_eq!(store.count(SecretOp::Delete), 1);
    let calls = store.calls();
    assert_eq!(calls.len(), 4);
    assert_eq!(calls[0].interaction, None);
    assert_eq!(calls[1].op, SecretOp::Read);
    assert_eq!(calls[1].interaction, Some(Interaction::AllowPrompt));
    assert_eq!(calls[3].reference, admin);
    store.insert(&admin, VALUE);
    assert_eq!(store.calls().len(), 4, "setup calls are not counted");
}

#[test]
fn a_held_call_waits_until_released_while_state_still_answers() {
    let store = FakeSecretStore::new();
    let admin = reference("local:erp/admin");
    store.insert(&admin, VALUE);
    let hold = store.hold_next();

    let worker = {
        let store = store.clone();
        let admin = admin.clone();
        thread::spawn(move || {
            store
                .read(&admin, Interaction::AllowPrompt)
                .map(|value| value.len())
        })
    };
    assert!(hold.wait_until_held(Duration::from_secs(5)));
    assert!(!worker.is_finished());
    let started = Instant::now();
    assert_eq!(store.state(), StoreState::Unknown);
    assert_eq!(store.count(SecretOp::Read), 1);
    assert!(started.elapsed() < Duration::from_secs(1));

    hold.release();
    assert_eq!(worker.join().unwrap(), Ok(VALUE.len()));
    assert_eq!(store.state(), StoreState::Available);
    assert_eq!(
        store.read(&admin, Interaction::Never).unwrap().expose(),
        VALUE,
        "only the next call is held"
    );
}

#[test]
fn dropping_an_unused_hold_disarms_it() {
    let store = FakeSecretStore::new();
    let admin = reference("local:erp/admin");
    let hold = store.hold_next();
    assert!(!hold.wait_until_held(Duration::from_millis(10)));
    drop(hold);

    assert_eq!(store.probe(&admin), Ok(SecretPresence::Missing));
}

#[test]
fn store_keys_name_the_service_and_the_reference_path() {
    let store = FakeSecretStore::new();

    assert_eq!(
        store.store_key(&reference("local:instance/abc/admin-password")),
        "fake:stackly-secret/instance/abc/admin-password"
    );
}

#[test]
fn values_never_show_in_the_debug_output_of_the_store() {
    let store = FakeSecretStore::new();
    store.insert(&reference("local:erp/admin"), VALUE);
    let shared: Arc<dyn SecretStore> = Arc::new(store.clone());

    assert!(!format!("{store:?}").contains(VALUE));
    assert_eq!(shared.backend(), SecretBackend::Fake);
}

#[cfg(unix)]
#[test]
fn fake_platform_installs_one_shared_fake_secret_store() {
    let root = stackly_testkit::TestRoot::new().unwrap();
    let fake = stackly_testkit::FakePlatform::new(&root);
    let admin = reference("local:erp/admin");
    fake.secrets().insert(&admin, VALUE);

    let platform = fake.platform();
    assert_eq!(platform.secrets().backend(), SecretBackend::Fake);
    assert_eq!(
        platform
            .secrets()
            .read(&admin, Interaction::Never)
            .unwrap()
            .expose(),
        VALUE
    );
    fake.secrets().lock();
    let locked = fake.platform().secrets().read(&admin, Interaction::Never);
    assert_eq!(locked.unwrap_err(), SecretStoreError::Locked);
    assert_eq!(fake.secrets().count(SecretOp::Read), 2);
}
