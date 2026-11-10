//! AC-15 and AC-18 against a scripted daemon: the goldens, the exit codes,
//! the params, and the escaping.

use std::path::PathBuf;

use serde_json::{Value, json};
use stackly_protocol::codes::{protocol, request};
use stackly_protocol::doctor::{DoctorCheck, DoctorCheckId, DoctorReport, DoctorSeverity};
use stackly_protocol::instance::{InstancePage, InstanceSummary};
use stackly_protocol::{Call, Count, ErrorBody, ProtocolVersion, Reply};
use stackly_testkit::TestRoot;
use stackly_testkit::evidence::{Kind, evidence};

use crate::common::scripted::{Handshake, ScriptedDaemon, Step};
use crate::common::{Run, detail, envelope, golden, listing, paths, run, stackly};

/// The instance of the fixture's process and start findings.
const PG_MAIN: &str = "0192f3a4-0000-7000-8000-0000000000b2";

/// The reply content of the fixture frame `name`.
fn fixture(name: &str) -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../protocols/fixtures/ipc")
        .join(name);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("fixture {} reads: {error}", path.display()));
    let frame: Value = serde_json::from_str(&text).expect("a fixture is JSON");
    frame["content"]["reply"]["content"].clone()
}

/// The report of `frame-response-doctor-run.json`: Errors, Warnings, and
/// OK checks, a remedy, evidence, and an excerpt.
fn report() -> DoctorReport {
    serde_json::from_value(fixture("frame-response-doctor-run.json"))
        .expect("the doctor fixture decodes")
}

/// The fixture report with only its checks of `kept` severities, and the
/// counts that go with them.
fn report_with(kept: &[DoctorSeverity]) -> DoctorReport {
    let mut report = report();
    report.checks.retain(|check| kept.contains(&check.severity));
    let count = |severity: DoctorSeverity| {
        let n = report
            .checks
            .iter()
            .flat_map(|check| &check.findings)
            .filter(|finding| finding.severity == severity)
            .count();
        Count::saturating(n as u64)
    };
    report.errors = count(DoctorSeverity::Error);
    report.warnings = count(DoctorSeverity::Warning);
    report
}

/// The fixture instance `pg-main`.
fn pg_main() -> InstanceSummary {
    let mut value = fixture("frame-response-instance-get.json");
    value["id"] = PG_MAIN.into();
    value["name"] = "pg-main".into();
    serde_json::from_value(value).expect("a fixture instance decodes")
}

/// A daemon that lists `pg-main` and answers `doctor.run` with `report`.
fn daemon(root: &TestRoot, report: DoctorReport) -> ScriptedDaemon {
    let instance = pg_main();
    ScriptedDaemon::serve(root, move |call, _| match call {
        Call::InstanceList(_) => vec![Step::Reply(Reply::InstanceList(Box::new(InstancePage {
            items: vec![instance.clone()],
            next: None,
        })))],
        Call::DoctorRun(_) => vec![Step::Reply(Reply::DoctorRun(Box::new(report.clone())))],
        _ => Vec::new(),
    })
}

/// The params of every `doctor.run` the daemon got, in order.
fn sent(daemon: &ScriptedDaemon) -> Vec<Value> {
    daemon
        .log()
        .into_iter()
        .filter(|logged| logged.method == "doctor.run")
        .map(|logged| logged.raw["call"]["content"].clone())
        .collect()
}

/// `text` with this build's protocol in the protocol check's summary
/// written as a placeholder, so a protocol bump does not move the goldens.
fn normalized(text: &str) -> String {
    let protocol = ProtocolVersion::CURRENT;
    text.replace(
        &format!(
            "App and daemon both speak protocol {}.{}",
            protocol.major, protocol.minor
        ),
        "App and daemon both speak protocol <major>.<minor>",
    )
}

/// The `stackly doctor` text of `run`, with the socket path of `root`
/// written as `<socket>`.
fn text_of(root: &TestRoot, run: &Run) -> String {
    let socket = paths(root).socket.display().to_string();
    normalized(&run.stdout).replace(&socket, "<socket>")
}

/// The ids of the client checks and their severities.
fn local(value: &Value) -> Vec<(String, String)> {
    value["result"]["local"]
        .as_array()
        .expect("the client checks")
        .iter()
        .map(|check| {
            (
                check["id"].as_str().expect("a check has an id").to_owned(),
                check["severity"]
                    .as_str()
                    .expect("a check has a severity")
                    .to_owned(),
            )
        })
        .collect()
}

