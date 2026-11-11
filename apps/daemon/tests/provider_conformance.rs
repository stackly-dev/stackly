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
    let table = ProcessTable::snapshot().expect("ps runs");
    let left = table.containing_all(&["stackly-test-", conformance::site::ROOT_NAME]);
    assert!(left.is_empty(), "processes outlived the run: {left:?}");
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
