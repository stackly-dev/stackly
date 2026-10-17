//! `cargo xtask evidence run --kind <fixture|native> [--out <folder>] -- <command>`:
//! runs a test command as evidence and writes `report.json`.

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use cargo_metadata::MetadataCommand;
use jiff::Timestamp;
use stackly_evidence::{EVIDENCE_DIR_ENV, EVIDENCE_KIND_ENV, Kind, RunReport, write_json_atomic};
use uuid::Uuid;

use super::{collect, context};

/// The pid of the running command, so the signal handler can forward to it.
static CHILD_PID: AtomicU32 = AtomicU32::new(0);
/// Set when the wrapper receives SIGINT or SIGTERM.
static INTERRUPTED: AtomicBool = AtomicBool::new(false);
/// How long the output copies may keep draining after the command exits.
/// A leftover process (a server a crashed test never reaped) can hold the
/// pipes open forever; the report must not wait for it.
const DRAIN_GRACE: Duration = Duration::from_secs(5);

/// Parsed arguments of `evidence run`.
#[derive(Debug, PartialEq, Eq)]
struct Options {
    kind: Kind,
    out: Option<PathBuf>,
    command: Vec<String>,
}

/// How the command ended.
struct Outcome {
    exit_code: Option<i32>,
    signal: Option<i32>,
}

pub fn run(args: &[&str]) -> Result<ExitCode> {
    let options = parse(args)?;
    if std::env::var_os(EVIDENCE_DIR_ENV).is_some() {
        bail!("{EVIDENCE_DIR_ENV} is already set: evidence runs never nest");
    }
    let started_at = Timestamp::now();
    let run_id = Uuid::now_v7();
    let metadata = MetadataCommand::new()
        .no_deps()
        .exec()
        .context("cannot read `cargo metadata`")?;
    let workspace_root = metadata.workspace_root.as_std_path().to_path_buf();
    if let Some(out) = &options.out {
        refuse_used_folder(out)?;
    }
    let run_dir = options.out.clone().unwrap_or_else(|| {
        metadata
            .target_directory
            .as_std_path()
            .join("evidence")
            .join(run_id.to_string())
    });
    for folder in ["records", "artifacts"] {
        fs::create_dir_all(run_dir.join(folder))
            .with_context(|| format!("cannot create {}", run_dir.join(folder).display()))?;
    }
    let run_dir = run_dir
        .canonicalize()
        .with_context(|| format!("cannot resolve {}", run_dir.display()))?;

    let context = context::probe(&workspace_root, run_id, options.kind, &options.command)?;
    write_json_atomic(&run_dir.join("run.json"), &context).context("cannot write run.json")?;

    let outcome = execute(&run_dir, options.kind, &options.command)?;
    let collected = collect::collect(
        &run_dir.join("records"),
        &context,
        options.kind,
        &workspace_root,
        outcome.exit_code,
    )?;
    let mut report = RunReport::new(
        run_id,
        options.kind,
        env!("CARGO_PKG_VERSION"),
        context,
        started_at,
        Timestamp::now(),
    );
    report.command_exit_code = outcome.exit_code;
    report.command_signal = outcome.signal;
    report.interrupted = INTERRUPTED.load(Ordering::SeqCst);
    report.set_records(collected.records, collected.problems);
    let report_path = run_dir.join("report.json");
    write_json_atomic(&report_path, &report).context("cannot write report.json")?;
    print_summary(&report);
    eprintln!("evidence report: {}", report_path.display());
    Ok(ExitCode::from(collect::exit_code(
        outcome.exit_code,
        outcome.signal,
        report.problems.len(),
    )))
}

/// `--out` must name a new or empty folder: records left by an earlier run
/// would otherwise be reported under this run's context.
fn refuse_used_folder(out: &Path) -> Result<()> {
    if !out.exists() {
        return Ok(());
    }
    let mut entries =
        fs::read_dir(out).with_context(|| format!("cannot read {}", out.display()))?;
    if entries.next().is_some() {
        bail!(
            "--out {} is not empty; every evidence run needs a new folder",
            out.display()
        );
    }
    Ok(())
}

fn parse(args: &[&str]) -> Result<Options> {
    let mut kind = None;
    let mut out = None;
    let mut rest = args.iter();
    loop {
        match rest.next() {
            Some(&"--kind") => {
                let value = rest.next().context("--kind needs `fixture` or `native`")?;
                kind = Some(Kind::parse(value).with_context(|| {
                    format!("--kind must be `fixture` or `native`, found `{value}`")
                })?);
            }
            Some(&"--out") => {
                out = Some(PathBuf::from(rest.next().context("--out needs a folder")?));
            }
            Some(&"--") => break,
            Some(other) => bail!("unexpected argument `{other}`; put the test command after `--`"),
            None => bail!("missing `-- <command>` after the options"),
        }
    }
    let command: Vec<String> = rest.map(|arg| (*arg).to_owned()).collect();
    if command.is_empty() {
        bail!("missing the test command after `--`");
    }
    Ok(Options {
        kind: kind.context("`--kind fixture` or `--kind native` is required")?,
        out,
        command,
    })
}

