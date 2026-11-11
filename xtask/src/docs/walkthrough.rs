//! `cargo xtask docs walkthrough [--keep]` (spec 0038, AC-8): proves the
//! quick start of the provider guide from a clean clone.
//!
//! The task clones `HEAD` into a new temporary folder, reads the first
//! `bash` block after the `<!-- docs-walkthrough -->` comment in the clone's
//! `docs/providers/README.md`, and runs each line with `/bin/sh -c` at the
//! root of the clone, in order, each in its own process group, stopping at
//! the first failure. The lines see a short environment: `HOME`, `USER`,
//! `TMPDIR`, `LANG`, `CARGO_HOME`, and `RUSTUP_HOME` when set, a `PATH` that
//! starts with the clone's `target/debug`, `STACKLY_ROOT` in a 0700 folder
//! of the temporary folder, and the clone's own target folder.
//!
//! After every line exits 0, the clone must hold exactly one passing
//! harness report per `cargo xtask provider test` line, and `git status`
//! in the clone may list only new files under the package folders the
//! block's `cargo xtask provider new <name>` lines made. Ctrl+C goes on to
//! the running line's process group; the task waits for it, then removes
//! the temporary folder unless `--keep`. It never runs
//! `cargo xtask provider test --cleanup` itself, because that also reaches
//! harness runs of the same user that are not its own.

use std::collections::BTreeSet;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, ExitStatus};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde_json::Value;

use super::markdown::Markdown;

/// The guide whose walkthrough block runs, relative to the clone.
pub const GUIDE: &str = "docs/providers/README.md";
/// The comment that marks the walkthrough block.
pub const MARKER: &str = "<!-- docs-walkthrough -->";
/// The least free space in the temporary folder the task needs.
pub const MIN_FREE_BYTES: u64 = 8 * 1024 * 1024 * 1024;
/// The schema of a harness report (spec 0036).
pub const REPORT_SCHEMA: &str = "stackly.provider-conformance/v1";
/// The verdict of a passing harness report (spec 0036).
pub const REPORT_PASS: &str = "pass";
/// Environment variables the lines inherit when they are set.
const INHERITED: [&str; 6] = [
    "HOME",
    "USER",
    "TMPDIR",
    "LANG",
    "CARGO_HOME",
    "RUSTUP_HOME",
];
/// How often the task looks at a running line and at Ctrl+C.
const POLL: Duration = Duration::from_millis(100);

/// How one walkthrough is run.
#[derive(Debug, Clone)]
pub struct Options {
    /// Keep the temporary folder after the run.
    pub keep: bool,
    /// The least free space the run needs, in bytes.
    pub min_free_bytes: u64,
    /// The folder the temporary folder goes into; the system default when
    /// `None`.
    pub temp_parent: Option<PathBuf>,
}

/// How a walkthrough ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Every line exited 0 and the clone holds what it should.
    Pass,
    /// A line failed, a report is wrong, or the clone changed elsewhere.
    Fail,
    /// A prerequisite is missing, such as free space or the block.
    Blocked,
}

impl Outcome {
    /// The exit code of this outcome: 0, 1, or 2.
    #[must_use]
    pub fn exit_code(self) -> ExitCode {
        match self {
            Self::Pass => ExitCode::SUCCESS,
            Self::Fail => ExitCode::FAILURE,
            Self::Blocked => ExitCode::from(2),
        }
    }
}

/// Runs `cargo xtask docs walkthrough <args>` on the repo at `root`.
///
/// # Errors
///
/// Returns an error for an unknown argument.
pub fn command(root: &Path, args: &[&str]) -> Result<ExitCode> {
    let keep = match args {
        [] => false,
        ["--keep"] => true,
        _ => bail!(
            "usage: cargo xtask docs walkthrough [--keep], not `{}`",
            args.join(" ")
        ),
    };
    let interrupted = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&interrupted);
    ctrlc::set_handler(move || flag.store(true, Ordering::SeqCst))
        .context("cannot install the SIGINT and SIGTERM handler")?;
    let options = Options {
        keep,
        min_free_bytes: MIN_FREE_BYTES,
        temp_parent: None,
    };
    Ok(run(root, &options, &interrupted).exit_code())
}

