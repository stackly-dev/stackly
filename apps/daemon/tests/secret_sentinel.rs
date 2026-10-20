//! The sentinel suite of the daemon (spec 0011 build step 7, AC-3, AC-13,
//! AC-14, AC-16): a real `stacklyd` with `STACKLY_ROOT` on a test root, so
//! it opens a file keychain of its own and never touches the login
//! Keychain. A recording relay sits between the client and the socket and
//! keeps every IPC frame. The only credentials are fixture credentials,
//! and their prefix may appear only in a `secret.set` request that carries
//! the value and in a `secret.reveal` response, never in any other frame,
//! the daemon log, its stderr, or a registry file including its WAL.
#![cfg(target_os = "macos")]

mod common;

use std::path::Path;
use std::process::{Command, Output};
use std::time::{Duration, Instant};

use common::frame_tap::{FrameTap, Seen, Way};
use common::{client, daemon_paths, runtime, start_daemon};
use stackly_domain::SecretRef;
use stackly_protocol::events::SubscribeParams;
use stackly_protocol::secret::{
    DeleteSecretParams, ListSecretsParams, RevealPurpose, RevealSecretParams, SecretInput,
    SecretMode, SecretStatus, SecretStatusParams, SecretStoreState, SecretText, SetSecretParams,
};
use stackly_protocol::{Call, ErrorBody, Reply, Revision};
use stackly_testkit::TestRoot;
use stackly_testkit::evidence::{Evidence, FixtureCredential, Kind, evidence};

const REFERENCE: &str = "local:erp/postgres/admin";
const COPY: &str = "local:erp/postgres/copy";
const GENERATED: &str = "local:erp/postgres/generated";
const OTHER_USER: &str = "local:user/other";

/// The longest a call that may never show a dialog is allowed to take.
const NO_DIALOG_LIMIT: Duration = Duration::from_secs(1);
/// Who can unblock a run without a GUI login session.
const GUI_OWNER: &str = "the user of this Mac, logged in to a GUI session";

fn set(reference: &str, input: SecretInput, mode: SecretMode) -> Call {
    Call::SecretSet(SetSecretParams {
        reference: reference.to_owned(),
        input,
        mode,
        expected_revision: None,
        confirm_stored_only: false,
    })
}

fn text(value: &str) -> SecretInput {
    SecretInput::Value(SecretText::new(value.to_owned()))
}

fn reveal(reference: &str) -> Call {
    Call::SecretReveal(RevealSecretParams {
        reference: reference.to_owned(),
        purpose: RevealPurpose::Show,
    })
}

async fn ok(client: &stackly_ipc::Client, call: Call) -> Reply {
    match client.call(call).await {
        Ok(reply) => reply,
        Err(error) => panic!("the call failed: {error:?}"),
    }
}

async fn err(client: &stackly_ipc::Client, call: Call) -> ErrorBody {
    match client.call(call).await {
        Ok(reply) => panic!("the call should fail, got {reply:?}"),
        Err(error) => *error,
    }
}

fn run(program: &str, args: &[&str]) -> Output {
    Command::new(program)
        .args(args)
        .output()
        .expect("the macOS command runs")
}

/// Notes the run as blocked and returns false when this process has no
/// GUI login session, which the Security framework needs.
fn has_gui_session(ev: &Evidence) -> bool {
    let output = run("launchctl", &["managername"]);
    let manager = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if manager == "Aqua" {
        return true;
    }
    ev.blocked(
        &format!("no GUI login session (launchd manager `{manager}`)"),
        GUI_OWNER,
    );
    false
}

fn keychain_file(root: &TestRoot) -> String {
    daemon_paths(root)
        .layout
        .keychain_dir()
        .join("stackly.keychain-db")
        .to_str()
        .expect("a UTF-8 path")
        .to_owned()
}

/// The frames that are allowed to carry a credential: a `secret.set`
/// request whose input is a value, and a `secret.reveal` response.
fn may_carry_a_value(frame: &Seen) -> bool {
    match frame.way {
        Way::Request => {
            let json = frame.json();
            json["call"]["type"] == "secret.set"
                && json["call"]["content"]["input"]["type"] == "value"
        }
        Way::Answer => frame.name() == "secret.reveal",
    }
}

/// Reads one file as lossy text, `""` when it is missing.
fn lossy(path: &Path) -> String {
    std::fs::read(path)
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_default()
}

