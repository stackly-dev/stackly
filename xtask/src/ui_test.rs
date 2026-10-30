//! `cargo xtask ui-test [--only <Class>[/<method>]]`: runs the XCUITest target of the macOS
//! app on fixture scenarios (spec 0068).
//!
//! The task checks what the run needs before it starts `xcodebuild`: macOS, an absolute
//! evidence folder, Automation Mode, and an unlocked screen. It never turns Automation Mode
//! on, because that is a system setting the user owns. After the run it exports the
//! attachments of the result bundle, also when tests failed.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use anyhow::{Context, Result, bail};
use jiff::Timestamp;

const PROJECT: &str = "apps/macos/Stackly.xcodeproj";
const SCHEME: &str = "Stackly";
const DERIVED_DATA: &str = "target/xcode";
const RUNS_DIR: &str = "target/ui-tests";
const TEST_TARGET: &str = "StacklyAppUITests";
/// Texts of `automationmodetool` that mean a UI test run can enter Automation Mode without a
/// prompt: the mode is on, or the user allowed it without authentication. macOS turns the mode
/// on only while a test session runs, so outside a run the first line says "disabled" even on
/// a ready Mac, and the second line decides. Compared in lowercase, because macOS 27 prints
/// "DOES NOT REQUIRE" in capitals.
const AUTOMATION_READY: [&str; 2] = [
    "automation mode is enabled",
    "does not require user authentication to enable automation mode",
];
/// Where the UI test runner may write, relative to the home folder. Xcode always signs the macOS
/// XCUITest runner with App Sandbox and a read only exception for `/`, whatever the target sets,
/// so the runner cannot write to `STACKLY_EVIDENCE_DIR` itself. It writes its records to a
/// staging folder in its own container, and the task moves them to the run folder afterwards.
/// The container name is the test target's bundle ID plus `.xctrunner`.
const RUNNER_STAGING: &str =
    "Library/Containers/com.ojiepermana.stackly.uitests.xctrunner/Data/tmp/stackly-evidence";
/// The variables of the evidence run that reach the test runner.
const FORWARDED: [&str; 2] = ["STACKLY_EVIDENCE_DIR", "STACKLY_EVIDENCE_KIND"];
/// Exit code for a Mac that is not ready: Automation Mode off or unknown, or a locked screen.
const NOT_READY: u8 = 2;

const USAGE: &str = "usage: cargo xtask ui-test [--only <Class>[/<method>]]";

/// Runs `cargo xtask ui-test <args>`.
///
/// # Errors
///
/// Fails on bad arguments or when a tool cannot start. Outside macOS and for a relative
/// `STACKLY_EVIDENCE_DIR` it exits 1 with a message; a Mac that is not ready exits 2.
pub fn run(root: &Path, args: &[&str]) -> Result<ExitCode> {
    let options = Options::parse(args)?;
    let environment: std::collections::HashMap<String, String> = std::env::vars().collect();
    let host = Host::probe();
    if let Verdict::Stop(code, message) = preflight(&host, &environment) {
        eprintln!("{message}");
        return Ok(ExitCode::from(code));
    }
    execute(root, &options, &environment)
}

/// The parsed command line.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Options {
    /// A test class or `Class/method` to run alone.
    only: Option<String>,
}

impl Options {
    fn parse(args: &[&str]) -> Result<Self> {
        match args {
            [] => Ok(Self { only: None }),
            ["--only", only] if is_test_filter(only) => Ok(Self {
                only: Some((*only).to_string()),
            }),
            ["--only", only] => bail!("`{only}` is not a test class or Class/method\n{USAGE}"),
            _ => bail!("unknown ui-test arguments `{}`\n{USAGE}", args.join(" ")),
        }
    }
}

/// `Class` or `Class/method`, in Swift identifier characters.
fn is_test_filter(value: &str) -> bool {
    let valid = |part: &str| {
        !part.is_empty()
            && part
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '_')
    };
    match value.split_once('/') {
        Some((class, method)) => valid(class) && valid(method),
        None => valid(value),
    }
}

/// What the task found out about this Mac.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Host {
    macos: bool,
    /// The stdout of `automationmodetool`, or `None` when it did not run or exited non zero.
    automation_output: Option<String>,
    /// Why `automation_output` is missing.
    automation_error: Option<String>,
    screen_locked: bool,
}

