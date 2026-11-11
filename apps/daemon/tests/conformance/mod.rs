//! The provider conformance harness (spec 0036, *Harness*): one run checks
//! a provider package folder, prepares a marked test root with the
//! package's runtime, starts `stacklyd` in this process, and then drives
//! every scenario through contract methods on the socket only, from
//! `catalog` to `start_again`. It ends with a `stackly.provider-conformance/v1`
//! report.
//!
//! The `provider_conformance` test and the `provider_harness` example both
//! load this module, each using a part of it.

pub mod process;
pub mod report;
pub mod site;
pub mod steps;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde_json::{Value, json};
use stackly_core::provider::scenarios::{Expect, Scenario};
use stackly_core::provider::{NameCheck, PackageCheck, Severity, check_package};
use stackly_platform_macos::MacosFilesystem;
use uuid::Uuid;

pub use report::Report;

/// The `schema` of a report.
pub const REPORT_SCHEMA: &str = "stackly.provider-conformance/v1";
/// The longest one scenario may run.
pub const SCENARIO_LIMIT: Duration = Duration::from_secs(15 * 60);
/// The namespaces the harness runs.
pub const NAMESPACES: [&str; 2] = ["official", "testing"];

/// Set by the first SIGINT or SIGTERM: the run stops between steps and
/// at every poll, then runs the end of run order with `interrupted: true`.
pub static INTERRUPTED: AtomicBool = AtomicBool::new(false);

/// Whether a signal asked the run to stop.
#[must_use]
pub fn interrupted() -> bool {
    INTERRUPTED.load(Ordering::SeqCst)
}

/// What one run checks and where its report goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunRequest {
    /// The absolute package folder.
    pub package_dir: PathBuf,
    /// The runtime line to run, when the package has more than one.
    pub line: Option<String>,
    /// Scenarios that replace the package's own; only tests set them.
    pub scenarios: Option<Vec<Scenario>>,
    /// Only the scenarios with these names, when not empty.
    pub only: Vec<String>,
    /// Where the report is written.
    pub report_path: PathBuf,
    /// Keep the data folder of the root after the run.
    pub keep: bool,
    /// The run ID, when the caller chose the report folder by it
    /// (`cargo xtask provider test`); a new UUID v7 otherwise.
    pub run_id: Option<Uuid>,
}

impl RunRequest {
    /// A request for `package_dir` with the package's scenarios, its report
    /// at `report_path`, and the root removed afterwards.
    #[must_use]
    pub fn new(package_dir: PathBuf, report_path: PathBuf) -> Self {
        Self {
            package_dir,
            line: None,
            scenarios: None,
            only: Vec::new(),
            report_path,
            keep: false,
            run_id: None,
        }
    }
}

/// The verdict of a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Every scenario passed and the cleanup held.
    Pass,
    /// A scenario failed, the package did not validate, or the cleanup
    /// failed.
    Fail,
    /// The run could not be prepared on this Mac.
    Blocked,
}

impl Verdict {
    /// The report value.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Fail => "fail",
            Self::Blocked => "blocked",
        }
    }
}

/// The status of one step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepStatus {
    /// The step held.
    Pass,
    /// The step did not hold.
    Fail,
    /// The step does not apply, with a reason in `detail`.
    Skipped,
}

impl StepStatus {
    /// The report value.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Fail => "fail",
            Self::Skipped => "skipped",
        }
    }
}

/// Why a step failed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Failure {
    /// A reason of *Kontrak proses* or a short code of the harness.
    pub reason: Option<String>,
    /// The error code of a failed call or operation.
    pub code: Option<String>,
    /// The error message of a failed call or operation, or what the
    /// harness saw.
    pub message: Option<String>,
}

impl Failure {
    /// A failure with a harness reason and what it saw.
    #[must_use]
    pub fn reason(reason: &str, message: impl Into<String>) -> Self {
        Self {
            reason: Some(reason.to_owned()),
            code: None,
            message: Some(message.into()),
        }
    }

    /// A failure with an error code and message of the daemon.
    #[must_use]
    pub fn error(code: &str, message: &str) -> Self {
        Self {
            reason: None,
            code: Some(code.to_owned()),
            message: Some(message.to_owned()),
        }
    }
}

/// One step of a scenario as the report shows it.
#[derive(Debug, Clone)]
pub struct StepResult {
    /// The step name, such as `start`.
    pub name: &'static str,
    /// Its status.
    pub status: StepStatus,
    /// How long it ran.
    pub duration_ms: u64,
    /// Numbers, codes, and names it noted; never a settings text value.
    pub detail: Option<Value>,
    /// Why it failed.
    pub failure: Option<Failure>,
}

