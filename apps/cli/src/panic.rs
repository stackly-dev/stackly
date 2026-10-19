//! The CLI's own panic hook (spec 0008, AC-10). The default hook prints the
//! panic message before `catch_unwind` can catch it, and that message may
//! hold data, so this hook only keeps the location for `-v`. A panic on the
//! main thread (`catch_unwind`) or in the command's tokio task (its
//! `JoinError`) becomes `cli.panic`, exit 1, written as an envelope when
//! `--json` is known.

use std::io::Write;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::failure::CliFailure;
use crate::output::envelope::{Envelope, to_line};
use crate::output::stderr::StderrSink;

/// Where the first panic happened, as `file:line`.
static LOCATION: OnceLock<String> = OnceLock::new();
/// `--json` was found in the arguments.
static JSON: AtomicBool = AtomicBool::new(false);
/// `-v` was given.
static VERBOSE: AtomicBool = AtomicBool::new(false);
/// The chosen command path, once clap parsed the arguments.
static COMMAND: OnceLock<String> = OnceLock::new();

/// Installs the hook. Called first thing in `main`.
pub fn install_hook() {
    std::panic::set_hook(Box::new(|info| {
        if let Some(location) = info.location() {
            let _ = LOCATION.set(format!("{}:{}", location.file(), location.line()));
        }
    }));
}

/// Records whether `--json` is in the arguments, before clap parses them.
pub fn set_json(json: bool) {
    JSON.store(json, Ordering::Relaxed);
}

/// Records the command clap chose and whether `-v` was given.
pub fn set_command(command: &str, verbose: bool) {
    let _ = COMMAND.set(command.to_owned());
    VERBOSE.store(verbose, Ordering::Relaxed);
}

/// The exit code of the command's task: its own, or `cli.panic` when the
/// task panicked (or was cancelled, which only a panic can cause here).
pub fn join(joined: Result<u8, tokio::task::JoinError>) -> u8 {
    joined_or(joined, report)
}

fn joined_or(joined: Result<u8, tokio::task::JoinError>, on_panic: impl FnOnce() -> u8) -> u8 {
    joined.unwrap_or_else(|_| on_panic())
}

/// Writes `cli.panic` to this process's stdout or stderr and returns its
/// exit code, 1.
pub fn report() -> u8 {
    let mut stdout = std::io::stdout();
    let mut stderr = StderrSink::new(Box::new(std::io::stderr()), false);
    write_report(
        &mut stdout,
        &mut stderr,
        JSON.load(Ordering::Relaxed),
        VERBOSE.load(Ordering::Relaxed),
        COMMAND.get().map_or("", String::as_str),
        LOCATION.get().map(String::as_str),
    )
}

/// Writes `cli.panic` as an envelope on `stdout` when `json`, or as an
/// error block on `stderr`; with `verbose`, the panic location goes to
/// stderr too. The panic message is never written.
fn write_report(
    stdout: &mut dyn Write,
    stderr: &mut StderrSink,
    json: bool,
    verbose: bool,
    command: &str,
    location: Option<&str>,
) -> u8 {
    let failure = CliFailure::panic();
    if json {
        let envelope = Envelope::<()>::new(command, None, Some(&*failure.body), None, &[]);
        if let Ok(line) = to_line(&envelope) {
            let _ = stdout.write_all(&line).and_then(|()| stdout.flush());
        }
    } else {
        stderr.error(&failure.body, None, &[], None);
    }
    if verbose {
        stderr.line(&format!(
            "panic at {}",
            location.unwrap_or("an unknown location")
        ));
    }
    failure.exit_code()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::stderr::Captured;
    use std::sync::{Mutex, MutexGuard, PoisonError};

    /// The panic hook is process global, so tests that swap it run one at a
    /// time; otherwise one test's panic can reach another test's hook.
    static HOOK: Mutex<()> = Mutex::new(());

    fn hook_lock() -> MutexGuard<'static, ()> {
        HOOK.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn report_to(json: bool, verbose: bool, location: Option<&str>) -> (u8, String, String) {
        let mut stdout = Vec::new();
        let stderr = Captured::default();
        let mut sink = StderrSink::new(Box::new(stderr.clone()), false);
        let code = write_report(
            &mut stdout,
            &mut sink,
            json,
            verbose,
            "daemon status",
            location,
        );
        (code, String::from_utf8(stdout).unwrap(), stderr.text())
    }

    #[test]
    fn a_panic_in_json_mode_is_one_cli_panic_envelope_and_exit_1() {
        let (code, stdout, stderr) = report_to(true, false, Some("src/x.rs:3"));
        assert_eq!(code, 1);
        let value: serde_json::Value = serde_json::from_str(&stdout).unwrap();
        assert_eq!(value["command"], "daemon status");
        assert_eq!(value["error"]["code"], "cli.panic");
        assert_eq!(value["error"]["category"], "internal");
        assert_eq!(value["result"], serde_json::Value::Null);
        assert_eq!(stderr, "");
    }

    #[test]
    fn a_panic_in_text_mode_names_the_location_only_with_verbose() {
        let (code, stdout, stderr) = report_to(false, false, Some("src/x.rs:3"));
        assert_eq!((code, stdout.as_str()), (1, ""));
        assert!(stderr.contains("code: cli.panic"));
        assert!(!stderr.contains("src/x.rs"));
        let (_, _, verbose) = report_to(false, true, Some("src/x.rs:3"));
        assert!(verbose.ends_with("panic at src/x.rs:3\n"), "{verbose}");
    }

    #[test]
    fn the_hook_keeps_the_location_and_never_the_message() {
        let _hook = hook_lock();
        let previous = std::panic::take_hook();
        install_hook();
        let caught = std::panic::catch_unwind(|| {
            std::panic::panic_any("secret-sentinel".to_owned());
        });
        std::panic::set_hook(previous);
        assert!(caught.is_err());
        let location = LOCATION.get().unwrap();
        assert!(location.starts_with("apps/cli/src/panic.rs:"), "{location}");
        assert!(!location.contains("secret-sentinel"));
    }

    #[test]
    fn a_panic_in_the_command_task_becomes_the_cli_panic_report() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let _hook = hook_lock();
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let (panicked, worked) = runtime.block_on(async {
            let panicked = tokio::spawn(async {
                if std::hint::black_box(true) {
                    panic!("secret-sentinel");
                }
                0_u8
            })
            .await;
            let worked = tokio::spawn(async { 3_u8 }).await;
            (panicked, worked)
        });
        std::panic::set_hook(previous);
        let mut stdout = Vec::new();
        let code = joined_or(panicked, || {
            let mut sink = StderrSink::new(Box::new(Captured::default()), false);
            write_report(&mut stdout, &mut sink, true, false, "operation list", None)
        });
        assert_eq!(code, 1);
        let text = String::from_utf8(stdout).unwrap();
        assert!(text.contains("\"code\":\"cli.panic\""), "{text}");
        assert!(!text.contains("secret-sentinel"));
        assert_eq!(joined_or(worked, || unreachable!()), 3);
    }
}