/// Runs one walkthrough of the repo at `root`, printing one line per step.
/// `interrupted` turns true on Ctrl+C.
#[must_use]
pub fn run(root: &Path, options: &Options, interrupted: &AtomicBool) -> Outcome {
    match walk(root, options, interrupted) {
        Ok(outcome) => outcome,
        Err(Stop::Blocked(err)) => {
            eprintln!("docs walkthrough: {err:#}");
            Outcome::Blocked
        }
        Err(Stop::Failed(err)) => {
            eprintln!("docs walkthrough: {err:#}");
            Outcome::Fail
        }
    }
}

/// Why a walkthrough stopped early.
enum Stop {
    /// A prerequisite is missing (exit 2).
    Blocked(anyhow::Error),
    /// Something the walkthrough checks went wrong (exit 1).
    Failed(anyhow::Error),
}

fn blocked(err: anyhow::Error) -> Stop {
    Stop::Blocked(err)
}

fn walk(root: &Path, options: &Options, interrupted: &AtomicBool) -> Result<Outcome, Stop> {
    let parent = options
        .temp_parent
        .clone()
        .unwrap_or_else(std::env::temp_dir);
    let free = free_bytes(&parent).map_err(blocked)?;
    if free < options.min_free_bytes {
        return Err(Stop::Blocked(anyhow::anyhow!(
            "{} has {} free, and the walkthrough needs at least {}",
            parent.display(),
            gib(free),
            gib(options.min_free_bytes)
        )));
    }
    let sha = git_text(root, &["rev-parse", "HEAD"]).map_err(blocked)?;
    if !git_text(root, &["status", "--porcelain"])
        .map_err(blocked)?
        .is_empty()
    {
        eprintln!(
            "warning: the working tree has uncommitted changes; the walkthrough runs {sha} without them"
        );
    }
    let temp = tempfile::Builder::new()
        .prefix("stackly-walkthrough-")
        .tempdir_in(&parent)
        .with_context(|| format!("cannot make a temporary folder in {}", parent.display()))
        .map_err(blocked)?;
    let base = temp.path().to_path_buf();
    let outcome = in_clone(root, &base, &sha, interrupted);
    if options.keep {
        let kept = temp.keep();
        println!("kept {}", kept.display());
    } else if let Err(err) = temp.close() {
        eprintln!("warning: cannot remove {}: {err}", base.display());
    }
    outcome
}

/// Clones `sha`, runs the block, and checks the clone.
fn in_clone(
    root: &Path,
    base: &Path,
    sha: &str,
    interrupted: &AtomicBool,
) -> Result<Outcome, Stop> {
    let clone = base.join("stackly");
    git(
        base,
        &[
            "clone",
            "--quiet",
            "--local",
            "--no-hardlinks",
            "--no-checkout",
            &root.display().to_string(),
            &clone.display().to_string(),
        ],
    )
    .map_err(blocked)?;
    git(&clone, &["checkout", "--quiet", "--detach", sha]).map_err(blocked)?;
    println!("clone {sha}");
    let stackly_root = base.join("root");
    fs::create_dir(&stackly_root)
        .and_then(|()| fs::set_permissions(&stackly_root, fs::Permissions::from_mode(0o700)))
        .with_context(|| format!("cannot make {}", stackly_root.display()))
        .map_err(blocked)?;
    let guide = fs::read_to_string(clone.join(GUIDE))
        .with_context(|| format!("cannot read {GUIDE} in the clone"))
        .map_err(blocked)?;
    let lines = block(&guide).map_err(blocked)?;
    let env = environment(&clone, &stackly_root);
    for line in &lines {
        println!("run {line}");
        let status = run_line(&clone, line, &env, interrupted).map_err(Stop::Failed)?;
        if interrupted.load(Ordering::SeqCst) {
            println!("fail interrupted");
            leftovers_hint();
            return Ok(Outcome::Fail);
        }
        if status.success() {
            println!("ok");
        } else {
            println!(
                "fail {}",
                status
                    .code()
                    .map_or_else(|| "signal".to_owned(), |code| code.to_string())
            );
            leftovers_hint();
            return Ok(Outcome::Fail);
        }
    }
    let expected = lines.iter().filter(|line| is_provider_test(line)).count();
    let reports = check_reports(&clone.join("target/provider-test"), expected);
    let status = git_text(&clone, &["status", "--porcelain", "--untracked-files=all"])
        .map_err(Stop::Failed)?;
    let clean = check_status(&status, &scaffolded(&lines));
    let mut problems = Vec::new();
    problems.extend(reports.as_ref().err().cloned());
    problems.extend(clean.err());
    if !problems.is_empty() {
        for problem in &problems {
            println!("fail {problem}");
        }
        return Ok(Outcome::Fail);
    }
    let count = reports.unwrap_or_default();
    println!("ok walkthrough {sha} {count}");
    Ok(Outcome::Pass)
}

