//! AC-16: `runtime inspect` writes the runtime, its artifact, its
//! signatures, and its compatibility records.

use stackly_protocol::{Call, Reply};
use stackly_testkit::TestRoot;
use stackly_testkit::evidence::{Kind, evidence};

use crate::common::scripted::{ScriptedDaemon, Step, error};
use crate::common::{envelope, golden, run};
use crate::data;

const ID: &str = "0192f3a6-0000-7000-8000-0000000000a1";

/// Spec 0018 AC-16.
#[test]
fn inspect_writes_the_block_the_artifact_and_the_tables_and_the_runtime_as_json() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "runtime inspect sends runtime.get with the id in lower case and writes the summary block, the artifact, the signatures, the dependencies, and the compatibility records, and the detail itself with --json",
    );
    let root = TestRoot::new().expect("a test root is made");
    let daemon = ScriptedDaemon::serve(&root, |call, _| match call {
        Call::RuntimeGet(_) => vec![Step::Reply(Reply::RuntimeGet(Box::new(data::detail())))],
        _ => Vec::new(),
    });

    let text = run(root.path(), &["runtime", "inspect", &ID.to_uppercase()]);
    let json = run(root.path(), &["runtime", "inspect", ID, "--json"]);

    assert_eq!(text.code, Some(0), "{text:?}");
    golden::check("runtime_inspect.txt", &text.stdout);
    golden::check("runtime_inspect.json", &json.stdout);
    let value = envelope(&json);
    assert_eq!(value["command"], "runtime inspect");
    assert_eq!(value["result"]["summary"]["version"], "18.6");
    let sent = daemon
        .log()
        .into_iter()
        .find(|logged| logged.method == "runtime.get")
        .expect("runtime.get was sent");
    assert_eq!(sent.raw["call"]["content"]["runtimeId"], ID);
    for part in [
        "Signature",
        "Dependencies",
        "Compatibility records",
        "libssl.3.dylib",
    ] {
        assert!(
            text.stdout.contains(part),
            "{part} is missing:\n{}",
            text.stdout
        );
    }
    ev.pass();
}

/// Spec 0018 AC-16.
#[test]
fn an_unknown_runtime_exits_10_and_a_bad_id_exits_2() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "resource.not_found exits 10 and an id that is not a whole UUID exits 2 before any call",
    );
    let root = TestRoot::new().expect("a test root is made");
    let daemon = ScriptedDaemon::serve(&root, |call, _| match call {
        Call::RuntimeGet(_) => vec![Step::Fail(error(
            "resource.not_found",
            "not_found",
            "the runtime does not exist",
        ))],
        _ => Vec::new(),
    });

    let missing = run(root.path(), &["runtime", "inspect", ID]);
    let bad = run(root.path(), &["runtime", "inspect", "orders"]);

    assert_eq!(missing.code, Some(10), "{missing:?}");
    assert_eq!(bad.code, Some(2), "{bad:?}");
    assert_eq!(
        daemon
            .methods()
            .iter()
            .filter(|m| *m == "runtime.get")
            .count(),
        1
    );
    ev.pass();
}