/// Spec 0034 AC-15, AC-18.
#[test]
fn a_report_with_errors_is_written_by_severity_with_its_commands_and_exits_7() {
    let ev = evidence!(
        covers = ["REQ-019", "REQ-028", "AC-028"],
        kind = Kind::Fixture,
        expected = "stackly doctor against a scripted daemon that answers the doctor.run fixture writes the client checks, then the daemon checks Errors first with evidence, excerpt, remedy, and one Next command per safe action, then the count line; --json holds local and the report as the daemon sent it; both exit 7 with cli.doctor_found_errors counting 3 errors and 5 warnings, send one doctor.run with no instance and no checks, and create nothing",
    );
    let root = TestRoot::new().unwrap();
    let daemon = daemon(&root, report());
    let before = listing(root.path());

    let text = run(root.path(), &["doctor"]);
    let json = run(root.path(), &["doctor", "--json"]);

    assert_eq!(text.code, Some(7), "{text:?}");
    golden::check("doctor.txt", &text_of(&root, &text));
    assert_eq!(
        text.stderr,
        "Running the doctor…\nerror: the doctor found errors\n  code: cli.doctor_found_errors\n  hint: Each error above says what to do next.\n"
    );
    for command in [
        "Next: stackly instance endpoint set mysql-legacy --port mysql=3308",
        "Next: stackly instance inspect pg-main",
        "Next: stackly storage locations",
    ] {
        assert!(text.stdout.contains(command), "{command}:\n{}", text.stdout);
    }

    assert_eq!(json.code, Some(7), "{json:?}");
    let value = envelope(&json);
    assert_eq!(value["command"], "doctor");
    assert_eq!(value["error"]["code"], "cli.doctor_found_errors");
    assert_eq!(detail(&value["error"], "errors").as_deref(), Some("3"));
    assert_eq!(detail(&value["error"], "warnings").as_deref(), Some("5"));
    assert_eq!(
        local(&value),
        [
            ("connection".to_owned(), "ok".to_owned()),
            ("protocol".to_owned(), "ok".to_owned()),
            ("background_agent".to_owned(), "ok".to_owned()),
        ]
    );
    assert_eq!(
        value["result"]["report"],
        fixture("frame-response-doctor-run.json")
    );
    golden::check("doctor.json", &normalized(&json.stdout));
    assert_eq!(json.stderr, "Running the doctor…\n");

    assert_eq!(
        sent(&daemon),
        [
            json!({ "instanceId": null, "checks": [] }),
            json!({ "instanceId": null, "checks": [] })
        ]
    );
    assert_eq!(listing(root.path()), before, "the doctor created a path");
    ev.pass();
}

/// Spec 0034 AC-15: Warnings alone are exit 0.
#[test]
fn warnings_alone_exit_0_and_a_report_without_findings_says_no_issues() {
    let root = TestRoot::new().unwrap();
    let warnings = daemon(
        &root,
        report_with(&[DoctorSeverity::Warning, DoctorSeverity::Ok]),
    );

    let text = run(root.path(), &["doctor"]);
    let json = run(root.path(), &["doctor", "--json"]);
    drop(warnings);
    let clean = daemon(&root, report_with(&[DoctorSeverity::Ok]));
    let ok = run(root.path(), &["doctor"]);
    drop(clean);

    assert_eq!(text.code, Some(0), "{text:?}");
    assert!(
        text.stdout.ends_with(
            "\n0 errors, 3 warnings · 9 checks · run 0192f3a7-0000-7000-8000-0000000000e1\n"
        ),
        "{}",
        text.stdout
    );
    assert_eq!(text.stderr, "Running the doctor…\n");
    let value = envelope(&json);
    assert_eq!(value["error"], Value::Null);
    assert_eq!(value["result"]["report"]["warnings"], 3);
    assert_eq!(ok.code, Some(0), "{ok:?}");
    golden::check("doctor_ok.txt", &text_of(&root, &ok));
}

/// Spec 0034 AC-15, spec 0022 AC-11: `--instance` takes a name or a whole
/// ID through `InstanceTarget::resolve`.
#[test]
fn an_instance_name_is_sent_as_its_id_and_an_unknown_name_exits_10_without_a_run() {
    let root = TestRoot::new().unwrap();
    let daemon = daemon(&root, report());

    let by_name = run(root.path(), &["doctor", "--instance", "pg-main", "--json"]);
    let by_id = run(
        root.path(),
        &["doctor", "--instance", &PG_MAIN.to_uppercase(), "--json"],
    );
    let runs = sent(&daemon).len();
    let missing = run(root.path(), &["doctor", "--instance", "gamma", "--json"]);

    assert_eq!(by_name.code, Some(7), "{by_name:?}");
    assert_eq!(by_id.code, Some(7), "{by_id:?}");
    assert_eq!(
        sent(&daemon),
        [
            json!({ "instanceId": PG_MAIN, "checks": [] }),
            json!({ "instanceId": PG_MAIN, "checks": [] })
        ]
    );
    assert_eq!(runs, 2);
    assert_eq!(missing.code, Some(10), "{missing:?}");
    let value = envelope(&missing);
    assert_eq!(value["error"]["code"], "cli.instance_not_found");
    assert_eq!(detail(&value["error"], "name").as_deref(), Some("gamma"));
    assert_eq!(local(&value).len(), 3);
    assert_eq!(value["result"]["report"], Value::Null);
}