/// The lines of the walkthrough block: the first `bash` block after the
/// marker comment, without blank lines and comments.
///
/// # Errors
///
/// Returns an error when the guide has no marker, no `bash` block after
/// it, or an empty block.
pub fn block(guide: &str) -> Result<Vec<String>> {
    let Some(marker) = guide
        .lines()
        .position(|line| line.trim() == MARKER)
        .map(|index| index + 1)
    else {
        bail!("{GUIDE} has no `{MARKER}` comment");
    };
    let doc = Markdown::parse(guide);
    let Some(fence) = doc
        .fences
        .iter()
        .find(|fence| fence.line > marker && fence.lang == "bash")
    else {
        bail!("{GUIDE} has no bash block after `{MARKER}`");
    };
    let lines: Vec<String> = fence
        .body
        .iter()
        .map(|(_, text)| text.trim().to_owned())
        .filter(|text| !text.is_empty() && !text.starts_with('#'))
        .collect();
    if lines.is_empty() {
        bail!("the walkthrough block in {GUIDE} is empty");
    }
    Ok(lines)
}

/// Whether `line` runs the conformance harness.
fn is_provider_test(line: &str) -> bool {
    words(line).starts_with(&["cargo", "xtask", "provider", "test"])
}

/// The package names the block's `cargo xtask provider new <name>` lines
/// make.
fn scaffolded(lines: &[String]) -> BTreeSet<String> {
    lines
        .iter()
        .filter_map(|line| {
            let words = words(line);
            match words.as_slice() {
                ["cargo", "xtask", "provider", "new", name, ..] => Some((*name).to_owned()),
                _ => None,
            }
        })
        .collect()
}

fn words(line: &str) -> Vec<&str> {
    line.split_whitespace().collect()
}

/// Checks that `dir` holds exactly `expected` reports, `<runId>/report.json`,
/// each a passing harness report, and returns how many there are.
///
/// # Errors
///
/// Returns one message naming what is wrong.
pub fn check_reports(dir: &Path, expected: usize) -> Result<usize, String> {
    let mut found = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let report = entry.path().join("report.json");
            if report.is_file() {
                found.push(report);
            }
        }
    }
    found.sort();
    if found.len() != expected {
        return Err(format!(
            "{} harness report(s) in target/provider-test, expected {expected}, one per cargo xtask provider test line",
            found.len()
        ));
    }
    for report in &found {
        let shown = report
            .strip_prefix(dir)
            .unwrap_or(report)
            .display()
            .to_string();
        let value: Value = fs::read_to_string(report)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .ok_or_else(|| format!("report {shown} is not readable JSON"))?;
        if value.get("schema").and_then(Value::as_str) != Some(REPORT_SCHEMA) {
            return Err(format!("report {shown} is not `{REPORT_SCHEMA}`"));
        }
        let verdict = value.get("verdict").and_then(Value::as_str).unwrap_or("");
        if verdict != REPORT_PASS {
            return Err(format!("report {shown} has verdict `{verdict}`"));
        }
    }
    Ok(found.len())
}

/// Checks `git status --porcelain --untracked-files=all` of the clone:
/// only new files under `providers/<name>/` for a scaffolded name.
///
/// # Errors
///
/// Returns one message naming the first entry that is not allowed.
pub fn check_status(status: &str, scaffolded: &BTreeSet<String>) -> Result<(), String> {
    for entry in status.lines().filter(|line| !line.trim().is_empty()) {
        let allowed = entry.strip_prefix("?? ").is_some_and(|path| {
            scaffolded
                .iter()
                .any(|name| path.starts_with(&format!("providers/{name}/")))
        });
        if !allowed {
            return Err(format!(
                "the clone changed outside a new package: `{entry}`"
            ));
        }
    }
    Ok(())
}

