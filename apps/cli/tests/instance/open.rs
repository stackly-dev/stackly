//! `stackly instance open <instance> --list` against a scripted daemon
//! (spec 0070 AC-12, AC-14): one line per action in the text output, the
//! `openActions` of the detail under `--json`, `--list` required, a
//! provider without actions, and no `secret.reveal` or other call that
//! opens something.

use stackly_protocol::instance::{InstancePage, InstanceSummary};
use stackly_protocol::secret::{RevealSecretReply, SecretText};
use stackly_protocol::{Call, Reply};
use stackly_testkit::TestRoot;
use stackly_testkit::evidence::{FixtureCredential, Kind, evidence};

use crate::common::scripted::{ScriptedDaemon, Step};
use crate::common::{golden, run};
use crate::data::{self, Triple};

/// A daemon that knows the instance of `triple`.
fn serving(triple: &Triple) -> (TestRoot, ScriptedDaemon) {
    let root = TestRoot::new().expect("a test root is made");
    let instance: InstanceSummary = triple.instance.clone();
    let detail = triple.detail.clone();
    let daemon = ScriptedDaemon::serve(&root, move |call, _| match call {
        Call::InstanceList(_) => vec![Step::Reply(Reply::InstanceList(Box::new(InstancePage {
            items: vec![instance.clone()],
            next: None,
        })))],
        Call::InstanceGet(_) => vec![Step::Reply(Reply::InstanceGet(Box::new(instance.clone())))],
        Call::InstanceGetDetail(_) => {
            vec![Step::Reply(Reply::InstanceGetDetail(Box::new(
                detail.clone(),
            )))]
        }
        _ => Vec::new(),
    });
    (root, daemon)
}

/// The methods the CLI called after its handshake.
fn calls(daemon: &ScriptedDaemon) -> Vec<String> {
    daemon
        .methods()
        .into_iter()
        .filter(|method| method != "daemon.handshake")
        .collect()
}

// covers: AC-12
#[test]
fn list_writes_one_line_per_action_and_only_reads_the_detail() {
    let triple = data::triple("postgresql");
    let (root, daemon) = serving(&triple);
    let name = triple.instance.name.clone();

    let text = run(root.path(), &["instance", "open", &name, "--list"]);
    let by_id = run(
        root.path(),
        &[
            "instance",
            "open",
            &triple.instance.id.to_string(),
            "--list",
        ],
    );

    assert_eq!(text.code, Some(0), "{text:?}");
    assert_eq!(text.stderr, "");
    golden::check("instance_open_list.txt", &text.stdout);
    assert_eq!(by_id.stdout, text.stdout);
    assert_eq!(
        calls(&daemon),
        [
            "instance.list",
            "instance.get_detail",
            "instance.get_detail",
            "instance.get"
        ]
    );
}

// covers: AC-12, AC-14
#[test]
fn json_carries_the_instance_and_its_open_actions_without_a_secret_value() {
    let triple = data::triple("postgresql");
    let (root, daemon) = serving(&triple);

    let json = run(
        root.path(),
        &["instance", "open", "orders-db", "--list", "--json"],
    );

    assert_eq!(json.code, Some(0), "{json:?}");
    golden::check("instance_open_list.json", &json.stdout);
    let value: serde_json::Value =
        serde_json::from_str(json.stdout.trim()).expect("one JSON envelope");
    assert_eq!(value["result"]["instance"]["name"], "orders-db");
    assert_eq!(
        value["result"]["openActions"],
        serde_json::to_value(&triple.detail.open_actions).expect("encodes")
    );
    assert!(!json.stdout.contains("fixture-secret"));
    assert!(
        !calls(&daemon)
            .iter()
            .any(|method| method == "secret.reveal")
    );
}

// covers: AC-12
#[test]
fn unavailable_actions_show_a_dash_and_the_reason() {
    let triple = data::triple("nats");
    let (root, _daemon) = serving(&triple);
    let name = triple.instance.name.clone();

    let text = run(root.path(), &["instance", "open", &name, "--list"]);

    assert_eq!(text.code, Some(0), "{text:?}");
    assert_eq!(
        text.stdout,
        "monitoring  Open Monitoring in Browser  —  Start the instance to assign its port.\n"
    );
}

// covers: AC-12
#[test]
fn a_provider_without_actions_prints_no_open_actions_and_exits_0() {
    let triple = data::triple("package-unreadable");
    let (root, _daemon) = serving(&triple);
    let name = triple.instance.name.clone();

    let text = run(root.path(), &["instance", "open", &name, "--list"]);

    assert_eq!(text.code, Some(0), "{text:?}");
    assert_eq!(text.stdout, "No open actions.\n");
}

// covers: AC-12
#[test]
fn list_is_required() {
    let triple = data::triple("postgresql");
    let (root, daemon) = serving(&triple);

    let bare = run(root.path(), &["instance", "open", "orders-db"]);

    assert_eq!(bare.code, Some(2), "{bare:?}");
    assert!(bare.stderr.contains("--list"), "{}", bare.stderr);
    assert!(calls(&daemon).is_empty());
}

// covers: AC-14
#[test]
fn a_daemon_that_would_reveal_the_password_never_gets_asked_and_no_output_holds_it() {
    let ev = evidence!(
        covers = ["AC-016", "REQ-016"],
        kind = Kind::Fixture,
        expected = "the default and --json output of stackly instance open --list hold no prefix of the fixture credential, and the CLI never calls secret.reveal even when the daemon would answer it",
    );
    let credential = FixtureCredential::new();
    let triple = data::triple("postgresql");
    let instance: InstanceSummary = triple.instance.clone();
    let detail = triple.detail.clone();
    let value = credential.value().to_owned();
    let root = TestRoot::new().expect("a test root is made");
    let daemon = ScriptedDaemon::serve(&root, move |call, _| match call {
        Call::InstanceList(_) => vec![Step::Reply(Reply::InstanceList(Box::new(InstancePage {
            items: vec![instance.clone()],
            next: None,
        })))],
        Call::InstanceGet(_) => vec![Step::Reply(Reply::InstanceGet(Box::new(instance.clone())))],
        Call::InstanceGetDetail(_) => {
            vec![Step::Reply(Reply::InstanceGetDetail(Box::new(
                detail.clone(),
            )))]
        }
        Call::SecretReveal(params) => vec![Step::Reply(Reply::SecretReveal(RevealSecretReply {
            reference: params.reference.clone(),
            value: SecretText::new(value.clone()),
        }))],
        _ => Vec::new(),
    });

    let text = run(root.path(), &["instance", "open", "orders-db", "--list"]);
    let json = run(
        root.path(),
        &["instance", "open", "orders-db", "--list", "--json"],
    );

    for (what, output) in [("the text output", &text), ("the JSON output", &json)] {
        assert_eq!(output.code, Some(0), "{output:?}");
        credential.assert_absent(&output.stdout, what);
        credential.assert_absent(&output.stderr, what);
    }
    assert!(
        !calls(&daemon)
            .iter()
            .any(|method| method == "secret.reveal" || method == "instance.prepare_terminal"),
        "{:?}",
        calls(&daemon)
    );
    ev.pass();
}
