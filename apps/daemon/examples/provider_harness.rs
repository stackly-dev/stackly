//! The provider conformance harness as a program (spec 0036, *Harness*),
//! for `cargo xtask provider test`. It loads the module the
//! `provider_conformance` test uses and runs it once on a package folder:
//!
//! ```text
//! provider_harness --package <absolute dir> [--line <line>] [--scenario <name>]...
//!                  --report <file> [--run-id <uuid>] [--keep]
//! provider_harness --cleanup
//! ```
//!
//! It exits 0 for `pass`, 1 for `fail`, 2 for `blocked`, and 130 when a
//! signal stopped the run, and its last stderr line is
//! `provider test report: <absolute path>`, or `the report could not be
//! written to <path>` when writing it failed. The first SIGINT or SIGTERM
//! lets the run end its own way with `interrupted: true`; a second one
//! exits at once and leaves the root for `--cleanup`.

#[cfg(target_os = "macos")]
#[allow(
    dead_code,
    reason = "shared by the provider_harness example and the provider_conformance test"
)]
#[path = "../tests/conformance/mod.rs"]
mod conformance;

use std::process::ExitCode;

#[cfg(target_os = "macos")]
fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match macos::run(&args) {
        Ok(code) => code,
        Err(message) => {
            eprintln!("provider_harness: {message}");
            ExitCode::from(2)
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn main() -> ExitCode {
    eprintln!("provider_harness runs on macOS only");
    ExitCode::from(2)
}

#[cfg(target_os = "macos")]
mod macos {
    use std::path::{Path, PathBuf};
    use std::process::ExitCode;
    use std::sync::atomic::Ordering;

    use signal_hook::consts::{SIGINT, SIGTERM};
    use signal_hook::iterator::Signals;

    use super::conformance::{self, RunRequest, Verdict};

    /// Exit code of a run a signal stopped.
    const INTERRUPTED_EXIT: u8 = 130;

    /// What the arguments ask for.
    #[derive(Debug, PartialEq, Eq)]
    pub enum Task {
        /// One run.
        Run(Box<RunRequest>),
        /// Remove the marked roots of runs that are gone.
        Cleanup,
    }

    /// Reads the arguments.
    pub fn parse(args: &[String]) -> Result<Task, String> {
        if args == ["--cleanup"] {
            return Ok(Task::Cleanup);
        }
        let mut package = None;
        let mut report = None;
        let mut line = None;
        let mut run_id = None;
        let mut only = Vec::new();
        let mut keep = false;
        let mut rest = args.iter();
        while let Some(arg) = rest.next() {
            let mut value = |name: &str| {
                rest.next()
                    .cloned()
                    .ok_or_else(|| format!("{name} needs a value"))
            };
            match arg.as_str() {
                "--package" => package = Some(PathBuf::from(value("--package")?)),
                "--report" => report = Some(PathBuf::from(value("--report")?)),
                "--line" => line = Some(value("--line")?),
                "--scenario" => only.push(value("--scenario")?),
                "--run-id" => {
                    let text = value("--run-id")?;
                    run_id = Some(
                        text.parse::<uuid::Uuid>()
                            .map_err(|_| format!("--run-id {text:?} is not a UUID"))?,
                    );
                }
                "--keep" => keep = true,
                other => return Err(format!("unknown argument {other:?}")),
            }
        }
        let package = package.ok_or("--package <absolute dir> is required")?;
        if !package.is_absolute() {
            return Err(format!("--package {} is not absolute", package.display()));
        }
        let report = report.ok_or("--report <file> is required")?;
        let mut request = RunRequest::new(package, report);
        request.line = line;
        request.only = only;
        request.keep = keep;
        request.run_id = run_id;
        Ok(Task::Run(Box::new(request)))
    }

    /// Runs the task and returns the exit code.
    pub fn run(args: &[String]) -> Result<ExitCode, String> {
        match parse(args)? {
            Task::Cleanup => {
                let removed = stackly_testkit::sweep_marked();
                for path in &removed {
                    println!("removed {}", path.display());
                }
                eprintln!(
                    "provider test --cleanup: removed {} folder(s)",
                    removed.len()
                );
                Ok(ExitCode::SUCCESS)
            }
            Task::Run(request) => {
                watch_signals()?;
                let report = conformance::run(&request);
                summarize(&report);
                let shown = absolute(report.written().unwrap_or(&request.report_path));
                if let Some(kept) = report
                    .value()
                    .and_then(|value| value["cleanup"]["kept"].as_str())
                {
                    eprintln!("kept {kept}");
                }
                if report.written().is_none() {
                    eprintln!("the report could not be written to {}", shown.display());
                } else {
                    eprintln!("provider test report: {}", shown.display());
                }
                let code = if report.interrupted {
                    INTERRUPTED_EXIT
                } else {
                    match report.verdict() {
                        Verdict::Pass => 0,
                        Verdict::Fail => 1,
                        Verdict::Blocked => 2,
                    }
                };
                Ok(ExitCode::from(code))
            }
        }
    }

    /// The first SIGINT or SIGTERM marks the run interrupted; the second
    /// exits at once.
    fn watch_signals() -> Result<(), String> {
        let mut signals = Signals::new([SIGINT, SIGTERM])
            .map_err(|err| format!("cannot install the signal handlers: {err}"))?;
        std::thread::spawn(move || {
            for _ in signals.forever() {
                if conformance::INTERRUPTED.swap(true, Ordering::SeqCst) {
                    eprintln!("provider_harness: second signal, leaving the root for --cleanup");
                    std::process::exit(i32::from(INTERRUPTED_EXIT));
                }
                eprintln!("provider_harness: stopping after the current step");
            }
        });
        Ok(())
    }

    /// One line per scenario and the verdict, on stderr.
    fn summarize(report: &conformance::Report) {
        for scenario in &report.scenarios {
            let outcome = if scenario.passed() { "pass" } else { "fail" };
            match scenario.failure() {
                Some(step) => {
                    let failure = step.failure.clone().unwrap_or_default();
                    let why = failure
                        .reason
                        .or(failure.code)
                        .unwrap_or_else(|| "failed".to_owned());
                    eprintln!("{}: {outcome} at {} ({why})", scenario.name, step.name);
                }
                None => eprintln!("{}: {outcome}", scenario.name),
            }
        }
        if let Some(value) = report.value() {
            if let Some(reason) = value["failure"]["message"].as_str() {
                eprintln!("validate: fail ({reason})");
            }
            if let Some(reason) = value["blocked"]["reason"].as_str() {
                eprintln!("blocked: {reason}");
            }
        }
        eprintln!("verdict: {}", report.verdict().as_str());
    }

    fn absolute(path: &Path) -> PathBuf {
        std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn args(list: &[&str]) -> Vec<String> {
            list.iter().map(|arg| (*arg).to_owned()).collect()
        }

        #[test]
        fn the_arguments_name_a_package_a_report_and_the_options() {
            let Task::Run(request) = parse(&args(&[
                "--package",
                "/p",
                "--report",
                "/r.json",
                "--scenario",
                "a",
                "--keep",
            ]))
            .unwrap() else {
                panic!("a run");
            };
            assert_eq!(request.package_dir, PathBuf::from("/p"));
            assert_eq!(request.only, ["a"]);
            assert!(request.keep);
            assert_eq!(parse(&args(&["--cleanup"])).unwrap(), Task::Cleanup);
            assert!(parse(&args(&["--package", "rel", "--report", "/r"])).is_err());
            assert!(parse(&args(&["--report", "/r"])).is_err());
        }
    }
}
