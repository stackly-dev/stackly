//! AC-9 of spec 0032: `instance stop --all` reads the preview, prints it,
//! asks (or takes `--yes`), and sends `instance.stop_all` with the
//! `expectedInstanceIds` of that preview. A changed set exits 3, a missing
//! confirmation exits 2 before anything is sent, and `--json` writes one
//! envelope with `{ preview, outcome }`.

use serde_json::json;
use stackly_protocol::stop_all::{StopAllPreview, StopAllResult};
use stackly_protocol::{Call, Reply};
use stackly_testkit::TestRoot;
use stackly_testkit::evidence::{Kind, evidence};

use crate::common::pty::{PtyOptions, run_in_pty};
use crate::common::scripted::{ScriptedDaemon, Step, error};
use crate::common::{envelope, run};

const ALPHA: &str = "0192f3a5-0000-7000-8000-000000000001";
const BRAVO: &str = "0192f3a5-0000-7000-8000-000000000002";

fn item(id: &str, name: &str, demand: &[&str]) -> serde_json::Value {
    json!({
        "instanceId": id,
        "name": name,
        "lifecycle": "running",
        "demand": demand,
        "autostart": { "policy": "manual", "session": "none" },
    })
}

fn preview(items: Vec<serde_json::Value>, at_login: u64) -> StopAllPreview {
    serde_json::from_value(json!({
        "items": items,
        "alreadyStopping": [],
        "skipped": [],
        "atLoginCount": at_login,
    }))
    .expect("a preview decodes")
}

fn two() -> StopAllPreview {
    preview(
        vec![
            item(ALPHA, "alpha", &["manual"]),
            item(BRAVO, "bravo", &["manual", "startup"]),
        ],
        1,
    )
}

fn result(value: serde_json::Value) -> StopAllResult {
    serde_json::from_value(value).expect("a result decodes")
}

fn stopped_both() -> StopAllResult {
    result(json!({ "stopped": [ALPHA, BRAVO], "failed": [], "skipped": [] }))
}

/// A daemon that shows `shown` and answers the stop with `steps`.
fn daemon(
    root: &TestRoot,
    shown: StopAllPreview,
    steps: impl Fn() -> Vec<Step> + Send + Sync + 'static,
) -> ScriptedDaemon {
    ScriptedDaemon::serve(root, move |call, _| match call {
        Call::InstancePreviewStopAll(_) => {
            vec![Step::Reply(Reply::InstancePreviewStopAll(Box::new(
                shown.clone(),
            )))]
        }
        Call::InstanceStopAll(_) => steps(),
        _ => Vec::new(),
    })
}

fn reply(result: StopAllResult) -> Vec<Step> {
    vec![Step::Reply(Reply::InstanceStopAll(Box::new(result)))]
}

fn sent_stop(daemon: &ScriptedDaemon) -> Vec<serde_json::Value> {
    daemon
        .log()
        .into_iter()
        .filter(|logged| logged.method == "instance.stop_all")
        .map(|logged| logged.raw)
        .collect()
}

/// Spec 0032 AC-9.
#[test]
fn yes_sends_the_ids_of_the_preview_and_prints_the_result() {
    let ev = evidence!(
        covers = ["REQ-027"],
        kind = Kind::Fixture,
        expected = "stop --all --yes reads instance.preview_stop_all, then sends instance.stop_all with expectedInstanceIds equal to the preview items and a key, and writes Stopped 2 services. Data is kept.",
    );
    let root = TestRoot::new().expect("a test root is made");
    let daemon = daemon(&root, two(), || reply(stopped_both()));

    let stopped = run(root.path(), &["instance", "stop", "--all", "--yes"]);

    assert_eq!(stopped.code, Some(0), "{stopped:?}");
    assert_eq!(stopped.stdout, "Stopped 2 services. Data is kept.\n");
    let methods: Vec<String> = daemon
        .methods()
        .into_iter()
        .filter(|method| method.starts_with("instance."))
        .collect();
    assert_eq!(methods, ["instance.preview_stop_all", "instance.stop_all"]);
    let sent = &sent_stop(&daemon)[0];
    assert_eq!(
        sent["call"]["content"]["expectedInstanceIds"],
        json!([ALPHA, BRAVO])
    );
    let stop = daemon
        .log()
        .into_iter()
        .find(|logged| logged.method == "instance.stop_all")
        .expect("the stop was sent");
    assert!(stop.idempotency_key.is_some(), "the stop carries a key");
    ev.pass();
}

/// Spec 0032 AC-9.
#[test]
fn json_writes_one_envelope_with_the_preview_and_the_outcome() {
    let root = TestRoot::new().expect("a test root is made");
    let daemon = daemon(&root, two(), || reply(stopped_both()));

    let stopped = run(
        root.path(),
        &[
            "instance",
            "stop",
            "--all",
            "--yes",
            "--idempotency-key",
            "stop-all-0001",
            "--json",
        ],
    );

    assert_eq!(stopped.code, Some(0), "{stopped:?}");
    let value = envelope(&stopped);
    assert_eq!(value["result"]["preview"]["items"][0]["name"], "alpha");
    assert_eq!(value["result"]["preview"]["atLoginCount"], 1);
    assert_eq!(value["result"]["outcome"]["stopped"], json!([ALPHA, BRAVO]));
    let stop = daemon
        .log()
        .into_iter()
        .find(|logged| logged.method == "instance.stop_all")
        .expect("the stop was sent");
    assert_eq!(stop.idempotency_key.as_deref(), Some("stop-all-0001"));
}

