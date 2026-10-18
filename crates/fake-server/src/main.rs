//! `stackly-fake-server`, the dummy server behind the `testing/fake`
//! provider (spec 0009, "Fake provider dan dummy server").
//!
//! Tests of features 9, 10, 14, 20, and 26 run it as if it were a real
//! runtime binary, so they can drive a slow start, a crash, a log flood, a
//! process that ignores SIGTERM, and a config reload without a real
//! database. It is never bundled into the app.
//!
//! Every flag with a value is written `--name=value`; `--token-stdin` is the
//! only flag without one. Commands:
//!
//! - `version` prints `stackly-fake-server <crate version>`.
//! - `init --data=<dir> [--token-stdin] [--fail=<bool>]` records its argv in
//!   `<dir>/argv-init.json`, refuses a folder that holds anything else,
//!   writes the SHA-256 of the exact stdin bytes to `<dir>/token.sha256`
//!   with `--token-stdin`, and writes `<major>.<minor>` to
//!   `<dir>/FAKE_VERSION`.
//! - `serve --config=<file> --data=<dir> --port=<p> [--host=<ip>]
//!   [--label=<text>] [--ready-delay-ms=<n>] [--crash-after-ms=<n>]
//!   [--crash-exit-code=<c>] [--log-lines-per-second=<n>]
//!   [--ignore-term=<bool>]` records its argv in `<dir>/argv-serve.json`
//!   and answers HTTP/1.1 on a loopback address until a signal, a crash, or
//!   `POST /shutdown` ends it.
//! - `shutdown --port=<p> [--host=<ip>]` sends `POST /shutdown` to a
//!   running `serve`.
//!
//! Exit codes: 0 on success; 1 on a runtime failure (I/O, a port that
//! cannot be bound, an unreadable config, or a shutdown the server did not
//! answer with 200); 2 on bad arguments; 3 when `init` finds a data folder
//! that is not empty; 4 for `init --fail=true`; and the
//! `--crash-exit-code` (default 70) when `serve` crashes on purpose.
//!
//! Unix only: signals go through the safe API of `signal-hook`.

mod args;
mod http;
mod init;
mod serve;
mod shutdown;

use std::fmt;
use std::fs;
use std::path::Path;
use std::process::ExitCode;

/// Why a command stopped early, which decides the exit code.
#[derive(Debug)]
enum Failure {
    /// I/O, bind, or config trouble, or a shutdown that was not answered
    /// with 200. Exit 1.
    Runtime(String),
    /// Bad arguments. Exit 2.
    Usage(String),
    /// `init` found something besides its argv file in the data folder.
    /// Exit 3.
    DataNotEmpty(String),
    /// `init --fail=true` asked for a failed init. Exit 4.
    InitFailed,
}

impl Failure {
    /// A runtime failure that names what was being done and the I/O error.
    fn io(action: &str, path: &Path, error: &std::io::Error) -> Self {
        Self::Runtime(format!("cannot {action} {}: {error}", path.display()))
    }

    fn exit_code(&self) -> u8 {
        match self {
            Self::Runtime(_) => 1,
            Self::Usage(_) => 2,
            Self::DataNotEmpty(_) => 3,
            Self::InitFailed => 4,
        }
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Runtime(message) | Self::Usage(message) | Self::DataNotEmpty(message) => {
                f.write_str(message)
            }
            Self::InitFailed => f.write_str("init failed because --fail=true was given"),
        }
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(failure) => {
            eprintln!("stackly-fake-server: {failure}");
            ExitCode::from(failure.exit_code())
        }
    }
}

fn run() -> Result<(), Failure> {
    let argv = received_args()?;
    let Some((command, flags)) = argv.split_first() else {
        return Err(Failure::Usage(
            "missing command, expected version, init, serve, or shutdown".to_owned(),
        ));
    };
    match command.as_str() {
        "version" => version(flags),
        "init" => init::run(&argv, flags),
        "serve" => serve::run(&argv, flags),
        "shutdown" => shutdown::run(flags),
        other => Err(Failure::Usage(format!(
            "unknown command {other:?}, expected version, init, serve, or shutdown"
        ))),
    }
}

/// The arguments after argv\[0\], which must all be UTF-8 because plans
/// only ever pass strings.
fn received_args() -> Result<Vec<String>, Failure> {
    std::env::args_os()
        .skip(1)
        .map(|arg| {
            arg.into_string()
                .map_err(|_| Failure::Usage("an argument is not valid utf-8".to_owned()))
        })
        .collect()
}

fn version(flags: &[String]) -> Result<(), Failure> {
    if !flags.is_empty() {
        return Err(Failure::Usage("version takes no arguments".to_owned()));
    }
    println!("stackly-fake-server {}", env!("CARGO_PKG_VERSION"));
    Ok(())
}

/// Writes the arguments this process received, without argv\[0\], to
/// `path` as a JSON array of strings, so a test can compare them with the
/// plan that started it.
fn write_argv(path: &Path, argv: &[String]) -> Result<(), Failure> {
    let json = serde_json::to_vec(argv)
        .map_err(|error| Failure::Runtime(format!("cannot encode argv as json: {error}")))?;
    fs::write(path, json).map_err(|error| Failure::io("write", path, &error))
}