/// Runs the command with the evidence environment, copying its stdout and
/// stderr to the terminal and to `output.log`.
fn execute(run_dir: &Path, kind: Kind, command: &[String]) -> Result<Outcome> {
    install_signal_handler()?;
    let log_path = run_dir.join("output.log");
    let log =
        File::create(&log_path).with_context(|| format!("cannot create {}", log_path.display()))?;
    let log = Arc::new(Mutex::new(log));
    let (program, args) = command
        .split_first()
        .context("the test command is never empty")?;
    let mut child = Command::new(program)
        .args(args)
        .env(EVIDENCE_DIR_ENV, run_dir)
        .env(EVIDENCE_KIND_ENV, kind.as_str())
        .stdin(Stdio::inherit())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("cannot run `{program}`"))?;
    CHILD_PID.store(child.id(), Ordering::SeqCst);
    let stdout = child
        .stdout
        .take()
        .context("the command's stdout is piped")?;
    let stderr = child
        .stderr
        .take()
        .context("the command's stderr is piped")?;
    let copies = [
        tee(stdout, io::stdout(), Arc::clone(&log)),
        tee(stderr, io::stderr(), Arc::clone(&log)),
    ];
    let status = child.wait().context("cannot wait for the test command")?;
    CHILD_PID.store(0, Ordering::SeqCst);
    // Drain what is left, but never longer than the grace period, and stop
    // at once on SIGINT or SIGTERM.
    let deadline = Instant::now() + DRAIN_GRACE;
    while copies.iter().any(|copy| !copy.is_finished())
        && Instant::now() < deadline
        && !INTERRUPTED.load(Ordering::SeqCst)
    {
        thread::sleep(Duration::from_millis(50));
    }
    let mut leftover = false;
    for copy in copies {
        if copy.is_finished() {
            copy.join()
                .map_err(|_| anyhow!("an output copy thread panicked"))?
                .context("cannot write output.log")?;
        } else {
            // Dropping the handle detaches the thread; it ends with the process.
            leftover = true;
        }
    }
    if leftover {
        eprintln!(
            "warning: evidence: a leftover process still holds the test command's output; \
             output.log may be incomplete"
        );
    }
    Ok(Outcome {
        exit_code: status.code(),
        signal: signal_of(status),
    })
}

/// Copies `from` to the terminal (best effort) and to the log (required).
fn tee(
    mut from: impl Read + Send + 'static,
    mut terminal: impl Write + Send + 'static,
    log: Arc<Mutex<File>>,
) -> JoinHandle<io::Result<()>> {
    thread::spawn(move || {
        let mut buffer = [0_u8; 8192];
        loop {
            let read = match from.read(&mut buffer) {
                Ok(0) => return Ok(()),
                Ok(read) => read,
                Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
                Err(err) => return Err(err),
            };
            // A closed terminal (for example `| head`) must not lose the log.
            let _ = terminal
                .write_all(&buffer[..read])
                .and_then(|()| terminal.flush());
            log.lock()
                .unwrap_or_else(PoisonError::into_inner)
                .write_all(&buffer[..read])?;
        }
    })
}

/// On SIGINT or SIGTERM, note the interruption and forward SIGTERM to the
/// command, so the wrapper still writes `report.json` after it exits.
fn install_signal_handler() -> Result<()> {
    ctrlc::set_handler(|| {
        INTERRUPTED.store(true, Ordering::SeqCst);
        let pid = CHILD_PID.load(Ordering::SeqCst);
        if pid != 0 {
            forward_sigterm(pid);
        }
    })
    .context("cannot install the SIGINT and SIGTERM handler")
}

#[cfg(unix)]
fn forward_sigterm(pid: u32) {
    // `kill` keeps this crate free of `unsafe`; a failure leaves the
    // command to the signal the terminal already sent it.
    let _ = Command::new("kill")
        .args(["-TERM", &pid.to_string()])
        .status();
}

#[cfg(not(unix))]
fn forward_sigterm(_pid: u32) {}

#[cfg(unix)]
fn signal_of(status: ExitStatus) -> Option<i32> {
    use std::os::unix::process::ExitStatusExt;
    status.signal()
}

#[cfg(not(unix))]
fn signal_of(_status: ExitStatus) -> Option<i32> {
    None
}

fn print_summary(report: &RunReport) {
    let counts: Vec<String> = report
        .summary
        .by_status
        .iter()
        .map(|(status, count)| format!("{} {count}", status.as_str()))
        .collect();
    eprintln!(
        "evidence: {} {} records ({}), {} problems",
        report.summary.total,
        report.kind.as_str(),
        counts.join(", "),
        report.problems.len()
    );
    for problem in &report.problems {
        let record = problem
            .record_id
            .map_or_else(|| "run".to_owned(), |id| id.to_string());
        eprintln!("  {} {record}: {}", problem.code.as_str(), problem.message);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_reads_kind_out_and_the_command() {
        let options = parse(&[
            "--kind",
            "native",
            "--out",
            "/tmp/run",
            "--",
            "cargo",
            "test",
            "--",
            "--ignored",
        ])
        .unwrap();
        assert_eq!(
            options,
            Options {
                kind: Kind::Native,
                out: Some(PathBuf::from("/tmp/run")),
                command: ["cargo", "test", "--", "--ignored"]
                    .map(str::to_owned)
                    .to_vec(),
            }
        );
    }

    #[test]
    fn a_used_out_folder_is_refused_and_a_new_or_empty_one_is_not() {
        let dir = tempfile::tempdir().unwrap();
        assert!(refuse_used_folder(&dir.path().join("new")).is_ok());
        assert!(refuse_used_folder(dir.path()).is_ok());
        fs::create_dir(dir.path().join("records")).unwrap();
        assert!(refuse_used_folder(dir.path()).is_err());
    }

    #[test]
    fn parse_requires_a_kind_and_a_command() {
        assert!(parse(&["--", "cargo", "test"]).is_err());
        assert!(parse(&["--kind", "fixture", "--"]).is_err());
        assert!(parse(&["--kind", "fixture"]).is_err());
        assert!(parse(&["--kind", "ci", "--", "cargo"]).is_err());
        assert!(parse(&["cargo", "test"]).is_err());
    }
}
