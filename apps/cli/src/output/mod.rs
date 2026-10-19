//! The output layer every command shares (spec 0008): the mode, the
//! command's result and failure, and [`Output::emit`], which writes the
//! `--json` envelope or the text for people and returns the exit code.
//!
//! A command never writes to stdout itself and never exits the process.

pub mod envelope;
pub mod escape;
pub mod human;
pub mod stderr;
pub mod style;
pub mod time;

use std::io::Write;
use std::path::PathBuf;

use jiff::tz::TimeZone;
use serde::Serialize;

use self::envelope::{Envelope, Warning, Written, to_line, warning, write_all};
use self::human::{HumanWriter, Render};
use self::stderr::StderrSink;
use crate::exit::EXIT_SUCCESS;
use crate::failure::CliFailure;
use crate::session::human_hints;

/// How results are written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMode {
    /// Text for people: results on stdout, the rest on stderr.
    Human,
    /// One JSON envelope line on stdout (`--json`).
    Json,
}

/// What a command ends with. `failure` `None` means exit 0; a result next
/// to a failure is a partial result the command documents.
#[derive(Debug)]
pub struct CommandOutput<T> {
    /// The result, if there is one.
    pub result: Option<T>,
    /// The error the command ends with.
    pub failure: Option<CliFailure>,
    /// Warnings for the envelope, or for stderr in text mode.
    pub warnings: Vec<Warning>,
    /// Hints for people, written to stderr after the result; never in
    /// `--json` and never with `--quiet`.
    pub hints: Vec<String>,
}

impl<T> CommandOutput<T> {
    /// A result and exit 0.
    #[must_use]
    pub const fn ok(result: T) -> Self {
        Self {
            result: Some(result),
            failure: None,
            warnings: Vec::new(),
            hints: Vec::new(),
        }
    }

    /// A failure without a result.
    #[must_use]
    pub const fn failed(failure: CliFailure) -> Self {
        Self {
            result: None,
            failure: Some(failure),
            warnings: Vec::new(),
            hints: Vec::new(),
        }
    }

    /// A partial result with the failure that cut it short.
    #[must_use]
    pub const fn partial(result: T, failure: CliFailure) -> Self {
        Self {
            result: Some(result),
            failure: Some(failure),
            warnings: Vec::new(),
            hints: Vec::new(),
        }
    }

    /// This output with one more hint for people.
    #[must_use]
    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hints.push(hint.into());
        self
    }
}

/// Everything about where and how a command's output goes.
pub struct Output {
    /// Text or `--json`.
    pub mode: OutputMode,
    /// The command path for the envelope, such as `operation inspect`.
    pub command: String,
    /// `-v`: request lines on stderr and `request:` in error blocks.
    pub verbose: bool,
    /// `-q`: no progress, text warnings, or hints.
    pub quiet: bool,
    /// Where results go.
    pub stdout: Box<dyn Write + Send>,
    /// Whether text on stdout is colored.
    pub stdout_color: bool,
    /// The one owner of stderr.
    pub stderr: StderrSink,
    /// The zone times are written in.
    pub tz: TimeZone,
    /// The daemon socket path, for the message when no daemon runs.
    pub socket: Option<PathBuf>,
    /// Warnings the framework collected, written before the command's.
    pub warnings: Vec<Warning>,
}

impl std::fmt::Debug for Output {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Output")
            .field("mode", &self.mode)
            .field("command", &self.command)
            .field("verbose", &self.verbose)
            .field("quiet", &self.quiet)
            .finish_non_exhaustive()
    }
}

impl Output {
    /// A writer for one result, colored like stdout.
    #[must_use]
    pub fn human_writer(&self) -> HumanWriter {
        HumanWriter::new(self.stdout_color, jiff::Timestamp::now(), self.tz.clone())
    }

