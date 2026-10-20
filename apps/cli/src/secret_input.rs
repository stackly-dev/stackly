//! Secure secret input (spec 0011, *Input aman CLI*): where a new secret
//! value comes from and how it is read. `stackly secret set` uses it, and
//! so will every command that creates something with a secret field.
//!
//! A value never travels on the command line: there is no flag that takes
//! one. It comes from a hidden terminal prompt (`rpassword`, asked twice),
//! stdin, a regular file, or an environment variable, and is wrapped in a
//! [`SecretText`] as soon as it is read. `--generate` and `--copy-from`
//! name a source the daemon resolves, so the CLI never sees the value.

use std::ffi::OsString;
use std::fs::File;
use std::io::{self, Read};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

use clap::Args;
use stackly_protocol::ErrorBody;
use stackly_protocol::codes::cli;
use stackly_protocol::secret::{MAX_SECRET_LEN, SecretInput, SecretText};

use crate::args;
use crate::context::Context;
use crate::exit::EXIT_INTERRUPTED;
use crate::failure::CliFailure;

/// The most bytes read from stdin or a file: one past the longest value,
/// so a value of exactly [`MAX_SECRET_LEN`] bytes plus its newline fits.
pub const MAX_READ: usize = MAX_SECRET_LEN + 1;
/// How many times a mismatching pair of prompts is asked again.
pub const PROMPT_ATTEMPTS: usize = 3;
/// The warning code for a value file that group or others can read.
pub const WARNING_FILE_MODE: &str = "cli.secret_file_permissions";

/// Where one secret value comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecretSource {
    /// A hidden prompt on the terminal, asked twice.
    Prompt,
    /// Standard input until end of input.
    Stdin,
    /// A regular file.
    File(PathBuf),
    /// An environment variable, as it is.
    Env(String),
    /// A value the daemon generates.
    Generate,
    /// A copy of the value under another reference, made by the daemon.
    CopyFrom(String),
    /// An existing reference used without writing a value.
    Bind(String),
}

/// The source flags of a command that takes one secret value: at most
/// one, and none means the hidden prompt.
#[derive(Debug, Clone, Args)]
#[group(multiple = false)]
pub struct SourceArgs {
    /// Read the value from stdin until end of input
    #[arg(long)]
    pub stdin: bool,
    /// Read the value from a regular file (symbolic links are refused)
    #[arg(long, value_name = "PATH")]
    pub from_file: Option<PathBuf>,
    /// Read the value from an environment variable
    #[arg(long, value_name = "NAME")]
    pub from_env: Option<String>,
    /// Let the daemon generate a random value
    #[arg(long)]
    pub generate: bool,
    /// Let the daemon copy the value stored under another reference
    #[arg(long, value_name = "REFERENCE", value_parser = args::secret_reference)]
    pub copy_from: Option<String>,
}

impl SourceArgs {
    /// The source these flags name; the prompt when none is given.
    #[must_use]
    pub fn source(&self) -> SecretSource {
        if self.stdin {
            SecretSource::Stdin
        } else if let Some(path) = &self.from_file {
            SecretSource::File(path.clone())
        } else if let Some(name) = &self.from_env {
            SecretSource::Env(name.clone())
        } else if self.generate {
            SecretSource::Generate
        } else if let Some(reference) = &self.copy_from {
            SecretSource::CopyFrom(reference.clone())
        } else {
            SecretSource::Prompt
        }
    }
}

/// A value read from a file, and whether group or others can read it.
#[derive(Debug)]
pub struct FileRead {
    /// The value.
    pub text: SecretText,
    /// The file mode allows group or other access.
    pub loose_permissions: bool,
}

/// Reads one hidden line from the terminal.
pub trait HiddenInput {
    /// Reads one line without echo.
    ///
    /// # Errors
    ///
    /// The terminal could not be read.
    fn read_hidden(&mut self) -> io::Result<String>;
}

/// The controlling terminal, read with echo off through `rpassword`.
#[derive(Debug, Default)]
pub struct Tty;

