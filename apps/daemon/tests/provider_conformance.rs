//! The provider conformance harness on real packages (spec 0036, AC-9 to
//! AC-13): each test runs `conformance::run` on a package folder, with a
//! daemon in this process, and reads the report. They run one at a time
//! behind one static mutex, because each starts a daemon and measures
//! time. Every test is a fixture run on `stackly-fake-server`; the native
//! tests per runtime line come with step 14.
#![cfg(target_os = "macos")]

#[allow(
    dead_code,
    reason = "shared by the provider_harness example and the provider_conformance test"
)]
mod conformance;

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use conformance::{Report, RunRequest, StepStatus, Verdict};
use stackly_core::provider::scenarios::{Expect, Scenario};
use stackly_testkit::evidence::{Kind, evidence};
use stackly_testkit::{ProcessTable, TestRoot};

/// One run at a time.
static SERIAL: Mutex<()> = Mutex::new(());

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the repo root resolves")
}

/// Runs the harness on `request`, with its report in a fresh test root.
fn run_with(mut request: RunRequest) -> (Report, TestRoot) {
    let _serial = SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let out = TestRoot::new().expect("a report folder can be created");
    request.report_path = out.path().join("report.json");
    let report = conformance::run(&request);
    (report, out)
}

/// Runs the harness on the package folder `dir` with its own scenarios.
fn run_package(dir: &Path) -> (Report, TestRoot) {
    run_with(RunRequest::new(dir.to_path_buf(), PathBuf::new()))
}

/// The status of `step` in scenario `scenario`.
fn status(report: &Report, scenario: &str, step: &str) -> Option<StepStatus> {
    report
        .scenario(scenario)
        .and_then(|result| result.step(step))
        .map(|step| step.status)
}

/// Asserts the report passed, with the report on failure.
fn assert_pass(report: &Report) {
    assert_eq!(
        report.verdict(),
        Verdict::Pass,
        "{}",
        serde_json::to_string_pretty(report.value().expect("the report was built"))
            .expect("a report encodes")
    );
}

/// Asserts nothing of the run is left: no process names the run's root
/// folders and the report says the root was removed.
fn assert_clean(report: &Report) {
    let value = report.value().expect("the report was built");
    assert_eq!(value["cleanup"]["rootRemoved"], true, "{value:#}");
    assert_eq!(
        value["cleanup"]["processesLeft"],
        serde_json::json!([]),
        "{value:#}"
    );
    assert_no_process_left();
}

/// The steps of a passing `ready` scenario, in the order of AC-9.
const ORDER: [&str; 16] = [
    "catalog",
    "runtime",
    "form",
    "create",
    "start",
    "health",
    "connection",
    "process",
    "logs",
    "second_instance",
    "reload",
    "restart",
    "handover",
    "stop",
    "leftovers",
    "start_again",
];

// covers: AC-9, AC-13 (spec 0036)
#[test]
fn the_example_package_passes_conformance() {
    let ev = evidence!(
        covers = ["AC-029", "AC-034", "REQ-029"],
        kind = Kind::Fixture,
        expected = "conformance::run on providers/example runs default and slow-ready through catalog to start_again over the socket with a daemon in process, writes a stackly.provider-conformance/v1 report with verdict pass, and leaves no process and no root",
    );
    let (report, _out) = run_package(&repo().join("providers/example"));

    assert_pass(&report);
    let value = report.value().expect("the report was built");
    assert_eq!(value["schema"], "stackly.provider-conformance/v1");
    assert_eq!(value["kind"], "fixture");
    assert_eq!(value["package"]["provider"], "testing/example");
    assert_eq!(value["runtime"]["source"], "fixture");
    assert_eq!(value["handover"], "unproven");
    let written: serde_json::Value = serde_json::from_slice(
        &std::fs::read(report.written().expect("the report was written"))
            .expect("the report reads"),
    )
    .expect("the report is JSON");
    assert_eq!(&written, value);
    for scenario in ["default", "slow-ready"] {
        let steps: Vec<&str> = report
            .scenario(scenario)
            .unwrap_or_else(|| panic!("scenario {scenario} ran"))
            .steps
            .iter()
            .map(|step| step.name)
            .collect();
        assert_eq!(steps, ORDER, "{scenario}");
    }
    let text = serde_json::to_string(value).expect("a report encodes");
    assert!(
        !text.contains(&std::env::var("HOME").expect("HOME is set")),
        "{text}"
    );
    assert!(
        !text.contains("hello"),
        "a settings text value reached the report"
    );
    assert_clean(&report);
    ev.pass();
}

// covers: AC-9 (spec 0036)
#[test]
fn the_fake_package_passes_conformance() {
    let ev = evidence!(
        covers = ["AC-029"],
        kind = Kind::Fixture,
        expected = "conformance::run on providers/fake runs its implied default scenario through every step and passes, the connection step included",
    );
    let (report, _out) = run_package(&repo().join("providers/fake"));

    assert_pass(&report);
    assert_eq!(
        status(&report, "default", "connection"),
        Some(StepStatus::Pass)
    );
    assert_eq!(status(&report, "default", "reload"), Some(StepStatus::Pass));
    assert_eq!(
        status(&report, "default", "start_again"),
        Some(StepStatus::Pass)
    );
    assert_clean(&report);
    ev.pass();
}