    /// Writes `output` and returns the exit code: the failure's, or 0.
    ///
    /// With `--json`, exactly one envelope line goes to stdout and
    /// warnings only into it. Otherwise warnings go to stderr first, then
    /// the result to stdout, then the error and the hints to stderr. A
    /// broken pipe on stdout stops writing without a word; another write
    /// error is reported as `cli.output_failed` on stderr. Neither changes
    /// the exit code, so a mutation that worked is never reported failed.
    pub fn emit<T: Serialize + Render>(&mut self, output: CommandOutput<T>) -> u8 {
        let code = output
            .failure
            .as_ref()
            .map_or(EXIT_SUCCESS, CliFailure::exit_code);
        let mut warnings = std::mem::take(&mut self.warnings);
        warnings.extend(output.warnings);
        self.stderr.clear_live();
        match self.mode {
            OutputMode::Json => {
                let envelope = Envelope::new(
                    &self.command,
                    output.result.as_ref(),
                    output.failure.as_ref().map(|failure| &*failure.body),
                    output
                        .failure
                        .as_ref()
                        .and_then(|failure| failure.request_id.as_deref()),
                    &warnings,
                );
                match to_line(&envelope) {
                    Ok(line) => self.write_stdout(&line),
                    Err(_) => self.output_failed("the result cannot be written as json"),
                }
            }
            OutputMode::Human => {
                if !self.quiet {
                    for warning in &warnings {
                        self.stderr.warning(&warning.message);
                    }
                }
                if let Some(result) = &output.result {
                    let mut writer = self.human_writer();
                    result.render(&mut writer);
                    let text = writer.into_string();
                    if !text.is_empty() {
                        self.write_stdout(text.as_bytes());
                    }
                }
                if let Some(failure) = &output.failure {
                    self.write_error(failure);
                }
                if !self.quiet {
                    for hint in &output.hints {
                        self.stderr.line(&format!("hint: {hint}"));
                    }
                }
            }
        }
        code
    }

    /// Writes the error block of `failure` to stderr, with the CLI's own
    /// wording and hints for some codes.
    pub fn write_error(&mut self, failure: &CliFailure) {
        let extra = human_hints(&failure.body, self.socket.as_deref());
        let hints = if self.quiet { Vec::new() } else { extra.hints };
        let request_id = if self.verbose {
            failure.request_id.as_deref()
        } else {
            None
        };
        self.stderr
            .error(&failure.body, extra.message.as_deref(), &hints, request_id);
    }

    fn write_stdout(&mut self, bytes: &[u8]) {
        match write_all(&mut *self.stdout, bytes) {
            Written::All | Written::BrokenPipe => {}
            Written::Failed(kind) => {
                self.output_failed(&format!("cannot write the result to stdout ({kind})"));
            }
        }
    }

    fn output_failed(&mut self, message: &str) {
        self.stderr
            .line(&format!("warning: {}: {message}", warning::OUTPUT_FAILED));
    }
}

#[cfg(test)]
pub mod testing {
    //! An [`Output`] over captured streams.

    use super::stderr::Captured;
    use super::*;

    /// An output in `mode` whose stdout and stderr tests read back.
    pub fn output(mode: OutputMode) -> (Output, Captured, Captured) {
        let stdout = Captured::default();
        let stderr = Captured::default();
        let output = Output {
            mode,
            command: "test run".to_owned(),
            verbose: false,
            quiet: false,
            stdout: Box::new(stdout.clone()),
            stdout_color: false,
            stderr: StderrSink::new(Box::new(stderr.clone()), false),
            tz: TimeZone::UTC,
            socket: Some(PathBuf::from("/run/stacklyd.sock")),
            warnings: Vec::new(),
        };
        (output, stdout, stderr)
    }
}

#[cfg(test)]
mod tests {
    use std::io;

    use stackly_protocol::ErrorBody;
    use stackly_protocol::codes::ipc;

    use super::testing::output;
    use super::*;

