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

/// Spec 0026 AC-13: `restart`, `reload`, and `stop --force` by name.
#[test]
fn a_real_daemon_restarts_reloads_and_refuses_an_unconfirmed_force_stop() {
    let ev = evidence!(
        covers = ["REQ-028", "AC-028"],
        kind = Kind::Fixture,
        expected = "against a real stacklyd, instance restart --wait --json ends succeeded on a running instance with a new pid, reload --wait --json keeps the pid, stop --force without a terminal and without --yes exits 2 with cli.confirmation_required before anything is sent, --yes without --force is a usage error, and stop --force --yes on a stopped instance answers already stopped with exit 0",
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
            "beta",
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
    let wait = ["--wait", "--timeout", "60s", "--json"];
    let command = |verb: &str| {
        let mut args = vec!["instance", verb, "beta"];
        args.extend(wait);
        run(root.path(), &args)
    };

    let started = command("start");
    assert_eq!(started.code, Some(0), "{started:?}");
    let first = envelope(&started)["result"]["instance"]["process"]["pid"].clone();
    let restarted = command("restart");
    assert_eq!(restarted.code, Some(0), "{restarted:?}");
    let restarted = envelope(&restarted);
    assert_eq!(restarted["result"]["operation"]["kind"], "restart_instance");
    assert_eq!(restarted["result"]["operation"]["state"], "succeeded");
    let second = restarted["result"]["instance"]["process"]["pid"].clone();
    assert_ne!(second, first, "a restart runs a new process");
    assert_eq!(restarted["result"]["instance"]["lastExit"], "clean");
    assert_eq!(
        restarted["result"]["instance"]["capabilities"]["reload"],
        true
    );

    let text = run(
        root.path(),
        &["instance", "reload", "beta", "--wait", "--timeout", "60s"],
    );
    assert_eq!(text.code, Some(0), "{text:?}");
    assert_eq!(text.stdout, "reloaded beta\n");
    let reloaded = command("reload");
    assert_eq!(reloaded.code, Some(0), "{reloaded:?}");
    assert_eq!(
        envelope(&reloaded)["result"]["instance"]["process"]["pid"],
        second,
        "a reload keeps the pid"
    );

    // No terminal and no --yes: refused before any request.
    let unconfirmed = run(root.path(), &["instance", "stop", "beta", "--force"]);
    assert_eq!(unconfirmed.code, Some(2), "{unconfirmed:?}");
    let json = run(
        root.path(),
        &["instance", "stop", "beta", "--force", "--json"],
    );
    assert_eq!(json.code, Some(2), "{json:?}");
    assert_eq!(
        envelope(&json)["error"]["code"],
        "cli.confirmation_required"
    );
    let lonely_yes = run(root.path(), &["instance", "stop", "beta", "--yes"]);
    assert_eq!(lonely_yes.code, Some(2), "{lonely_yes:?}");
    let still = run(root.path(), &["instance", "get", "beta", "--json"]);
    assert_eq!(
        envelope(&still)["result"]["instance"]["lifecycle"],
        "running"
    );

    let stopped = command("stop");
    assert_eq!(stopped.code, Some(0), "{stopped:?}");
    let forced = run(
        root.path(),
        &["instance", "stop", "beta", "--force", "--yes"],
    );
    assert_eq!(forced.code, Some(0), "{forced:?}");
    assert_eq!(forced.stdout, "beta is already stopped\n");
    assert_eq!(daemon.stop().unwrap().code(), Some(0));
    ev.pass();
}
