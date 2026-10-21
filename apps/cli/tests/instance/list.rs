//! AC-10: `instance list` reads every page of `instance.list`, filters with
//! the groups of AC-1, and writes the table, the count line, and
//! `{ items, counts }` of spec 0022.

use stackly_protocol::instance::{InstancePage, InstanceSummary};
use stackly_protocol::{Call, Reply};
use stackly_testkit::TestRoot;
use stackly_testkit::evidence::{Kind, evidence};

use crate::common::scripted::{ScriptedDaemon, Step, error};
use crate::common::{envelope, golden, run};
use crate::data;

/// A daemon that lists `items` in pages of `per_page`, with the cursor
/// `<name>_<id>` of the last item of a page.
fn listing(items: Vec<InstanceSummary>, per_page: usize) -> (TestRoot, ScriptedDaemon) {
    let root = TestRoot::new().expect("a test root is made");
    let daemon = ScriptedDaemon::serve(&root, move |call, _| match call {
        Call::InstanceList(params) => {
            let start = params.before.as_deref().map_or(0, |cursor| {
                items
                    .iter()
                    .position(|item| format!("{}_{}", item.name, item.id) == cursor)
                    .expect("the cursor names an item")
                    + 1
            });
            let page: Vec<InstanceSummary> =
                items.iter().skip(start).take(per_page).cloned().collect();
            let next = (start + page.len() < items.len())
                .then(|| page.last().map(|item| format!("{}_{}", item.name, item.id)))
                .flatten();
            vec![Step::Reply(Reply::InstanceList(Box::new(InstancePage {
                items: page,
                next,
            })))]
        }
        _ => Vec::new(),
    });
    (root, daemon)
}

fn names(value: &serde_json::Value) -> Vec<&str> {
    value["items"]
        .as_array()
        .expect("items is an array")
        .iter()
        .map(|item| item["name"].as_str().expect("a name"))
        .collect()
}

/// Spec 0022 AC-10.
#[test]
fn list_writes_the_table_the_count_line_and_the_json_result() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "instance list over list-mixed.json writes the NAME to OWNERSHIP table with the count line last and { items, counts } with --json, counting instances by group",
    );
    let (root, daemon) = listing(data::list_mixed().items, 200);

    let text = run(root.path(), &["instance", "list"]);
    let json = run(root.path(), &["instance", "list", "--json"]);

    assert_eq!(text.code, Some(0), "{text:?}");
    assert_eq!(text.stderr, "");
    golden::check("instance_list.txt", &text.stdout);
    assert_eq!(
        text.stdout.lines().last(),
        Some("20 instances: 4 running, 4 stopped, 3 failed, 8 not ready, 1 unknown")
    );
    assert_eq!(json.code, Some(0), "{json:?}");
    golden::check("instance_list.json", &json.stdout);
    let value = envelope(&json);
    assert_eq!(value["command"], "instance list");
    let result = &value["result"];
    assert_eq!(
        result.as_object().unwrap().keys().collect::<Vec<_>>(),
        ["counts", "items"]
    );
    assert_eq!(
        result["counts"],
        serde_json::json!({
            "total": 20, "ready": 4, "notReady": 8, "stopped": 4, "failed": 3, "unknown": 1
        })
    );
    assert_eq!(result["items"].as_array().unwrap().len(), 20);
    assert_eq!(
        daemon.methods(),
        [
            "daemon.handshake",
            "instance.list",
            "daemon.handshake",
            "instance.list"
        ]
    );
    assert_eq!(daemon.log()[1].raw["call"]["content"]["limit"], 200);
    ev.pass();
}

/// Spec 0022 AC-10 and AC-1.
#[test]
fn status_running_keeps_ready_and_not_ready_and_the_counts_cover_every_instance() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "instance list --status running keeps the ready and notReady groups, stopped keeps stopped, failed keeps failed, and counts in the JSON always cover all instances",
    );
    let (root, _daemon) = listing(data::list_mixed().items, 200);

    let running = envelope(&run(
        root.path(),
        &["instance", "list", "--status", "running", "--json"],
    ));
    let stopped = envelope(&run(
        root.path(),
        &["instance", "list", "--status", "stopped", "--json"],
    ));
    let failed = envelope(&run(
        root.path(),
        &["instance", "list", "--status", "failed", "--json"],
    ));
    let text = run(root.path(), &["instance", "list", "--status", "failed"]);

    let mixed = envelope(&run(root.path(), &["instance", "list", "--json"]));
    let all = names(&mixed["result"]);
    assert_eq!(names(&running["result"]).len(), 4 + 8);
    assert_eq!(names(&stopped["result"]).len(), 4);
    assert_eq!(names(&failed["result"]).len(), 3);
    for name in ["orders-db", "analytics-db", "starting-db", "flapping"] {
        assert!(names(&running["result"]).contains(&name), "{name}");
    }
    assert!(!names(&running["result"]).contains(&"mystery"));
    assert_eq!(
        names(&failed["result"]),
        ["broken-db", "half-made", "double-trouble"]
    );
    for filtered in [&running, &stopped, &failed] {
        assert_eq!(filtered["result"]["counts"], mixed["result"]["counts"]);
    }
    // Every instance is in exactly one of the filters, or is unknown.
    assert_eq!(all.len(), 20);
    golden::check("instance_list_failed.txt", &text.stdout);
    ev.pass();
}

