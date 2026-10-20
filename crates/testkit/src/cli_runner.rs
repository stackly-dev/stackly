//! Runs the real `stackly` binary against a daemon on a test root
//! (spec 0014, *Suite ujung ke ujung*).

use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// How long a CLI process may run before the runner kills it.
pub const CLI_TIMEOUT: Duration = Duration::from_secs(90);
/// The `--timeout` the runner gives a command that has `--wait` and no
/// `--timeout` of its own.
const WAIT_TIMEOUT: &str = "60s";
/// How often the runner checks whether the process ended.
const POLL: Duration = Duration::from_millis(10);

/// What a CLI run wrote and how it ended.
#[derive(Debug, Clone)]
pub struct CliOutput {
    /// The exit status.
    pub status: ExitStatus,
    /// Everything written to stdout.
    pub stdout: String,
    /// Everything written to stderr.
    pub stderr: String,
}

impl CliOutput {
    /// Returns the exit code, `None` when a signal ended the process.
    #[must_use]
    pub fn code(&self) -> Option<i32> {
        self.status.code()
    }

    /// Parses stdout as the one JSON envelope the command wrote.
    ///
    /// # Errors
    ///
    /// Returns an error that quotes both streams when stdout is not JSON.
    pub fn json(&self) -> io::Result<serde_json::Value> {
        serde_json::from_str(&self.stdout).map_err(|error| {
            io::Error::other(format!(
                "stdout is not JSON ({error}); stdout:\n{}\nstderr:\n{}",
                self.stdout, self.stderr
            ))
        })
    }
}

/// Runs `stackly` for one test root.
///
/// Every run gets `STACKLY_ROOT=<root>`, `STACKLY_NO_INPUT=1`, and
/// `NO_COLOR=1`, so it talks to the daemon of that root and never waits for
/// a terminal or opens a Keychain dialog. The runner adds `--json`, adds
/// `--timeout 60s` to a command that has `--wait` and no `--timeout`, gives
/// stdin as `/dev/null` unless the test passes bytes, and kills a process
/// that runs longer than [`CLI_TIMEOUT`].
#[derive(Debug, Clone)]
pub struct CliRunner {
    bin: PathBuf,
    root: PathBuf,
}

impl CliRunner {
    /// Creates a runner for `bin` (see [`cargo_bin`](crate::cargo_bin)) and
    /// the test root `root`.
    #[must_use]
    pub fn new(bin: &Path, root: &Path) -> Self {
        Self {
            bin: bin.to_path_buf(),
            root: root.to_path_buf(),
        }
    }

    /// Runs `stackly <args> --json` with a null stdin.
    ///
    /// # Errors
    ///
    /// Returns the I/O error when the process cannot run, and a
    /// [`io::ErrorKind::TimedOut`] error with its output when it outlives
    /// [`CLI_TIMEOUT`].
    pub fn run(&self, args: &[&str]) -> io::Result<CliOutput> {
        self.run_with_timeout(args, None, CLI_TIMEOUT)
    }

    /// Runs `stackly <args> --json` with `stdin` as its standard input.
    ///
    /// # Errors
    ///
    /// The errors of [`run`](Self::run).
    pub fn run_with_stdin(&self, args: &[&str], stdin: &[u8]) -> io::Result<CliOutput> {
        self.run_with_timeout(args, Some(stdin), CLI_TIMEOUT)
    }

    /// [`run`](Self::run) with a chosen kill timeout and optional stdin.
    ///
    /// # Errors
    ///
    /// The errors of [`run`](Self::run).
    pub fn run_with_timeout(
        &self,
        args: &[&str],
        stdin: Option<&[u8]>,
        limit: Duration,
    ) -> io::Result<CliOutput> {
        let mut command = Command::new(&self.bin);
        command.args(args);
        if !args.contains(&"--json") {
            command.arg("--json");
        }
        if args.contains(&"--wait") && !args.contains(&"--timeout") {
            command.args(["--timeout", WAIT_TIMEOUT]);
        }
        let mut child = command
            .env("STACKLY_ROOT", &self.root)
            .env("STACKLY_NO_INPUT", "1")
            .env("NO_COLOR", "1")
            .stdin(if stdin.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let mut out = child.stdout.take().map(drain);
        let mut err = child.stderr.take().map(drain);
        if let (Some(bytes), Some(mut pipe)) = (stdin, child.stdin.take()) {
            // The command may exit before it reads stdin; that is not an
            // error of the runner.
            let _ = pipe.write_all(bytes);
        }
        let deadline = Instant::now() + limit;
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break Some(status);
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            thread::sleep(POLL);
        };
        let stdout = out.take().map(join).unwrap_or_default();
        let stderr = err.take().map(join).unwrap_or_default();
        match status {
            Some(status) => Ok(CliOutput {
                status,
                stdout,
                stderr,
            }),
            None => Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!(
                    "stackly {args:?} ran longer than {limit:?}; stdout:\n{stdout}\nstderr:\n{stderr}"
                ),
            )),
        }
    }
}

fn drain<R: Read + Send + 'static>(mut pipe: R) -> thread::JoinHandle<String> {
    thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = pipe.read_to_end(&mut bytes);
        String::from_utf8_lossy(&bytes).into_owned()
    })
}

fn join(handle: thread::JoinHandle<String>) -> String {
    handle.join().unwrap_or_default()
}