/// One scenario as the report shows it.
#[derive(Debug, Clone)]
pub struct ScenarioResult {
    /// The scenario name.
    pub name: String,
    /// What it had to end in.
    pub expect: Expect,
    /// The steps in run order.
    pub steps: Vec<StepResult>,
}

impl ScenarioResult {
    /// Whether no step failed.
    #[must_use]
    pub fn passed(&self) -> bool {
        self.steps
            .iter()
            .all(|step| step.status != StepStatus::Fail)
    }

    /// The first failed step.
    #[must_use]
    pub fn failure(&self) -> Option<&StepResult> {
        self.steps
            .iter()
            .find(|step| step.status == StepStatus::Fail)
    }

    /// The step called `name`.
    #[must_use]
    pub fn step(&self, name: &str) -> Option<&StepResult> {
        self.steps.iter().find(|step| step.name == name)
    }
}

/// The wire name of an expectation.
#[must_use]
pub const fn expect_name(expect: Expect) -> &'static str {
    match expect {
        Expect::Ready => "ready",
        Expect::StartFails => "start_fails",
        Expect::CreateFails => "create_fails",
    }
}

/// Runs the harness once and writes its report (spec 0036, AC-9, AC-13).
/// It never panics on what the package or the daemon does: every problem
/// ends up in the report.
#[must_use]
pub fn run(request: &RunRequest) -> Report {
    let run_id = request.run_id.unwrap_or_else(Uuid::now_v7);
    let mut report = Report::new(run_id);
    let check = check_package(
        &MacosFilesystem,
        &request.package_dir,
        NameCheck::FolderName,
    );
    report.package(&check);
    if let Some(reason) = refusal(&check) {
        report.refuse_validate(&reason);
        report.finish_and_write(&request.report_path);
        return report;
    }
    if let Some(reason) = unknown_scenarios(request, &check) {
        report.refuse_validate(&reason);
        report.finish_and_write(&request.report_path);
        return report;
    }
    let scenarios = chosen_scenarios(request, &check);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("the harness runtime starts");
    runtime.block_on(site::run_prepared(
        request,
        &check,
        &scenarios,
        run_id,
        &mut report,
    ));
    drop(runtime);
    report.interrupted |= interrupted();
    report.finish_and_write(&request.report_path);
    report
}

/// Why `validate` fails for the package, or none.
fn refusal(check: &PackageCheck) -> Option<String> {
    let Some(package) = check.package.as_ref().filter(|_| check.publishable()) else {
        let codes: Vec<String> = check
            .findings
            .iter()
            .filter(|finding| finding.severity == Severity::Error)
            .map(|finding| finding.issue.code.to_string())
            .collect();
        return Some(format!(
            "the package is not publishable: {}",
            codes.join(", ")
        ));
    };
    let provider = package.key().provider().to_string();
    let namespace = provider.split('/').next().unwrap_or_default();
    if !NAMESPACES.contains(&namespace) {
        return Some(format!(
            "the harness runs only the official and testing namespaces, not {namespace}"
        ));
    }
    None
}

/// Why `--scenario` names a scenario the package does not have, with the
/// names it has; `None` when every name is known. A typo then fails at
/// `validate` before any root is made, instead of a green run of nothing.
fn unknown_scenarios(request: &RunRequest, check: &PackageCheck) -> Option<String> {
    let all = request
        .scenarios
        .clone()
        .unwrap_or_else(|| check.scenarios.clone());
    let unknown: Vec<&str> = request
        .only
        .iter()
        .filter(|name| {
            !all.iter()
                .any(|scenario| scenario.name.as_str() == name.as_str())
        })
        .map(String::as_str)
        .collect();
    if unknown.is_empty() {
        return None;
    }
    let available: Vec<&str> = all.iter().map(|scenario| scenario.name.as_str()).collect();
    Some(format!(
        "unknown scenario {}; the package has {}",
        unknown.join(", "),
        if available.is_empty() {
            "no scenarios".to_owned()
        } else {
            available.join(", ")
        }
    ))
}

/// The scenarios the run takes: the request's or the package's, filtered
/// by `only`.
fn chosen_scenarios(request: &RunRequest, check: &PackageCheck) -> Vec<Scenario> {
    let all = request
        .scenarios
        .clone()
        .unwrap_or_else(|| check.scenarios.clone());
    if request.only.is_empty() {
        return all;
    }
    all.into_iter()
        .filter(|scenario| {
            request
                .only
                .iter()
                .any(|name| name == scenario.name.as_str())
        })
        .collect()
}

/// A JSON object of numbers, codes, and names for a step's `detail`.
#[must_use]
pub fn detail(value: Value) -> Option<Value> {
    Some(value)
}

/// The detail of a skipped step.
#[must_use]
pub fn skipped_because(reason: &str) -> Option<Value> {
    Some(json!({ "reason": reason }))
}