/// Spec 0032 AC-9.
#[test]
fn nothing_running_says_so_and_sends_no_stop() {
    let root = TestRoot::new().expect("a test root is made");
    let daemon = daemon(&root, preview(Vec::new(), 0), Vec::new);

    let text = run(root.path(), &["instance", "stop", "--all"]);
    let json = run(root.path(), &["instance", "stop", "--all", "--json"]);

    assert_eq!(text.code, Some(0), "{text:?}");
    assert_eq!(text.stdout, "Nothing is running.\n");
    assert_eq!(json.code, Some(0), "{json:?}");
    assert!(envelope(&json)["result"]["outcome"].is_null());
    assert!(sent_stop(&daemon).is_empty());
}

/// Spec 0032 AC-9.
#[test]
fn without_yes_and_without_a_terminal_it_exits_2_and_sends_no_stop() {
    let root = TestRoot::new().expect("a test root is made");
    let daemon = daemon(&root, two(), || reply(stopped_both()));

    let text = run(root.path(), &["instance", "stop", "--all"]);
    let json = run(root.path(), &["instance", "stop", "--all", "--json"]);

    for refused in [&text, &json] {
        assert_eq!(refused.code, Some(2), "{refused:?}");
    }
    assert!(text.stderr.contains("alpha · Started by you"), "{text:?}");
    assert!(
        text.stderr.contains("cli.confirmation_required"),
        "{text:?}"
    );
    let value = envelope(&json);
    assert_eq!(value["error"]["code"], "cli.confirmation_required");
    assert_eq!(value["result"]["preview"]["items"][1]["name"], "bravo");
    assert!(value["result"]["outcome"].is_null());
    assert!(sent_stop(&daemon).is_empty());
}

/// Spec 0032 AC-9: the answer at a terminal.
#[test]
fn at_a_terminal_y_confirms_and_enter_declines() {
    let root = TestRoot::new().expect("a test root is made");
    let daemon = daemon(&root, two(), || reply(stopped_both()));
    let tty = |input| PtyOptions {
        stdin_tty: true,
        input: Some(input),
        ..PtyOptions::default()
    };

    let declined = run_in_pty(root.path(), &["instance", "stop", "--all"], &tty("\n"));
    assert_eq!(declined.code, Some(9), "{declined:?}");
    assert!(
        declined.stderr.contains("Stop All Services?"),
        "{declined:?}"
    );
    assert!(
        declined
            .stderr
            .contains("Data is kept. 1 of them starts again at your next login."),
        "{declined:?}"
    );
    assert!(declined.stderr.contains("cli.confirmation_declined"));
    assert!(sent_stop(&daemon).is_empty());

    let confirmed = run_in_pty(root.path(), &["instance", "stop", "--all"], &tty("y\n"));
    assert_eq!(confirmed.code, Some(0), "{confirmed:?}");
    assert_eq!(confirmed.stdout, "Stopped 2 services. Data is kept.\n");
    assert_eq!(sent_stop(&daemon).len(), 1);
}

/// Spec 0032 AC-8, AC-9.
#[test]
fn a_set_that_changed_exits_3_and_shows_what_was_confirmed() {
    let root = TestRoot::new().expect("a test root is made");
    let daemon = daemon(&root, two(), || {
        vec![Step::Fail(error(
            "instance.stop_all_changed",
            "conflict",
            "the instances to stop changed since the preview",
        ))]
    });

    let changed = run(
        root.path(),
        &["instance", "stop", "--all", "--yes", "--json"],
    );

    assert_eq!(changed.code, Some(3), "{changed:?}");
    let value = envelope(&changed);
    assert_eq!(value["error"]["code"], "instance.stop_all_changed");
    assert_eq!(value["result"]["preview"]["items"][0]["name"], "alpha");
    assert!(value["result"]["outcome"].is_null());
    assert_eq!(sent_stop(&daemon).len(), 1, "no second stop is sent");
}

/// Spec 0032 AC-7, AC-9.
#[test]
fn an_instance_that_did_not_stop_is_named_and_fails_the_command() {
    let root = TestRoot::new().expect("a test root is made");
    let _daemon = daemon(&root, two(), || {
        reply(result(json!({
            "stopped": [ALPHA],
            "failed": [{ "instanceId": BRAVO, "code": "supervisor.stop_timeout" }],
            "skipped": [],
        })))
    });

    let partial = run(root.path(), &["instance", "stop", "--all", "--yes"]);
    let json = run(
        root.path(),
        &["instance", "stop", "--all", "--yes", "--json"],
    );

    assert_ne!(partial.code, Some(0), "{partial:?}");
    assert_eq!(partial.stdout, "Stopped 1 of 2. bravo didn’t stop.\n");
    let value = envelope(&json);
    assert_eq!(value["error"]["code"], "supervisor.stop_timeout");
    assert_eq!(value["result"]["outcome"]["failed"][0]["instanceId"], BRAVO);
}

/// Spec 0032 AC-9: the flag rules.
#[test]
fn all_cannot_be_mixed_with_an_instance_or_force_or_wait_and_yes_needs_a_reason() {
    let root = TestRoot::new().expect("a test root is made");
    let daemon = daemon(&root, two(), || reply(stopped_both()));

    for args in [
        &["instance", "stop", "--all", "orders-db"][..],
        &["instance", "stop", "--all", "--force"],
        &["instance", "stop", "--all", "--wait"],
        &["instance", "stop", "--yes"],
        &["instance", "stop"],
    ] {
        let mut all = args.to_vec();
        all.push("--json");
        let refused = run(root.path(), &all);
        assert_eq!(refused.code, Some(2), "{args:?}: {refused:?}");
    }
    assert_eq!(daemon.connections(), 0);
}
