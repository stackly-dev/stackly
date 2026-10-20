//! AC-7, AC-8, AC-15 against the real `stacklyd` binary (spec 0008 build
//! step 12): the daemon serves the operation methods, so an empty list and
//! an unknown ID come from it, not from a script. The full path through an
//! operation needs the test only starting method, which only the daemon's
//! own tests can register, so it lives in `apps/daemon/tests/cli.rs`.

use stackly_testkit::TestRoot;
use stackly_testkit::evidence::{Kind, evidence};
use uuid::Uuid;

use crate::common::stacklyd::start_daemon;
use crate::common::{envelope, listing, run};

#[test]
fn list_on_a_fresh_daemon_is_empty_with_every_filter_and_creates_nothing() {
    let ev = evidence!(
        covers = ["REQ-028", "REQ-054", "BR-011"],
        kind = Kind::Fixture,
        expected = "stackly operation list against a real stacklyd on a fresh root writes No operations. as text and items [] with next null as --json, with --active, --resource manager or instance:<uuid>, and --limit too, exits 0, and adds no file under the root",
    );
    let root = TestRoot::new().unwrap();
    let daemon = start_daemon(&root);
    let before = listing(root.path());
    let instance = format!("instance:{}", Uuid::now_v7());

    let text = run(root.path(), &["operation", "list"]);
    let filtered: Vec<_> = [
        vec!["operation", "list", "--json"],
        vec!["operation", "list", "--active", "--json"],
        vec!["operation", "list", "--resource", "manager", "--json"],
        vec!["operation", "list", "--resource", &instance, "--json"],
        vec!["operation", "list", "--limit", "1", "--json"],
        vec!["operation", "list", "--limit", "200", "--active", "--json"],
    ]
    .iter()
    .map(|args| run(root.path(), args))
    .collect();

    assert_eq!(text.code, Some(0), "{text:?}");
    assert_eq!(text.stdout, "No operations.\n");
    assert_eq!(text.stderr, "");
    for json in &filtered {
        assert_eq!(json.code, Some(0), "{json:?}");
        let value = envelope(json);
        assert_eq!(value["command"], "operation list");
        assert_eq!(
            value["result"],
            serde_json::json!({ "items": [], "next": null })
        );
        assert_eq!(value["warnings"], serde_json::json!([]));
        assert_eq!(json.stderr, "");
    }
    assert_eq!(listing(root.path()), before, "stackly created a path");
    assert_eq!(daemon.stop().unwrap().code(), Some(0));
    ev.pass();
}

#[test]
fn every_verb_on_an_unknown_id_exits_10_with_the_daemons_not_found() {
    let ev = evidence!(
        covers = ["REQ-028", "REQ-054", "AC-028"],
        kind = Kind::Fixture,
        expected = "against a real stacklyd, stackly operation inspect, inspect --wait, cancel, cancel --wait, resume, and abandon --yes of a random UUID exit 10 with resource.not_found from the daemon, unchanged in the envelope with the daemon's requestId, and the text error names the code",
    );
    let root = TestRoot::new().unwrap();
    let daemon = start_daemon(&root);
    let id = Uuid::now_v7().to_string();

    let text = run(root.path(), &["operation", "inspect", &id]);
    let runs: Vec<_> = [
        vec!["operation", "inspect", &id, "--json"],
        vec![
            "operation",
            "inspect",
            &id,
            "--wait",
            "--timeout",
            "30s",
            "--json",
        ],
        vec!["operation", "cancel", &id, "--json"],
        vec!["operation", "cancel", &id, "--wait", "--json"],
        vec!["operation", "resume", &id, "--json"],
        vec!["operation", "abandon", &id, "--yes", "--json"],
    ]
    .iter()
    .map(|args| (args[1], run(root.path(), args)))
    .collect();

    assert_eq!(text.code, Some(10), "{text:?}");
    assert_eq!(text.stdout, "");
    assert_eq!(
        text.stderr,
        "error: the operation does not exist\n  code: resource.not_found\n"
    );
    let mut request_ids = Vec::new();
    for (verb, json) in &runs {
        assert_eq!(json.code, Some(10), "{verb}: {json:?}");
        assert!(
            json.elapsed < std::time::Duration::from_secs(10),
            "{verb} did not wait for its timeout: {json:?}"
        );
        let value = envelope(json);
        assert_eq!(value["command"], format!("operation {verb}"));
        assert_eq!(value["result"], serde_json::Value::Null, "{verb}");
        let error = &value["error"];
        assert_eq!(error["code"], "resource.not_found", "{verb}");
        assert_eq!(error["category"], "not_found", "{verb}");
        assert_eq!(error["resource"]["kind"], "operation", "{verb}");
        assert_eq!(error["resource"]["id"], id.as_str(), "{verb}");
        let request_id = value["requestId"].as_str().expect("the daemon's request");
        request_ids.push((*verb, request_id.to_owned()));
    }
    let log = daemon.stderr_path().to_path_buf();
    assert_eq!(daemon.stop().unwrap().code(), Some(0));
    let log = std::fs::read_to_string(log).unwrap();
    for (verb, request_id) in request_ids {
        assert!(
            log.contains(&request_id),
            "{verb}: the daemon logged no request {request_id}"
        );
    }
    ev.pass();
}