impl Host {
    fn probe() -> Self {
        let macos = cfg!(target_os = "macos");
        if !macos {
            return Self {
                macos,
                automation_output: None,
                automation_error: None,
                screen_locked: false,
            };
        }
        let (automation_output, automation_error) =
            match Command::new("automationmodetool").output() {
                Ok(output) if output.status.success() => (
                    Some(String::from_utf8_lossy(&output.stdout).into_owned()),
                    None,
                ),
                Ok(output) => (
                    None,
                    Some(format!(
                        "automationmodetool exited with {}: {}",
                        output.status,
                        String::from_utf8_lossy(&output.stdout).trim()
                    )),
                ),
                Err(error) => (
                    None,
                    Some(format!("cannot run automationmodetool: {error}")),
                ),
            };
        let screen_locked = Command::new("ioreg")
            .args(["-n", "Root", "-d1", "-a"])
            .output()
            .map(|output| screen_is_locked(&String::from_utf8_lossy(&output.stdout)))
            .unwrap_or(false);
        Self {
            macos,
            automation_output,
            automation_error,
            screen_locked,
        }
    }
}

/// What the preflight decided.
#[derive(Debug, PartialEq, Eq)]
enum Verdict {
    /// Run the tests.
    Ready,
    /// Stop with this exit code and this message.
    Stop(u8, String),
}

/// Checks the host, in the order of the spec: platform, evidence folder, Automation Mode,
/// screen lock. The platform and folder checks stop with exit 1.
fn preflight(host: &Host, environment: &std::collections::HashMap<String, String>) -> Verdict {
    if !host.macos {
        return Verdict::Stop(1, "ui-test runs on macOS only".to_string());
    }
    if let Some(dir) = environment
        .get("STACKLY_EVIDENCE_DIR")
        .filter(|dir| !dir.is_empty())
        && !Path::new(dir).is_absolute()
    {
        return Verdict::Stop(
            1,
            format!("STACKLY_EVIDENCE_DIR must be an absolute path, got `{dir}`"),
        );
    }
    match &host.automation_output {
        Some(output)
            if AUTOMATION_READY
                .iter()
                .any(|text| output.to_lowercase().contains(text)) => {}
        other => {
            let shown = other
                .as_deref()
                .map(str::trim)
                .map(str::to_string)
                .or_else(|| host.automation_error.clone())
                .unwrap_or_default();
            return Verdict::Stop(
                NOT_READY,
                format!(
                    "Automation Mode is not enabled on this Mac, so the UI tests did not run.\n\
                     automationmodetool says: {shown}\n\
                     Only you can turn it on: it is a system setting that asks for an \
                     administrator password. This task never runs the command for you."
                ),
            );
        }
    }
    if host.screen_locked {
        return Verdict::Stop(
            NOT_READY,
            "the screen is locked, so the UI tests did not run; unlock it and run again"
                .to_string(),
        );
    }
    Verdict::Ready
}

/// Whether the plist of `ioreg -n Root -d1 -a` says the login session's screen is locked.
fn screen_is_locked(plist: &str) -> bool {
    let key = "<key>CGSSessionScreenIsLocked</key>";
    plist
        .find(key)
        .map(|at| plist[at + key.len()..].trim_start().starts_with("<true/>"))
        .unwrap_or(false)
}

/// The arguments of `xcodebuild` for one run.
fn xcodebuild_args(options: &Options, result_bundle: &str) -> Vec<String> {
    let only = match &options.only {
        Some(filter) => format!("{TEST_TARGET}/{filter}"),
        None => TEST_TARGET.to_string(),
    };
    [
        "test",
        "-project",
        PROJECT,
        "-scheme",
        SCHEME,
        "-configuration",
        "Debug",
        "-destination",
        "platform=macOS,arch=arm64",
        "-derivedDataPath",
        DERIVED_DATA,
        "-resultBundlePath",
        result_bundle,
        "-parallel-testing-enabled",
        "NO",
        "-only-testing",
        &only,
    ]
    .iter()
    .map(|argument| (*argument).to_string())
    .collect()
}

