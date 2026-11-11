//! End to end tests of `cargo xtask provider new`, `check`, and `test`
//! (spec 0036): a package outside the repository passes check and test and
//! the repository is the same afterwards (AC-14); `--keep` leaves the data
//! folder and `--cleanup` removes only marked roots of runs that are gone
//! (AC-12); Ctrl+C ends a run with an interrupted report, exit 130, and no
//! root (AC-12, AC-13); an unsupported `apiVersion` ends at `validate` with
//! exit 1 (AC-1, AC-13); and a run that cannot pick or find its artifact
//! ends `blocked` with exit 2 and no download (AC-13, AC-15).
#![cfg(target_os = "macos")]

use std::fs;
use std::os::unix::fs::{MetadataExt, symlink};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use stackly_testkit::TestRoot;
use stackly_testkit::evidence::{Kind, evidence};

fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask sits one directory below the repository root")
}

/// `cargo xtask provider <args>`, from the repo root.
fn provider(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_xtask"))
        .current_dir(repo_root())
        .arg("provider")
        .args(args)
        .output()
        .expect("the xtask binary runs")
}

/// `git status --porcelain` of the repository.
fn git_status() -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root())
        .args(["status", "--porcelain"])
        .output()
        .expect("git runs");
    assert!(output.status.success(), "git status fails");
    String::from_utf8(output.stdout).expect("git status is UTF-8")
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// Held by every test that runs `provider test --cleanup` or keeps a root,
/// so a sweep in one test never removes the kept root of another.
static SWEEP: Mutex<()> = Mutex::new(());

fn sweep_lock() -> std::sync::MutexGuard<'static, ()> {
    SWEEP
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The report a run wrote.
fn read_report(path: &Path) -> serde_json::Value {
    serde_json::from_slice(&fs::read(path).expect("the report was written"))
        .expect("the report is JSON")
}

/// The canonical system temporary directory, where data roots live.
fn temp_dir() -> PathBuf {
    std::env::temp_dir()
        .canonicalize()
        .expect("the temporary directory resolves")
}

/// Copies a folder tree with `std::fs`, as test setup.
fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).expect("the copy target can be created");
    for entry in fs::read_dir(from).expect("the folder can be listed") {
        let entry = entry.expect("the entry can be read");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("the entry has a type").is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target).expect("the file can be copied");
        }
    }
}

/// Writes a marker of run `run_id` and process `pid` into `dir`, the shape
/// `TestRoot::new_marked` writes.
fn write_marker(dir: &Path, run_id: &str, pid: u32) {
    let uid = fs::metadata(dir).expect("the folder has metadata").uid();
    let marker = serde_json::json!({
        "schema": stackly_testkit::MARKER_SCHEMA,
        "runId": run_id,
        "uid": uid,
        "pid": pid,
        "createdAt": "2026-11-11T00:00:00Z",
    });
    fs::write(
        dir.join(stackly_testkit::MARKER_FILE),
        format!("{marker:#}\n"),
    )
    .expect("the marker is written");
}

/// The PID of a process that has already ended.
fn dead_pid() -> u32 {
    let mut child = Command::new("/usr/bin/true").spawn().expect("true starts");
    let pid = child.id();
    child.wait().expect("true ends");
    pid
}

/// The process group of `pid`, or `None` once it is gone.
fn process_group(pid: u32) -> Option<u32> {
    let output = Command::new("ps")
        .args(["-o", "pgid=", "-p", &pid.to_string()])
        .output()
        .expect("ps runs");
    text(&output.stdout).trim().parse().ok()
}

/// The data roots in the temporary directory whose marker names a process
/// in process group `group`.
fn roots_of_group(group: u32) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for entry in fs::read_dir(temp_dir()).into_iter().flatten().flatten() {
        let marker = entry.path().join(stackly_testkit::MARKER_FILE);
        let Ok(bytes) = fs::read(&marker) else {
            continue;
        };
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
            continue;
        };
        let pid = value["pid"]
            .as_u64()
            .and_then(|pid| u32::try_from(pid).ok());
        if pid.and_then(process_group) == Some(group) {
            found.push(entry.path());
        }
    }
    found
}

