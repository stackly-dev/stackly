//! AC-16: `runtime list` reads every page and writes the table and
//! `{ items, installing }`.

use stackly_protocol::runtime::RuntimeListPage;
use stackly_protocol::{Call, Reply};
use stackly_testkit::TestRoot;
use stackly_testkit::evidence::{Kind, evidence};

use crate::common::scripted::{ScriptedDaemon, Step};
use crate::common::{envelope, golden, run};
use crate::data;

/// Spec 0018 AC-16.
#[test]
fn list_writes_the_table_the_installs_and_the_json_result() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "runtime list writes ID to USED BY with the digest cut to 12 characters, the install that has not ended under the table, and { items, installing } with --json",
    );
    let root = TestRoot::new().expect("a test root is made");
    let _daemon = ScriptedDaemon::serve(&root, |call, _| match call {
        Call::RuntimeList(_) => vec![Step::Reply(Reply::RuntimeList(Box::new(data::list())))],
        _ => Vec::new(),
    });

    let text = run(root.path(), &["runtime", "list"]);
    let json = run(root.path(), &["runtime", "list", "--json"]);

    assert_eq!(text.code, Some(0), "{text:?}");
    golden::check("runtime_list.txt", &text.stdout);
    golden::check("runtime_list.json", &json.stdout);
    let result = &envelope(&json)["result"];
    assert_eq!(
        result.as_object().unwrap().keys().collect::<Vec<_>>(),
        ["installing", "items"]
    );
    assert_eq!(result["items"].as_array().unwrap().len(), 2);
    assert_eq!(result["installing"].as_array().unwrap().len(), 1);
    assert!(text.stdout.contains("88bb7533e639"), "{}", text.stdout);
    assert!(!text.stdout.contains("88bb7533e639e"), "the digest is cut");
    ev.pass();
}

/// Spec 0018 AC-16.
#[test]
fn list_follows_the_cursor_and_says_so_when_there_is_nothing() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "runtime list asks for the next page with the cursor until next is null, keeps the installs of the first page, and writes No runtimes. for an empty daemon",
    );
    let root = TestRoot::new().expect("a test root is made");
    let all = data::list();
    let first = RuntimeListPage {
        items: vec![all.items[0].clone()],
        next: Some(all.items[0].id),
        installing: all.installing.clone(),
    };
    let second = RuntimeListPage {
        items: vec![all.items[1].clone()],
        next: None,
        installing: Vec::new(),
    };
    let daemon = ScriptedDaemon::serve(&root, move |call, _| match call {
        Call::RuntimeList(params) if params.after.is_none() => {
            vec![Step::Reply(Reply::RuntimeList(Box::new(first.clone())))]
        }
        Call::RuntimeList(_) => vec![Step::Reply(Reply::RuntimeList(Box::new(second.clone())))],
        _ => Vec::new(),
    });

    let json = run(root.path(), &["runtime", "list", "--json"]);

    let result = &envelope(&json)["result"];
    assert_eq!(result["items"].as_array().unwrap().len(), 2);
    assert_eq!(result["installing"].as_array().unwrap().len(), 1);
    let lists: Vec<_> = daemon
        .log()
        .into_iter()
        .filter(|logged| logged.method == "runtime.list")
        .collect();
    assert_eq!(lists.len(), 2);
    assert_eq!(lists[0].raw["call"]["content"]["limit"], 200);
    assert_eq!(
        lists[1].raw["call"]["content"]["after"],
        all.items[0].id.to_string()
    );
    drop(daemon);

    let _empty = ScriptedDaemon::serve(&root, |call, _| match call {
        Call::RuntimeList(_) => vec![Step::Reply(Reply::RuntimeList(Box::new(RuntimeListPage {
            items: Vec::new(),
            next: None,
            installing: Vec::new(),
        })))],
        _ => Vec::new(),
    });
    let empty = run(root.path(), &["runtime", "list"]);
    assert_eq!(empty.stdout, "No runtimes.\n");
    golden::check("runtime_list_empty.txt", &empty.stdout);
    ev.pass();
}
