//! `stackly settings get` and `set` (spec 0032 AC-16, AC-17).

use serde_json::json;
use stackly_protocol::{Call, ErrorBody, Reply};
use stackly_testkit::TestRoot;

use crate::common::scripted::{ScriptedDaemon, Step};
use crate::common::{envelope, run};
use crate::view;

fn revision_mismatch() -> ErrorBody {
    serde_json::from_value(json!({
        "code": "resource.revision_mismatch",
        "category": "conflict",
        "message": "the settings changed",
        "retryable": false,
        "resource": null,
        "fields": [],
        "hint": null,
        "details": [
            { "key": "expected", "value": "3" },
            { "key": "actual", "value": "4" }
        ],
        "operationId": null,
        "rule": null,
        "items": []
    }))
    .expect("an error body")
}

#[test]
fn get_prints_one_line_per_setting_and_the_json_carries_the_view() {
    // covers: AC-16
    let root = TestRoot::new().expect("a test root is made");
    let _daemon = ScriptedDaemon::serve(&root, |call, _| match call {
        Call::SettingsGet => vec![Step::Reply(Reply::SettingsGet(Box::new(view(
            3, "at_login", "beta", true,
        ))))],
        _ => Vec::new(),
    });

    let text = run(root.path(), &["settings", "get"]);
    let json = run(root.path(), &["settings", "get", "--json"]);

    assert_eq!(text.code, Some(0), "{text:?}");
    for line in [
        "default-autostart  on",
        "update-channel     beta",
        "telemetry          on",
        "data location      /Users/dev/Library/Application Support/Stackly (built in)",
        "log retention      5 files × 20 MiB per stream",
        "audit retention    30 days, up to 100000 entries",
        "revision           3",
    ] {
        assert!(text.stdout.contains(line), "{line:?} in {}", text.stdout);
    }
    assert_eq!(json.code, Some(0), "{json:?}");
    let value = envelope(&json);
    assert_eq!(value["command"], "settings get");
    assert_eq!(value["result"]["revision"], 3);
    assert_eq!(value["result"]["defaultStartup"], "at_login");
    assert_eq!(value["result"]["locations"]["data"]["builtIn"], true);
    assert_eq!(
        value["result"]["auditRetention"],
        json!({ "days": 30, "maxRows": 100_000 })
    );
}

#[test]
fn an_older_daemon_without_the_audit_retention_gets_no_line_for_it() {
    // covers: AC-6 (spec 0035)
    let root = TestRoot::new().expect("a test root is made");
    let mut older = serde_json::to_value(view(3, "manual", "stable", false)).expect("a view");
    older
        .as_object_mut()
        .expect("the view is an object")
        .remove("auditRetention");
    let older: stackly_protocol::settings::DaemonSettingsView =
        serde_json::from_value(older).expect("a view without auditRetention decodes");
    let _daemon = ScriptedDaemon::serve(&root, move |call, _| match call {
        Call::SettingsGet => vec![Step::Reply(Reply::SettingsGet(Box::new(older.clone())))],
        _ => Vec::new(),
    });

    let text = run(root.path(), &["settings", "get"]);

    assert_eq!(text.code, Some(0), "{text:?}");
    assert!(!text.stdout.contains("audit retention"), "{}", text.stdout);
    assert!(
        text.stdout.contains("revision           3"),
        "{}",
        text.stdout
    );
}

#[test]
fn audit_retention_days_sends_7_30_or_90_and_refuses_any_other_value_before_sending() {
    // covers: AC-6 (spec 0035)
    let root = TestRoot::new().expect("a test root is made");
    let daemon = ScriptedDaemon::serve(&root, |call, _| match call {
        Call::SettingsGet => vec![Step::Reply(Reply::SettingsGet(Box::new(view(
            3, "manual", "stable", false,
        ))))],
        Call::SettingsUpdate(_) => vec![Step::Reply(Reply::SettingsUpdate(Box::new(view(
            4, "manual", "stable", false,
        ))))],
        _ => Vec::new(),
    });

    let accepted: Vec<_> = ["7", "30", "90"]
        .iter()
        .map(|days| {
            run(
                root.path(),
                &["settings", "set", "audit-retention-days", days],
            )
        })
        .collect();
    let refused: Vec<_> = ["14", "0", "030", "thirty", "-7"]
        .iter()
        .map(|days| {
            run(
                root.path(),
                &["settings", "set", "audit-retention-days", days, "--json"],
            )
        })
        .collect();

    for run in &accepted {
        assert_eq!(run.code, Some(0), "{run:?}");
    }
    for run in &refused {
        assert_eq!(run.code, Some(2), "{run:?}");
        assert_eq!(envelope(run)["error"]["code"], "cli.usage", "{run:?}");
    }
    let patches: Vec<_> = daemon
        .log()
        .into_iter()
        .filter(|logged| logged.method == "settings.update")
        .map(|logged| logged.raw["call"]["content"].clone())
        .collect();
    assert_eq!(patches.len(), 3, "a refused value sends nothing");
    for (patch, days) in patches.iter().zip([7, 30, 90]) {
        assert_eq!(patch["expectedRevision"], 3);
        assert_eq!(patch["patch"]["auditRetentionDays"], days);
        assert_eq!(patch["patch"]["telemetry"], serde_json::Value::Null);
    }
}

