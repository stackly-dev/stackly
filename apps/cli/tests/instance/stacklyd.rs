//! `instance list` and `inspect` against a real `stacklyd` (spec 0022
//! AC-10 and AC-11): the daemon holds fake instances made from the fake
//! runtime and provider that `skeleton_seed` writes, and the real `stackly`
//! binary lists them and finds them by name or ID.

use std::process::Command;

use serde_json::Value;
use stackly_testkit::evidence::{Kind, evidence};
use stackly_testkit::{ReapGuard, TestRoot, cargo_example, fake_server};

use crate::common::stacklyd::start_daemon;
use crate::common::{detail, envelope, paths, run};

/// Seeds `root` with the fake runtime and provider and returns the runtime
/// ID.
fn seed(root: &TestRoot) -> String {
    let fake = fake_server::binary();
    let output = Command::new(cargo_example("stacklyd", "skeleton_seed"))
        .args(["--root".as_ref(), root.path().as_os_str()])
        .args([
            "--fake-server".as_ref(),
            fake.as_os_str(),
            "--json".as_ref(),
        ])
        .output()
        .expect("skeleton_seed runs");
    assert!(
        output.status.success(),
        "the seed failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let seeded: Value = serde_json::from_slice(&output.stdout).expect("the seed prints JSON");
    seeded["runtimeId"]
        .as_str()
        .expect("a runtime ID")
        .to_owned()
}

fn create(root: &TestRoot, runtime: &str, name: &str) -> String {
    let created = run(
        root.path(),
        &[
            "instance",
            "create",
            name,
            "--runtime",
            runtime,
            "--provider",
            "testing/fake",
            "--revision",
            "0.3.0",
            "--wait",
            "--timeout",
            "60s",
            "--json",
        ],
    );
    assert_eq!(created.code, Some(0), "{created:?}");
    envelope(&created)["result"]["instance"]["id"]
        .as_str()
        .expect("the created instance has an ID")
        .to_owned()
}

/// Spec 0022 AC-10 and AC-11.
#[test]
fn a_real_daemon_lists_and_inspects_its_instances_by_name_and_by_id() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "against a real stacklyd with two fake instances, instance list shows both in name order with the count line, --status running keeps only the started one, inspect finds one by name and by upper case ID with the same JSON, and an unknown name exits 10 with cli.instance_not_found",
    );
    let root = TestRoot::new().expect("a test root");
    let runtime = seed(&root);
    let _reap = ReapGuard::new(root.path(), &paths(&root).runtime);
    let daemon = start_daemon(&root);

    let empty = run(root.path(), &["instance", "list"]);
    let alpha = create(&root, &runtime, "alpha");
    let beta = create(&root, &runtime, "beta");
    let started = run(
        root.path(),
        &["instance", "start", &alpha, "--wait", "--timeout", "60s"],
    );
    let list = run(root.path(), &["instance", "list", "--json"]);
    let running = run(
        root.path(),
        &["instance", "list", "--status", "running", "--json"],
    );
    let text = run(root.path(), &["instance", "list"]);
    let by_name = run(root.path(), &["instance", "inspect", "beta", "--json"]);
    let by_id = run(
        root.path(),
        &["instance", "inspect", &beta.to_uppercase(), "--json"],
    );
    let missing = run(root.path(), &["instance", "inspect", "gamma", "--json"]);
    let unknown = run(
        root.path(),
        &[
            "instance",
            "inspect",
            "0192f3a5-ffff-7000-8000-00000000ffff",
            "--json",
        ],
    );

    assert_eq!(empty.stdout, "No instances.\n");
    assert_eq!(started.code, Some(0), "{started:?}");
    let listed = envelope(&list);
    let items = listed["result"]["items"].as_array().expect("items");
    assert_eq!(
        items
            .iter()
            .map(|item| item["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["alpha", "beta"]
    );
    assert_eq!(listed["result"]["counts"]["total"], 2);
    assert_eq!(listed["result"]["counts"]["ready"], 1);
    assert_eq!(listed["result"]["counts"]["stopped"], 1);
    let running = envelope(&running);
    assert_eq!(running["result"]["items"].as_array().unwrap().len(), 1);
    assert_eq!(running["result"]["items"][0]["name"], "alpha");
    assert_eq!(running["result"]["counts"], listed["result"]["counts"]);
    assert_eq!(
        text.stdout.lines().last(),
        Some("2 instances: 1 running, 1 stopped")
    );
    assert_eq!(by_name.code, Some(0), "{by_name:?}");
    assert_eq!(by_name.stdout, by_id.stdout);
    assert_eq!(envelope(&by_name)["result"]["instance"]["name"], "beta");
    assert_eq!(missing.code, Some(10), "{missing:?}");
    let missing = envelope(&missing);
    assert_eq!(missing["error"]["code"], "cli.instance_not_found");
    assert_eq!(detail(&missing["error"], "name").as_deref(), Some("gamma"));
    assert_eq!(unknown.code, Some(10), "{unknown:?}");
    assert_eq!(envelope(&unknown)["error"]["code"], "resource.not_found");
    assert_eq!(daemon.stop().unwrap().code(), Some(0));
    ev.pass();
}