/// The `TEST_RUNNER_` copies of the evidence variables that are set, in a fixed order. The
/// runner gets `staging` as its evidence folder, since it cannot write to the run folder.
fn runner_environment(
    environment: &std::collections::HashMap<String, String>,
    staging: Option<&Path>,
) -> Vec<(String, String)> {
    FORWARDED
        .iter()
        .filter_map(|name| {
            environment
                .get(*name)
                .filter(|value| !value.is_empty())
                .map(|value| match staging {
                    Some(staging) if *name == "STACKLY_EVIDENCE_DIR" => {
                        staging.to_string_lossy().into_owned()
                    }
                    _ => value.clone(),
                })
                .map(|value| (format!("TEST_RUNNER_{name}"), value))
        })
        .collect()
}

/// Moves the finished records the runner wrote in `staging` to the `records` folder of the run,
/// skipping the `.tmp` file of a record that was never finished, then removes `staging`.
fn collect_staged_records(staging: &Path, run_dir: &Path) -> Result<usize> {
    let staged = staging.join("records");
    let records = run_dir.join("records");
    std::fs::create_dir_all(&records)
        .with_context(|| format!("cannot create {}", records.display()))?;
    let mut moved = 0;
    if staged.is_dir() {
        for entry in std::fs::read_dir(&staged)
            .with_context(|| format!("cannot read {}", staged.display()))?
        {
            let path = entry?.path();
            if path.extension().is_none_or(|extension| extension != "json") {
                continue;
            }
            let Some(name) = path.file_name() else {
                continue;
            };
            let target = records.join(name);
            if std::fs::rename(&path, &target).is_err() {
                std::fs::copy(&path, &target).with_context(|| {
                    format!("cannot copy {} to {}", path.display(), target.display())
                })?;
            }
            moved += 1;
        }
    }
    std::fs::remove_dir_all(staging)
        .with_context(|| format!("cannot remove {}", staging.display()))?;
    Ok(moved)
}

/// The folder of one run, named by the UTC time.
fn run_folder(root: &Path, now: Timestamp) -> PathBuf {
    root.join(RUNS_DIR)
        .join(now.strftime("%Y%m%d-%H%M%S").to_string())
}

fn execute(
    root: &Path,
    options: &Options,
    environment: &std::collections::HashMap<String, String>,
) -> Result<ExitCode> {
    let folder = run_folder(root, Timestamp::now());
    std::fs::create_dir_all(&folder)
        .with_context(|| format!("cannot create {}", folder.display()))?;
    let bundle = folder.join("result.xcresult");
    let run_dir = environment
        .get("STACKLY_EVIDENCE_DIR")
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from);
    let staging = match &run_dir {
        Some(_) => {
            let home = environment
                .get("HOME")
                .context("HOME is not set, so the runner staging folder cannot be found")?;
            let name = folder.file_name().context("the run folder has no name")?;
            let staging = Path::new(home).join(RUNNER_STAGING).join(name);
            std::fs::create_dir_all(&staging)
                .with_context(|| format!("cannot create {}", staging.display()))?;
            Some(staging)
        }
        None => None,
    };
    let status = Command::new("xcodebuild")
        .current_dir(root)
        .args(xcodebuild_args(options, &bundle.to_string_lossy()))
        .envs(runner_environment(environment, staging.as_deref()))
        .status()
        .context("cannot run xcodebuild; install Xcode 27 and select it with `xcode-select`")?;
    // Collect also after a failed run, because a failing test still wrote its record.
    if let (Some(staging), Some(run_dir)) = (&staging, &run_dir) {
        let moved = collect_staged_records(staging, run_dir)?;
        println!(
            "evidence: moved {moved} record(s) from the runner to {}",
            run_dir.display()
        );
    }
    // Export also after a failed run, because the screenshots of a failure matter most.
    if bundle.exists() {
        let attachments = folder.join("attachments");
        let export = Command::new("xcrun")
            .args(["xcresulttool", "export", "attachments", "--path"])
            .arg(&bundle)
            .arg("--output-path")
            .arg(&attachments)
            .status();
        match export {
            Ok(done) if done.success() => println!("attachments: {}", attachments.display()),
            Ok(done) => eprintln!("xcresulttool exported nothing: {done}"),
            Err(error) => eprintln!("cannot run xcresulttool: {error}"),
        }
        println!("result bundle: {}", bundle.display());
    }
    Ok(match status.code() {
        Some(0) => ExitCode::SUCCESS,
        Some(code) => u8::try_from(code).map_or(ExitCode::FAILURE, ExitCode::from),
        None => ExitCode::FAILURE,
    })
}