/// Spec 0034 AC-15: an `--instance` that names no instance ends with
/// `cli.instance_not_found` and exit 10 after the client checks, and the
/// text writes no summary line, so nothing reads as all clear for the
/// daemon checks that never ran.
#[test]
fn an_unknown_instance_writes_the_client_checks_without_an_all_clear_line() {
    let root = TestRoot::new().unwrap();
    let daemon = daemon(&root, report());

    let missing = run(root.path(), &["doctor", "--instance", "gamma"]);

    assert_eq!(missing.code, Some(10), "{missing:?}");
    assert!(sent(&daemon).is_empty(), "no doctor.run is sent");
    let text = text_of(&root, &missing);
    assert!(!text.contains("No issues"), "{text}");
    assert!(!text.contains(" checks"), "{text}");
    golden::check("doctor_unknown_instance.txt", &text);
    assert!(
        missing.stderr.contains("code: cli.instance_not_found"),
        "{}",
        missing.stderr
    );
    assert!(
        missing.stderr.contains("no instance named gamma"),
        "{}",
        missing.stderr
    );
}

/// Spec 0034 AC-15: `--check` takes the daemon checks by their snake_case
/// ID and clap refuses a client check with exit 2.
#[test]
fn checks_are_sent_once_each_and_a_client_check_is_a_usage_error() {
    let root = TestRoot::new().unwrap();
    let daemon = daemon(&root, report());

    let some = run(
        root.path(),
        &[
            "doctor",
            "--check",
            "endpoints",
            "--check",
            "disk_space",
            "--check",
            "endpoints",
            "--json",
        ],
    );
    let client = run(root.path(), &["doctor", "--check", "connection", "--json"]);
    let text = run(root.path(), &["doctor", "--check", "background_agent"]);

    assert_eq!(some.code, Some(7), "{some:?}");
    assert_eq!(
        sent(&daemon),
        [json!({ "instanceId": null, "checks": ["endpoints", "disk_space"] })]
    );
    assert_eq!(client.code, Some(2), "{client:?}");
    let value = envelope(&client);
    assert_eq!(value["error"]["code"], "cli.usage");
    assert_eq!(text.code, Some(2), "{text:?}");
    assert!(text.stderr.contains("disk_space"), "{}", text.stderr);
}

/// Spec 0034 AC-15: no daemon is the connection error and its exit, with
/// the client checks in `result.local`.
#[test]
fn without_a_daemon_the_client_checks_show_and_the_connection_error_exits_6() {
    let ev = evidence!(
        covers = ["REQ-019", "REQ-028"],
        kind = Kind::Fixture,
        expected = "with no daemon, stackly doctor writes the connection check as an Error, the protocol check as not checked, and the background agent check, exits 6 with ipc.endpoint_missing, keeps result.local with report null in --json, and creates nothing",
    );
    let root = TestRoot::new().unwrap();

    let text = run(root.path(), &["doctor"]);
    let json = run(root.path(), &["doctor", "--json"]);

    assert_eq!(text.code, Some(6), "{text:?}");
    golden::check("doctor_no_daemon.txt", &text_of(&root, &text));
    assert!(
        text.stderr
            .contains("  code: ipc.endpoint_missing\n  hint: start it with stackly daemon start\n"),
        "{}",
        text.stderr
    );
    assert_eq!(json.code, Some(6), "{json:?}");
    let value = envelope(&json);
    assert_eq!(value["error"]["code"], "ipc.endpoint_missing");
    assert_eq!(
        local(&value),
        [
            ("connection".to_owned(), "error".to_owned()),
            ("protocol".to_owned(), "warning".to_owned()),
            ("background_agent".to_owned(), "ok".to_owned()),
        ]
    );
    let finding = &value["result"]["local"][0]["findings"][0];
    assert_eq!(finding["code"], "daemon_unreachable");
    assert_eq!(finding["details"][0]["value"], "ipc.endpoint_missing");
    assert_eq!(value["result"]["report"], Value::Null);
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
    ev.pass();
}