    #[derive(Debug, Serialize)]
    struct Text(&'static str);

    impl Render for Text {
        fn render(&self, out: &mut HumanWriter) {
            out.line(self.0);
        }
    }

    #[test]
    fn json_mode_writes_one_envelope_line_and_warnings_only_into_it() {
        let (mut out, stdout, stderr) = output(OutputMode::Json);
        out.warnings
            .push(Warning::new(warning::ROOT_OVERRIDE, "override"));
        let code = out.emit(CommandOutput::ok(Text("hello")).with_hint("not in json"));
        assert_eq!(code, 0);
        assert_eq!(
            stdout.text(),
            "{\"schemaVersion\":1,\"type\":\"result\",\"command\":\"test run\",\"result\":\"hello\",\
             \"error\":null,\"requestId\":null,\"warnings\":[{\"code\":\"cli.root_override\",\
             \"message\":\"override\"}]}\n"
        );
        assert_eq!(stderr.text(), "");
    }

    #[test]
    fn a_failure_fills_error_and_request_id_and_sets_the_exit_code() {
        let (mut out, stdout, _) = output(OutputMode::Json);
        let body = ErrorBody::new(&ipc::CALL_TIMEOUT, "late")
            .with_detail("method", "daemon.get_info")
            .with_detail("seconds", "10");
        let failure = CliFailure::daemon(body.clone(), Some("r-9".to_owned()));
        let code = out.emit(CommandOutput::partial(Text("partial"), failure));
        assert_eq!(code, 8);
        let value: serde_json::Value = serde_json::from_str(&stdout.text()).unwrap();
        assert_eq!(value["result"], "partial");
        assert_eq!(value["requestId"], "r-9");
        let decoded: ErrorBody = serde_json::from_value(value["error"].clone()).unwrap();
        assert_eq!(decoded, body);
    }

    #[test]
    fn text_mode_writes_warnings_result_error_and_hints_in_that_order() {
        let (mut out, stdout, stderr) = output(OutputMode::Human);
        out.warnings
            .push(Warning::new(warning::CONTRACT_DRIFT, "drift"));
        let failure = CliFailure::confirmation_declined();
        let code = out.emit(CommandOutput::partial(Text("body"), failure).with_hint("h"));
        assert_eq!(code, 9);
        assert_eq!(stdout.text(), "body\n");
        assert_eq!(
            stderr.text(),
            "warning: drift\nerror: not confirmed, so nothing was done\n  code: cli.confirmation_declined\nhint: h\n"
        );
    }

    #[test]
    fn quiet_drops_warnings_and_hints_but_never_the_error() {
        let (mut out, _, stderr) = output(OutputMode::Human);
        out.quiet = true;
        out.warnings
            .push(Warning::new(warning::CONTRACT_DRIFT, "drift"));
        let body = ErrorBody::new(&ipc::ENDPOINT_MISSING, "nothing exists at the socket path")
            .with_detail("path", "/run/stacklyd.sock");
        let code =
            out.emit(CommandOutput::<Text>::failed(CliFailure::daemon(body, None)).with_hint("h"));
        assert_eq!(code, 6);
        assert_eq!(
            stderr.text(),
            "error: daemon is not running (no socket at /run/stacklyd.sock)\n  code: ipc.endpoint_missing\n"
        );
    }

    #[test]
    fn the_request_id_shows_in_text_only_with_verbose() {
        let failure = CliFailure::daemon(
            ErrorBody::new(&ipc::CONNECTION_CLOSED, "closed"),
            Some("r-7".to_owned()),
        );
        let (mut quiet, _, plain) = output(OutputMode::Human);
        quiet.emit(CommandOutput::<Text>::failed(failure.clone()));
        let (mut verbose, _, detailed) = output(OutputMode::Human);
        verbose.verbose = true;
        verbose.emit(CommandOutput::<Text>::failed(failure));
        assert!(!plain.text().contains("request:"));
        assert!(detailed.text().ends_with("  request: r-7\n"));
    }

    struct Refusing(io::ErrorKind);

    impl Write for Refusing {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(self.0.into())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn a_broken_pipe_is_silent_and_another_write_error_is_one_warning_line() {
        for (kind, expected) in [
            (io::ErrorKind::BrokenPipe, String::new()),
            (
                io::ErrorKind::PermissionDenied,
                "warning: cli.output_failed: cannot write the result to stdout (permission denied)\n"
                    .to_owned(),
            ),
        ] {
            for mode in [OutputMode::Human, OutputMode::Json] {
                let (mut out, _, stderr) = output(mode);
                out.stdout = Box::new(Refusing(kind));
                assert_eq!(out.emit(CommandOutput::ok(Text("x"))), 0);
                assert_eq!(stderr.text(), expected, "{kind:?} {mode:?}");
            }
        }
    }
}