/// Spec 0068, AC-13: the task stops with exit 1 outside macOS or for a relative evidence folder,
/// with exit 2 when Automation Mode is not enabled or the screen is locked, and otherwise builds
/// the `xcodebuild` command line without retries.
#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn ready_host() -> Host {
        Host {
            macos: true,
            automation_output: Some("Automation Mode is enabled.\n".to_string()),
            automation_error: None,
            screen_locked: false,
        }
    }

    fn environment(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
            .collect()
    }

    #[test]
    fn a_ready_mac_runs_the_tests() {
        assert_eq!(preflight(&ready_host(), &environment(&[])), Verdict::Ready);
    }

    #[test]
    fn outside_macos_the_task_stops_with_exit_one() {
        let host = Host {
            macos: false,
            ..ready_host()
        };
        let Verdict::Stop(code, _) = preflight(&host, &environment(&[])) else {
            panic!("a stop was expected");
        };
        assert_eq!(code, 1);
    }

    #[test]
    fn a_relative_evidence_folder_stops_with_exit_one() {
        let env = environment(&[("STACKLY_EVIDENCE_DIR", "target/evidence")]);
        let Verdict::Stop(code, message) = preflight(&ready_host(), &env) else {
            panic!("a stop was expected");
        };
        assert_eq!(code, 1);
        assert!(message.contains("absolute"));
        let absolute = environment(&[("STACKLY_EVIDENCE_DIR", "/tmp/evidence")]);
        assert_eq!(preflight(&ready_host(), &absolute), Verdict::Ready);
        let empty = environment(&[("STACKLY_EVIDENCE_DIR", "")]);
        assert_eq!(preflight(&ready_host(), &empty), Verdict::Ready);
    }

    #[test]
    fn a_mac_that_allows_automation_without_authentication_runs_the_tests() {
        let allowed = Host {
            automation_output: Some(
                "Automation Mode is disabled.\nThis device DOES NOT REQUIRE user authentication \
                 to enable Automation Mode.\n"
                    .to_string(),
            ),
            ..ready_host()
        };
        assert_eq!(preflight(&allowed, &environment(&[])), Verdict::Ready);
    }

    #[test]
    fn every_reading_but_enabled_or_allowed_stops_with_exit_two_and_shows_the_tool_text() {
        let disabled = Host {
            automation_output: Some(
                "Automation Mode is disabled.\nThis device requires user authentication to \
                 enable Automation Mode.\n"
                    .to_string(),
            ),
            ..ready_host()
        };
        let Verdict::Stop(code, message) = preflight(&disabled, &environment(&[])) else {
            panic!("a stop was expected");
        };
        assert_eq!(code, 2);
        assert!(message.contains("Automation Mode is disabled."));
        assert!(message.contains("never runs the command"));

        for output in [
            Some("something unexpected".to_string()),
            Some(String::new()),
            None,
        ] {
            let host = Host {
                automation_output: output,
                automation_error: Some("cannot run automationmodetool: not found".to_string()),
                ..ready_host()
            };
            let Verdict::Stop(code, _) = preflight(&host, &environment(&[])) else {
                panic!("a stop was expected");
            };
            assert_eq!(code, 2);
        }
    }

    #[test]
    fn a_locked_screen_stops_with_exit_two() {
        let host = Host {
            screen_locked: true,
            ..ready_host()
        };
        let Verdict::Stop(code, message) = preflight(&host, &environment(&[])) else {
            panic!("a stop was expected");
        };
        assert_eq!(code, 2);
        assert!(message.contains("locked"));
    }

    #[test]
    fn the_lock_flag_is_read_from_the_session_plist() {
        let locked = "<key>CGSSessionScreenIsLocked</key>\n\t\t\t<true/>";
        let unlocked = "<key>CGSSessionScreenIsLocked</key><false/>";
        assert!(screen_is_locked(locked));
        assert!(!screen_is_locked(unlocked));
        assert!(!screen_is_locked(
            "<key>kCGSSessionOnConsoleKey</key><true/>"
        ));
    }

    #[test]
    fn the_xcodebuild_arguments_follow_the_spec_and_never_retry() {
        let bundle = "/r/result.xcresult";
        let all = xcodebuild_args(&Options { only: None }, bundle);
        let joined = all.join(" ");
        assert!(joined.starts_with("test -project apps/macos/Stackly.xcodeproj -scheme Stackly"));
        assert!(joined.contains("-configuration Debug"));
        assert!(joined.contains("-destination platform=macOS,arch=arm64"));
        assert!(joined.contains("-derivedDataPath target/xcode"));
        assert!(joined.contains("-resultBundlePath /r/result.xcresult"));
        assert!(joined.contains("-parallel-testing-enabled NO"));
        assert!(joined.ends_with("-only-testing StacklyAppUITests"));
        assert!(!joined.contains("retry"));

        let one = xcodebuild_args(
            &Options {
                only: Some("ShellUITests/testFooter".to_string()),
            },
            bundle,
        );
        assert_eq!(
            one.last().map(String::as_str),
            Some("StacklyAppUITests/ShellUITests/testFooter")
        );
    }

    #[test]
    fn only_the_evidence_variables_that_are_set_reach_the_runner() {
        let env = environment(&[
            ("STACKLY_EVIDENCE_DIR", "/e"),
            ("STACKLY_EVIDENCE_KIND", "fixture"),
            ("OTHER", "x"),
        ]);
        assert_eq!(
            runner_environment(&env, None),
            vec![
                (
                    "TEST_RUNNER_STACKLY_EVIDENCE_DIR".to_string(),
                    "/e".to_string()
                ),
                (
                    "TEST_RUNNER_STACKLY_EVIDENCE_KIND".to_string(),
                    "fixture".to_string()
                ),
            ]
        );
        assert!(runner_environment(&environment(&[("STACKLY_EVIDENCE_DIR", "")]), None).is_empty());
    }

    #[test]
    fn the_runner_gets_the_staging_folder_instead_of_the_run_folder() {
        let env = environment(&[
            ("STACKLY_EVIDENCE_DIR", "/e"),
            ("STACKLY_EVIDENCE_KIND", "native"),
        ]);
        assert_eq!(
            runner_environment(&env, Some(Path::new("/staging"))),
            vec![
                (
                    "TEST_RUNNER_STACKLY_EVIDENCE_DIR".to_string(),
                    "/staging".to_string()
                ),
                (
                    "TEST_RUNNER_STACKLY_EVIDENCE_KIND".to_string(),
                    "native".to_string()
                ),
            ]
        );
    }

    #[test]
    fn staged_records_move_to_the_run_folder_and_the_staging_folder_goes_away() {
        let staging = tempfile::tempdir().expect("a temporary staging folder");
        let run = tempfile::tempdir().expect("a temporary run folder");
        let staged = staging.path().join("records");
        std::fs::create_dir_all(&staged).unwrap();
        std::fs::write(staged.join("a.json"), "{}").unwrap();
        std::fs::write(staged.join("b.json"), "{}").unwrap();
        std::fs::write(staged.join("c.json.tmp"), "{").unwrap();

        assert_eq!(
            collect_staged_records(staging.path(), run.path()).unwrap(),
            2
        );
        let mut names: Vec<_> = std::fs::read_dir(run.path().join("records"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        assert_eq!(names, ["a.json", "b.json"]);
        assert!(!staging.path().exists());
    }

    #[test]
    fn a_runner_that_wrote_nothing_still_leaves_no_staging_folder() {
        let staging = tempfile::tempdir().expect("a temporary staging folder");
        let run = tempfile::tempdir().expect("a temporary run folder");
        assert_eq!(
            collect_staged_records(staging.path(), run.path()).unwrap(),
            0
        );
        assert!(!staging.path().exists());
    }

    #[test]
    fn the_command_line_accepts_a_class_or_a_method_and_nothing_else() {
        assert_eq!(Options::parse(&[]).unwrap(), Options { only: None });
        assert_eq!(
            Options::parse(&["--only", "ShellUITests"]).unwrap().only,
            Some("ShellUITests".to_string())
        );
        assert!(Options::parse(&["--only", "ShellUITests/testA"]).is_ok());
        assert!(Options::parse(&["--only", "a/b/c"]).is_err());
        assert!(Options::parse(&["--only", "bad name"]).is_err());
        assert!(Options::parse(&["--only"]).is_err());
        assert!(Options::parse(&["extra"]).is_err());
    }

    #[test]
    fn a_run_folder_is_named_by_the_utc_time() {
        let at: Timestamp = "2026-10-21T08:09:10Z".parse().unwrap();
        assert_eq!(
            run_folder(Path::new("/repo"), at),
            Path::new("/repo/target/ui-tests/20261021-080910")
        );
    }
}