#[test]
fn the_sentinel_value_crosses_the_socket_only_in_a_set_request_and_a_reveal_response() {
    let ev = evidence!(
        covers = ["AC-016", "REQ-016", "BR-018"],
        kind = Kind::Fixture,
        expected = "after secret.set (create, replace, upsert, copy, generate), list, status, reveal, delete, and every failure the daemon answers run against a real stacklyd on a test root, a fixture credential appears in no IPC frame except a secret.set request that carries the value and a secret.reveal response, and its prefix is in no error or event frame, not in the daemon log or stderr at debug, and not in the registry bytes including the WAL",
    );
    if !has_gui_session(&ev) {
        return;
    }
    let root = TestRoot::new().unwrap();
    let daemon = start_daemon(&root);
    let paths = daemon_paths(&root);
    let first = FixtureCredential::new();
    let second = FixtureCredential::new();
    let rt = runtime();

    let (tap, errors) = rt.block_on(async {
        let tap = FrameTap::start(&root.path().join("t"), daemon.socket());
        let (client, _) = client(tap.socket()).await;
        // The events of every secret change are frames too.
        let (_, mut events) = client
            .subscribe(SubscribeParams {
                since: None,
                kinds: vec!["secret.".to_owned()],
                resource: None,
            })
            .await
            .expect("the subscription is accepted");

        ok(
            &client,
            set(REFERENCE, text(first.value()), SecretMode::Create),
        )
        .await;
        let Reply::SecretReveal(shown) = ok(&client, reveal(REFERENCE)).await else {
            panic!("not a reveal reply");
        };
        assert_eq!(shown.value.expose(), first.value());
        ok(
            &client,
            Call::SecretSet(SetSecretParams {
                reference: REFERENCE.to_owned(),
                input: text(second.value()),
                mode: SecretMode::Replace,
                expected_revision: Some(Revision::saturating(1)),
                confirm_stored_only: false,
            }),
        )
        .await;
        let Reply::SecretReveal(shown) = ok(&client, reveal(REFERENCE)).await else {
            panic!("not a reveal reply");
        };
        assert_eq!(shown.value.expose(), second.value());
        ok(
            &client,
            set(OTHER_USER, text(first.value()), SecretMode::Upsert),
        )
        .await;
        ok(
            &client,
            set(
                COPY,
                SecretInput::CopyFrom(REFERENCE.to_owned()),
                SecretMode::Create,
            ),
        )
        .await;
        ok(
            &client,
            set(GENERATED, SecretInput::Generate, SecretMode::Create),
        )
        .await;
        let Reply::SecretReveal(copied) = ok(&client, reveal(COPY)).await else {
            panic!("not a reveal reply");
        };
        assert_eq!(copied.value.expose(), second.value());

        // State, never a value.
        let Reply::SecretList(listed) =
            ok(&client, Call::SecretList(ListSecretsParams::default())).await
        else {
            panic!("not a list reply");
        };
        assert_eq!(listed.items.len(), 4, "{listed:?}");
        let Reply::SecretStatus(status) = ok(
            &client,
            Call::SecretStatus(SecretStatusParams {
                reference: REFERENCE.to_owned(),
            }),
        )
        .await
        else {
            panic!("not a status reply");
        };
        assert_eq!(status.summary.status, SecretStatus::Present);

        // Failures the daemon answers, each carrying the value in its
        // request and none of them echoing it.
        let mut errors = Vec::new();
        errors.push(
            err(
                &client,
                set(REFERENCE, text(first.value()), SecretMode::Create),
            )
            .await,
        );
        errors.push(
            err(
                &client,
                set(
                    "local:erp/none/here",
                    text(first.value()),
                    SecretMode::Replace,
                ),
            )
            .await,
        );
        errors.push(
            err(
                &client,
                Call::SecretSet(SetSecretParams {
                    reference: REFERENCE.to_owned(),
                    input: text(first.value()),
                    mode: SecretMode::Replace,
                    expected_revision: Some(Revision::saturating(99)),
                    confirm_stored_only: false,
                }),
            )
            .await,
        );
        errors.push(
            err(
                &client,
                set(
                    "local:stackly/internal/key",
                    text(first.value()),
                    SecretMode::Create,
                ),
            )
            .await,
        );
        errors.push(
            err(
                &client,
                set("keychain:bad", text(first.value()), SecretMode::Create),
            )
            .await,
        );
        errors.push(
            err(
                &client,
                set(
                    "local:erp/nul/value",
                    text(&format!("{}\0tail", first.value())),
                    SecretMode::Create,
                ),
            )
            .await,
        );
        errors.push(
            err(
                &client,
                set(
                    "local:erp/long/value",
                    text(&format!("{}{}", first.value(), "x".repeat(65_537))),
                    SecretMode::Create,
                ),
            )
            .await,
        );
        errors.push(err(&client, reveal("local:erp/none/here")).await);

        ok(
            &client,
            Call::SecretDelete(DeleteSecretParams {
                reference: COPY.to_owned(),
                expected_revision: None,
            }),
        )
        .await;
        errors.push(
            err(
                &client,
                Call::SecretDelete(DeleteSecretParams {
                    reference: COPY.to_owned(),
                    expected_revision: None,
                }),
            )
            .await,
        );

        // Let the change events cross the relay before the scan.
        let deadline = Instant::now() + Duration::from_secs(5);
        while tap
            .frames()
            .iter()
            .filter(|frame| frame.way == Way::Answer && frame.name() == "secret.changed")
            .count()
            < 6
            && Instant::now() < deadline
        {
            let _ = tokio::time::timeout(Duration::from_millis(100), events.next()).await;
        }
        (tap, errors)
    });
    drop(rt);
    assert_eq!(daemon.stop().unwrap().code(), Some(0));

    let frames = tap.frames();
    let events = frames
        .iter()
        .filter(|frame| frame.way == Way::Answer && frame.name() == "secret.changed")
        .count();
    assert!(events >= 6, "the relay saw {events} secret.changed events");
    let mut carriers = 0;
    for frame in &frames {
        let seen = frame.text();
        if may_carry_a_value(frame) {
            if seen.contains("stackly-fixture-credential-") {
                carriers += 1;
            }
            continue;
        }
        first.assert_absent(
            &seen,
            &format!("the {:?} frame {}", frame.way, frame.name()),
        );
    }
    // Six set requests with a value (create, replace, upsert, and the
    // three failures that still carry one) plus the two reveals of a
    // value are the only frames allowed to hold it.
    assert!(carriers >= 8, "only {carriers} frames carried a value");
    for error in &errors {
        first.assert_absent(&format!("{error:?}"), "an error the client received");
        second.assert_absent(&format!("{error:?}"), "an error the client received");
    }
    assert!(
        errors.iter().any(|error| error.code() == "secret.conflict"),
        "{errors:?}"
    );
    assert!(
        errors
            .iter()
            .any(|error| error.code() == "request.invalid_params"),
        "{errors:?}"
    );

    let log_dir = paths.layout.daemon_log_dir();
    let mut logs = lossy(&log_dir.join("stacklyd.log"));
    logs.push_str(&std::fs::read_to_string(daemon_stderr(&root)).unwrap_or_default());
    first.assert_absent(&logs, "the daemon log and stderr");
    let registry = paths.layout.registry_db();
    for suffix in ["", "-wal", "-shm"] {
        let mut name = registry.clone().into_os_string();
        name.push(suffix);
        first.assert_absent(
            &lossy(Path::new(&name)),
            &format!("the registry file{suffix}"),
        );
    }
    ev.pass_with(&format!(
        "{} frames relayed, {carriers} carried a value, {events} secret.changed events, every other frame, the log, and the registry bytes were clean",
        frames.len()
    ));
}

