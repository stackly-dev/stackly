//! `stackly daemon status` against a real `stacklyd` and against endpoints
//! that must be refused (spec 0006 AC-13, spec 0008 AC-6). The CLI never
//! starts the daemon and never creates a file under the test root.
#![cfg(target_os = "macos")]

mod common;

use std::ffi::OsStr;
use std::path::Path;
use std::time::{Duration, Instant};

use stackly_protocol::{CONTRACT_DIGEST, ProtocolVersion};
use stackly_testkit::evidence::{Kind, evidence};
use stackly_testkit::{SpawnedDaemon, TestRoot, cargo_bin};

use common::stacklyd::start_daemon;
use common::{
    HandMadeRuntime, Run, detail, envelope, listing, paths, run, run_command, stackly, uid,
};

fn status(root: &Path, json: bool) -> Run {
    if json {
        run(root, &["daemon", "status", "--json"])
    } else {
        run(root, &["daemon", "status"])
    }
}

#[test]
fn status_of_a_running_daemon_prints_its_facts_as_text_and_json_and_creates_nothing() {
    let ev = evidence!(
        covers = ["REQ-028", "BR-011", "AC-028"],
        kind = Kind::Fixture,
        expected = "stackly daemon status against a real stacklyd shows status, version, pid, uid, roots, socket, and override as text and as one --json envelope with the client block and the cli.root_override warning, exits 0, and adds no file under the root",
    );
    let root = TestRoot::new().unwrap();
    let daemon = start_daemon(&root);
    let paths = paths(&root);
    let before = listing(root.path());

    let text = status(root.path(), false);
    let json = status(root.path(), true);

    assert_eq!(text.code, Some(0), "{text:?}");
    let protocol = ProtocolVersion::CURRENT;
    // The key column is as wide as the longest key shown, which depends on
    // the subsystem rows, so keys and values are compared apart.
    let rows: Vec<(&str, &str)> = text
        .stdout
        .lines()
        .filter_map(|line| line.split_once("  "))
        .map(|(key, value)| (key.trim_end(), value.trim_start()))
        .collect();
    for (key, value) in [
        ("Status", "ready".to_owned()),
        (
            "Version",
            format!(
                "{} (protocol {}.{})",
                env!("CARGO_PKG_VERSION"),
                protocol.major,
                protocol.minor
            ),
        ),
        ("PID", daemon.pid().to_string()),
        ("UID", uid().to_string()),
        ("Socket", paths.socket.display().to_string()),
        ("Data", paths.data.display().to_string()),
        ("Override", "yes (STACKLY_ROOT)".to_owned()),
    ] {
        assert!(
            rows.contains(&(key, value.as_str())),
            "missing {key:?} = {value:?} in:\n{}",
            text.stdout
        );
    }
    assert_eq!(
        text.stderr,
        format!(
            "warning: the STACKLY_ROOT override is active, with the data root at {}\n",
            paths.data.display()
        )
    );

    assert_eq!(json.code, Some(0), "{json:?}");
    let value = envelope(&json);
    assert_eq!(value["command"], "daemon status");
    let result = &value["result"];
    assert_eq!(result["info"]["pid"], daemon.pid());
    assert_eq!(
        result["info"]["socketPath"],
        paths.socket.display().to_string()
    );
    assert_eq!(result["health"]["status"], "ready");
    assert_eq!(result["client"]["contractDigest"], CONTRACT_DIGEST);
    assert_eq!(result["client"]["contractDrift"], false);
    assert_eq!(result["client"]["protocol"]["major"], protocol.major);
    assert_eq!(value["warnings"][0]["code"], "cli.root_override");
    assert_eq!(json.stderr, "");

    assert_eq!(
        listing(root.path()),
        before,
        "stackly created or removed a path"
    );
    assert_eq!(daemon.stop().unwrap().code(), Some(0));
    ev.pass();
}

