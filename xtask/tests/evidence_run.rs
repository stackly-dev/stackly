//! End to end tests of `cargo xtask evidence run` (spec 0002, "Perintah
//! xtask"): the real wrapper binary runs a small command in a fresh run
//! folder inside a test root, without the evidence variables of an outer
//! run.

use std::fs;
use std::path::Path;
use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use stackly_evidence::{
    EVIDENCE_DIR_ENV, EVIDENCE_KIND_ENV, FIXTURE_CREDENTIAL_PREFIX, Kind, evidence,
};
use stackly_testkit::TestRoot;

/// Set on the child that `a_test_guard_under_the_wrapper_becomes_a_report_record`
/// starts, so `child_test_for_the_end_to_end_run` does its work only there.
const CHILD_ENV: &str = "XTASK_EVIDENCE_RUN_CHILD";
/// The name of the test the end to end run starts in a child process.
const CHILD_TEST: &str = "child_test_for_the_end_to_end_run";

fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask sits one directory below the repository root")
}

/// `cargo xtask evidence run --kind <kind> --out <out> -- <command>`, started
/// from the repo root with no evidence variables inherited from an outer run.
fn wrapper(kind: &str, out: &Path, command: &[&str]) -> Command {
    let mut wrapper = Command::new(env!("CARGO_BIN_EXE_xtask"));
    wrapper
        .current_dir(repo_root())
        .env_remove(EVIDENCE_DIR_ENV)
        .env_remove(EVIDENCE_KIND_ENV)
        .env_remove(CHILD_ENV)
        .args(["evidence", "run", "--kind", kind, "--out"])
        .arg(out)
        .arg("--")
        .args(command);
    wrapper
}

fn run(kind: &str, out: &Path, command: &[&str]) -> Output {
    wrapper(kind, out, command)
        .output()
        .expect("the xtask binary runs")
}

fn report(out: &Path) -> Value {
    let bytes = fs::read(out.join("report.json")).expect("the wrapper wrote report.json");
    serde_json::from_slice(&bytes).expect("report.json is JSON")
}