/// The stderr file `SpawnedDaemon` left in the test root.
fn daemon_stderr(root: &TestRoot) -> std::path::PathBuf {
    std::fs::read_dir(root.path())
        .expect("the test root can be read")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| path.extension().is_some_and(|ext| ext == "stderr"))
        .unwrap_or_default()
}

#[test]
fn a_locked_keychain_answers_secret_calls_without_a_dialog_and_get_info_reports_it() {
    let ev = evidence!(
        covers = ["REQ-016"],
        kind = Kind::Fixture,
        expected = "through a real stacklyd on a file keychain locked with security lock-keychain, secret.reveal and a secret.set upsert from a client that cannot show a dialog answer secret.store_locked in under a second, secret.status still reads present, and daemon.get_info reports the store locked",
    );
    if !has_gui_session(&ev) {
        return;
    }
    let root = TestRoot::new().unwrap();
    let daemon = start_daemon(&root);
    let credential = FixtureCredential::new();
    let keychain = keychain_file(&root);
    let rt = runtime();

    let outcome = rt.block_on(async {
        let (client, _) = client(daemon.socket()).await;
        ok(
            &client,
            set(REFERENCE, text(credential.value()), SecretMode::Create),
        )
        .await;
        let lock = run("security", &["lock-keychain", &keychain]);
        assert!(lock.status.success(), "security lock-keychain failed");

        let started = Instant::now();
        let revealed = err(&client, reveal(REFERENCE)).await;
        let reveal_took = started.elapsed();
        let started = Instant::now();
        let upserted = err(
            &client,
            set(REFERENCE, text(credential.value()), SecretMode::Upsert),
        )
        .await;
        let upsert_took = started.elapsed();
        let Reply::SecretStatus(status) = ok(
            &client,
            Call::SecretStatus(SecretStatusParams {
                reference: REFERENCE.to_owned(),
            }),
        )
        .await
        else {
            panic!("not a status reply");
        };
        let Reply::DaemonGetInfo(info) = ok(&client, Call::DaemonGetInfo).await else {
            panic!("not an info reply");
        };
        (revealed, reveal_took, upserted, upsert_took, status, info)
    });
    let (revealed, reveal_took, upserted, upsert_took, status, info) = outcome;

    assert_eq!(revealed.code(), "secret.store_locked");
    assert_eq!(upserted.code(), "secret.store_locked");
    assert!(reveal_took < NO_DIALOG_LIMIT, "reveal took {reveal_took:?}");
    assert!(upsert_took < NO_DIALOG_LIMIT, "set took {upsert_took:?}");
    assert_eq!(status.summary.status, SecretStatus::Present);
    assert_eq!(
        info.secret_store.expect("the store is reported").state,
        SecretStoreState::Locked
    );
    drop(rt);
    assert_eq!(daemon.stop().unwrap().code(), Some(0));
    ev.pass_with(&format!(
        "reveal secret.store_locked in {reveal_took:?}, set secret.store_locked in {upsert_took:?}, status present, get_info locked"
    ));
}

