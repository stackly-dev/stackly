//! `stackly instance autostart` and the autostart row of `inspect` (spec
//! 0031 AC-18): the policy goes to `instance.set_startup` with a key, the
//! text names the state, and `inspect` tells `off`, `at login`, and
//! `at login (stopped for this session)`.

use serde_json::json;
use stackly_protocol::{Call, Reply};
use stackly_testkit::TestRoot;
use stackly_testkit::evidence::{Kind, evidence};

use crate::common::scripted::{ScriptedDaemon, Step};
use crate::common::{envelope, run};
use crate::data;

const ID: &str = "0192f3a5-0000-7000-8000-000000000001";

/// The fixture instance with `policy` and `session` as its autostart.
fn with_autostart(policy: &str, session: &str) -> stackly_protocol::instance::InstanceSummary {
    data::edited(|value| {
        value["autostart"] = json!({ "policy": policy, "session": session });
    })
}

#[test]
fn autostart_on_and_off_send_the_policy_and_write_the_state() {
    let ev = evidence!(
        covers = ["AC-014"],
        kind = Kind::Fixture,
        expected = "instance autostart on sends instance.set_startup at_login with an idempotency key and writes the state; off sends manual; --json writes the instance",
    );
    let root = TestRoot::new().expect("a test root is made");
    let daemon = ScriptedDaemon::serve(&root, |call, _| match call {
        Call::InstanceSetStartup(params) => {
            let (policy, session) =
                if params.startup == stackly_protocol::instance::StartupPolicyKind::AtLogin {
                    ("at_login", "none")
                } else {
                    ("manual", "none")
                };
            vec![Step::Reply(Reply::InstanceSetStartup(Box::new(
                with_autostart(policy, session),
            )))]
        }
        _ => Vec::new(),
    });

    let on = run(root.path(), &["instance", "autostart", ID, "on"]);
    let off = run(root.path(), &["instance", "autostart", ID, "off", "--json"]);

    assert_eq!(on.code, Some(0), "{on:?}");
    assert!(
        on.stdout.contains("Autostart for “orders-db”: at login"),
        "{}",
        on.stdout
    );
    assert_eq!(off.code, Some(0), "{off:?}");
    let value = envelope(&off);
    assert_eq!(value["command"], "instance autostart");
    assert_eq!(value["result"]["autostart"]["policy"], "manual");
    let sent: Vec<_> = daemon
        .log()
        .into_iter()
        .filter(|logged| logged.method == "instance.set_startup")
        .collect();
    assert_eq!(sent.len(), 2);
    assert_eq!(sent[0].raw["call"]["content"]["startup"], "at_login");
    assert_eq!(sent[1].raw["call"]["content"]["startup"], "manual");
    assert!(sent[0].idempotency_key.is_some());
    assert_ne!(sent[0].idempotency_key, sent[1].idempotency_key);
    ev.pass();
}

#[test]
fn autostart_needs_a_switch() {
    let root = TestRoot::new().expect("a test root is made");
    let run = run(root.path(), &["instance", "autostart", ID, "maybe"]);

    assert_eq!(run.code, Some(2), "{run:?}");
}

fn inspect_row(policy: &str, session: &str) -> String {
    let root = TestRoot::new().expect("a test root is made");
    let instance = with_autostart(policy, session);
    let _daemon = ScriptedDaemon::serve(&root, {
        let instance = instance.clone();
        let policy = policy.to_owned();
        move |call, _| match call {
            Call::InstanceGet(_) => {
                vec![Step::Reply(Reply::InstanceGet(Box::new(instance.clone())))]
            }
            Call::InstanceGetDetail(_) => {
                let mut detail = data::detail_of(&instance);
                detail.startup = if policy == "at_login" {
                    stackly_protocol::instance::StartupPolicyKind::AtLogin
                } else {
                    stackly_protocol::instance::StartupPolicyKind::Manual
                };
                vec![Step::Reply(Reply::InstanceGetDetail(Box::new(detail)))]
            }
            other => data::detail_steps(other, &instance).unwrap_or_default(),
        }
    });
    let text = run(root.path(), &["instance", "inspect", ID]);
    assert_eq!(text.code, Some(0), "{text:?}");
    text.stdout
        .lines()
        .find(|line| line.starts_with("Autostart"))
        .unwrap_or_else(|| panic!("an Autostart row in {}", text.stdout))
        .split_whitespace()
        .skip(1)
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn inspect_tells_off_at_login_and_stopped_for_this_session() {
    assert_eq!(inspect_row("manual", "none"), "off");
    assert_eq!(inspect_row("at_login", "active"), "at login");
    assert_eq!(
        inspect_row("at_login", "suppressed"),
        "at login (stopped for this session)"
    );
}
