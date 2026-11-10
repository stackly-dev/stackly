//! `stackly doctor` against a real `stacklyd` (spec 0034, AC-11, AC-15,
//! AC-18): a fake instance whose port the test holds, and the record of a
//! start that failed on a root others could write.
//!
//! These runs leave out `disk_space`, whose level depends on how full this
//! Mac's volume is.

use std::fs::Permissions;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

use serde_json::Value;
use stackly_testkit::evidence::{Kind, evidence};
use stackly_testkit::{ReapGuard, TestRoot, allocate_port, cargo_bin};

use crate::common::stacklyd::{seed, start_daemon};
use crate::common::{detail, envelope, paths, run};

/// Every daemon check but `disk_space`.
const STEADY_CHECKS: [&str; 24] = [
    "--check",
    "daemon",
    "--check",
    "registry",
    "--check",
    "permissions",
    "--check",
    "runtimes",
    "--check",
    "provider_packages",
    "--check",
    "endpoints",
    "--check",
    "processes",
    "--check",
    "instance_files",
    "--check",
    "instance_failures",
    "--check",
    "config_drift",
    "--check",
    "secrets",
    "--check",
    "operations",
];

/// `stackly doctor` with every steady check and `extra`.
fn doctor(root: &TestRoot, extra: &[&str]) -> crate::common::Run {
    let mut args = vec!["doctor"];
    args.extend(STEADY_CHECKS);
    args.extend(extra);
    run(root.path(), &args)
}

/// Creates the fake instance `name` with its `http` endpoint on `port` and
/// the settings `sets`.
fn create(root: &TestRoot, runtime: &str, name: &str, port: u16, sets: &[&str]) {
    let request = format!("http={port}");
    let mut args = vec![
        "instance",
        "create",
        name,
        "--runtime",
        runtime,
        "--provider",
        "testing/fake",
        "--revision",
        "0.4.0",
        "--port",
        &request,
    ];
    for set in sets {
        args.extend(["--set", set]);
    }
    args.extend(["--wait", "--timeout", "60s", "--json"]);
    let created = run(root.path(), &args);
    assert_eq!(created.code, Some(0), "{created:?}");
}

/// The check `id` of a doctor envelope's report.
fn check<'a>(value: &'a Value, id: &str) -> &'a Value {
    value["result"]["report"]["checks"]
        .as_array()
        .expect("the report's checks")
        .iter()
        .find(|check| check["id"] == id)
        .unwrap_or_else(|| panic!("no {id} check in {value}"))
}

/// Every finding of a doctor envelope's report.
fn findings(value: &Value) -> Vec<Value> {
    value["result"]["report"]["checks"]
        .as_array()
        .expect("the report's checks")
        .iter()
        .flat_map(|check| check["findings"].as_array().cloned().unwrap_or_default())
        .collect()
}

/// The `port_in_use` finding of instance `name`.
fn port_finding(value: &Value, name: &str) -> Value {
    check(value, "endpoints")["findings"]
        .as_array()
        .expect("the endpoint findings")
        .iter()
        .find(|finding| finding["code"] == "port_in_use" && finding["instance"]["name"] == name)
        .cloned()
        .unwrap_or_else(|| panic!("no port_in_use for {name} in {value}"))
}

/// The suggested port of `finding`, after checking that it names the test
/// process as the owner of `port` and carries a `change_port` action to
/// the suggestion.
fn owned_by_this_test(finding: &Value, port: u16) -> u16 {
    assert_eq!(finding["portOwner"]["kind"], "process", "{finding}");
    assert_eq!(finding["portOwner"]["process"]["pid"], std::process::id());
    assert_eq!(
        detail(finding, "port").as_deref(),
        Some(port.to_string().as_str())
    );
    let suggested: u16 = detail(finding, "suggestedPort")
        .expect("a suggested port")
        .parse()
        .expect("a port number");
    assert_ne!(suggested, port);
    assert_eq!(finding["action"]["kind"], "change_port");
    assert_eq!(finding["action"]["endpoint"], "http");
    assert_eq!(finding["action"]["port"], suggested);
    suggested
}

/// The lifecycle of `name`, as `instance inspect` shows it.
fn lifecycle(root: &TestRoot, name: &str) -> Value {
    let inspected = run(root.path(), &["instance", "inspect", name, "--json"]);
    assert_eq!(inspected.code, Some(0), "{inspected:?}");
    envelope(&inspected)["result"]["instance"]["lifecycle"].clone()
}