#[test]
fn set_reads_the_revision_and_sends_the_patch_with_a_key() {
    // covers: AC-16
    let root = TestRoot::new().expect("a test root is made");
    let daemon = ScriptedDaemon::serve(&root, |call, _| match call {
        Call::SettingsGet => vec![Step::Reply(Reply::SettingsGet(Box::new(view(
            3, "manual", "stable", false,
        ))))],
        Call::SettingsUpdate(_) => vec![Step::Reply(Reply::SettingsUpdate(Box::new(view(
            4, "at_login", "stable", false,
        ))))],
        _ => Vec::new(),
    });

    let on = run(root.path(), &["settings", "set", "default-autostart", "on"]);
    let beta = run(
        root.path(),
        &["settings", "set", "update-channel", "beta", "--json"],
    );
    let telemetry = run(root.path(), &["settings", "set", "telemetry", "off"]);

    for run in [&on, &beta, &telemetry] {
        assert_eq!(run.code, Some(0), "{run:?}");
    }
    assert!(on.stdout.contains("default-autostart  on"), "{}", on.stdout);
    assert_eq!(envelope(&beta)["command"], "settings set");
    let sent: Vec<_> = daemon
        .log()
        .into_iter()
        .filter(|logged| logged.method == "settings.update")
        .collect();
    assert_eq!(sent.len(), 3);
    let content = |n: usize| sent[n].raw["call"]["content"].clone();
    assert_eq!(content(0)["expectedRevision"], 3);
    assert_eq!(content(0)["patch"]["defaultStartup"], "at_login");
    assert_eq!(content(1)["patch"]["updateChannel"], "beta");
    assert_eq!(content(2)["patch"]["telemetry"], false);
    assert!(sent.iter().all(|logged| logged.idempotency_key.is_some()));
}

#[test]
fn a_value_that_does_not_fit_the_key_is_a_usage_error_and_sends_nothing() {
    // covers: AC-16
    let root = TestRoot::new().expect("a test root is made");
    let daemon = ScriptedDaemon::serve(&root, |_, _| Vec::new());

    let bad_switch = run(root.path(), &["settings", "set", "telemetry", "maybe"]);
    let bad_channel = run(
        root.path(),
        &["settings", "set", "update-channel", "nightly"],
    );
    let bad_key = run(root.path(), &["settings", "set", "colour", "blue"]);

    for run in [&bad_switch, &bad_channel, &bad_key] {
        assert_eq!(run.code, Some(2), "{run:?}");
    }
    assert!(
        daemon
            .log()
            .iter()
            .all(|logged| logged.method != "settings.update"),
        "nothing was sent"
    );
}

#[test]
fn a_stale_revision_exits_3_and_says_to_run_the_command_again_without_trying_again() {
    // covers: AC-17
    let root = TestRoot::new().expect("a test root is made");
    let daemon = ScriptedDaemon::serve(&root, |call, _| match call {
        Call::SettingsGet => vec![Step::Reply(Reply::SettingsGet(Box::new(view(
            3, "manual", "stable", false,
        ))))],
        Call::SettingsUpdate(_) => vec![Step::Fail(revision_mismatch())],
        _ => Vec::new(),
    });

    let run = run(
        root.path(),
        &["settings", "set", "telemetry", "on", "--json"],
    );

    assert_eq!(run.code, Some(3), "{run:?}");
    let value = envelope(&run);
    assert_eq!(value["error"]["code"], "resource.revision_mismatch");
    assert_eq!(
        value["error"]["message"],
        "Settings changed while saving. Run the command again."
    );
    let updates = daemon
        .log()
        .into_iter()
        .filter(|logged| logged.method == "settings.update")
        .count();
    assert_eq!(updates, 1, "the command does not try again");
}