/// The environment of every line.
fn environment(clone: &Path, stackly_root: &Path) -> Vec<(String, String)> {
    let mut env: Vec<(String, String)> = INHERITED
        .iter()
        .filter_map(|name| {
            std::env::var(name)
                .ok()
                .map(|value| ((*name).to_owned(), value))
        })
        .collect();
    let bin = clone.join("target/debug");
    let path = match std::env::var("PATH") {
        Ok(rest) if !rest.is_empty() => format!("{}:{rest}", bin.display()),
        _ => format!("{}:/usr/bin:/bin", bin.display()),
    };
    env.push(("PATH".to_owned(), path));
    env.push((
        "STACKLY_ROOT".to_owned(),
        stackly_root.display().to_string(),
    ));
    env.push((
        "CARGO_TARGET_DIR".to_owned(),
        clone.join("target").display().to_string(),
    ));
    env.push(("CARGO_PROFILE_DEV_DEBUG".to_owned(), "0".to_owned()));
    env.push(("CARGO_INCREMENTAL".to_owned(), "0".to_owned()));
    env
}

/// Runs one line in its own process group and waits for it. On Ctrl+C the
/// group gets SIGINT once, and the task still waits for it to end.
fn run_line(
    clone: &Path,
    line: &str,
    env: &[(String, String)],
    interrupted: &AtomicBool,
) -> Result<ExitStatus> {
    let mut child = Command::new("/bin/sh")
        .arg("-c")
        .arg(line)
        .current_dir(clone)
        .env_clear()
        .envs(env.iter().map(|(key, value)| (key, value)))
        .process_group(0)
        .spawn()
        .with_context(|| format!("cannot run `{line}`"))?;
    let mut forwarded = false;
    loop {
        if let Some(status) = child
            .try_wait()
            .with_context(|| format!("cannot wait for `{line}`"))?
        {
            return Ok(status);
        }
        if !forwarded && interrupted.load(Ordering::SeqCst) {
            forwarded = true;
            // `kill` keeps this crate free of `unsafe`; the line's group
            // id is its PID, because it was spawned with process_group(0).
            let _ = Command::new("kill")
                .args(["-INT", "--", &format!("-{}", child.id())])
                .status();
        }
        std::thread::sleep(POLL);
    }
}

fn leftovers_hint() {
    eprintln!(
        "if a harness run was cut short, its test root may be left; run `cargo xtask provider test --cleanup` yourself to remove it"
    );
}

/// The free space of the file system that holds `dir`, from `df -Pk`.
fn free_bytes(dir: &Path) -> Result<u64> {
    let output = Command::new("df")
        .arg("-Pk")
        .arg(dir)
        .output()
        .context("cannot run df")?;
    if !output.status.success() {
        bail!("df failed for {}", dir.display());
    }
    parse_df(&String::from_utf8_lossy(&output.stdout))
        .with_context(|| format!("cannot read the free space of {}", dir.display()))
}

/// The available bytes of `df -Pk` output: the fourth column of the second
/// line, in KiB.
///
/// # Errors
///
/// Returns an error when the output has no such number.
pub fn parse_df(output: &str) -> Result<u64> {
    let kib: u64 = output
        .lines()
        .nth(1)
        .and_then(|line| line.split_whitespace().nth(3))
        .context("df printed no data line")?
        .parse()
        .context("df printed no available size")?;
    Ok(kib.saturating_mul(1024))
}

fn gib(bytes: u64) -> String {
    #[expect(
        clippy::cast_precision_loss,
        reason = "a size shown with one decimal needs no exact value"
    )]
    let value = bytes as f64 / (1024.0 * 1024.0 * 1024.0);
    format!("{value:.1} GiB")
}

fn git(dir: &Path, args: &[&str]) -> Result<()> {
    let status = Command::new("git")
        .current_dir(dir)
        .args(args)
        .status()
        .with_context(|| format!("cannot run git {}", args.join(" ")))?;
    if !status.success() {
        bail!("git {} failed with {status}", args.join(" "));
    }
    Ok(())
}

