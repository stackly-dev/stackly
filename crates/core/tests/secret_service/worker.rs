//! The one store thread and its wait limits (spec 0011 AC-4, AC-8).

use std::sync::Arc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use stackly_core::secret::{SecretWorker, WorkerConfig};
use stackly_domain::SecretStatus;
use stackly_platform::{Interaction, SecretStore, SecretStoreError, StoreState, WriteMode};
use stackly_testkit::evidence::{Kind, evidence};
use stackly_testkit::{FakePlatform, FakeSecretStore, SecretOp, TestRoot};

use crate::common::{Harness, reference, value};

/// Long enough for a held call to be waiting, short enough for a test.
const SHORT: Duration = Duration::from_millis(150);
const LONG: Duration = Duration::from_secs(5);

fn worker(
    store: &FakeSecretStore,
    prompt_wait: Duration,
    background_wait: Duration,
) -> Arc<SecretWorker> {
    SecretWorker::spawn(
        Arc::new(store.clone()),
        WorkerConfig {
            prompt_wait,
            background_wait,
        },
    )
    .expect("the worker thread starts")
}

#[test]
fn the_default_limits_are_180_seconds_with_a_dialog_and_10_without() {
    assert_eq!(
        WorkerConfig::default().prompt_wait,
        Duration::from_secs(180)
    );
    assert_eq!(
        WorkerConfig::DEFAULT.background_wait,
        Duration::from_secs(10)
    );
}

#[test]
fn a_call_queued_behind_a_held_one_fails_busy_and_never_runs() {
    let ev = evidence!(
        covers = ["REQ-016"],
        kind = Kind::Fixture,
        expected = "with one call held on the worker thread, a call without a dialog queued \
                    behind it fails busy at its limit and never reaches the store, and the \
                    store state answers at once",
    );
    let store = FakeSecretStore::new();
    let held_ref = reference("local:erp/held");
    let queued_ref = reference("local:erp/queued");
    store.insert(&held_ref, "the-held-value-here");
    store.insert(&queued_ref, "the-queued-value-here");
    let worker = worker(&store, LONG, SHORT);
    let hold = store.hold_next();
    let held = {
        let worker = Arc::clone(&worker);
        let reference = held_ref.clone();
        std::thread::spawn(move || {
            worker
                .read(&reference, Interaction::AllowPrompt)
                .map(|value| value.len())
        })
    };
    assert!(hold.wait_until_held(LONG));

    let started = Instant::now();
    let queued = worker.read(&queued_ref, Interaction::Never);
    let state = {
        let asked = Instant::now();
        let state = worker.state();
        (state, asked.elapsed())
    };

    assert_eq!(queued.unwrap_err(), SecretStoreError::Busy);
    assert!(started.elapsed() < LONG);
    assert!(
        state.1 < Duration::from_millis(50),
        "state() waited {:?}",
        state.1
    );
    assert_eq!(state.0, StoreState::Unknown, "no call has finished yet");
    hold.release();
    assert_eq!(held.join().unwrap(), Ok("the-held-value-here".len()));
    // Let the worker reach the queued job, which it skips.
    let probe = worker.probe(&held_ref);
    assert!(probe.is_ok());
    assert_eq!(store.count(SecretOp::Read), 1, "the queued read never ran");
    ev.pass();
}

#[test]
fn a_held_read_past_its_limit_is_busy_and_a_held_write_is_outcome_unknown_then_probed() {
    let root = TestRoot::new().expect("a test root can be created");
    let fake = FakePlatform::new(&root);
    let worker = worker(fake.secrets(), SHORT, SHORT);
    let platform = Arc::new(
        fake.platform()
            .with_secrets(Arc::clone(&worker) as Arc<dyn SecretStore>),
    );
    let harness = Harness::on(root, fake, platform);
    let target = reference("local:erp/admin");
    harness.store().insert(&target, "the-value-before");
    harness.set(&target, "the-value-registered");
    let (probed, probes) = mpsc::channel();
    {
        let service = Arc::clone(&harness.service);
        worker.on_late_change(move |reference| {
            let view = service.probe_one(reference);
            let _ = probed.send(view.map(|view| view.record.status));
        });
    }

    let hold = harness.store().hold_next();
    let err = worker.read(&target, Interaction::Never).unwrap_err();
    assert_eq!(err, SecretStoreError::Busy);
    hold.release();

    harness.store().remove(&target);
    let hold = harness.store().hold_next();
    let err = worker
        .write(
            &target,
            &value("the-value-written-late"),
            WriteMode::Create,
            Interaction::AllowPrompt,
        )
        .unwrap_err();
    assert_eq!(err, SecretStoreError::OutcomeUnknown);
    assert!(probes.try_recv().is_err(), "no probe while the write runs");
    hold.release();

    let status = probes
        .recv_timeout(LONG)
        .expect("the late write is followed by a probe");
    assert_eq!(status, Ok(SecretStatus::Present));
    assert_eq!(
        harness.store().stored_value(&target).as_deref(),
        Some("the-value-written-late")
    );
    assert_eq!(
        harness.view(&target).unwrap().record.status,
        SecretStatus::Present
    );
    assert_eq!(worker.state(), StoreState::Available);
}

#[test]
fn a_dialog_waits_its_own_longer_limit() {
    let store = FakeSecretStore::new();
    let target = reference("local:erp/admin");
    store.insert(&target, "the-value-of-the-test");
    let worker = worker(&store, LONG, SHORT);
    let hold = store.hold_next();
    let release = std::thread::spawn(move || {
        assert!(hold.wait_until_held(LONG));
        std::thread::sleep(SHORT * 2);
        hold.release();
    });

    let read = worker.read(&target, Interaction::AllowPrompt);

    assert!(
        read.is_ok(),
        "a dialog may take longer than a background call: {read:?}"
    );
    release.join().unwrap();
}