fn output_log(out: &Path) -> String {
    fs::read_to_string(out.join("output.log")).expect("the wrapper wrote output.log")
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// Kills and reaps the wrapper if a test ends before it does.
struct Reaped(Child);

impl Drop for Reaped {
    fn drop(&mut self) {
        if matches!(self.0.try_wait(), Ok(None)) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

/// Waits until `done` holds, or panics with `what` after 30 s.
fn wait_for(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn a_run_hands_the_command_its_folder_and_kind_and_names_the_report_last() {
    let root = TestRoot::new().unwrap();
    let out = root.path().join("run");

    let output = run(
        "fixture",
        &out,
        &[
            "sh",
            "-c",
            "echo \"dir=$STACKLY_EVIDENCE_DIR kind=$STACKLY_EVIDENCE_KIND\"",
        ],
    );

    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert!(output_log(&out).contains(&format!("dir={} kind=fixture", out.display())));
    for entry in ["run.json", "records", "artifacts", "report.json"] {
        assert!(out.join(entry).exists(), "{entry} is missing");
    }
    assert_eq!(
        stderr(&output).lines().last(),
        Some(format!("evidence report: {}", out.join("report.json").display()).as_str())
    );
    let report = report(&out);
    assert_eq!(report["schema"], "stackly.evidence-report/v1");
    assert_eq!(report["kind"], "fixture");
    assert_eq!(report["context"]["runKind"], "fixture");
    assert_eq!(report["context"]["runId"], report["runId"]);
    assert_eq!(report["context"]["command"][0], "sh");
    assert_eq!(report["commandExitCode"], 0);
    assert_eq!(report["interrupted"], false);
    assert_eq!(report["problems"], json!([]));
    assert_eq!(report["records"], json!([]));
}

#[test]
fn the_exit_code_is_the_command_s_then_one_for_problems_then_zero() {
    let root = TestRoot::new().unwrap();

    let failed = root.path().join("failed");
    let output = run("fixture", &failed, &["sh", "-c", "exit 7"]);
    assert_eq!(output.status.code(), Some(7), "{}", stderr(&output));
    assert_eq!(report(&failed)["commandExitCode"], 7);

    // The prefix is printed in two halves, so it is whole only in the output.
    let (head, tail) = FIXTURE_CREDENTIAL_PREFIX.split_at(8);
    let leaked = root.path().join("leaked");
    let output = run("fixture", &leaked, &["printf", "%s%s\\n", head, tail]);
    assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
    let report = report(&leaked);
    assert_eq!(report["commandExitCode"], 0);
    assert_eq!(
        report["problems"],
        json!([{
            "code": "credential_leak",
            "recordId": null,
            "message": "fixture credential prefix found in output.log",
        }])
    );

    let clean = root.path().join("clean");
    let output = run("native", &clean, &["true"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
}

#[test]
fn a_run_inside_a_run_is_refused_before_it_creates_anything() {
    let root = TestRoot::new().unwrap();
    let out = root.path().join("nested");

    let output = wrapper("fixture", &out, &["true"])
        .env(EVIDENCE_DIR_ENV, root.path())
        .output()
        .expect("the xtask binary runs");

    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("evidence runs never nest"),
        "{}",
        stderr(&output)
    );
    assert!(!out.exists());
}

#[cfg(unix)]
#[test]
fn sigint_reaches_the_command_as_sigterm_and_the_report_says_interrupted() {
    let root = TestRoot::new().unwrap();
    let out = root.path().join("interrupted");
    let script = "trap 'kill $child; echo got-term; exit 143' TERM; \
                  sleep 30 & child=$!; echo ready; wait";
    let mut wrapper = Reaped(
        wrapper("fixture", &out, &["sh", "-c", script])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("the xtask binary starts"),
    );
    wait_for("the command to start", || {
        fs::read_to_string(out.join("output.log")).is_ok_and(|log| log.contains("ready"))
    });

    let sent = Command::new("kill")
        .args(["-INT", &wrapper.0.id().to_string()])
        .status()
        .expect("kill runs");
    assert!(sent.success());
    let mut status = None;
    wait_for("the wrapper to exit", || {
        status = wrapper.0.try_wait().expect("the wrapper can be waited on");
        status.is_some()
    });

    assert_eq!(status.and_then(|status| status.code()), Some(143));
    assert!(output_log(&out).contains("got-term"));
    let report = report(&out);
    assert_eq!(report["interrupted"], true);
    assert_eq!(report["commandExitCode"], 143);
    assert_eq!(report["commandSignal"], Value::Null);
}

#[cfg(unix)]
#[test]
fn a_command_killed_by_a_signal_exits_128_plus_the_signal() {
    let root = TestRoot::new().unwrap();
    let out = root.path().join("killed");

    let output = run("fixture", &out, &["sh", "-c", "kill -9 $$"]);

    assert_eq!(output.status.code(), Some(128 + 9), "{}", stderr(&output));
    let report = report(&out);
    assert_eq!(report["commandExitCode"], Value::Null);
    assert_eq!(report["commandSignal"], 9);
    assert_eq!(report["interrupted"], false);
}

#[test]
fn a_test_guard_under_the_wrapper_becomes_a_report_record() {
    let ev = evidence!(
        covers = ["REQ-051"],
        kind = Kind::Fixture,
        expected = "a Rust test run through cargo xtask evidence run leaves one report record with its status, the run context, and a repo relative file",
    );
    let root = TestRoot::new().unwrap();
    let out = root.path().join("guarded");
    let this_test_binary = std::env::current_exe().unwrap();

    let output = wrapper(
        "fixture",
        &out,
        &[this_test_binary.to_str().unwrap(), "--exact", CHILD_TEST],
    )
    .env(CHILD_ENV, "1")
    .output()
    .expect("the xtask binary runs");

    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let report = report(&out);
    assert_eq!(report["problems"], json!([]));
    assert_eq!(report["summary"]["total"], 1);
    let record = &report["records"][0];
    assert_eq!(record["status"], "PASS");
    assert_eq!(record["kind"], "fixture");
    assert_eq!(record["method"], "automated");
    assert_eq!(record["covers"], json!(["REQ-051"]));
    assert_eq!(record["actual"], "the child test ran under the wrapper");
    assert_eq!(record["evidencePath"], "output.log");
    assert_eq!(
        record["test"],
        json!({
            "language": "rust",
            "package": "xtask",
            "target": "evidence_run",
            "name": CHILD_TEST,
            "file": "xtask/tests/evidence_run.rs",
            "line": record["test"]["line"],
        })
    );
    assert!(record["finishedAt"].is_string());
    assert_eq!(record["context"], report["context"]);
    ev.pass();
}

/// The child test `a_test_guard_under_the_wrapper_becomes_a_report_record`
/// starts under the wrapper. In a normal run it does nothing, so it writes
/// no record of its own.
#[test]
fn child_test_for_the_end_to_end_run() {
    if std::env::var_os(CHILD_ENV).is_none() {
        return;
    }
    let ev = evidence!(
        covers = ["REQ-051"],
        kind = Kind::Fixture,
        expected = "the child test passes under the wrapper",
    );
    ev.pass_with("the child test ran under the wrapper");
}