/// Spec 0022 AC-10.
#[test]
fn no_instances_writes_one_line_and_a_filter_without_a_match_adds_the_counts() {
    let (root, _daemon) = listing(Vec::new(), 200);
    let empty = run(root.path(), &["instance", "list"]);
    let empty_filtered = run(root.path(), &["instance", "list", "--status", "failed"]);
    let empty_json = envelope(&run(root.path(), &["instance", "list", "--json"]));
    drop(_daemon);
    let one = vec![data::instance("")];
    let (root2, _daemon2) = listing(one, 200);
    let none = run(root2.path(), &["instance", "list", "--status", "failed"]);

    assert_eq!(empty.code, Some(0), "{empty:?}");
    assert_eq!(empty.stdout, "No instances.\n");
    assert_eq!(empty_filtered.stdout, "No instances.\n");
    golden::check("instance_list_empty.txt", &empty.stdout);
    assert_eq!(empty_json["result"]["items"], serde_json::json!([]));
    assert_eq!(empty_json["result"]["counts"]["total"], 0);
    assert_eq!(none.code, Some(0), "{none:?}");
    assert_eq!(
        none.stdout,
        "No failed instances.\n1 instance: 1 running, 0 stopped\n"
    );
    drop(root);
}

/// Spec 0022 AC-10.
#[test]
fn every_page_is_read_so_the_list_and_the_counts_do_not_stop_at_the_first() {
    let (root, daemon) = listing(data::list_mixed().items, 7);

    let json = run(root.path(), &["instance", "list", "--json"]);

    assert_eq!(json.code, Some(0), "{json:?}");
    let value = envelope(&json);
    assert_eq!(value["result"]["items"].as_array().unwrap().len(), 20);
    assert_eq!(value["result"]["counts"]["total"], 20);
    let calls: Vec<_> = daemon
        .log()
        .into_iter()
        .filter(|logged| logged.method == "instance.list")
        .collect();
    assert_eq!(calls.len(), 3);
    assert!(calls[0].raw["call"]["content"]["before"].is_null());
    assert!(calls[1].raw["call"]["content"]["before"].is_string());
    assert!(calls[2].raw["call"]["content"]["before"].is_string());
}

/// Spec 0022 AC-10.
#[test]
fn a_page_that_fails_in_the_middle_fails_the_command_without_a_table() {
    let items = data::list_mixed().items;
    let root = TestRoot::new().expect("a test root is made");
    let _daemon = ScriptedDaemon::serve(&root, move |call, ctx| match call {
        Call::InstanceList(_) if ctx.nth == 0 => {
            vec![Step::Reply(Reply::InstanceList(Box::new(InstancePage {
                items: items[..3].to_vec(),
                next: Some(format!("{}_{}", items[2].name, items[2].id)),
            })))]
        }
        Call::InstanceList(_) => vec![Step::Fail(error(
            "registry.unavailable",
            "unavailable",
            "the registry is not available",
        ))],
        _ => Vec::new(),
    });

    let text = run(root.path(), &["instance", "list"]);
    let json = run(root.path(), &["instance", "list", "--json"]);

    assert_ne!(text.code, Some(0), "{text:?}");
    assert_eq!(text.stdout, "");
    assert!(text.stderr.contains("registry.unavailable"), "{text:?}");
    assert_eq!(json.code, text.code);
    assert_eq!(envelope(&json)["error"]["code"], "registry.unavailable");
    assert!(envelope(&json)["result"].is_null());
}

/// Spec 0022 AC-10.
#[test]
fn an_unknown_status_is_a_usage_error() {
    let root = TestRoot::new().expect("a test root is made");

    let bad = run(root.path(), &["instance", "list", "--status", "unknown"]);

    assert_eq!(bad.code, Some(2), "{bad:?}");
}

/// Spec 0022 AC-10: text from the daemon never reaches the terminal raw.
#[test]
fn names_and_reasons_from_the_daemon_are_escaped_in_the_table() {
    let item = data::edited(|value| {
        value["name"] = "evil\u{1b}[31mname".into();
        value["readiness"]["state"] = "not_ready".into();
        value["readiness"]["reason"] = "why\u{1b}]0;x\u{7}".into();
    });
    let (root, _daemon) = listing(vec![item], 200);

    let text = run(root.path(), &["instance", "list"]);

    assert_eq!(text.code, Some(0), "{text:?}");
    assert!(!text.stdout.contains('\u{1b}'), "{}", text.stdout);
    assert!(
        text.stdout.contains("evil\\u{1b}[31mname"),
        "{}",
        text.stdout
    );
}