impl HiddenInput for Tty {
    fn read_hidden(&mut self) -> io::Result<String> {
        interrupt::guarded(rpassword::read_password)
    }
}

/// What keeps Ctrl+C at the hidden prompt from leaving the terminal raw.
///
/// `rpassword` turns off `ISIG` on the terminal, so Ctrl+C arrives as a
/// byte, and `rpassword` raises `SIGINT` itself before it restores the
/// terminal modes. With no handler that signal ends the process inside the
/// raise, and a shell without job control (`sh`, a wrapper script, a pty of
/// another tool) is left with echo off. A listener on its own thread, like
/// the one `wait` keeps for a second Ctrl+C, turns the signal into a
/// returned `Interrupted` error, so the modes are restored on drop.
///
/// The listener stays for the rest of the process, since a registered
/// handler cannot be taken back. Outside a prompt it ends the command with
/// exit 130, as the default action would.
mod interrupt {
    use std::io;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Once, OnceLock};
    use std::time::{Duration, Instant};

    use tokio::signal::unix::{SignalKind, signal};

    use super::EXIT_INTERRUPTED;

    /// A prompt is reading, so a `SIGINT` is the prompt's own Ctrl+C.
    static READING: AtomicBool = AtomicBool::new(false);
    /// How many `SIGINT` the listener has taken so far.
    static SEEN: AtomicUsize = AtomicUsize::new(0);
    static STARTED: Once = Once::new();
    static LISTENING: OnceLock<bool> = OnceLock::new();

    /// How long the prompt waits for the listener to take the signal it
    /// raised itself.
    const SETTLE: Duration = Duration::from_millis(500);

    fn listen() -> bool {
        STARTED.call_once(|| {
            let (ready, registered) = std::sync::mpsc::channel();
            let spawned = std::thread::Builder::new()
                .name("stackly-prompt-interrupt".to_owned())
                .spawn(move || {
                    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                    else {
                        let _ = ready.send(false);
                        return;
                    };
                    runtime.block_on(async move {
                        let Ok(mut interrupts) = signal(SignalKind::interrupt()) else {
                            let _ = ready.send(false);
                            return;
                        };
                        let _ = ready.send(true);
                        while interrupts.recv().await.is_some() {
                            // Read before the count moves: the prompt clears
                            // the flag only after it saw the count move.
                            let at_prompt = READING.load(Ordering::SeqCst);
                            SEEN.fetch_add(1, Ordering::SeqCst);
                            if !at_prompt {
                                std::process::exit(i32::from(EXIT_INTERRUPTED));
                            }
                        }
                    });
                });
            let listening = spawned.is_ok() && registered.recv_timeout(SETTLE).unwrap_or(false);
            let _ = LISTENING.set(listening);
        });
        LISTENING.get().copied().unwrap_or(false)
    }

    /// Runs `read` with the listener on. An `Interrupted` result comes back
    /// only after the listener took the signal `rpassword` raised, so that
    /// signal never counts as a later Ctrl+C.
    pub(super) fn guarded<T>(read: impl FnOnce() -> io::Result<T>) -> io::Result<T> {
        let listening = listen();
        let before = SEEN.load(Ordering::SeqCst);
        READING.store(true, Ordering::SeqCst);
        let result = read();
        if listening && matches!(&result, Err(error) if error.kind() == io::ErrorKind::Interrupted)
        {
            let waited = Instant::now();
            while SEEN.load(Ordering::SeqCst) == before && waited.elapsed() < SETTLE {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        READING.store(false, Ordering::SeqCst);
        result
    }

    #[cfg(test)]
    pub(super) fn seen() -> usize {
        SEEN.load(Ordering::SeqCst)
    }
}

/// The value of an environment variable, or `None` when it is unset.
pub type EnvLookup<'a> = &'a dyn Fn(&str) -> Option<OsString>;

/// Reads the input for `source`. `label` names what is asked for, in the
/// prompt and in `cli.input_required`.
///
/// # Errors
///
/// `cli.input_required` when the prompt is the source but the CLI runs
/// noninteractive, and `cli.usage` with a `kind` such as
/// `secret_value_too_long` for a value or source the CLI refuses. No
/// message holds a value.
pub fn read_input(
    cx: &mut Context,
    source: &SecretSource,
    label: &str,
    hidden: &mut dyn HiddenInput,
    env: EnvLookup<'_>,
) -> Result<SecretInput, CliFailure> {
    match source {
        SecretSource::Prompt => {
            cx.require_prompt(&format!("value for {label}"))?;
            prompt_twice(cx, label, hidden).map(SecretInput::Value)
        }
        SecretSource::Stdin => read_bounded(&mut cx.input).map(SecretInput::Value),
        SecretSource::File(path) => {
            let read = read_file(path)?;
            if read.loose_permissions {
                cx.warn(
                    WARNING_FILE_MODE,
                    "the value file can be read by group or others; run chmod 600 on it",
                );
            }
            Ok(SecretInput::Value(read.text))
        }
        SecretSource::Env(name) => read_env(name, env).map(SecretInput::Value),
        SecretSource::Generate => Ok(SecretInput::Generate),
        SecretSource::CopyFrom(reference) => Ok(SecretInput::CopyFrom(reference.clone())),
        SecretSource::Bind(reference) => Ok(SecretInput::Bind(reference.clone())),
    }
}

/// Asks for a value and again to confirm it, on stderr, up to
/// [`PROMPT_ATTEMPTS`] times while the two differ.
fn prompt_twice(
    cx: &mut Context,
    label: &str,
    hidden: &mut dyn HiddenInput,
) -> Result<SecretText, CliFailure> {
    for attempt in 0..PROMPT_ATTEMPTS {
        let first = prompt_once(cx, &format!("Value for {label}: "), hidden)?;
        let second = prompt_once(cx, "Repeat the value: ", hidden)?;
        if first == second {
            return checked(first);
        }
        if attempt + 1 < PROMPT_ATTEMPTS {
            cx.output.stderr.line("the two entries differ, try again");
        }
    }
    Err(refuse(
        "secret_entries_differ",
        "the two entries differed three times, so nothing was set",
    ))
}

/// One hidden line after `prompt`, followed by the newline the hidden
/// entry did not echo.
fn prompt_once(
    cx: &mut Context,
    prompt: &str,
    hidden: &mut dyn HiddenInput,
) -> Result<SecretText, CliFailure> {
    cx.output.stderr.prompt(prompt);
    let read = hidden.read_hidden();
    cx.output.stderr.line("");
    match read {
        Ok(line) => Ok(SecretText::new(line)),
        Err(error) if error.kind() == io::ErrorKind::Interrupted => Err(interrupted()),
        Err(_) => Err(refuse(
            "secret_prompt_failed",
            "the value could not be read from the terminal",
        )),
    }
}

/// Reads at most [`MAX_READ`] bytes from `reader` and builds the value:
/// exactly one trailing `\n` or `\r\n` is dropped.
///
/// # Errors
///
/// `cli.usage` with kind `secret_value_too_long`, `secret_value_not_utf8`,
/// `secret_value_invalid`, or `secret_read_failed`.
pub fn read_bounded<R: Read>(reader: &mut R) -> Result<SecretText, CliFailure> {
    // Room for one byte past the limit, so the buffer never reallocates and
    // leaves a copy of the value behind.
    let mut bytes = Vec::with_capacity(MAX_READ + 1);
    let read = Read::by_ref(reader)
        .take(MAX_READ as u64 + 1)
        .read_to_end(&mut bytes);
    if read.is_err() {
        bytes.fill(0);
        return Err(refuse("secret_read_failed", "the value could not be read"));
    }
    if bytes.len() > MAX_READ {
        bytes.fill(0);
        return Err(too_long());
    }
    text_from_bytes(bytes)
}

/// The value in `bytes`, less one trailing newline.
fn text_from_bytes(mut bytes: Vec<u8>) -> Result<SecretText, CliFailure> {
    if bytes.last() == Some(&b'\n') {
        bytes.pop();
        if bytes.last() == Some(&b'\r') {
            bytes.pop();
        }
    }
    match String::from_utf8(bytes) {
        Ok(text) => checked(SecretText::new(text)),
        Err(error) => {
            let mut bytes = error.into_bytes();
            bytes.fill(0);
            Err(refuse(
                "secret_value_not_utf8",
                "the value is not valid UTF-8",
            ))
        }
    }
}

/// Reads a value file: a regular file that is not a symbolic link, at most
/// [`MAX_READ`] bytes.
///
/// # Errors
///
/// `cli.usage` with kind `secret_file_unreadable`, `secret_file_symlink`,
/// `secret_file_not_regular`, or `secret_file_changed`, and the errors of
/// [`read_bounded`].
pub fn read_file(path: &Path) -> Result<FileRead, CliFailure> {
    let link = std::fs::symlink_metadata(path).map_err(|error| file_error(&error))?;
    if link.file_type().is_symlink() {
        return Err(refuse(
            "secret_file_symlink",
            "the value file is a symbolic link, which is not followed",
        ));
    }
    if !link.is_file() {
        return Err(refuse(
            "secret_file_not_regular",
            "the value file is not a regular file",
        ));
    }
    let mut file = File::open(path).map_err(|error| file_error(&error))?;
    let opened = file.metadata().map_err(|error| file_error(&error))?;
    if opened.dev() != link.dev() || opened.ino() != link.ino() {
        return Err(refuse(
            "secret_file_changed",
            "the value file was replaced while it was opened",
        ));
    }
    if opened.len() > MAX_READ as u64 {
        return Err(too_long());
    }
    let text = read_bounded(&mut file)?;
    Ok(FileRead {
        text,
        loose_permissions: opened.permissions().mode() & 0o077 != 0,
    })
}

/// Reads the value of environment variable `name` as it is.
///
/// # Errors
///
/// `cli.usage` with kind `secret_env_missing` for a variable that is unset
/// or empty, `secret_value_not_utf8`, or `secret_value_invalid`.
pub fn read_env(name: &str, env: EnvLookup<'_>) -> Result<SecretText, CliFailure> {
    let value = env(name).filter(|value| !value.is_empty()).ok_or_else(|| {
        refuse(
            "secret_env_missing",
            &format!("the environment variable {name} is not set or is empty"),
        )
    })?;
    match value.into_string() {
        Ok(text) => checked(SecretText::new(text)),
        Err(_) => Err(refuse(
            "secret_value_not_utf8",
            "the value is not valid UTF-8",
        )),
    }
}

/// `value` when it follows the rules of spec 0011 AC-7: 1 to 65536 bytes
/// and no NUL.
fn checked(value: SecretText) -> Result<SecretText, CliFailure> {
    match value.field_code() {
        None => Ok(value),
        Some(stackly_protocol::json::FieldCode::TooLong) => Err(too_long()),
        Some(_) => Err(refuse(
            "secret_value_invalid",
            "the value is empty or holds a NUL byte",
        )),
    }
}

fn too_long() -> CliFailure {
    refuse(
        "secret_value_too_long",
        &format!("the value is longer than {MAX_SECRET_LEN} bytes"),
    )
}

fn file_error(error: &io::Error) -> CliFailure {
    let why = match error.kind() {
        io::ErrorKind::NotFound => "it does not exist",
        io::ErrorKind::PermissionDenied => "permission was denied",
        _ => "the read failed",
    };
    refuse(
        "secret_file_unreadable",
        &format!("the value file cannot be read: {why}"),
    )
}

/// `cli.usage` for input the CLI refuses, with a stable `kind`.
/// `cli.interrupted` for Ctrl+C at the hidden prompt: nothing was sent, so
/// no operation is left running. Exits 130.
fn interrupted() -> CliFailure {
    CliFailure {
        body: Box::new(ErrorBody::new(
            &cli::INTERRUPTED,
            "interrupted at the secret prompt; nothing was set",
        )),
        request_id: None,
        exit_override: Some(EXIT_INTERRUPTED),
    }
}

fn refuse(kind: &str, message: &str) -> CliFailure {
    CliFailure::usage(kind, message)
}

/// A `--secret <field>=<source>` argument of a command with several secret
/// fields (spec 0011, *Input aman CLI*; the spelling is spec 0020's).
///
/// # Errors
///
/// Returns why `text` is not one, for clap's message.
pub fn field_source(text: &str) -> Result<(String, SecretSource), String> {
    let refused = || {
        "expected <field>=<generate|prompt|stdin|file:<path>|env:<NAME>|copy:<reference>|bind:<reference>>"
            .to_owned()
    };
    let (field, source) = text.split_once('=').ok_or_else(refused)?;
    if field.is_empty() || !field.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(refused());
    }
    let source = match source.split_once(':') {
        None => match source {
            "generate" => SecretSource::Generate,
            "prompt" => SecretSource::Prompt,
            "stdin" => SecretSource::Stdin,
            _ => return Err(refused()),
        },
        Some(("file", path)) if !path.is_empty() => SecretSource::File(PathBuf::from(path)),
        Some(("env", name)) if !name.is_empty() && !name.contains('=') => {
            SecretSource::Env(name.to_owned())
        }
        Some(("copy", reference)) => SecretSource::CopyFrom(args::secret_reference(reference)?),
        Some(("bind", reference)) => SecretSource::Bind(args::secret_reference(reference)?),
        Some(_) => return Err(refused()),
    };
    Ok((field.to_owned(), source))
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;
    use crate::context::testing::context;

    /// Answers the hidden prompt from a script and counts the reads.
    struct Scripted {
        lines: Vec<String>,
        reads: usize,
    }

    impl Scripted {
        fn new(lines: &[&str]) -> Self {
            Self {
                lines: lines.iter().rev().map(|line| (*line).to_owned()).collect(),
                reads: 0,
            }
        }
    }

    impl HiddenInput for Scripted {
        fn read_hidden(&mut self) -> io::Result<String> {
            self.reads += 1;
            self.lines
                .pop()
                .ok_or_else(|| io::Error::from(io::ErrorKind::UnexpectedEof))
        }
    }

    fn no_env(_: &str) -> Option<OsString> {
        None
    }

    fn kind(failure: &CliFailure) -> &str {
        assert!(failure.body.is(&cli::USAGE), "{failure:?}");
        failure.body.detail("kind").expect("usage has a kind")
    }

    fn value(input: SecretInput) -> String {
        match input {
            SecretInput::Value(text) => text.expose().to_owned(),
            other => panic!("expected a value, got {other:?}"),
        }
    }

    #[test]
    fn ctrl_c_at_the_prompt_is_cli_interrupted_with_exit_130_and_no_retry() {
        struct Interrupts(usize);
        impl HiddenInput for Interrupts {
            fn read_hidden(&mut self) -> io::Result<String> {
                self.0 += 1;
                Err(io::Error::new(io::ErrorKind::Interrupted, "interrupted"))
            }
        }
        let (mut cx, _out) = context(true, "");
        let mut hidden = Interrupts(0);

        let failure = read_input(
            &mut cx,
            &SecretSource::Prompt,
            "adminPassword",
            &mut hidden,
            &no_env,
        )
        .expect_err("an interrupted prompt sets nothing");

        assert!(failure.body.is(&cli::INTERRUPTED), "{failure:?}");
        assert_eq!(failure.exit_code(), 130);
        assert_eq!(hidden.0, 1, "no second prompt after Ctrl+C");
    }

    #[test]
    fn a_sigint_raised_during_a_guarded_read_does_not_end_the_process() {
        let before = interrupt::seen();

        let result = interrupt::guarded(|| {
            // What `rpassword` does when it reads Ctrl+C, here from the
            // shell so the signal reaches the whole process.
            let status = std::process::Command::new("/bin/kill")
                .args(["-INT", &std::process::id().to_string()])
                .status()
                .expect("kill runs");
            assert!(status.success());
            Err::<String, _>(io::Error::new(io::ErrorKind::Interrupted, "interrupted"))
        });

        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::Interrupted);
        assert_eq!(
            interrupt::seen(),
            before + 1,
            "the listener took the signal"
        );
    }

    #[test]
    fn stdin_drops_exactly_one_trailing_newline() {
        for (bytes, expected) in [
            (&b"hunter2\n"[..], "hunter2"),
            (b"hunter2\r\n", "hunter2"),
            (b"hunter2", "hunter2"),
            (b"hunter2\n\n", "hunter2\n"),
            (b"hunter2\r\n\r\n", "hunter2\r\n"),
            (b"hunter2\r", "hunter2\r"),
            (b"two\nlines\n", "two\nlines"),
        ] {
            let text = read_bounded(&mut Cursor::new(bytes.to_vec())).unwrap();
            assert_eq!(text.expose(), expected, "{bytes:?}");
        }
    }

    #[test]
    fn a_value_of_the_limit_passes_with_its_newline_and_one_more_byte_is_refused() {
        let mut exact = vec![b'a'; MAX_SECRET_LEN];
        exact.push(b'\n');
        assert_eq!(
            read_bounded(&mut Cursor::new(exact.clone())).unwrap().len(),
            MAX_SECRET_LEN
        );
        exact.pop();
        assert_eq!(
            read_bounded(&mut Cursor::new(exact)).unwrap().len(),
            MAX_SECRET_LEN
        );
        for len in [MAX_SECRET_LEN + 1, MAX_READ + 10, MAX_READ * 2] {
            let failure = read_bounded(&mut Cursor::new(vec![b'a'; len])).unwrap_err();
            assert_eq!(kind(&failure), "secret_value_too_long", "{len}");
            assert_eq!(failure.exit_code(), 2);
        }
    }

    #[test]
    fn an_empty_nul_or_non_utf8_value_is_refused_without_echoing_it() {
        for (bytes, expected) in [
            (&b""[..], "secret_value_invalid"),
            (b"\n", "secret_value_invalid"),
            (b"ab\0cd\n", "secret_value_invalid"),
            (b"\xff\xfe\n", "secret_value_not_utf8"),
        ] {
            let failure = read_bounded(&mut Cursor::new(bytes.to_vec())).unwrap_err();
            assert_eq!(kind(&failure), expected, "{bytes:?}");
            assert!(!failure.body.message().contains("ab"));
        }
    }

    #[test]
    fn a_failing_reader_is_refused_with_a_plain_message() {
        struct Broken;
        impl Read for Broken {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::other("secret-in-the-error"))
            }
        }
        let failure = read_bounded(&mut Broken).unwrap_err();
        assert_eq!(kind(&failure), "secret_read_failed");
        assert!(!failure.body.message().contains("secret-in-the-error"));
    }

    #[test]
    fn the_prompt_asks_twice_and_takes_equal_entries() {
        let (mut cx, stderr) = context(true, "");
        let mut hidden = Scripted::new(&["s3cret-value", "s3cret-value"]);
        let input = read_input(
            &mut cx,
            &SecretSource::Prompt,
            "local:user/token",
            &mut hidden,
            &no_env,
        )
        .unwrap();
        assert_eq!(value(input), "s3cret-value");
        assert_eq!(hidden.reads, 2);
        let shown = stderr.text();
        assert!(shown.contains("Value for local:user/token: "), "{shown}");
        assert!(shown.contains("Repeat the value: "), "{shown}");
        assert!(!shown.contains("s3cret-value"), "{shown}");
    }

    #[test]
    fn differing_entries_are_asked_again_and_fail_after_three_pairs() {
        let (mut cx, stderr) = context(true, "");
        let mut hidden = Scripted::new(&["a", "b", "a", "c", "a", "d"]);
        let failure = read_input(
            &mut cx,
            &SecretSource::Prompt,
            "local:user/token",
            &mut hidden,
            &no_env,
        )
        .unwrap_err();
        assert_eq!(kind(&failure), "secret_entries_differ");
        assert_eq!(failure.exit_code(), 2);
        assert_eq!(hidden.reads, 6);
        assert_eq!(
            stderr
                .text()
                .matches("the two entries differ, try again")
                .count(),
            2
        );

        let (mut cx, _) = context(true, "");
        let mut hidden = Scripted::new(&["a", "b", "x", "x"]);
        let input = read_input(
            &mut cx,
            &SecretSource::Prompt,
            "local:user/token",
            &mut hidden,
            &no_env,
        )
        .unwrap();
        assert_eq!(value(input), "x");
    }

    #[test]
    fn the_prompt_fails_at_once_noninteractive_without_reading_anything() {
        let (mut cx, stderr) = context(false, "typed\n");
        let mut hidden = Scripted::new(&["a", "a"]);
        let failure = read_input(
            &mut cx,
            &SecretSource::Prompt,
            "local:user/token",
            &mut hidden,
            &no_env,
        )
        .unwrap_err();
        assert!(failure.body.is(&cli::INPUT_REQUIRED), "{failure:?}");
        assert_eq!(failure.exit_code(), 2);
        assert_eq!(hidden.reads, 0);
        assert_eq!(stderr.text(), "");
    }

    #[test]
    fn a_failed_terminal_read_is_refused_without_a_value() {
        let (mut cx, _) = context(true, "");
        let mut hidden = Scripted::new(&[]);
        let failure = read_input(
            &mut cx,
            &SecretSource::Prompt,
            "local:user/token",
            &mut hidden,
            &no_env,
        )
        .unwrap_err();
        assert_eq!(kind(&failure), "secret_prompt_failed");
    }

    #[test]
    fn stdin_reads_cx_input_even_noninteractive() {
        let (mut cx, _) = context(false, "from-stdin\r\n");
        let mut hidden = Scripted::new(&[]);
        let input = read_input(&mut cx, &SecretSource::Stdin, "x", &mut hidden, &no_env).unwrap();
        assert_eq!(value(input), "from-stdin");
    }

    #[test]
    fn an_environment_value_is_taken_as_it_is_and_unset_or_empty_is_refused() {
        let env = |name: &str| match name {
            "SET" => Some(OsString::from("  padded value\n")),
            "EMPTY" => Some(OsString::new()),
            "BAD" => Some(std::os::unix::ffi::OsStringExt::from_vec(vec![0xff, 0xfe])),
            _ => None,
        };
        assert_eq!(read_env("SET", &env).unwrap().expose(), "  padded value\n");
        for (name, expected) in [
            ("EMPTY", "secret_env_missing"),
            ("UNSET", "secret_env_missing"),
            ("BAD", "secret_value_not_utf8"),
        ] {
            assert_eq!(kind(&read_env(name, &env).unwrap_err()), expected, "{name}");
        }
    }

    #[test]
    fn generate_and_copy_from_name_a_source_for_the_daemon() {
        let (mut cx, _) = context(false, "");
        let mut hidden = Scripted::new(&[]);
        assert_eq!(
            read_input(&mut cx, &SecretSource::Generate, "x", &mut hidden, &no_env),
            Ok(SecretInput::Generate)
        );
        assert_eq!(
            read_input(
                &mut cx,
                &SecretSource::CopyFrom("local:user/other".to_owned()),
                "x",
                &mut hidden,
                &no_env
            ),
            Ok(SecretInput::CopyFrom("local:user/other".to_owned()))
        );
    }

    #[test]
    fn a_value_file_must_be_a_regular_file_and_a_loose_mode_is_flagged() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("value");
        std::fs::write(&path, "from-file\r\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let read = read_file(&path).unwrap();
        assert_eq!(read.text.expose(), "from-file");
        assert!(!read.loose_permissions);

        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(read_file(&path).unwrap().loose_permissions);

        let link = dir.path().join("link");
        symlink(&path, &link).unwrap();
        assert_eq!(kind(&read_file(&link).unwrap_err()), "secret_file_symlink");
        assert_eq!(
            kind(&read_file(dir.path()).unwrap_err()),
            "secret_file_not_regular"
        );
        assert_eq!(
            kind(&read_file(&dir.path().join("missing")).unwrap_err()),
            "secret_file_unreadable"
        );

        let big = dir.path().join("big");
        std::fs::write(&big, vec![b'a'; MAX_READ + 1]).unwrap();
        assert_eq!(kind(&read_file(&big).unwrap_err()), "secret_value_too_long");
        let empty = dir.path().join("empty");
        std::fs::write(&empty, "").unwrap();
        assert_eq!(
            kind(&read_file(&empty).unwrap_err()),
            "secret_value_invalid"
        );
    }

    #[test]
    fn a_loose_value_file_adds_a_warning_and_still_reads() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("value");
        std::fs::write(&path, "from-file").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();
        let (mut cx, _) = context(false, "");
        let mut hidden = Scripted::new(&[]);
        let input = read_input(
            &mut cx,
            &SecretSource::File(path),
            "x",
            &mut hidden,
            &no_env,
        )
        .unwrap();
        assert_eq!(value(input), "from-file");
        assert_eq!(cx.output.warnings.len(), 1);
        assert_eq!(cx.output.warnings[0].code, WARNING_FILE_MODE);
    }

    #[test]
    fn no_source_flag_means_the_prompt_and_two_flags_conflict() {
        #[derive(Debug, clap::Parser)]
        struct Probe {
            #[command(flatten)]
            source: SourceArgs,
        }
        use clap::Parser;

        let source = |list: &[&str]| {
            Probe::try_parse_from(std::iter::once("probe").chain(list.iter().copied()))
                .map(|probe| probe.source.source())
        };
        assert_eq!(source(&[]).unwrap(), SecretSource::Prompt);
        assert_eq!(source(&["--stdin"]).unwrap(), SecretSource::Stdin);
        assert_eq!(source(&["--generate"]).unwrap(), SecretSource::Generate);
        assert_eq!(
            source(&["--from-env", "TOKEN"]).unwrap(),
            SecretSource::Env("TOKEN".to_owned())
        );
        assert_eq!(
            source(&["--from-file", "/x"]).unwrap(),
            SecretSource::File(PathBuf::from("/x"))
        );
        assert_eq!(
            source(&["--copy-from", "local:user/other"]).unwrap(),
            SecretSource::CopyFrom("local:user/other".to_owned())
        );
        for list in [
            &["--stdin", "--generate"][..],
            &["--from-env", "A", "--from-file", "b"],
            &["--generate", "--copy-from", "local:user/other"],
            &["--copy-from", "bad"],
            &["--password", "x"],
            &["--value", "x"],
        ] {
            assert!(source(list).is_err(), "{list:?}");
        }
    }

    #[test]
    fn the_secret_flag_spells_every_source_of_a_field() {
        let ok = |text: &str| field_source(text).unwrap();
        assert_eq!(
            ok("password=generate"),
            ("password".to_owned(), SecretSource::Generate)
        );
        assert_eq!(ok("password=prompt").1, SecretSource::Prompt);
        assert_eq!(ok("password=stdin").1, SecretSource::Stdin);
        assert_eq!(
            ok("tls.key=file:/tmp/key.pem").1,
            SecretSource::File(PathBuf::from("/tmp/key.pem"))
        );
        assert_eq!(
            ok("p=env:DB_PASSWORD").1,
            SecretSource::Env("DB_PASSWORD".to_owned())
        );
        assert_eq!(
            ok("p=copy:local:user/other").1,
            SecretSource::CopyFrom("local:user/other".to_owned())
        );
        assert_eq!(
            ok("p=bind:local:user/other").1,
            SecretSource::Bind("local:user/other".to_owned())
        );
        for text in [
            "",
            "password",
            "=generate",
            "password=",
            "password=value",
            "password=file:",
            "password=env:",
            "password=env:A=B",
            "password=copy:bad",
            "password=bind:",
            "password=other:x",
            "pass word=generate",
        ] {
            assert!(field_source(text).is_err(), "{text}");
        }
    }
}