/// Spec 0034 AC-1, AC-5, AC-15, AC-18 against a real `stacklyd`.
#[test]
fn a_real_daemon_reports_held_ports_with_their_owner_a_warning_exits_0_and_an_error_exits_7() {
    let ev = evidence!(
        covers = ["REQ-019", "REQ-028", "AC-028"],
        kind = Kind::Fixture,
        expected = "against a real stacklyd with three fake instances: the stopped one whose start was refused because the test holds its port is a port_in_use Warning, so stackly doctor exits 0; once a second one crashed into failed and the test holds its port too, that one is a port_in_use Error with a start_failed Error, and stackly doctor exits 7 with cli.doctor_found_errors; each conflict names the test process as the owner with a suggested port and a change_port action, the text writes the matching endpoint set command, the third instance gets no finding, the other daemon checks stay OK, and the listeners and the lifecycles are unchanged",
    );
    let root = TestRoot::new().expect("a test root");
    let runtime = seed(&root);
    let _reap = ReapGuard::new(root.path(), &paths(&root).runtime);
    let daemon = start_daemon(&root);
    let held_port = allocate_port().expect("a free port");
    let crashy_port = allocate_port().expect("a second free port");
    let spare_port = allocate_port().expect("a third free port");
    create(&root, &runtime, "held", held_port, &[]);
    create(
        &root,
        &runtime,
        "crashy",
        crashy_port,
        &["crashAfterMs=50", "crashExitCode=3", "readyDelayMs=60000"],
    );
    create(&root, &runtime, "spare", spare_port, &[]);
    // Create checks the port too, so the test takes it only afterwards.
    let held = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, held_port))
        .expect("the test holds the first port");
    let refused = run(
        root.path(),
        &["instance", "start", "held", "--wait", "--timeout", "60s"],
    );
    assert_eq!(refused.code, Some(3), "{refused:?}");

    let warned = doctor(&root, &["--json"]);

    assert_eq!(warned.code, Some(0), "{warned:?}");
    let value = envelope(&warned);
    assert_eq!(check(&value, "endpoints")["severity"], "warning");
    let finding = port_finding(&value, "held");
    assert_eq!(finding["severity"], "warning");
    owned_by_this_test(&finding, held_port);
    assert_eq!(value["result"]["report"]["errors"], 0);

    // The real restart constants pause 0.5 to 31 seconds in all.
    let crashed = run(
        root.path(),
        &["instance", "start", "crashy", "--wait", "--timeout", "120s"],
    );
    assert_eq!(crashed.code, Some(7), "{crashed:?}");
    let crashy_listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, crashy_port))
        .expect("the test holds the second port");
    let before = (lifecycle(&root, "held"), lifecycle(&root, "crashy"));
    assert_eq!(before.1, "failed");

    let json = doctor(&root, &["--json"]);
    let text = doctor(&root, &[]);

    assert_eq!(json.code, Some(7), "{json:?}");
    let value = envelope(&json);
    assert_eq!(value["error"]["code"], "cli.doctor_found_errors");
    assert_eq!(check(&value, "endpoints")["severity"], "error");
    let crashy = port_finding(&value, "crashy");
    assert_eq!(crashy["severity"], "error");
    let suggested = owned_by_this_test(&crashy, crashy_port);
    assert_eq!(port_finding(&value, "held")["severity"], "warning");
    let failures = check(&value, "instance_failures");
    assert_eq!(
        failures["findings"][0]["code"], "start_failed",
        "{failures}"
    );
    assert_eq!(failures["findings"][0]["instance"]["name"], "crashy");
    for id in [
        "daemon",
        "registry",
        "permissions",
        "processes",
        "operations",
    ] {
        assert_eq!(check(&value, id)["severity"], "ok", "{}", check(&value, id));
    }
    assert!(
        !findings(&value)
            .iter()
            .any(|finding| finding["instance"]["name"] == "spare"),
        "the spare instance has no finding: {value}"
    );

    assert_eq!(text.code, Some(7), "{text:?}");
    assert!(
        text.stdout.contains(&format!(
            "Next: stackly instance endpoint set crashy --port http={suggested}\n"
        )),
        "{}",
        text.stdout
    );
    assert!(
        text.stdout
            .contains("Next: stackly instance inspect crashy\n"),
        "{}",
        text.stdout
    );
    assert_eq!(
        (lifecycle(&root, "held"), lifecycle(&root, "crashy")),
        before
    );
    for port in [held_port, crashy_port] {
        assert!(
            std::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port)).is_ok(),
            "the test still holds port {port}"
        );
    }
    drop((held, crashy_listener));
    assert_eq!(daemon.stop().unwrap().code(), Some(0));
    ev.pass();
}