fn git_text(dir: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .with_context(|| format!("cannot run git {}", args.join(" ")))?;
    if !output.status.success() {
        bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    const GUIDE_TEXT: &str = "# Guide\n\n```bash\nnot this\n```\n\n<!-- docs-walkthrough -->\n```bash\ncargo xtask provider check providers/example\n# a comment\n\ncargo xtask provider test providers/example\ncargo xtask provider new mytool\ncargo xtask provider test providers/mytool\n```\n\n```bash\nnor this\n```\n";

    // covers: AC-8 (spec 0038)
    #[test]
    fn the_block_is_the_first_bash_block_after_the_marker_without_comments() {
        let lines = block(GUIDE_TEXT).unwrap();
        assert_eq!(
            lines,
            vec![
                "cargo xtask provider check providers/example",
                "cargo xtask provider test providers/example",
                "cargo xtask provider new mytool",
                "cargo xtask provider test providers/mytool",
            ]
        );
        assert_eq!(lines.iter().filter(|l| is_provider_test(l)).count(), 2);
        assert_eq!(
            scaffolded(&lines).into_iter().collect::<Vec<_>>(),
            vec!["mytool"]
        );
    }

    // covers: AC-8 (spec 0038)
    #[test]
    fn a_guide_without_the_marker_or_its_block_is_refused() {
        assert!(block("# Guide\n\n```bash\ntrue\n```\n").is_err());
        assert!(block("# Guide\n\n<!-- docs-walkthrough -->\n\n```text\ntrue\n```\n").is_err());
        assert!(block("<!-- docs-walkthrough -->\n```bash\n# only a comment\n```\n").is_err());
    }

    // covers: AC-8 (spec 0038)
    #[test]
    fn only_new_files_under_a_scaffolded_package_keep_the_clone_clean() {
        let names: BTreeSet<String> = ["mytool".to_owned()].into();
        assert!(check_status("", &names).is_ok());
        assert!(
            check_status(
                "?? providers/mytool/provider.yaml\n?? providers/mytool/README.md",
                &names
            )
            .is_ok()
        );
        assert_eq!(
            check_status(" M Cargo.lock", &names).unwrap_err(),
            "the clone changed outside a new package: ` M Cargo.lock`"
        );
        assert!(check_status("?? providers/other/provider.yaml", &names).is_err());
        assert!(check_status("?? stray.txt", &names).is_err());
        assert!(check_status(" M providers/mytool/provider.yaml", &names).is_err());
    }

    fn write_report(dir: &Path, run: &str, text: &str) {
        let folder = dir.join(run);
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("report.json"), text).unwrap();
    }

    // covers: AC-8 (spec 0038)
    #[test]
    fn reports_must_be_one_per_test_line_and_passing() {
        let dir = tempfile::tempdir().unwrap();
        let pass = r#"{"schema":"stackly.provider-conformance/v1","verdict":"pass"}"#;
        write_report(dir.path(), "a", pass);
        assert_eq!(check_reports(dir.path(), 1), Ok(1));
        assert!(
            check_reports(dir.path(), 2)
                .unwrap_err()
                .starts_with("1 harness report(s)")
        );
        write_report(
            dir.path(),
            "b",
            r#"{"schema":"stackly.provider-conformance/v1","verdict":"fail"}"#,
        );
        assert_eq!(
            check_reports(dir.path(), 2).unwrap_err(),
            "report b/report.json has verdict `fail`"
        );
        write_report(dir.path(), "b", r#"{"schema":"other","verdict":"pass"}"#);
        assert!(
            check_reports(dir.path(), 2)
                .unwrap_err()
                .contains("is not `stackly.provider-conformance/v1`")
        );
        assert_eq!(check_reports(&dir.path().join("missing"), 0), Ok(0));
    }

    // covers: AC-8 (spec 0038)
    #[test]
    fn df_output_gives_the_available_bytes() {
        let out = "Filesystem 1024-blocks Used Available Capacity Mounted on\n/dev/disk3s1 100 40 60 40% /\n";
        assert_eq!(parse_df(out).unwrap(), 60 * 1024);
        assert!(parse_df("Filesystem\n").is_err());
    }

    /// A small committed git repo whose guide runs `lines`.
    fn repo(lines: &str) -> tempfile::TempDir {
        repo_with(lines, &[])
    }

    /// A small committed git repo whose guide runs `lines`, with `files`
    /// (path, text, executable) committed beside it.
    fn repo_with(lines: &str, files: &[(&str, &str, bool)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("docs/providers")).unwrap();
        fs::write(
            root.join(GUIDE),
            format!("# Guide\n\n{MARKER}\n```bash\n{lines}\n```\n"),
        )
        .unwrap();
        for (rel, text, executable) in files {
            let path = root.join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, text).unwrap();
            if *executable {
                fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
            }
        }
        for args in [
            vec!["init", "--quiet"],
            vec!["add", "-A"],
            vec![
                "-c",
                "user.name=fixture",
                "-c",
                "user.email=fixture@localhost",
                "commit",
                "--quiet",
                "-m",
                "fixture",
            ],
        ] {
            git(root, &args).unwrap();
        }
        dir
    }

    fn options(parent: &Path) -> Options {
        Options {
            keep: false,
            min_free_bytes: 0,
            temp_parent: Some(parent.to_path_buf()),
        }
    }

    fn is_empty(dir: &Path) -> bool {
        fs::read_dir(dir).unwrap().next().is_none()
    }

    // covers: AC-8 (spec 0038)
    #[test]
    fn a_block_whose_lines_pass_and_leave_the_clone_clean_passes_and_removes_the_temp_folder() {
        let repo = repo("test -d \"$STACKLY_ROOT\"\ntest \"$CARGO_INCREMENTAL\" = 0");
        let parent = tempfile::tempdir().unwrap();
        let flag = AtomicBool::new(false);
        assert_eq!(
            run(repo.path(), &options(parent.path()), &flag),
            Outcome::Pass
        );
        assert!(is_empty(parent.path()));
    }

    // covers: AC-8 (spec 0038)
    #[test]
    fn a_failing_line_stops_the_block_with_exit_1_and_removes_the_temp_folder() {
        let repo = repo("false\ntouch never-run");
        let parent = tempfile::tempdir().unwrap();
        let flag = AtomicBool::new(false);
        assert_eq!(
            run(repo.path(), &options(parent.path()), &flag),
            Outcome::Fail
        );
        assert!(is_empty(parent.path()));
    }

    // covers: AC-8 (spec 0038)
    #[test]
    fn a_change_outside_a_new_package_fails_the_walkthrough() {
        let repo = repo("touch stray.txt");
        let parent = tempfile::tempdir().unwrap();
        let flag = AtomicBool::new(false);
        assert_eq!(
            run(repo.path(), &options(parent.path()), &flag),
            Outcome::Fail
        );
    }

    // covers: AC-8 (spec 0038)
    #[test]
    fn too_little_free_space_blocks_before_any_clone() {
        let repo = repo("true");
        let parent = tempfile::tempdir().unwrap();
        let flag = AtomicBool::new(false);
        let mut opts = options(parent.path());
        opts.min_free_bytes = u64::MAX;
        assert_eq!(run(repo.path(), &opts, &flag), Outcome::Blocked);
        assert!(is_empty(parent.path()));
    }

    // covers: AC-8 (spec 0038)
    #[test]
    fn the_stackly_root_is_private_and_keep_leaves_the_folder() {
        let repo = repo(
            "test \"$(stat -f %Lp \"$STACKLY_ROOT\")\" = 700\ncase \"$PATH\" in */stackly/target/debug:*) true ;; *) false ;; esac",
        );
        let parent = tempfile::tempdir().unwrap();
        let flag = AtomicBool::new(false);
        let mut opts = options(parent.path());
        opts.keep = true;
        assert_eq!(run(repo.path(), &opts, &flag), Outcome::Pass);
        assert!(!is_empty(parent.path()));
    }

    // covers: AC-8 (spec 0038)
    #[test]
    fn an_interrupt_stops_the_running_line_and_fails() {
        let repo = repo("sleep 30\ntouch never-run");
        let parent = tempfile::tempdir().unwrap();
        let flag = Arc::new(AtomicBool::new(false));
        let setter = Arc::clone(&flag);
        let timer = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(500));
            setter.store(true, Ordering::SeqCst);
        });
        let started = std::time::Instant::now();
        assert_eq!(
            run(repo.path(), &options(parent.path()), &flag),
            Outcome::Fail
        );
        timer.join().unwrap();
        assert!(started.elapsed() < Duration::from_secs(20));
        assert!(is_empty(parent.path()));
    }

    /// The one folder the walkthrough kept under `parent`.
    fn kept(parent: &Path) -> PathBuf {
        let mut entries: Vec<PathBuf> = fs::read_dir(parent)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        assert_eq!(entries.len(), 1, "{entries:?}");
        let folder = entries.remove(0);
        assert!(
            folder
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("stackly-walkthrough-")
        );
        folder
    }

    // covers: AC-8 (spec 0038)
    #[test]
    fn a_failing_line_never_runs_the_lines_after_it() {
        let repo = repo("true\nfalse\ntouch never-run");
        let parent = tempfile::tempdir().unwrap();
        let flag = AtomicBool::new(false);
        let mut opts = options(parent.path());
        opts.keep = true;
        assert_eq!(run(repo.path(), &opts, &flag), Outcome::Fail);
        let clone = kept(parent.path()).join("stackly");
        assert!(clone.join(GUIDE).is_file());
        assert!(!clone.join("never-run").exists());
    }

    // covers: AC-8 (spec 0038)
    #[test]
    fn a_dirty_tree_runs_the_committed_head_without_its_uncommitted_changes() {
        let repo = repo("test ! -e dirty.txt\ntest ! -e untracked.txt");
        fs::write(repo.path().join("untracked.txt"), "x").unwrap();
        fs::write(
            repo.path().join(GUIDE),
            format!("# Guide\n\n{MARKER}\n```bash\nfalse\n```\n"),
        )
        .unwrap();
        fs::write(repo.path().join("dirty.txt"), "x").unwrap();
        let parent = tempfile::tempdir().unwrap();
        let flag = AtomicBool::new(false);
        assert_eq!(
            run(repo.path(), &options(parent.path()), &flag),
            Outcome::Pass
        );
        assert!(is_empty(parent.path()));
        // The walkthrough never touches the working tree it was started from.
        assert!(repo.path().join("dirty.txt").is_file());
        assert!(repo.path().join("untracked.txt").is_file());
    }

    // covers: AC-8 (spec 0038)
    #[test]
    fn a_guide_without_the_block_at_head_blocks_and_removes_the_temp_folder() {
        let repo = repo("true");
        fs::write(repo.path().join(GUIDE), "# Guide\n\nNo block.\n").unwrap();
        git(repo.path(), &["add", "-A"]).unwrap();
        git(
            repo.path(),
            &[
                "-c",
                "user.name=fixture",
                "-c",
                "user.email=fixture@localhost",
                "commit",
                "--quiet",
                "-m",
                "drop the block",
            ],
        )
        .unwrap();
        let parent = tempfile::tempdir().unwrap();
        let flag = AtomicBool::new(false);
        assert_eq!(
            run(repo.path(), &options(parent.path()), &flag),
            Outcome::Blocked
        );
        assert!(is_empty(parent.path()));
    }

    // covers: AC-8 (spec 0038)
    #[test]
    fn an_interrupt_with_keep_leaves_the_clone_and_never_runs_the_next_line() {
        let repo = repo("sleep 30\ntouch never-run");
        let parent = tempfile::tempdir().unwrap();
        let flag = Arc::new(AtomicBool::new(false));
        let setter = Arc::clone(&flag);
        let timer = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(500));
            setter.store(true, Ordering::SeqCst);
        });
        let mut opts = options(parent.path());
        opts.keep = true;
        let started = std::time::Instant::now();
        assert_eq!(run(repo.path(), &opts, &flag), Outcome::Fail);
        timer.join().unwrap();
        assert!(started.elapsed() < Duration::from_secs(20));
        assert!(!kept(parent.path()).join("stackly/never-run").exists());
    }

    /// A fake `cargo` committed at `target/debug/cargo`, which the line
    /// `PATH` finds first: `provider new <name>` writes a package file,
    /// and `provider test` writes one report with `verdict`, or none when
    /// `verdict` is empty.
    fn fake_cargo(verdict: &str) -> String {
        let report = if verdict.is_empty() {
            String::new()
        } else {
            format!(
                "mkdir -p target/provider-test/run-$$ && printf '{{\"schema\":\"{REPORT_SCHEMA}\",\"verdict\":\"{verdict}\"}}' > target/provider-test/run-$$/report.json"
            )
        };
        format!(
            "#!/bin/sh\ncase \"$3\" in\n  new) mkdir -p \"providers/$4\" && echo id > \"providers/$4/provider.yaml\" ;;\n  test) {report} ;;\nesac\n"
        )
    }

    const FOUR_LINES: &str = "cargo xtask provider check providers/example\ncargo xtask provider test providers/example\ncargo xtask provider new mytool\ncargo xtask provider test providers/mytool";

    fn four_line_repo(verdict: &str) -> tempfile::TempDir {
        let cargo = fake_cargo(verdict);
        repo_with(
            FOUR_LINES,
            &[
                ("target/debug/cargo", &cargo, true),
                (".gitignore", "/target/provider-test/\n", false),
            ],
        )
    }

    // covers: AC-8 (spec 0038)
    #[test]
    fn the_four_line_quick_start_passes_with_one_passing_report_per_test_line() {
        let repo = four_line_repo(REPORT_PASS);
        let parent = tempfile::tempdir().unwrap();
        let flag = AtomicBool::new(false);
        let mut opts = options(parent.path());
        opts.keep = true;
        assert_eq!(run(repo.path(), &opts, &flag), Outcome::Pass);
        let clone = kept(parent.path()).join("stackly");
        assert_eq!(check_reports(&clone.join("target/provider-test"), 2), Ok(2));
        assert!(clone.join("providers/mytool/provider.yaml").is_file());
    }

    // covers: AC-8 (spec 0038)
    #[test]
    fn a_harness_report_with_a_failing_verdict_fails_the_walkthrough() {
        let repo = four_line_repo("fail");
        let parent = tempfile::tempdir().unwrap();
        let flag = AtomicBool::new(false);
        assert_eq!(
            run(repo.path(), &options(parent.path()), &flag),
            Outcome::Fail
        );
        assert!(is_empty(parent.path()));
    }

    // covers: AC-8 (spec 0038)
    #[test]
    fn a_test_line_that_writes_no_report_fails_the_walkthrough() {
        let repo = four_line_repo("");
        let parent = tempfile::tempdir().unwrap();
        let flag = AtomicBool::new(false);
        assert_eq!(
            run(repo.path(), &options(parent.path()), &flag),
            Outcome::Fail
        );
    }

    // covers: AC-8 (spec 0038)
    #[test]
    fn outcomes_map_to_exit_codes_0_1_and_2() {
        assert_eq!(Outcome::Pass.exit_code(), ExitCode::SUCCESS);
        assert_eq!(Outcome::Fail.exit_code(), ExitCode::FAILURE);
        assert_eq!(Outcome::Blocked.exit_code(), ExitCode::from(2));
    }

    // covers: AC-8 (spec 0038)
    #[test]
    fn the_free_space_message_shows_sizes_in_gib_with_one_decimal() {
        assert_eq!(gib(MIN_FREE_BYTES), "8.0 GiB");
        assert_eq!(gib(3 * 1024 * 1024 * 1024 / 2), "1.5 GiB");
    }

    // covers: AC-8 (spec 0038)
    #[test]
    fn the_lines_see_only_the_documented_environment() {
        let repo = repo(
            "test -z \"$STACKLY_LOG\"\ntest -z \"$STACKLY_CATALOG_DIR\"\ntest \"$CARGO_PROFILE_DEV_DEBUG\" = 0\ncase \"$CARGO_TARGET_DIR\" in */stackly/target) true ;; *) false ;; esac\ncase \"$STACKLY_ROOT\" in */root) true ;; *) false ;; esac",
        );
        let parent = tempfile::tempdir().unwrap();
        let flag = AtomicBool::new(false);
        // `STACKLY_*` variables of the test process, if any, never reach a
        // line, because the line environment is built from scratch.
        let env = environment(Path::new("/x/stackly"), Path::new("/x/root"));
        assert!(env.iter().all(|(key, _)| {
            INHERITED.contains(&key.as_str())
                || [
                    "PATH",
                    "STACKLY_ROOT",
                    "CARGO_TARGET_DIR",
                    "CARGO_PROFILE_DEV_DEBUG",
                    "CARGO_INCREMENTAL",
                ]
                .contains(&key.as_str())
        }));
        assert_eq!(
            run(repo.path(), &options(parent.path()), &flag),
            Outcome::Pass
        );
    }
}
