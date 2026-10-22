//! `instance start` and `stop` by name against a real `stacklyd` (spec 0022
//! AC-15): the daemon holds a fake instance made from the fake runtime and
//! provider that `skeleton_seed` writes, and the real `stackly` binary
//! starts and stops it by name, by upper case ID, and fails an unknown name
//! before any operation.

use stackly_testkit::evidence::{Kind, evidence};
use stackly_testkit::{ReapGuard, TestRoot};

use crate::common::stacklyd::start_daemon;
use crate::common::{detail, envelope, paths, run};
use crate::stacklyd::seed;

/// Spec 0022 AC-15.
#[test]
fn a_real_daemon_starts_and_stops_an_instance_by_name_and_refuses_an_unknown_name() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "against a real stacklyd with one fake instance, instance start and stop by name and by upper case ID reach running and stopped, a second start by name answers already running, and an unknown name exits 10 with cli.instance_not_found",
    );
    let root = TestRoot::new().expect("a test root");
    let runtime = seed(&root);
    let _reap = ReapGuard::new(root.path(), &paths(&root).runtime);
    let daemon = start_daemon(&root);

    let created = run(
        root.path(),
        &[
            "instance",
            "create",
            "alpha",
            "--runtime",
            &runtime,
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
    let id = envelope(&created)["result"]["instance"]["id"]
        .as_str()
        .expect("the created instance has an ID")
        .to_owned();

    let wait = ["--wait", "--timeout", "60s", "--json"];
    let command = |verb: &str, target: &str| {
        let mut args = vec!["instance", verb, target];
        args.extend(wait);
        run(root.path(), &args)
    };
    let started = command("start", "alpha");
    let again = run(root.path(), &["instance", "start", "alpha"]);
    let stopped = command("stop", "alpha");
    let upper = command("start", &id.to_uppercase());
    let upper_stopped = command("stop", &id.to_uppercase());
    let missing = command("start", "gamma");

    assert_eq!(started.code, Some(0), "{started:?}");
    let started = envelope(&started);
    assert_eq!(started["result"]["operation"]["state"], "succeeded");
    assert_eq!(started["result"]["instance"]["name"], "alpha");
    assert_eq!(started["result"]["instance"]["lifecycle"], "running");
    assert_eq!(again.code, Some(0), "{again:?}");
    assert_eq!(again.stdout, "already running\n");
    assert_eq!(stopped.code, Some(0), "{stopped:?}");
    assert_eq!(
        envelope(&stopped)["result"]["instance"]["lifecycle"],
        "stopped"
    );
    assert_eq!(upper.code, Some(0), "{upper:?}");
    assert_eq!(
        envelope(&upper)["result"]["instance"]["lifecycle"],
        "running"
    );
    assert_eq!(upper_stopped.code, Some(0), "{upper_stopped:?}");
    assert_eq!(missing.code, Some(10), "{missing:?}");
    let missing = envelope(&missing);
    assert_eq!(missing["error"]["code"], "cli.instance_not_found");
    assert_eq!(detail(&missing["error"], "name").as_deref(), Some("gamma"));
    assert_eq!(daemon.stop().unwrap().code(), Some(0));
    ev.pass();
}