/// A scenario for `RunRequest.scenarios`.
fn scenario(name: &str, settings: serde_json::Value, expect: Expect) -> Scenario {
    Scenario::new(
        name.parse().expect("a valid scenario name"),
        settings,
        expect,
    )
}

/// Runs `package_dir` with the one scenario `scenario`.
fn run_scenario(package_dir: &Path, scenario: Scenario) -> (Report, TestRoot) {
    let mut request = RunRequest::new(package_dir.to_path_buf(), PathBuf::new());
    request.scenarios = Some(vec![scenario]);
    run_with(request)
}

/// The rogue fixture package of spec 0036 (*Harness*).
fn rogue_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/conformance/packages/fake-rogue")
}

/// The failure of scenario `name` as the report wrote it.
fn failure(report: &Report, name: &str) -> serde_json::Value {
    let value = report.value().expect("the report was built");
    value["scenarios"]
        .as_array()
        .and_then(|scenarios| scenarios.iter().find(|scenario| scenario["name"] == name))
        .map(|scenario| scenario["failure"].clone())
        .unwrap_or(serde_json::Value::Null)
}

/// Asserts that a failed run still removed its root and left no process.
fn assert_failed_run_cleaned_up(report: &Report) {
    let value = report.value().expect("the report was built");
    assert_eq!(value["cleanup"]["rootRemoved"], true, "{value:#}");
    assert_no_process_left();
}

/// Asserts that no process of the run is left on this Mac.
fn assert_no_process_left() {
    let table = ProcessTable::snapshot().expect("ps runs");
    // `ps` escapes the non ASCII letters of the data root name, so the
    // search uses its ASCII parts: a process whose program lives in a test
    // root, the runtime folder or the copy a rogue server made.
    let left: Vec<_> = table
        .entries()
        .iter()
        .filter(|entry| {
            entry.args.find("/Test Root ").is_some_and(|at| {
                let program_root = &entry.args[..at];
                program_root.contains("/stackly-test-") && !program_root.contains(' ')
            })
        })
        .collect();
    assert!(left.is_empty(), "processes outlived the run: {left:?}");
}

// covers: AC-10 (spec 0036)
#[test]
fn a_server_that_closes_fd_3_fails_the_process_contract() {
    let ev = evidence!(
        covers = ["REQ-013"],
        kind = Kind::Fixture,
        expected = "testing/fake-rogue with closeLockFd fails the process step with lock_not_held, and the harness leaves no process behind",
    );
    let (report, _out) = run_scenario(
        &rogue_dir(),
        scenario(
            "close-fd",
            serde_json::json!({ "closeLockFd": true }),
            Expect::Ready,
        ),
    );

    assert_eq!(report.verdict(), Verdict::Fail);
    let failure = failure(&report, "close-fd");
    assert_eq!(failure["step"], "process", "{failure:#}");
    assert_eq!(failure["reason"], "lock_not_held", "{failure:#}");
    assert_failed_run_cleaned_up(&report);
    ev.pass();
}

// covers: AC-10 (spec 0036)
#[test]
fn a_server_that_re_execs_fails_the_process_contract() {
    let ev = evidence!(
        covers = ["REQ-013"],
        kind = Kind::Fixture,
        expected = "testing/fake-rogue with reExec fails the process step with re_exec, and the harness leaves no process behind",
    );
    let (report, _out) = run_scenario(
        &rogue_dir(),
        scenario(
            "re-exec",
            serde_json::json!({ "reExec": true }),
            Expect::Ready,
        ),
    );

    assert_eq!(report.verdict(), Verdict::Fail);
    let failure = failure(&report, "re-exec");
    assert_eq!(failure["step"], "process", "{failure:#}");
    assert_eq!(failure["reason"], "re_exec", "{failure:#}");
    assert_failed_run_cleaned_up(&report);
    ev.pass();
}

// covers: AC-10 (spec 0036)
#[test]
fn a_server_that_goes_to_the_background_fails_start_and_leaves_nothing_behind() {
    let ev = evidence!(
        covers = ["REQ-013"],
        kind = Kind::Fixture,
        expected = "testing/fake-rogue with background fails at start, leftovers records lock_still_held and process_left for the child it left, and the harness ends that child",
    );
    let (report, _out) = run_scenario(
        &rogue_dir(),
        scenario(
            "background",
            serde_json::json!({ "background": true }),
            Expect::Ready,
        ),
    );

    assert_eq!(report.verdict(), Verdict::Fail);
    let failure = failure(&report, "background");
    assert_eq!(failure["step"], "start", "{failure:#}");
    let result = report.scenario("background").expect("the scenario ran");
    let leftovers = result
        .step("leftovers")
        .expect("leftovers ran after the failed start");
    assert_eq!(leftovers.status, StepStatus::Fail);
    let reasons = &leftovers
        .detail
        .as_ref()
        .expect("leftovers notes its reasons")["reasons"];
    assert_eq!(
        reasons,
        &serde_json::json!(["lock_still_held", "process_left"]),
        "{reasons:#}"
    );
    let left = &report.value().expect("the report was built")["cleanup"]["processesLeft"];
    assert!(
        left.as_array().is_some_and(|rows| rows
            .iter()
            .any(|row| row["command"] == "stackly-fake-server")),
        "the child the server left is noted before it is ended: {left:#}"
    );
    assert_failed_run_cleaned_up(&report);
    ev.pass();
}
