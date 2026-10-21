//! `stackly daemon install`, `start`, `stop`, and `restart`, `completion`,
//! and `manpage` against the real binary (spec 0015 AC-7 to AC-12). The
//! tests run with `STACKLY_ROOT` set, which the background agent never
//! serves, so none of them can touch the real registration; the paths that
//! call the OS are covered with `FakeBackgroundAgent` in the unit tests.
#![cfg(target_os = "macos")]

mod common;

use std::time::Duration;

use stackly_testkit::TestRoot;
use stackly_testkit::evidence::{Kind, evidence};

use common::stacklyd::start_daemon;
use common::{detail, envelope, listing, paths, run};

#[test]
fn install_and_start_with_a_root_override_exit_2_before_touching_anything() {
    let ev = evidence!(
        covers = ["REQ-028", "REQ-014"],
        kind = Kind::Fixture,
        expected = "with STACKLY_ROOT set, daemon install and daemon start for a daemon that does not run fail cli.agent_root_override with exit 2, a partial agent result, the hint for the override, and create nothing",
    );
    let root = TestRoot::new().unwrap();
    let before = listing(root.path());

    for args in [
        ["daemon", "install", "--json"],
        ["daemon", "start", "--json"],
    ] {
        let json = run(root.path(), &args);

        assert_eq!(json.code, Some(2), "{args:?}: {json:?}");
        let value = envelope(&json);
        assert_eq!(value["error"]["code"], "cli.agent_root_override");
        assert_eq!(
            detail(&value["error"], "variable").as_deref(),
            Some("STACKLY_ROOT")
        );
        assert_eq!(value["result"]["agent"]["state"], "unavailable");
        assert_eq!(value["result"]["agent"]["reason"], "root_override");
        assert_eq!(value["result"]["info"], serde_json::Value::Null);
    }
    let text = run(root.path(), &["daemon", "start"]);
    assert_eq!(text.code, Some(2));
    assert!(
        text.stderr
            .contains("hint: start stacklyd yourself for STACKLY_ROOT"),
        "{}",
        text.stderr
    );
    assert_eq!(listing(root.path()), before);
    ev.pass();
}

#[test]
fn start_of_a_running_daemon_exits_0_and_asks_the_os_nothing() {
    let ev = evidence!(
        covers = ["REQ-028", "BR-011"],
        kind = Kind::Fixture,
        expected = "daemon start against a daemon that answers exits 0 with alreadyRunning true and a null agent, in text and in --json",
    );
    let root = TestRoot::new().unwrap();
    let daemon = start_daemon(&root);

    let json = run(root.path(), &["daemon", "start", "--json"]);
    let text = run(root.path(), &["daemon", "start"]);

    assert_eq!(json.code, Some(0), "{json:?}");
    let value = envelope(&json);
    assert_eq!(value["result"]["alreadyRunning"], true);
    assert_eq!(value["result"]["agent"], serde_json::Value::Null);
    assert_eq!(
        value["result"]["info"]["pid"].as_u64(),
        Some(u64::from(daemon.pid()))
    );
    assert_eq!(text.code, Some(0), "{text:?}");
    assert!(text.stdout.contains("already running"), "{}", text.stdout);
    assert_eq!(daemon.stop().unwrap().code(), Some(0));
    ev.pass();
}

#[test]
fn stop_ends_a_running_daemon_with_exit_0_and_a_second_stop_finds_nothing() {
    let ev = evidence!(
        covers = ["REQ-028", "AC-006"],
        kind = Kind::Fixture,
        expected = "daemon stop sends daemon.shutdown, reads the connection to its end, sees no socket answer, and exits 0 with wasRunning true; the daemon process exits 0; a second stop exits 0 with wasRunning false",
    );
    let root = TestRoot::new().unwrap();
    let daemon = start_daemon(&root);
    let paths = paths(&root);

    let json = run(root.path(), &["daemon", "stop", "--json"]);

    assert_eq!(json.code, Some(0), "{json:?}");
    let value = envelope(&json);
    assert_eq!(value["result"]["wasRunning"], true);
    assert!(value["result"]["serverInstanceId"].is_string());
    assert!(!paths.socket.exists(), "the socket is gone");
    let status = daemon.wait_exit(Duration::from_secs(15)).unwrap();
    assert_eq!(status.code(), Some(0));

    let again = run(root.path(), &["daemon", "stop", "--json"]);
    assert_eq!(again.code, Some(0), "{again:?}");
    let value = envelope(&again);
    assert_eq!(value["result"]["wasRunning"], false);
    assert_eq!(value["result"]["serverInstanceId"], serde_json::Value::Null);
    ev.pass();
}

