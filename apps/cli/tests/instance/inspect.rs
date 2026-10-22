//! AC-11: `instance inspect` takes a whole UUID in either case or an exact
//! name, writes the block of `get`, and `get` is its alias.

use stackly_protocol::{Call, Reply};
use stackly_testkit::TestRoot;
use stackly_testkit::evidence::{Kind, evidence};

use crate::common::scripted::{ScriptedDaemon, Step, error};
use crate::common::{detail, envelope, golden, run};
use crate::data;

/// A daemon with the instances of `list-mixed.json`.
fn serving() -> (TestRoot, ScriptedDaemon) {
    let root = TestRoot::new().expect("a test root is made");
    let page = data::list_mixed();
    let listed: Vec<_> = page.items.clone();
    let daemon = ScriptedDaemon::serve(&root, move |call, _| match call {
        Call::InstanceList(_) => vec![Step::Reply(Reply::InstanceList(Box::new(
            stackly_protocol::instance::InstancePage {
                items: listed.clone(),
                next: None,
            },
        )))],
        Call::InstanceGet(params) => {
            match listed.iter().find(|item| item.id == params.instance_id) {
                Some(item) => vec![Step::Reply(Reply::InstanceGet(Box::new(item.clone())))],
                None => vec![Step::Fail(error(
                    "resource.not_found",
                    "not_found",
                    "no instance has this id",
                ))],
            }
        }
        Call::InstanceGetDetail(params) => {
            let item = listed.iter().find(|item| item.id == params.instance_id);
            item.and_then(|item| data::detail_steps(call, item))
                .unwrap_or_default()
        }
        Call::InstanceGetDataUsage(params) => {
            let item = listed.iter().find(|item| item.id == params.instance_id);
            item.and_then(|item| data::detail_steps(call, item))
                .unwrap_or_default()
        }
        _ => Vec::new(),
    });
    (root, daemon)
}

fn id_of(name: &str) -> String {
    data::list_mixed()
        .items
        .into_iter()
        .find(|item| item.name == name)
        .map(|item| item.id.to_string())
        .expect("the fixture has the instance")
}

/// Spec 0022 AC-11.
#[test]
fn inspect_by_id_in_either_case_calls_instance_get_once_and_writes_the_block() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "instance inspect of a whole UUID, upper or lower case, calls instance.get with the lower case ID and no instance.list, and writes the kv block with the endpoint table in text and { instance } with --json",
    );
    let (root, daemon) = serving();
    let id = id_of("broken-db");

    let text = run(root.path(), &["instance", "inspect", &id]);
    let json = run(
        root.path(),
        &["instance", "inspect", &id.to_uppercase(), "--json"],
    );

    assert_eq!(text.code, Some(0), "{text:?}");
    assert_eq!(text.stderr, "");
    golden::check("instance_inspect.txt", &text.stdout);
    assert_eq!(json.code, Some(0), "{json:?}");
    golden::check("instance_inspect.json", &json.stdout);
    let value = envelope(&json);
    assert_eq!(value["command"], "instance inspect");
    assert_eq!(
        value["result"]
            .as_object()
            .unwrap()
            .keys()
            .collect::<Vec<_>>(),
        ["dataUsage", "detail", "instance"]
    );
    assert_eq!(value["result"]["instance"]["name"], "broken-db");
    assert_eq!(
        daemon.methods(),
        [
            "daemon.handshake",
            "instance.get",
            "instance.get_detail",
            "instance.get_data_usage",
            "daemon.handshake",
            "instance.get",
            "instance.get_detail",
            "instance.get_data_usage"
        ]
    );
    assert_eq!(daemon.log()[3].raw["call"]["content"]["instanceId"], id);
    ev.pass();
}

