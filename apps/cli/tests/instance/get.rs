//! AC-7: `instance get` writes the `kv` block, the endpoint table, and
//! `Failure output:` of spec 0022, and `{ instance }` with `--json`.

use stackly_protocol::{Call, Reply};
use stackly_testkit::TestRoot;
use stackly_testkit::evidence::{Kind, evidence};

use crate::common::scripted::{ScriptedDaemon, Step, error};
use crate::common::{envelope, golden, run};
use crate::data;

const ID: &str = "0192f3a5-0000-7000-8000-000000000001";

fn serving(instance: stackly_protocol::instance::InstanceSummary) -> (TestRoot, ScriptedDaemon) {
    let root = TestRoot::new().expect("a test root is made");
    let daemon = ScriptedDaemon::serve(&root, move |call, _| match call {
        Call::InstanceGet(_) => vec![Step::Reply(Reply::InstanceGet(Box::new(instance.clone())))],
        _ => Vec::new(),
    });
    (root, daemon)
}

#[test]
fn get_writes_the_block_the_endpoints_and_the_json_result() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "instance get calls instance.get once and writes the kv block with the endpoint table in text and { instance } with --json",
    );
    let (root, daemon) = serving(data::instance(""));

    let text = run(root.path(), &["instance", "get", ID]);
    let json = run(root.path(), &["instance", "get", ID, "--json"]);

    assert_eq!(text.code, Some(0), "{text:?}");
    assert_eq!(text.stderr, "");
    golden::check("instance_get.txt", &text.stdout);
    assert_eq!(json.code, Some(0), "{json:?}");
    golden::check("instance_get.json", &json.stdout);
    let value = envelope(&json);
    assert_eq!(value["command"], "instance get");
    assert_eq!(value["result"]["instance"]["name"], "orders-db");
    assert_eq!(
        value["result"]
            .as_object()
            .unwrap()
            .keys()
            .collect::<Vec<_>>(),
        ["instance"]
    );
    assert_eq!(
        daemon.methods(),
        [
            "daemon.handshake",
            "instance.get",
            "daemon.handshake",
            "instance.get"
        ]
    );
    assert_eq!(daemon.log()[1].raw["call"]["content"]["instanceId"], ID);
    ev.pass();
}

#[test]
fn a_failed_instance_shows_its_last_failure_and_the_failure_output() {
    let (root, _daemon) = serving(data::instance("failed"));

    let text = run(root.path(), &["instance", "get", ID]);

    assert_eq!(text.code, Some(0), "{text:?}");
    golden::check("instance_get_failed.txt", &text.stdout);
}

#[test]
fn an_adopted_instance_and_a_locked_one_show_their_process_and_blocker() {
    let (root, _daemon) = serving(data::adopted());
    let adopted = run(root.path(), &["instance", "get", ID]);
    drop(_daemon);
    let (root2, _daemon2) = serving(data::locked());
    let locked = run(root2.path(), &["instance", "get", ID]);

    golden::check("instance_get_adopted.txt", &adopted.stdout);
    golden::check("instance_get_locked.txt", &locked.stdout);
    drop(root);
}

#[test]
fn text_from_the_daemon_never_reaches_the_terminal_raw() {
    let instance = data::edited(|value| {
        value["name"] = "evil\u{1b}[31m\nname".into();
        value["lastFailure"] = serde_json::json!({
            "code": "crash_loop",
            "at": "2026-10-20T09:03:00.000Z",
            "exitCode": 1,
            "signal": null,
            "excerpt": ["line\u{1b}]0;title\u{7}"]
        });
    });
    let (root, _daemon) = serving(instance);

    let text = run(root.path(), &["instance", "get", ID]);

    assert_eq!(text.code, Some(0), "{text:?}");
    assert!(!text.stdout.contains('\u{1b}'), "{}", text.stdout);
    assert!(
        text.stdout.contains("evil\\u{1b}[31m\\u{a}name"),
        "{}",
        text.stdout
    );
    assert!(
        text.stdout.contains("  line\\u{1b}]0;title\\u{7}\n"),
        "{}",
        text.stdout
    );
}

#[test]
fn an_unknown_instance_exits_10_and_a_bad_id_exits_2() {
    let root = TestRoot::new().expect("a test root is made");
    let daemon = ScriptedDaemon::serve(&root, |call, _| match call {
        Call::InstanceGet(_) => vec![Step::Fail(error(
            "resource.not_found",
            "not_found",
            "no instance has this id",
        ))],
        _ => Vec::new(),
    });

    let missing = run(root.path(), &["instance", "get", ID, "--json"]);
    let bad = run(root.path(), &["instance", "get", "orders-db", "--json"]);

    assert_eq!(missing.code, Some(10), "{missing:?}");
    assert_eq!(envelope(&missing)["error"]["code"], "resource.not_found");
    assert_eq!(bad.code, Some(2), "{bad:?}");
    assert_eq!(envelope(&bad)["error"]["code"], "cli.usage");
    assert_eq!(daemon.methods(), ["daemon.handshake", "instance.get"]);
}
