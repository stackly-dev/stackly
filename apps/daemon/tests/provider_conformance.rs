//! The provider conformance harness on real packages (spec 0036, AC-9 to
//! AC-13): each test runs `conformance::run` on a package folder, with a
//! daemon in this process, and reads the report. They run one at a time
//! behind one static mutex, because each starts a daemon and measures
//! time. The fixture tests run `stackly-fake-server`; the native tests,
//! one per runtime line (AC-15), are ignored and run the managed artifact
//! from the runtime cache of `cargo xtask` with
//! `cargo test -p stacklyd --test provider_conformance -- --ignored`. An
//! artifact that is not in the cache ends such a test as `BLOCKED`, never
//! as a download.
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
use stackly_testkit::evidence::{Evidence, Kind, Source, evidence};
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
    assert_no_process_left(report);
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
    assert_no_process_left(report);
}

/// Asserts that no process names a folder of the run's test root, whose
/// ASCII names `ps` prints as they are. Other runs on this Mac, such as
/// another session's, are not this test's business.
fn assert_no_process_left(report: &Report) {
    let table = ProcessTable::snapshot().expect("ps runs");
    let left: Vec<_> = report
        .root_folders
        .iter()
        .flat_map(|folder| table.under(folder))
        .collect();
    assert!(left.is_empty(), "processes outlived the run: {left:?}");
    for folder in &report.root_folders {
        assert!(!folder.exists(), "{} is left", folder.display());
    }
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

/// `providers/fake`, the package of the fault scenarios.
fn fake_dir() -> PathBuf {
    repo().join("providers/fake")
}

/// The `detail` of `step` in scenario `scenario`.
fn step_detail(report: &Report, scenario: &str, step: &str) -> serde_json::Value {
    report
        .scenario(scenario)
        .and_then(|result| result.step(step))
        .and_then(|step| step.detail.clone())
        .unwrap_or(serde_json::Value::Null)
}

// covers: AC-11 (spec 0036)
#[test]
fn a_slow_ready_scenario_passes_within_the_readiness_limit() {
    let ev = evidence!(
        covers = ["AC-029"],
        kind = Kind::Fixture,
        expected = "providers/fake with readyDelayMs 1500 passes every step, and its start step lasts at least 1500 ms",
    );
    let (report, _out) = run_scenario(
        &fake_dir(),
        scenario(
            "slow-ready",
            serde_json::json!({ "readyDelayMs": 1500 }),
            Expect::Ready,
        ),
    );

    assert_pass(&report);
    let start = report
        .scenario("slow-ready")
        .and_then(|result| result.step("start"))
        .expect("start ran");
    assert!(
        start.duration_ms >= 1500,
        "start took {} ms",
        start.duration_ms
    );
    assert_clean(&report);
    ev.pass();
}

// covers: AC-11 (spec 0036)
#[test]
fn a_crashing_scenario_fails_at_start_unless_the_scenario_expects_it() {
    let ev = evidence!(
        covers = ["AC-029"],
        kind = Kind::Fixture,
        expected = "providers/fake that crashes 300 ms after start, before it is ready, passes with expect start_fails and fails at start with the operation error code when the scenario expects ready",
    );
    let crash = serde_json::json!({ "readyDelayMs": 60000, "crashAfterMs": 300 });

    let (expected, _out) = run_scenario(
        &fake_dir(),
        scenario("crash-expected", crash.clone(), Expect::StartFails),
    );
    let (unexpected, _out2) = run_scenario(&fake_dir(), scenario("crash", crash, Expect::Ready));

    assert_pass(&expected);
    assert_eq!(
        status(&expected, "crash-expected", "start"),
        Some(StepStatus::Pass)
    );
    assert_eq!(
        status(&expected, "crash-expected", "leftovers"),
        Some(StepStatus::Pass)
    );
    assert_eq!(
        status(&expected, "crash-expected", "health"),
        Some(StepStatus::Skipped)
    );
    assert_clean(&expected);
    assert_eq!(unexpected.verdict(), Verdict::Fail);
    let failure = failure(&unexpected, "crash");
    assert_eq!(failure["step"], "start", "{failure:#}");
    assert!(
        failure["code"].is_string(),
        "the operation error code is noted: {failure:#}"
    );
    assert_failed_run_cleaned_up(&unexpected);
    ev.pass();
}

// covers: AC-11 (spec 0036)
#[test]
fn a_log_spamming_scenario_still_completes() {
    let ev = evidence!(
        covers = ["AC-029"],
        kind = Kind::Fixture,
        expected = "providers/fake writing 2000 log lines a second passes every step, and the logs step reads a page of 1000 lines within its limit",
    );
    let (report, _out) = run_scenario(
        &fake_dir(),
        scenario(
            "spam",
            serde_json::json!({ "logLinesPerSecond": 2000 }),
            Expect::Ready,
        ),
    );

    assert_pass(&report);
    // A page holds 1000 lines or the 64 KiB of one log batch, whichever is
    // less; 100 byte lines fill the batch first.
    let logs = step_detail(&report, "spam", "logs");
    assert_eq!(logs["full"], true, "{logs:#}");
    assert!(
        logs["lines"].as_u64().is_some_and(|lines| lines >= 100),
        "{logs:#}"
    );
    assert_clean(&report);
    ev.pass();
}

/// Copies a package folder into `into/<name>` with `std::fs`, as setup.
fn copy_package(from: &Path, into: &Path, name: &str) -> PathBuf {
    let dest = into.join(name);
    let mut pending = vec![(from.to_path_buf(), dest.clone())];
    while let Some((source, target)) = pending.pop() {
        std::fs::create_dir_all(&target).expect("a folder can be created");
        for entry in std::fs::read_dir(&source).expect("the package lists") {
            let entry = entry.expect("an entry reads");
            let path = entry.path();
            if path.is_dir() {
                pending.push((path, target.join(entry.file_name())));
            } else {
                std::fs::copy(&path, target.join(entry.file_name())).expect("a file copies");
            }
        }
    }
    dest
}

/// The marked roots of this uid now, by folder.
fn marked_roots() -> Vec<PathBuf> {
    let mut found = Vec::new();
    let bases = [
        std::env::temp_dir()
            .canonicalize()
            .expect("the temporary directory resolves"),
        PathBuf::from("/private/tmp"),
    ];
    for base in bases {
        for entry in std::fs::read_dir(&base).into_iter().flatten().flatten() {
            if entry.path().join(stackly_testkit::MARKER_FILE).is_file() {
                found.push(entry.path());
            }
        }
    }
    found.sort();
    found
}

// covers: AC-1 (spec 0036, the harness part)
#[test]
fn an_unsupported_api_version_ends_before_any_lifecycle() {
    let ev = evidence!(
        covers = ["AC-030", "REQ-030"],
        kind = Kind::Fixture,
        expected = "a copy of providers/example with apiVersion stackly/v2 ends the run at validate with verdict fail, one unsupported_api_version finding that names stackly/v1alpha1, no scenario, and no test root made",
    );
    let source = TestRoot::new().expect("a source folder can be created");
    let package = copy_package(&repo().join("providers/example"), source.path(), "example");
    let manifest = package.join("provider.yaml");
    let text = std::fs::read_to_string(&manifest).expect("the manifest reads");
    std::fs::write(
        &manifest,
        text.replace("apiVersion: stackly/v1alpha1", "apiVersion: stackly/v2"),
    )
    .expect("the manifest is written");
    let out = TestRoot::new().expect("a report folder can be created");
    let request = RunRequest::new(package.clone(), out.path().join("report.json"));
    let _serial = SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let before = marked_roots();

    let report = conformance::run(&request);

    assert_eq!(report.verdict(), Verdict::Fail);
    let value = report.value().expect("the report was built");
    assert_eq!(value["failure"]["step"], "validate", "{value:#}");
    assert_eq!(value["scenarios"], serde_json::json!([]));
    assert_eq!(value["runtime"], serde_json::Value::Null);
    let findings = value["validation"]["findings"]
        .as_array()
        .expect("findings are listed");
    assert_eq!(findings.len(), 1, "{findings:#?}");
    assert_eq!(findings[0]["code"], "unsupported_api_version");
    assert!(
        findings[0]["message"]
            .as_str()
            .is_some_and(|message| message.contains("stackly/v1alpha1")),
        "{findings:#?}"
    );
    assert_eq!(marked_roots(), before, "no test root was made");
    assert!(
        package.is_dir(),
        "the package folder the user gave is never removed"
    );
    ev.pass();
}

// covers: AC-9 (spec 0036)
#[test]
fn an_unknown_scenario_name_fails_at_validate_with_the_names_the_package_has() {
    let ev = evidence!(
        covers = ["REQ-029"],
        kind = Kind::Fixture,
        expected = "a run of providers/example with --scenario nope ends at validate with verdict fail, a reason that names nope and the package's default and slow-ready scenarios, no scenario run, and no test root made",
    );
    let out = TestRoot::new().expect("a report folder can be created");
    let mut request = RunRequest::new(
        repo().join("providers/example"),
        out.path().join("report.json"),
    );
    request.only = vec!["default".to_owned(), "nope".to_owned()];
    let _serial = SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let before = marked_roots();

    let report = conformance::run(&request);

    assert_eq!(report.verdict(), Verdict::Fail);
    let value = report.value().expect("the report was built");
    assert_eq!(value["failure"]["step"], "validate", "{value:#}");
    let reason = value["failure"]["message"].as_str().unwrap_or_default();
    assert!(reason.contains("unknown scenario nope"), "{reason}");
    assert!(
        reason.contains("default") && reason.contains("slow-ready"),
        "{reason}"
    );
    assert_eq!(value["scenarios"], serde_json::json!([]));
    assert_eq!(marked_roots(), before, "no test root was made");
    ev.pass();
}

// covers: AC-9 (spec 0036)
#[test]
fn step_limits_come_from_the_checked_manifest_in_any_yaml_style() {
    let ev = evidence!(
        covers = ["REQ-029"],
        kind = Kind::Fixture,
        expected = "the step limits of providers/postgresql, whose timeouts block is one flow-style line, are init 180 s, readiness 120 s, stop 60 s, and reload 30 s as its manifest says, and providers/nats has readiness 30 s with the default init 120 s",
    );
    let limits = |name: &str| {
        let check = stackly_core::provider::check_package(
            &stackly_platform_macos::MacosFilesystem,
            &repo().join("providers").join(name),
            stackly_core::provider::NameCheck::FolderName,
        );
        conformance::steps::Limits::of(check.package.as_ref().expect("the package validates"))
    };

    let postgresql = limits("postgresql");
    let nats = limits("nats");

    let secs = std::time::Duration::from_secs;
    assert_eq!(
        (
            postgresql.init,
            postgresql.readiness,
            postgresql.stop,
            postgresql.reload
        ),
        (secs(180), secs(120), secs(60), secs(30))
    );
    assert_eq!(
        (nats.init, nats.readiness, nats.stop),
        (secs(120), secs(30), secs(30))
    );
    ev.pass();
}

// covers: AC-12 (spec 0036)
#[test]
fn a_root_without_this_runs_marker_is_never_removed() {
    let _serial = SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let other = TestRoot::new_marked("another-run").expect("a marked root can be created");
    let data = other.path().to_path_buf();
    let run = other.run_dir().to_path_buf();
    // The marker now names a run other than the one the handle was made
    // with, the way a root of another harness run would.
    let marker = data.join(stackly_testkit::MARKER_FILE);
    let text = std::fs::read_to_string(&marker).expect("the marker reads");
    std::fs::write(&marker, text.replace("another-run", "a-third-run"))
        .expect("the marker is written");
    std::fs::remove_file(run.join(stackly_testkit::MARKER_FILE)).expect("the run marker can go");

    let refused = other
        .remove_checked()
        .expect_err("a root of another run is refused");

    assert_eq!(refused.path, data);
    assert_eq!(refused.reason, "the marker belongs to another run");
    assert!(data.is_dir() && run.is_dir(), "both folders stay in place");
    std::fs::remove_dir_all(&data).expect("the test removes its own data folder");
    std::fs::remove_dir_all(&run).expect("the test removes its own run folder");
}

/// Runs line `line` of the repo package `name` natively and notes the
/// report on `ev`: `BLOCKED` with the report's reason and owner, else the
/// subject from the report's runtime, then the verdict.
fn native(ev: &Evidence, name: &str, line: &str) {
    let mut request = RunRequest::new(repo().join("providers").join(name), PathBuf::new());
    request.line = Some(line.to_owned());
    let (report, _out) = run_with(request);
    let value = report.value().expect("the report was built");
    if report.verdict() == Verdict::Blocked {
        let reason = value["blocked"]["reason"].as_str().unwrap_or("blocked");
        let owner = value["blocked"]["owner"]
            .as_str()
            .unwrap_or("runtime maintainer");
        ev.blocked(reason, owner);
        return;
    }
    assert_eq!(value["kind"], "native", "{value:#}");
    let runtime = &value["runtime"];
    ev.subject(
        name,
        runtime["version"]
            .as_str()
            .expect("a native run names its version"),
        runtime["sha256"].as_str(),
        Source::Managed,
    );
    assert_pass(&report);
    assert_clean(&report);
    let steps: Vec<String> = report
        .scenarios
        .iter()
        .flat_map(|scenario| {
            scenario.steps.iter().map(move |step| {
                format!("{}:{}={}", scenario.name, step.name, step.status.as_str())
            })
        })
        .collect();
    ev.pass_with(&format!("verdict pass; {}", steps.join(", ")));
}

// covers: AC-15 (spec 0036)
#[test]
#[ignore = "native: needs the managed NATS 2.15.1 artifact in the runtime cache"]
fn nats_2_15_passes_conformance() {
    let ev = evidence!(
        covers = ["AC-029", "REQ-029"],
        kind = Kind::Native,
        expected = "conformance::run on providers/nats line 2.15 with the managed artifact from the runtime cache passes every step of its scenarios, writes a native report with verdict pass, and leaves no process and no root",
    );
    native(&ev, "nats", "2.15");
}

// covers: AC-15 (spec 0036)
#[test]
#[ignore = "native: needs the managed PostgreSQL 17.11 artifact in the runtime cache"]
fn postgresql_17_passes_conformance() {
    let ev = evidence!(
        covers = ["AC-029", "REQ-029"],
        kind = Kind::Native,
        expected = "conformance::run on providers/postgresql line 17 with the managed artifact from the runtime cache passes every step of its scenarios, writes a native report with verdict pass, and leaves no process and no root",
    );
    native(&ev, "postgresql", "17");
}

// covers: AC-15 (spec 0036)
#[test]
#[ignore = "native: needs the managed PostgreSQL 18.6 artifact in the runtime cache"]
fn postgresql_18_passes_conformance() {
    let ev = evidence!(
        covers = ["AC-029", "REQ-029"],
        kind = Kind::Native,
        expected = "conformance::run on providers/postgresql line 18 with the managed artifact from the runtime cache passes every step of its scenarios, writes a native report with verdict pass, and leaves no process and no root",
    );
    native(&ev, "postgresql", "18");
}

// covers: AC-15 (spec 0036)
#[test]
#[ignore = "native: needs the managed Traefik 3.7.15 artifact in the runtime cache"]
fn traefik_3_7_passes_conformance() {
    let ev = evidence!(
        covers = ["AC-029", "REQ-029"],
        kind = Kind::Native,
        expected = "conformance::run on providers/traefik line 3.7 with the managed artifact from the runtime cache passes every step of its scenarios, writes a native report with verdict pass, and leaves no process and no root",
    );
    native(&ev, "traefik", "3.7");
}