/// Spec 0034 AC-15: a daemon of another major is the handshake's error and
/// exit, which wins over the doctor's exit 7.
#[test]
fn a_daemon_of_another_major_is_the_protocol_error_and_no_run_is_sent() {
    let root = TestRoot::new().unwrap();
    let refusal = ErrorBody::new(
        &protocol::MAJOR_MISMATCH,
        "the client speaks another protocol major",
    )
    .with_detail("serverMajor", "9")
    .with_detail("serverMinor", "0");
    let daemon = ScriptedDaemon::start(
        &root,
        move |_| Handshake::Refuse(Box::new(refusal.clone())),
        |_, _| Vec::new(),
    );

    let json = run(root.path(), &["doctor", "--json"]);

    assert_eq!(json.code, Some(5), "{json:?}");
    let value = envelope(&json);
    assert_eq!(value["error"]["code"], "protocol.major_mismatch");
    assert_eq!(
        local(&value),
        [
            ("connection".to_owned(), "ok".to_owned()),
            ("protocol".to_owned(), "error".to_owned()),
            ("background_agent".to_owned(), "ok".to_owned()),
        ]
    );
    let finding = &value["result"]["local"][1]["findings"][0];
    assert_eq!(finding["code"], "protocol_incompatible");
    assert_eq!(value["result"]["report"], Value::Null);
    assert!(sent(&daemon).is_empty());
}

/// Spec 0034 AC-15: a daemon that does not know `doctor.run` keeps the
/// client checks and ends with its own error.
#[test]
fn an_older_daemon_without_the_doctor_keeps_the_client_checks_and_exits_5() {
    let root = TestRoot::new().unwrap();
    let _daemon = ScriptedDaemon::serve(&root, |call, _| match call {
        Call::DoctorRun(_) => vec![Step::Fail(
            ErrorBody::new(&request::UNKNOWN_METHOD, "unknown method doctor.run")
                .with_detail("method", "doctor.run"),
        )],
        _ => Vec::new(),
    });

    let text = run(root.path(), &["doctor"]);

    assert_eq!(text.code, Some(5), "{text:?}");
    assert!(
        text.stdout
            .contains("Connection        OK      Connected to stacklyd 0.1.0"),
        "{}",
        text.stdout
    );
    assert!(
        !text.stdout.contains("No issues"),
        "no all clear without the daemon's checks: {}",
        text.stdout
    );
    assert!(
        text.stderr
            .contains("hint: update stacklyd to a version that supports doctor.run"),
        "{}",
        text.stderr
    );
}

/// Spec 0034 *CLI*, spec 0008 color rules: marks on a colored stdout,
/// words without color, and daemon text escaped either way.
#[test]
fn color_writes_marks_no_color_writes_words_and_daemon_text_is_escaped() {
    let root = TestRoot::new().unwrap();
    let mut hostile = report();
    let check: &mut DoctorCheck = hostile
        .checks
        .iter_mut()
        .find(|check| check.id == DoctorCheckId::Endpoints)
        .expect("the endpoints check of the fixture");
    check.findings[0].title = "Port conflict \u{1b}[2J\u{202e}".to_owned();
    check.findings[0].explanation = "line\nbreak".to_owned();
    let _daemon = daemon(&root, hostile);

    let plain = run(root.path(), &["doctor"]);
    let mut colored = stackly(root.path());
    colored.args(["doctor", "--color", "always"]);
    let colored = crate::common::run_command(colored);

    assert!(
        plain
            .stdout
            .contains("ERROR   Port conflict \\u{1b}[2J\\u{202e}"),
        "{}",
        plain.stdout
    );
    assert!(plain.stdout.contains("line\\u{a}break"), "{}", plain.stdout);
    assert!(!plain.stdout.contains('\u{1b}'), "{}", plain.stdout);
    assert!(!plain.stdout.contains('✗'), "{}", plain.stdout);
    assert!(
        colored
            .stdout
            .contains("\u{1b}[31m✗\u{1b}[0m Port conflict \\u{1b}[2J"),
        "{:?}",
        colored.stdout
    );
    assert!(
        colored.stdout.contains("\u{1b}[33m!\u{1b}[0m "),
        "{:?}",
        colored.stdout
    );
    assert!(
        colored.stdout.contains("\u{1b}[32m✓\u{1b}[0m "),
        "{:?}",
        colored.stdout
    );
    assert!(!colored.stdout.contains("ERROR"), "{}", colored.stdout);
}
