//! `cargo xtask provider test <dir> [--line <line>] [--scenario <name>]...
//! [--report <file>] [--keep]` and `cargo xtask provider test --cleanup`
//! (spec 0036, *Harness*, AC-12, AC-13): builds `stackly-fake-server`, then
//! runs the `provider_harness` example of `stacklyd`, which runs the
//! conformance harness in its own process, and passes its exit code on.
//!
//! The report goes to `<target dir>/provider-test/<runId>/report.json`
//! unless `--report` names a file; the harness prints
//! `provider test report: <path>` as its last stderr line, and this task
//! prints nothing after it. While the harness runs, this process ignores
//! SIGINT and SIGTERM; the terminal sends SIGINT to the harness too.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use anyhow::{Context, Result, bail};
use uuid::Uuid;

/// The exit code of a harness a signal ended.
const INTERRUPTED_EXIT: u8 = 130;

/// What `provider test` was asked.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Task {
    /// One run on a package folder.
    Run {
        dir: String,
        line: Option<String>,
        scenarios: Vec<String>,
        report: Option<String>,
        keep: bool,
    },
    /// Remove the marked roots of runs that are gone.
    Cleanup,
}

fn parse(args: &[&str]) -> Result<Task> {
    const USAGE: &str = "usage: cargo xtask provider test <dir> [--line <line>] [--scenario <name>]... [--report <file>] [--keep], or --cleanup";
    if args == ["--cleanup"] {
        return Ok(Task::Cleanup);
    }
    let mut dir = None;
    let mut line = None;
    let mut scenarios = Vec::new();
    let mut report = None;
    let mut keep = false;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        let mut value = |name: &str| {
            rest.next()
                .map(|value| (*value).to_owned())
                .with_context(|| format!("{name} needs a value; {USAGE}"))
        };
        match *arg {
            "--line" => line = Some(value("--line")?),
            "--scenario" => scenarios.push(value("--scenario")?),
            "--report" => report = Some(value("--report")?),
            "--keep" => keep = true,
            flag if flag.starts_with('-') => bail!("unknown flag {flag}; {USAGE}"),
            folder if dir.is_none() => dir = Some(folder.to_owned()),
            extra => bail!("one package folder only, got another: {extra}; {USAGE}"),
        }
    }
    let Some(dir) = dir else {
        bail!("{USAGE}");
    };
    Ok(Task::Run {
        dir,
        line,
        scenarios,
        report,
        keep,
    })
}

/// The cargo target directory of the workspace at `root`.
fn target_dir(root: &Path) -> Result<PathBuf> {
    let metadata = cargo_metadata::MetadataCommand::new()
        .manifest_path(root.join("Cargo.toml"))
        .no_deps()
        .exec()
        .context("cannot read `cargo metadata`")?;
    Ok(metadata.target_directory.into_std_path_buf())
}

fn cargo() -> Command {
    Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
}

/// Runs `provider test` from the repository at `root`.
///
/// # Errors
///
/// Returns an error for bad arguments, a package folder that does not
/// exist, or a build that fails.
pub fn run(root: &Path, args: &[&str]) -> Result<ExitCode> {
    let task = parse(args)?;
    let cwd = std::env::current_dir().context("cannot read the current directory")?;
    let mut harness = match &task {
        Task::Cleanup => vec!["--cleanup".to_owned()],
        Task::Run {
            dir,
            line,
            scenarios,
            report,
            keep,
        } => {
            let package = cwd
                .join(dir)
                .canonicalize()
                .with_context(|| format!("cannot find the package folder {dir}"))?;
            let run_id = Uuid::now_v7();
            let report = match report {
                Some(file) => std::path::absolute(cwd.join(file))
                    .with_context(|| format!("cannot resolve the report path {file}"))?,
                None => target_dir(root)?
                    .join("provider-test")
                    .join(run_id.to_string())
                    .join("report.json"),
            };
            let mut list = vec![
                "--package".to_owned(),
                package.display().to_string(),
                "--report".to_owned(),
                report.display().to_string(),
                "--run-id".to_owned(),
                run_id.to_string(),
            ];
            if let Some(line) = line {
                list.extend(["--line".to_owned(), line.clone()]);
            }
            for scenario in scenarios {
                list.extend(["--scenario".to_owned(), scenario.clone()]);
            }
            if *keep {
                list.push("--keep".to_owned());
            }
            list
        }
    };
    let built = cargo()
        .current_dir(root)
        .args(["build", "-q", "-p", "stackly-fake-server", "--locked"])
        .status()
        .context("cannot run cargo build")?;
    if !built.success() {
        bail!("cargo build -p stackly-fake-server failed with {built}");
    }
    let mut command = cargo();
    command
        .current_dir(root)
        .args([
            "run",
            "-q",
            "--locked",
            "-p",
            "stacklyd",
            "--example",
            "provider_harness",
            "--",
        ])
        .args(harness.drain(..));
    ignore_signals_while_the_harness_runs()?;
    let mut child = command
        .spawn()
        .context("cannot run the provider_harness example")?;
    let status = child
        .wait()
        .context("cannot wait for the provider_harness example")?;
    Ok(exit_code(status))
}

/// The harness's exit code, or 130 when a signal ended it.
fn exit_code(status: std::process::ExitStatus) -> ExitCode {
    match status.code() {
        Some(code) => ExitCode::from(u8::try_from(code).unwrap_or(1)),
        None => ExitCode::from(INTERRUPTED_EXIT),
    }
}

/// SIGINT reaches the harness from the terminal, so this process only
/// stays alive to pass the exit code on. The handler cannot tell SIGINT
/// from SIGTERM, and a signal it forwarded would be the harness's second
/// one, which ends it at once; so both are ignored here.
fn ignore_signals_while_the_harness_runs() -> Result<()> {
    ctrlc::set_handler(|| {}).context("cannot install the SIGINT and SIGTERM handler")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_arguments_are_a_folder_and_the_options_or_cleanup_alone() {
        assert_eq!(parse(&["--cleanup"]).unwrap(), Task::Cleanup);
        assert_eq!(
            parse(&[
                "p",
                "--scenario",
                "a",
                "--scenario",
                "b",
                "--keep",
                "--line",
                "18"
            ])
            .unwrap(),
            Task::Run {
                dir: "p".to_owned(),
                line: Some("18".to_owned()),
                scenarios: vec!["a".to_owned(), "b".to_owned()],
                report: None,
                keep: true,
            }
        );
        for args in [
            &[][..],
            &["p", "q"],
            &["p", "--nope"],
            &["p", "--report"],
            &["--cleanup", "p"],
        ] {
            assert!(parse(args).is_err(), "{args:?}");
        }
    }
}