#[test]
fn status_without_a_daemon_names_the_socket_and_exits_6() {
    let ev = evidence!(
        covers = ["REQ-028", "BR-011"],
        kind = Kind::Fixture,
        expected = "with no daemon running, stackly daemon status says the daemon is not running, names the socket path, hints at stacklyd, exits 6 with ipc.endpoint_missing in the envelope, and creates nothing",
    );
    let root = TestRoot::new().unwrap();
    let paths = paths(&root);

    let text = status(root.path(), false);
    let json = status(root.path(), true);

    assert_eq!(text.code, Some(6), "{text:?}");
    assert_eq!(text.stdout, "");
    assert_eq!(
        text.stderr,
        format!(
            "error: daemon is not running (no socket at {})\n  code: ipc.endpoint_missing\n  hint: start it with stacklyd\n",
            paths.socket.display()
        )
    );
    assert_eq!(json.code, Some(6));
    let value = envelope(&json);
    assert_eq!(value["error"]["code"], "ipc.endpoint_missing");
    assert_eq!(
        detail(&value["error"], "path"),
        Some(paths.socket.display().to_string())
    );
    assert_eq!(json.stderr, "");
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
    assert!(!paths.runtime.exists());
    ev.pass();
}

#[test]
fn status_refuses_an_endpoint_that_is_not_a_socket_and_exits_4() {
    let ev = evidence!(
        covers = ["REQ-028", "AC-070"],
        kind = Kind::Fixture,
        expected = "a regular file at the socket path is refused as an untrusted endpoint without a byte sent, and the CLI exits 4",
    );
    let root = TestRoot::new().unwrap();
    let paths = paths(&root);
    let _runtime = HandMadeRuntime::new(&paths.runtime);
    std::fs::write(&paths.socket, b"").unwrap();

    let json = status(root.path(), true);

    assert_eq!(json.code, Some(4), "{json:?}");
    let value = envelope(&json);
    assert_eq!(value["error"]["code"], "ipc.endpoint_untrusted");
    assert_eq!(
        detail(&value["error"], "reason").as_deref(),
        Some("not_a_socket")
    );
    assert_eq!(
        std::fs::read(&paths.socket).unwrap(),
        b"",
        "nothing was sent"
    );
    ev.pass();
}

#[test]
fn a_degraded_daemon_still_exits_0() {
    let root = TestRoot::new().unwrap();
    let paths = paths(&root);
    let first = start_daemon(&root);
    assert_eq!(first.stop().unwrap().code(), Some(0));
    let mut bytes = std::fs::read(&paths.registry_db).unwrap();
    bytes[60..64].copy_from_slice(&999_u32.to_be_bytes());
    std::fs::write(&paths.registry_db, &bytes).unwrap();
    let daemon = start_daemon(&root);

    let text = status(root.path(), false);
    let json = status(root.path(), true);

    assert_eq!(text.code, Some(0), "{text:?}");
    assert!(
        text.stdout.contains("Status      degraded\n"),
        "{}",
        text.stdout
    );
    assert!(
        text.stdout.contains(
            "Subsystems  registry: unavailable (the registry needs recovery (schema_newer_than_app))\n"
        ),
        "{}",
        text.stdout
    );
    assert!(
        text.stdout.contains("Recovery    schema_newer_than_app\n"),
        "{}",
        text.stdout
    );
    assert!(
        text.stdout.contains("Schema      999 > "),
        "{}",
        text.stdout
    );
    assert!(
        text.stderr.ends_with(
            "hint: restore the registry from a snapshot or backup, then restart stacklyd\n"
        ),
        "{}",
        text.stderr
    );
    let value = envelope(&json);
    assert_eq!(value["result"]["health"]["status"], "degraded");
    assert_eq!(
        value["result"]["health"]["recovery"]["reason"],
        "schema_newer_than_app"
    );
    assert_eq!(daemon.stop().unwrap().code(), Some(0));
}

