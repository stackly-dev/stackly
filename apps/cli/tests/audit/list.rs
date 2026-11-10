//! AC-16: the flags, the table, the footer, `--json`, and the exit codes.

use std::path::PathBuf;
use std::sync::Arc;

use serde_json::{Value, json};
use stackly_protocol::audit::AuditPage;
use stackly_protocol::instance::{InstancePage, InstanceSummary};
use stackly_protocol::{Call, Reply};
use stackly_testkit::TestRoot;
use stackly_testkit::evidence::{Kind, evidence};

use crate::common::scripted::{ScriptedDaemon, Step, error};
use crate::common::{envelope, golden, run, stackly};

/// The instance the fixture rows are about.
const INSTANCE: &str = "0192f3a4-0000-7000-8000-0000000000b1";
/// The operation of the failed start in the fixture.
const OPERATION: &str = "0192f3a4-0000-7000-8000-000000000002";
/// The `next` of the fixture page.
const NEXT: &str = "2026-11-09T21:00:00.000000000Z_0192f3a6-0000-7000-8000-0000000000c5";

/// The reply content of the fixture frame `name`.
fn fixture(name: &str) -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../protocols/fixtures/ipc")
        .join(name);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("fixture {} reads: {error}", path.display()));
    let frame: Value = serde_json::from_str(&text).expect("a fixture is JSON");
    frame["content"]["reply"]["content"].clone()
}

/// The page of `frame-response-audit-list.json`: five rows of every kind
/// and a `next`.
fn page() -> AuditPage {
    serde_json::from_value(fixture("frame-response-audit-list.json"))
        .expect("the audit list fixture decodes")
}

/// The running fixture instance, named `pg17-dev` with the fixture rows'
/// instance ID.
fn instance() -> InstanceSummary {
    let mut value = fixture("frame-response-instance-get.json");
    value["id"] = INSTANCE.into();
    value["name"] = "pg17-dev".into();
    serde_json::from_value(value).expect("a fixture instance decodes")
}

/// A daemon that lists `pg17-dev` and answers `audit.list` with the
/// fixture page, or an empty page when the params ask for the page after
/// it.
fn daemon(root: &TestRoot) -> ScriptedDaemon {
    let page = Arc::new(page());
    let instance = instance();
    ScriptedDaemon::serve(root, move |call, _| match call {
        Call::InstanceList(_) => vec![Step::Reply(Reply::InstanceList(Box::new(InstancePage {
            items: vec![instance.clone()],
            next: None,
        })))],
        Call::AuditList(params) if params.before.as_deref() == Some(NEXT) => {
            vec![Step::Reply(Reply::AuditList(AuditPage {
                items: Vec::new(),
                next: None,
            }))]
        }
        Call::AuditList(_) => vec![Step::Reply(Reply::AuditList((*page).clone()))],
        _ => Vec::new(),
    })
}

/// The params of every `audit.list` the daemon got, in order.
fn sent(daemon: &ScriptedDaemon) -> Vec<Value> {
    daemon
        .log()
        .into_iter()
        .filter(|logged| logged.method == "audit.list")
        .map(|logged| logged.raw["call"]["content"].clone())
        .collect()
}

#[test]
fn list_writes_the_table_with_details_and_a_footer_and_json_carries_the_page() {
    let ev = evidence!(
        covers = ["REQ-045"],
        kind = Kind::Fixture,
        expected = "audit list sends audit.list with limit 50 and no filter, writes TIME, BY, ACTION, RESULT, OPERATION, DETAILS with errorCode then configRevision then the other details, a footer hint that repeats the flags, and the AuditPage as the --json result; the page after the last one says No audit entries.",
    );
    let root = TestRoot::new().expect("a test root is made");
    let daemon = daemon(&root);

    let text = run(root.path(), &["audit", "list"]);
    let json = run(root.path(), &["audit", "list", "--json"]);
    let last = run(root.path(), &["audit", "list", "--before", NEXT]);

    assert_eq!(text.code, Some(0), "{text:?}");
    golden::check("audit_list.txt", &text.stdout);
    assert_eq!(
        text.stderr,
        format!("hint: more audit entries: stackly audit list --before {NEXT}\n")
    );
    let failed = text
        .stdout
        .lines()
        .find(|line| line.contains(OPERATION))
        .expect("the failed start has a line");
    assert!(
        failed.ends_with(
            "errorCode=supervisor.endpoint_in_use configRevision=3 message=\"port 5432 is in use\" ports=\"main 5432\" reason=step_failed stage=spawn state=failed"
        ),
        "{failed}"
    );
    assert_eq!(json.code, Some(0), "{json:?}");
    assert_eq!(json.stderr, "", "no footer with --json");
    let value = envelope(&json);
    assert_eq!(value["command"], "audit list");
    assert_eq!(
        value["result"],
        serde_json::to_value(page()).expect("the page encodes"),
        "the result is the page as the daemon sent it"
    );
    assert_eq!(last.code, Some(0), "{last:?}");
    assert_eq!(last.stdout, "No audit entries.\n");
    assert_eq!(last.stderr, "", "no footer on the last page");

    let params = sent(&daemon);
    assert_eq!(
        params[0],
        json!({ "instanceId": null, "operationId": null, "limit": 50, "before": null })
    );
    assert_eq!(params[2]["before"], NEXT);
    assert_eq!(
        daemon
            .methods()
            .iter()
            .filter(|m| *m == "instance.list")
            .count(),
        0,
        "no filter, no lookup"
    );
    ev.pass();
}