// covers: AC-14, AC-13 (spec 0036)
#[test]
fn a_scaffold_outside_the_repo_passes_check_and_test_without_changing_the_repo() {
    let ev = evidence!(
        covers = ["AC-029", "AC-034"],
        kind = Kind::Fixture,
        expected = "cargo xtask provider new mytool --out <test root>, then provider check and provider test on the result, exit 0 with an ok line and a pass report, the last stderr line names the report, and git status of the repository is the same before and after",
    );
    let root = TestRoot::new().expect("a test root can be created");
    let out = root.path().display().to_string();
    let package = root.path().join("mytool");
    let shown = package.display().to_string();
    let report = root.path().join("report.json");
    let before = git_status();

    let made = provider(&["new", "mytool", "--out", &out]);
    let checked = provider(&["check", &shown]);
    let tested = provider(&["test", &shown, "--report", &report.display().to_string()]);

    assert!(made.status.success(), "{}", text(&made.stderr));
    assert!(checked.status.success(), "{}", text(&checked.stderr));
    assert!(text(&checked.stdout).starts_with("ok testing/mytool 0.1.0 "));
    assert_eq!(tested.status.code(), Some(0), "{}", text(&tested.stderr));
    let stderr = text(&tested.stderr);
    assert_eq!(
        stderr.lines().last(),
        Some(format!("provider test report: {}", report.display()).as_str()),
        "{stderr}"
    );
    let value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&report).expect("the report was written"))
            .expect("the report is JSON");
    assert_eq!(value["schema"], "stackly.provider-conformance/v1");
    assert_eq!(value["verdict"], "pass", "{value:#}");
    assert_eq!(value["package"]["provider"], "testing/mytool");
    assert_eq!(git_status(), before, "the repository did not change");
    assert!(package.is_dir(), "the package folder the user gave stays");
    ev.pass();
}

// covers: AC-12 (spec 0036)
#[test]
fn cleanup_runs_alone_and_exits_0() {
    let _sweep = sweep_lock();
    let output = provider(&["test", "--cleanup"]);

    assert!(output.status.success(), "{}", text(&output.stderr));
    assert!(
        text(&output.stderr)
            .lines()
            .last()
            .is_some_and(|line| line.starts_with("provider test --cleanup: removed ")),
        "{}",
        text(&output.stderr)
    );
}

// covers: AC-12 (spec 0036)
#[test]
fn keep_leaves_the_data_folder_and_cleanup_removes_only_marked_roots_of_runs_that_are_gone() {
    let ev = evidence!(
        covers = ["AC-029"],
        kind = Kind::Fixture,
        expected = "cargo xtask provider test providers/example --scenario default --keep exits 0, prints kept <path> for a data folder that holds the run's marker, and its report has cleanup.kept and rootRemoved false; then provider test --cleanup removes that folder and leaves a stackly-test folder without a marker, one whose marker pid is alive, and a stackly-test symlink to a marked folder of a run that is gone",
    );
    let _sweep = sweep_lock();
    let root = TestRoot::new().expect("a test root can be created");
    let tmp = temp_dir();
    let tag = uuid::Uuid::now_v7().simple().to_string();
    let unmarked = tmp.join(format!("stackly-test-unmarked-{tag}"));
    fs::create_dir(&unmarked).expect("the unmarked folder can be created");
    fs::write(unmarked.join("kept.txt"), b"x").expect("a file writes");
    let live = tmp.join(format!("stackly-test-live-{tag}"));
    fs::create_dir(&live).expect("the live folder can be created");
    write_marker(&live, &format!("live-{tag}"), std::process::id());
    let elsewhere = root.path().join("elsewhere");
    fs::create_dir(&elsewhere).expect("the link target can be created");
    write_marker(&elsewhere, &format!("gone-{tag}"), dead_pid());
    let link = tmp.join(format!("stackly-test-link-{tag}"));
    symlink(&elsewhere, &link).expect("the symlink can be created");
    let report = root.path().join("keep.json");

    let kept_run = provider(&[
        "test",
        "providers/example",
        "--scenario",
        "default",
        "--keep",
        "--report",
        &report.display().to_string(),
    ]);
    let stderr = text(&kept_run.stderr);
    let shown = stderr
        .lines()
        .find_map(|line| line.strip_prefix("kept "))
        .unwrap_or_else(|| panic!("a kept line: {stderr}"))
        .to_owned();
    let kept = PathBuf::from(shown.replacen("$TMPDIR", &tmp.display().to_string(), 1));
    let value = read_report(&report);
    let kept_marker: serde_json::Value = serde_json::from_slice(
        &fs::read(kept.join(stackly_testkit::MARKER_FILE)).expect("the kept root has its marker"),
    )
    .expect("the marker is JSON");
    let swept = provider(&["test", "--cleanup"]);

    assert_eq!(kept_run.status.code(), Some(0), "{stderr}");
    assert_eq!(value["verdict"], "pass", "{value:#}");
    assert_eq!(value["cleanup"]["rootRemoved"], false, "{value:#}");
    assert_eq!(value["cleanup"]["kept"].as_str(), Some(shown.as_str()));
    assert_eq!(kept_marker["runId"], value["runId"]);
    assert!(swept.status.success(), "{}", text(&swept.stderr));
    let removed: Vec<String> = text(&swept.stdout)
        .lines()
        .filter_map(|line| line.strip_prefix("removed "))
        .map(str::to_owned)
        .collect();
    let kept_name = kept.file_name().expect("the kept root has a name");
    assert!(
        removed
            .iter()
            .any(|path| Path::new(path).file_name() == Some(kept_name)),
        "{removed:?}"
    );
    assert!(
        !kept.exists(),
        "the kept root of a run that is gone is removed"
    );
    assert!(
        unmarked.join("kept.txt").is_file(),
        "a folder without a marker stays"
    );
    assert!(live.is_dir(), "a root whose run is alive stays");
    assert!(
        fs::symlink_metadata(&link).is_ok_and(|meta| meta.file_type().is_symlink()),
        "a symlink stays"
    );
    assert!(elsewhere.is_dir(), "the symlink's target stays");
    fs::remove_dir_all(&unmarked).expect("the test removes its unmarked folder");
    fs::remove_dir_all(&live).expect("the test removes its live folder");
    fs::remove_file(&link).expect("the test removes its symlink");
    ev.pass();
}