fn chmod(path: &Path, mode: u32) {
    std::fs::set_permissions(path, Permissions::from_mode(mode)).expect("the mode can be set");
}

/// The `background_agent` check of a doctor envelope.
fn agent_check(value: &Value) -> &Value {
    &value["result"]["local"][2]
}

/// The finding codes of the `background_agent` check.
fn agent_codes(run: &crate::common::Run) -> Vec<String> {
    let value = envelope(run);
    agent_check(&value)["findings"]
        .as_array()
        .expect("findings")
        .iter()
        .map(|finding| {
            finding["code"]
                .as_str()
                .expect("a finding has a code")
                .to_owned()
        })
        .collect()
}

/// Spec 0034 AC-11 against a real `stacklyd`.
#[test]
fn a_failed_daemon_start_shows_while_no_daemon_answers_and_only_from_a_private_record() {
    let ev = evidence!(
        covers = ["REQ-019", "REQ-028"],
        kind = Kind::Fixture,
        expected = "after stacklyd exits 1 on a STACKLY_ROOT with mode 0777 and writes its start failure record, stackly doctor on the fixed root exits 6 with ipc.endpoint_missing and a daemon_start_failed Error with the time, code, and refusal; the same record with mode 0644 or behind a symlink shows nothing; and once a daemon starts on the root the record is gone and the doctor shows no start failure",
    );
    let root = TestRoot::new().expect("a test root");
    let data = root.path().join("data");
    std::fs::create_dir(&data).unwrap();
    chmod(&data, 0o700);
    chmod(root.path(), 0o777);
    let failed = Command::new(cargo_bin("stacklyd", "stacklyd"))
        .env("STACKLY_ROOT", root.path())
        .env_remove("STACKLY_LOG")
        .env_remove("STACKLY_CATALOG_DIR")
        .env_remove("XPC_SERVICE_NAME")
        .output()
        .expect("stacklyd runs");
    assert_eq!(failed.status.code(), Some(1), "{failed:?}");
    chmod(root.path(), 0o700);
    let record = data.join("daemon-start-failure.json");
    assert!(record.is_file(), "stacklyd wrote its record");

    let json = run(root.path(), &["doctor", "--json"]);
    let text = run(root.path(), &["doctor"]);

    assert_eq!(json.code, Some(6), "{json:?}");
    let value = envelope(&json);
    assert_eq!(value["error"]["code"], "ipc.endpoint_missing");
    assert_eq!(agent_check(&value)["severity"], "error");
    let finding = &agent_check(&value)["findings"][0];
    assert_eq!(finding["code"], "daemon_start_failed");
    assert_eq!(
        detail(finding, "code").as_deref(),
        Some("daemon.start_failed")
    );
    assert!(
        detail(finding, "message")
            .is_some_and(|message| message.contains("writable by group or others")),
        "{finding}"
    );
    assert!(detail(finding, "at").is_some(), "{finding}");
    assert_eq!(text.code, Some(6), "{text:?}");
    assert!(
        text.stdout.contains("ERROR   stacklyd failed to start\n"),
        "{}",
        text.stdout
    );

    chmod(&record, 0o644);
    let loose = run(root.path(), &["doctor", "--json"]);
    chmod(&record, 0o600);
    let moved = data.join("elsewhere.json");
    std::fs::rename(&record, &moved).unwrap();
    std::os::unix::fs::symlink(&moved, &record).unwrap();
    let linked = run(root.path(), &["doctor", "--json"]);
    std::fs::remove_file(&record).unwrap();
    std::fs::rename(&moved, &record).unwrap();

    assert_eq!(loose.code, Some(6), "{loose:?}");
    assert!(agent_codes(&loose).is_empty(), "{loose:?}");
    assert_eq!(linked.code, Some(6), "{linked:?}");
    assert!(agent_codes(&linked).is_empty(), "{linked:?}");

    let daemon = start_daemon(&root);
    let after = doctor(&root, &["--json"]);

    assert!(
        !record.exists(),
        "the start that bound its socket removed the record"
    );
    assert!(agent_codes(&after).is_empty(), "{after:?}");
    assert_eq!(after.code, Some(0), "{after:?}");
    assert_eq!(daemon.stop().unwrap().code(), Some(0));
    ev.pass();
}