#[test]
fn after_a_second_daemon_was_refused_status_names_the_first() {
    let ev = evidence!(
        covers = ["BR-011", "REQ-028"],
        kind = Kind::Fixture,
        expected = "after a second stacklyd for the same root exits 3, stackly daemon status shows the first daemon's pid and server instance",
    );
    let root = TestRoot::new().unwrap();
    let paths = paths(&root);
    let first = start_daemon(&root);
    let second = SpawnedDaemon::spawn(
        &cargo_bin("stacklyd", "stacklyd"),
        root.path(),
        &paths.socket,
        &paths.runtime,
        &[],
    )
    .unwrap();
    assert_eq!(
        second.wait_exit(Duration::from_secs(10)).unwrap().code(),
        Some(3)
    );

    let json = status(root.path(), true);

    assert_eq!(json.code, Some(0), "{json:?}");
    let value = envelope(&json);
    assert_eq!(value["result"]["info"]["pid"], first.pid());
    assert!(
        first.stderr().contains(
            value["result"]["info"]["serverInstanceId"]
                .as_str()
                .unwrap()
        )
    );
    assert_eq!(first.stop().unwrap().code(), Some(0));
    ev.pass();
}

#[test]
fn a_rejected_root_override_exits_2_before_anything_else() {
    let mut text = stackly(Path::new("relative/root"));
    text.args(["daemon", "status"]);
    let text = run_command(text);
    let mut json = stackly(OsStr::new("relative/root").as_ref());
    json.args(["daemon", "status", "--json"]);
    let json = run_command(json);

    assert_eq!(text.code, Some(2), "{text:?}");
    assert_eq!(
        text.stderr,
        "error: the value of STACKLY_ROOT is refused: not absolute\n  code: cli.invalid_environment\n"
    );
    assert_eq!(json.code, Some(2));
    let value = envelope(&json);
    assert_eq!(value["command"], "daemon status");
    assert_eq!(value["error"]["code"], "cli.invalid_environment");
    assert_eq!(
        detail(&value["error"], "variable").as_deref(),
        Some("STACKLY_ROOT")
    );
}

#[test]
fn a_daemon_that_never_answers_the_handshake_times_out_after_5_seconds_and_exits_8() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "when connect plus handshake gets no answer in 5 seconds, stackly daemon status exits 8 with ipc.call_timeout naming daemon.handshake and 5 seconds",
    );
    let root = TestRoot::new().unwrap();
    let daemon = common::scripted::ScriptedDaemon::start(
        &root,
        |_| common::scripted::Handshake::Hang,
        |_, _| Vec::new(),
    );

    let started = Instant::now();
    let json = status(root.path(), true);

    assert_eq!(json.code, Some(8), "{json:?}");
    assert!(started.elapsed() >= Duration::from_secs(5));
    let value = envelope(&json);
    assert_eq!(value["error"]["code"], "ipc.call_timeout");
    assert_eq!(
        (
            detail(&value["error"], "method"),
            detail(&value["error"], "seconds"),
            detail(&value["error"], "idempotencyKey"),
        ),
        (
            Some("daemon.handshake".to_owned()),
            Some("5".to_owned()),
            None
        )
    );
    assert_eq!(daemon.methods(), ["daemon.handshake"]);
    ev.pass();
}

#[test]
fn a_call_the_daemon_never_answers_times_out_after_10_seconds_and_exits_8() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "when daemon.get_info gets no answer in 10 seconds, stackly daemon status exits 8 with ipc.call_timeout naming the method and 10 seconds, without an idempotency key",
    );
    let root = TestRoot::new().unwrap();
    let daemon = common::scripted::ScriptedDaemon::serve(&root, |_, _| Vec::new());
    let before = listing(root.path());

    let started = Instant::now();
    let json = status(root.path(), true);

    assert_eq!(json.code, Some(8), "{json:?}");
    assert!(started.elapsed() >= Duration::from_secs(10));
    let value = envelope(&json);
    assert_eq!(
        (
            detail(&value["error"], "method"),
            detail(&value["error"], "seconds"),
            detail(&value["error"], "idempotencyKey"),
        ),
        (
            Some("daemon.get_info".to_owned()),
            Some("10".to_owned()),
            None
        )
    );
    assert_eq!(daemon.methods(), ["daemon.handshake", "daemon.get_info"]);
    assert_eq!(listing(root.path()), before, "the CLI created files");
    ev.pass();
}