// covers: AC-12, AC-13 (spec 0036)
#[test]
fn ctrl_c_ends_a_run_with_an_interrupted_report_exit_130_and_no_root() {
    let ev = evidence!(
        covers = ["AC-034"],
        kind = Kind::Fixture,
        expected = "SIGINT to the process group of cargo xtask provider test providers/example, as Ctrl+C sends it, once the run has made its marked root, ends it with exit 130, a report with interrupted true and verdict fail whose cleanup removed the root, the report line as the last stderr line, and no data folder of that run left",
    );
    let root = TestRoot::new().expect("a test root can be created");
    let report = root.path().join("interrupted.json");
    let stderr_file = root.path().join("stderr.txt");
    let mut child = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .current_dir(repo_root())
        .args(["provider", "test", "providers/example", "--report"])
        .arg(&report)
        .process_group(0)
        .stdout(Stdio::null())
        .stderr(fs::File::create(&stderr_file).expect("the stderr file can be created"))
        .spawn()
        .expect("the xtask binary runs");
    let group = child.id();
    let deadline = Instant::now() + Duration::from_secs(600);
    let data = loop {
        if let Some(found) = roots_of_group(group).pop() {
            break found;
        }
        if let Some(status) = child.try_wait().expect("the child can be polled") {
            panic!(
                "the run ended with {status} before its root appeared: {}",
                fs::read_to_string(&stderr_file).unwrap_or_default()
            );
        }
        assert!(Instant::now() < deadline, "the run made no root in time");
        std::thread::sleep(Duration::from_millis(200));
    };
    // Into the lifecycle, past the root and the daemon start.
    std::thread::sleep(Duration::from_secs(2));

    let sent = Command::new("kill")
        .args(["-INT", "--", &format!("-{group}")])
        .status()
        .expect("kill runs");
    let deadline = Instant::now() + Duration::from_secs(180);
    let status = loop {
        if let Some(status) = child.try_wait().expect("the child can be polled") {
            break status;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            panic!("the interrupted run did not end in time");
        }
        std::thread::sleep(Duration::from_millis(200));
    };

    let stderr = fs::read_to_string(&stderr_file).expect("stderr reads");
    assert!(sent.success(), "the signal was sent");
    assert_eq!(status.code(), Some(130), "{stderr}");
    let value = read_report(&report);
    assert_eq!(value["interrupted"], true, "{value:#}");
    assert_eq!(value["verdict"], "fail", "{value:#}");
    assert_eq!(value["cleanup"]["rootRemoved"], true, "{value:#}");
    assert_eq!(value["cleanup"]["processesLeft"], serde_json::json!([]));
    assert_eq!(
        stderr.lines().last(),
        Some(format!("provider test report: {}", report.display()).as_str()),
        "{stderr}"
    );
    assert!(
        !data.exists(),
        "the run's data folder {} is gone",
        data.display()
    );
    ev.pass();
}