/// Spec 0022 AC-11.
#[test]
fn inspect_by_name_finds_the_id_in_the_list_and_reads_the_instance() {
    let (root, daemon) = serving();
    let id = id_of("cache");

    let by_name = run(root.path(), &["instance", "inspect", "cache", "--json"]);
    let by_id = run(root.path(), &["instance", "inspect", &id, "--json"]);

    assert_eq!(by_name.code, Some(0), "{by_name:?}");
    assert_eq!(by_name.stdout, by_id.stdout);
    assert_eq!(
        daemon.methods(),
        [
            "daemon.handshake",
            "instance.list",
            "instance.get",
            "instance.get_detail",
            "instance.get_data_usage",
            "daemon.handshake",
            "instance.get",
            "instance.get_detail",
            "instance.get_data_usage"
        ]
    );
    assert_eq!(daemon.log()[2].raw["call"]["content"]["instanceId"], id);
}

/// Spec 0022 AC-11.
#[test]
fn an_unknown_name_exits_10_with_instance_not_found_and_reads_no_instance() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "instance inspect of a name no instance has exits 10 with cli.instance_not_found, details name, the message no instance named <name>, and a hint to instance list, and sends no instance.get",
    );
    let (root, daemon) = serving();

    let text = run(root.path(), &["instance", "inspect", "Cache"]);
    let json = run(root.path(), &["instance", "inspect", "Cache", "--json"]);

    assert_eq!(text.code, Some(10), "{text:?}");
    assert_eq!(text.stdout, "");
    assert!(text.stderr.contains("no instance named Cache"), "{text:?}");
    assert!(text.stderr.contains("cli.instance_not_found"), "{text:?}");
    assert!(
        text.stderr
            .contains("list instances with stackly instance list"),
        "{text:?}"
    );
    assert_eq!(json.code, Some(10), "{json:?}");
    let value = envelope(&json);
    assert_eq!(value["error"]["code"], "cli.instance_not_found");
    assert_eq!(value["error"]["category"], "not_found");
    assert_eq!(detail(&value["error"], "name").as_deref(), Some("Cache"));
    assert_eq!(
        value["error"]["hint"],
        "list instances with stackly instance list"
    );
    assert!(!daemon.methods().contains(&"instance.get".to_owned()));
    ev.pass();
}

/// Spec 0022 AC-11.
#[test]
fn an_unknown_uuid_exits_10_with_the_daemons_not_found() {
    let (root, _daemon) = serving();

    let json = run(
        root.path(),
        &[
            "instance",
            "inspect",
            "0192f3a5-ffff-7000-8000-00000000ffff",
            "--json",
        ],
    );

    assert_eq!(json.code, Some(10), "{json:?}");
    assert_eq!(envelope(&json)["error"]["code"], "resource.not_found");
}

/// Spec 0022 AC-11.
#[test]
fn get_is_the_alias_of_inspect_with_the_same_output() {
    let (root, daemon) = serving();

    let inspect = run(root.path(), &["instance", "inspect", "queue"]);
    let get = run(root.path(), &["instance", "get", "queue"]);
    let get_json = run(
        root.path(),
        &["instance", "get", &id_of("orders-db"), "--json"],
    );

    assert_eq!(get.code, Some(0), "{get:?}");
    assert_eq!(get.stdout, inspect.stdout);
    assert_eq!(envelope(&get_json)["command"], "instance get");
    assert_eq!(daemon.methods().len(), 5 + 5 + 4);
}

/// Spec 0022 AC-11.
#[test]
fn without_a_daemon_inspect_and_list_exit_6() {
    let root = TestRoot::new().expect("a test root is made");

    let inspect = run(root.path(), &["instance", "inspect", "orders-db"]);
    let list = run(root.path(), &["instance", "list"]);
    let id = run(
        root.path(),
        &[
            "instance",
            "inspect",
            "0192f3a5-0000-7000-8000-000000000001",
        ],
    );

    assert_eq!(inspect.code, Some(6), "{inspect:?}");
    assert_eq!(list.code, Some(6), "{list:?}");
    assert_eq!(id.code, Some(6), "{id:?}");
}