#[test]
fn a_name_is_looked_up_a_whole_uuid_goes_as_it_is_and_an_unknown_name_exits_10() {
    let ev = evidence!(
        covers = ["REQ-045"],
        kind = Kind::Fixture,
        expected = "audit list --instance pg17-dev reads instance.list and sends that instance's ID; --instance with a whole UUID of no listed instance sends it unchanged without instance.list, so the trail of a removed instance stays readable; an unknown name exits 10 with cli.instance_not_found before audit.list; --operation, --limit, and --before go as params and come back in the footer",
    );
    let root = TestRoot::new().expect("a test root is made");
    let daemon = daemon(&root);
    let removed = "0192f3a4-0000-7000-8000-0000000000ff";

    let named = run(
        root.path(),
        &[
            "audit",
            "list",
            "--instance",
            "pg17-dev",
            "--operation",
            OPERATION,
            "--limit",
            "20",
        ],
    );
    let by_id = run(
        root.path(),
        &["audit", "list", "--instance", removed, "--json"],
    );
    let unknown = run(
        root.path(),
        &["audit", "list", "--instance", "nope", "--json"],
    );
    let unknown_text = run(root.path(), &["audit", "list", "--instance", "nope"]);

    assert_eq!(named.code, Some(0), "{named:?}");
    assert_eq!(
        named.stderr,
        format!(
            "hint: more audit entries: stackly audit list --instance pg17-dev --operation {OPERATION} --limit 20 --before {NEXT}\n"
        )
    );
    assert_eq!(by_id.code, Some(0), "{by_id:?}");
    assert_eq!(unknown.code, Some(10), "{unknown:?}");
    let value = envelope(&unknown);
    assert_eq!(value["error"]["code"], "cli.instance_not_found");
    assert_eq!(value["result"], Value::Null);
    assert_eq!(unknown_text.code, Some(10));
    assert!(
        unknown_text.stderr.contains("no instance named nope"),
        "{}",
        unknown_text.stderr
    );

    let params = sent(&daemon);
    assert_eq!(params.len(), 2, "an unknown name sends no audit.list");
    assert_eq!(
        params[0],
        json!({ "instanceId": INSTANCE, "operationId": OPERATION, "limit": 20, "before": null })
    );
    assert_eq!(params[1]["instanceId"], removed);
    let methods = daemon.methods();
    assert_eq!(
        methods,
        [
            "daemon.handshake",
            "instance.list",
            "audit.list",
            "daemon.handshake",
            "audit.list",
            "daemon.handshake",
            "instance.list",
            "daemon.handshake",
            "instance.list",
        ],
        "a whole UUID needs no lookup"
    );
    ev.pass();
}

#[test]
fn flags_the_contract_refuses_exit_2_before_anything_is_sent() {
    let root = TestRoot::new().expect("a test root is made");
    let daemon = daemon(&root);
    for args in [
        &["--limit", "0"][..],
        &["--limit", "201"],
        &["--operation", "0192f3a4"],
        &["--operation", "nope"],
        &["--before", "page-2"],
        &["--instance", ""],
        &["--resource", "manager"],
    ] {
        let mut all = vec!["audit", "list", "--json"];
        all.extend_from_slice(args);
        let refused = run(root.path(), &all);
        assert_eq!(refused.code, Some(2), "{args:?}: {refused:?}");
        assert_eq!(envelope(&refused)["error"]["code"], "cli.usage", "{args:?}");
    }
    let widest = run(root.path(), &["audit", "list", "--limit", "200", "--json"]);
    assert_eq!(widest.code, Some(0), "{widest:?}");
    assert_eq!(
        daemon.methods(),
        ["daemon.handshake", "audit.list"],
        "only the valid run reached the daemon"
    );
    assert_eq!(sent(&daemon)[0]["limit"], 200);
}

#[test]
fn a_daemon_refusal_keeps_its_code_and_exits_by_its_category() {
    let root = TestRoot::new().expect("a test root is made");
    let _daemon = ScriptedDaemon::serve(&root, |call, _| match call {
        Call::AuditList(_) => vec![Step::Fail(error(
            "request.invalid_params",
            "invalid_input",
            "the params are invalid",
        ))],
        _ => Vec::new(),
    });

    let refused = run(root.path(), &["audit", "list", "--json"]);

    assert_eq!(refused.code, Some(2), "{refused:?}");
    assert_eq!(
        envelope(&refused)["error"]["code"],
        "request.invalid_params"
    );
}

#[test]
fn columns_cut_nothing_when_stdout_is_not_a_terminal() {
    let root = TestRoot::new().expect("a test root is made");
    let _daemon = daemon(&root);

    let mut narrow = stackly(root.path());
    narrow.env("COLUMNS", "40").args(["audit", "list"]);
    let narrow = crate::common::run_command(narrow);
    let mut plain = stackly(root.path());
    plain.env_remove("COLUMNS").args(["audit", "list"]);
    let plain = crate::common::run_command(plain);

    assert_eq!(narrow.code, Some(0), "{narrow:?}");
    assert_eq!(
        narrow.stdout, plain.stdout,
        "a pipe keeps every detail whole"
    );
    assert!(!narrow.stdout.contains('…'), "{}", narrow.stdout);
}