// covers: AC-1, AC-13 (spec 0036)
#[test]
fn an_unsupported_api_version_exits_1_at_validate_with_a_report() {
    let ev = evidence!(
        covers = ["AC-030", "REQ-030"],
        kind = Kind::Fixture,
        expected = "cargo xtask provider test on a copy of providers/example with apiVersion stackly/v2 exits 1 with validate: fail, a report whose verdict is fail with one unsupported_api_version finding, no runtime, and no scenario, the report line as the last stderr line, and the package folder still there",
    );
    let root = TestRoot::new().expect("a test root can be created");
    let package = root.path().join("example");
    copy_dir(&repo_root().join("providers/example"), &package);
    let manifest = package.join("provider.yaml");
    let yaml = fs::read_to_string(&manifest).expect("the manifest reads");
    fs::write(
        &manifest,
        yaml.replace("apiVersion: stackly/v1alpha1", "apiVersion: stackly/v2"),
    )
    .expect("the manifest is written");
    let report = root.path().join("v2.json");

    let output = provider(&[
        "test",
        &package.display().to_string(),
        "--report",
        &report.display().to_string(),
    ]);

    let stderr = text(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(
        stderr
            .lines()
            .any(|line| line.starts_with("validate: fail")),
        "{stderr}"
    );
    assert_eq!(
        stderr.lines().last(),
        Some(format!("provider test report: {}", report.display()).as_str())
    );
    let value = read_report(&report);
    assert_eq!(value["verdict"], "fail");
    assert_eq!(value["runtime"], serde_json::Value::Null);
    assert_eq!(value["scenarios"], serde_json::json!([]));
    let findings = value["validation"]["findings"]
        .as_array()
        .expect("findings are listed");
    assert_eq!(findings.len(), 1, "{findings:#?}");
    assert_eq!(findings[0]["code"], "unsupported_api_version");
    assert!(manifest.is_file(), "the package the user gave stays");
    ev.pass();
}

// covers: AC-13 (spec 0036)
#[test]
fn a_package_with_two_runnable_lines_and_no_line_is_blocked_with_exit_2() {
    let ev = evidence!(
        covers = ["AC-034"],
        kind = Kind::Fixture,
        expected = "cargo xtask provider test providers/postgresql without --line exits 2 with a blocked report that names the lines 18 and 17, no scenario, and the report line as the last stderr line",
    );
    let root = TestRoot::new().expect("a test root can be created");
    let report = root.path().join("blocked.json");

    let output = provider(&[
        "test",
        "providers/postgresql",
        "--report",
        &report.display().to_string(),
    ]);

    let stderr = text(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "{stderr}");
    assert_eq!(
        stderr.lines().last(),
        Some(format!("provider test report: {}", report.display()).as_str())
    );
    let value = read_report(&report);
    assert_eq!(value["verdict"], "blocked", "{value:#}");
    let reason = value["blocked"]["reason"].as_str().unwrap_or_default();
    assert!(
        reason.contains("more than one runnable line")
            && reason.contains("18")
            && reason.contains("17"),
        "{reason}"
    );
    assert_eq!(value["scenarios"], serde_json::json!([]));
    ev.pass();
}

// covers: AC-15 (spec 0036)
#[test]
fn an_artifact_that_is_not_in_the_runtime_cache_is_blocked_without_a_download() {
    let ev = evidence!(
        covers = ["AC-029"],
        kind = Kind::Fixture,
        expected = "cargo xtask provider test on a copy of providers/nats whose artifact sha256 is in no cache exits 2 with a blocked report, reason artifact <id> is not in the runtime cache and owner runtime maintainer, no runtime installed, and no scenario run",
    );
    let root = TestRoot::new().expect("a test root can be created");
    let package = root.path().join("nats");
    copy_dir(&repo_root().join("providers/nats"), &package);
    let versions = package.join("versions.json");
    let mut value: serde_json::Value =
        serde_json::from_slice(&fs::read(&versions).expect("versions.json reads"))
            .expect("versions.json is JSON");
    let mut changed = 0;
    replace_sha256(&mut value, &mut changed);
    assert!(changed > 0, "the copy has an artifact digest to change");
    fs::write(&versions, format!("{value:#}\n")).expect("versions.json is written");
    let report = root.path().join("not-cached.json");

    let output = provider(&[
        "test",
        &package.display().to_string(),
        "--report",
        &report.display().to_string(),
    ]);

    let stderr = text(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "{stderr}");
    let value = read_report(&report);
    assert_eq!(value["verdict"], "blocked", "{value:#}");
    let reason = value["blocked"]["reason"].as_str().unwrap_or_default();
    assert!(
        reason.starts_with("artifact ") && reason.ends_with(" is not in the runtime cache"),
        "{reason}"
    );
    assert_eq!(value["blocked"]["owner"], "runtime maintainer");
    assert_eq!(value["scenarios"], serde_json::json!([]));
    ev.pass();
}

/// Replaces every `sha256` string in `value` with a digest no artifact has.
fn replace_sha256(value: &mut serde_json::Value, changed: &mut usize) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, inner) in map.iter_mut() {
                if key == "sha256" && inner.is_string() {
                    *inner = serde_json::Value::String("0".repeat(64));
                    *changed += 1;
                } else {
                    replace_sha256(inner, changed);
                }
            }
        }
        serde_json::Value::Array(items) => {
            for inner in items {
                replace_sha256(inner, changed);
            }
        }
        _ => {}
    }
}