#[test]
fn an_item_that_does_not_trust_the_daemon_answers_access_denied_not_by_the_user() {
    let ev = evidence!(
        covers = ["REQ-016"],
        kind = Kind::Fixture,
        expected = "through a real stacklyd, a reference whose keychain item was remade with security add-generic-password -T /usr/bin/true answers secret.reveal with secret.store_access_denied and byUser false in under a second without a dialog, while secret.status still reads present",
    );
    if !has_gui_session(&ev) {
        return;
    }
    let root = TestRoot::new().unwrap();
    let daemon = start_daemon(&root);
    let credential = FixtureCredential::new();
    let keychain = keychain_file(&root);
    let account = SecretRef::parse(REFERENCE).expect("a valid reference");
    let rt = runtime();

    let (denied, took, status) = rt.block_on(async {
        let (client, _) = client(daemon.socket()).await;
        ok(
            &client,
            set(REFERENCE, text(credential.value()), SecretMode::Create),
        )
        .await;
        let removed = run(
            "security",
            &[
                "delete-generic-password",
                "-s",
                "stackly-secret",
                "-a",
                account.path(),
                &keychain,
            ],
        );
        assert!(
            removed.status.success(),
            "security delete-generic-password failed"
        );
        let added = run(
            "security",
            &[
                "add-generic-password",
                "-s",
                "stackly-secret",
                "-a",
                account.path(),
                "-w",
                credential.value(),
                "-T",
                "/usr/bin/true",
                &keychain,
            ],
        );
        assert!(
            added.status.success(),
            "security add-generic-password failed"
        );

        let started = Instant::now();
        let denied = err(&client, reveal(REFERENCE)).await;
        let took = started.elapsed();
        let Reply::SecretStatus(status) = ok(
            &client,
            Call::SecretStatus(SecretStatusParams {
                reference: REFERENCE.to_owned(),
            }),
        )
        .await
        else {
            panic!("not a status reply");
        };
        (denied, took, status)
    });

    assert_eq!(denied.code(), "secret.store_access_denied");
    assert_eq!(denied.detail("byUser"), Some("false"));
    assert!(took < NO_DIALOG_LIMIT, "reveal took {took:?}");
    assert_eq!(status.summary.status, SecretStatus::Present);
    credential.assert_absent(&format!("{denied:?}"), "the access denied error");
    drop(rt);
    assert_eq!(daemon.stop().unwrap().code(), Some(0));
    ev.pass_with(&format!(
        "reveal secret.store_access_denied with byUser false in {took:?}, status present"
    ));
}