#[test]
fn restart_of_a_daemon_started_by_hand_exits_5_and_leaves_it_running() {
    let ev = evidence!(
        covers = ["REQ-028", "AC-006"],
        kind = Kind::Fixture,
        expected = "daemon restart against a daemon that launchd did not start fails daemon.restart_unavailable with exit 5 and the daemon keeps running",
    );
    let root = TestRoot::new().unwrap();
    let mut daemon = start_daemon(&root);

    let json = run(root.path(), &["daemon", "restart", "--json"]);

    assert_eq!(json.code, Some(5), "{json:?}");
    let value = envelope(&json);
    assert_eq!(value["error"]["code"], "daemon.restart_unavailable");
    assert_eq!(
        detail(&value["error"], "launchContext").as_deref(),
        Some("manual")
    );
    assert!(daemon.try_status().unwrap().is_none(), "the daemon runs on");
    assert_eq!(daemon.stop().unwrap().code(), Some(0));
    ev.pass();
}

#[test]
fn status_names_the_launch_of_a_daemon_started_by_hand() {
    let root = TestRoot::new().unwrap();
    let daemon = start_daemon(&root);

    let text = run(root.path(), &["daemon", "status"]);
    let json = run(root.path(), &["daemon", "status", "--json"]);

    assert_eq!(text.code, Some(0), "{text:?}");
    assert!(
        text.stdout
            .lines()
            .any(|line| line.starts_with("Launched") && line.ends_with("by hand")),
        "{}",
        text.stdout
    );
    let value = envelope(&json);
    assert_eq!(value["result"]["info"]["launch"]["context"], "manual");
    assert_eq!(value["result"]["agent"]["reason"], "root_override");
    assert_eq!(daemon.stop().unwrap().code(), Some(0));
}

#[test]
fn completion_writes_a_script_per_shell_and_refuses_another_shell() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "stackly completion bash, zsh, and fish print a script naming the commands, with --json the result is { shell, script }, and another shell exits 2",
    );
    let root = TestRoot::new().unwrap();
    for shell in ["bash", "zsh", "fish"] {
        let text = run(root.path(), &["completion", shell]);
        assert_eq!(text.code, Some(0), "{shell}: {text:?}");
        assert!(text.stdout.contains("daemon"), "{shell}");
        let json = run(root.path(), &["completion", shell, "--json"]);
        let value = envelope(&json);
        assert_eq!(value["result"]["shell"], shell);
        assert!(
            value["result"]["script"]
                .as_str()
                .unwrap()
                .contains("restart")
        );
    }
    let refused = run(root.path(), &["completion", "powershell"]);
    assert_eq!(refused.code, Some(2), "{refused:?}");
    ev.pass();
}

#[test]
fn manpage_is_hidden_and_writes_every_page() {
    let root = TestRoot::new().unwrap();

    let help = run(root.path(), &["--help"]);
    let json = run(root.path(), &["manpage", "--json"]);

    assert!(!help.stdout.contains("manpage"), "{}", help.stdout);
    assert_eq!(json.code, Some(0), "{json:?}");
    let value = envelope(&json);
    let pages = value["result"]["pages"].as_array().unwrap();
    let files: Vec<&str> = pages
        .iter()
        .map(|page| page["file"].as_str().unwrap())
        .collect();
    assert_eq!(files[0], "stackly.1");
    assert!(files.contains(&"stackly-daemon-restart.1"), "{files:?}");
    assert!(pages[0]["roff"].as_str().unwrap().contains(".TH"));
}

#[test]
fn the_old_hint_is_gone_from_the_cli_sources() {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut stack = vec![src];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                let text = std::fs::read_to_string(&path).unwrap();
                assert!(
                    !text.contains(&["start it with ", "stacklyd\""].concat()),
                    "{} still has the old hint",
                    path.display()
                );
            }
        }
    }
}
