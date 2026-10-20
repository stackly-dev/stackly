//! Running `stackly` with stderr (and stdin when asked) on a real pty,
//! through `/usr/bin/script -q /dev/null /bin/sh -c '...'`. stdout goes to
//! a file in the test root, so the pty's `\n` to `\r\n` never touches it,
//! and the exit code is read from a file, not from `script`.

use std::path::Path;
use std::process::{Command, Stdio};

use super::STACKLY;

/// What one pty run left.
#[derive(Debug)]
pub struct PtyRun {
    /// The exit code `stackly` returned.
    pub code: Option<i32>,
    /// stdout, from its file, byte for byte.
    pub stdout: String,
    /// What the pty showed, which is stderr, with the `^D` echo of the
    /// closed input, the echo of the typed input, and the pty's `\r\n`
    /// taken out.
    pub stderr: String,
    /// What the pty showed, exactly.
    pub raw_stderr: String,
}

/// Options of one pty run.
#[derive(Debug, Clone, Default)]
pub struct PtyOptions<'a> {
    /// Extra environment variables.
    pub env: Vec<(&'a str, &'a str)>,
    /// Whether stdin is the pty too; otherwise it is `/dev/null`.
    pub stdin_tty: bool,
    /// What to type on the pty, when stdin is the pty.
    pub input: Option<&'a str>,
    /// Sends `SIGINT` to `stackly` this long after it started, as Ctrl+C
    /// does, without touching the terminal modes.
    pub interrupt_after: Option<std::time::Duration>,
}

fn quote(arg: &str) -> String {
    format!("'{}'", arg.replace('\'', "'\\''"))
}

/// Runs `stackly <args>` for `root` with stderr on a pty.
pub fn run_in_pty(root: &Path, args: &[&str], options: &PtyOptions<'_>) -> PtyRun {
    let dir = tempfile_dir(root);
    let out = dir.join("stdout");
    let status = dir.join("status");
    let mut line = String::new();
    if let Some(after) = options.interrupt_after {
        // `stackly` takes the shell's place, so the file holds its own pid
        // and runs in the foreground with the default signal handling.
        line.push_str(&format!(
            "(sleep {}; kill -INT \"$(cat \"$PIDFILE\")\") & /bin/sh -c 'echo $$ >\"$PIDFILE\"; exec \"$@\"' sh ",
            after.as_secs_f32()
        ));
    }
    line.push_str(&quote(STACKLY));
    for arg in args {
        line.push(' ');
        line.push_str(&quote(arg));
    }
    line.push_str(" >\"$OUT\"");
    if !options.stdin_tty {
        line.push_str(" </dev/null");
    }
    line.push_str("; echo $? >\"$STATUS\"");
    let mut command = Command::new("/usr/bin/script");
    command
        .args(["-q", "/dev/null", "/bin/sh", "-c", &line])
        .env("OUT", &out)
        .env("STATUS", &status)
        .env("PIDFILE", dir.join("pid"))
        .env("STACKLY_ROOT", root)
        .env("TZ", "UTC")
        .env("TERM", "xterm-256color")
        .env_remove("NO_COLOR")
        .env_remove("CLICOLOR")
        .env_remove("CLICOLOR_FORCE")
        .env_remove("STACKLY_NO_INPUT")
        .env_remove("CI")
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    for (key, value) in &options.env {
        command.env(key, value);
    }
    let output = match options.input {
        Some(input) => {
            use std::io::{Read, Write};
            command.stdin(Stdio::piped());
            let mut child = command.spawn().expect("script runs");
            // `script` sends EOF to the pty as soon as its own stdin ends,
            // so stdin stays open until the child is done.
            let mut stdin = child.stdin.take().expect("stdin is piped");
            stdin
                .write_all(input.as_bytes())
                .expect("the pty takes the input");
            let mut shown = Vec::new();
            child
                .stdout
                .take()
                .expect("stdout is piped")
                .read_to_end(&mut shown)
                .expect("the pty output can be read");
            let status = child.wait().expect("script ends");
            drop(stdin);
            std::process::Output {
                status,
                stdout: shown,
                stderr: Vec::new(),
            }
        }
        None => command.stdin(Stdio::null()).output().expect("script runs"),
    };
    let raw_stderr = String::from_utf8_lossy(&output.stdout).into_owned();
    let typed = options.input.map(|input| input.replace('\n', "\r\n"));
    let stderr = raw_stderr
        .strip_prefix("^D\u{8}\u{8}")
        .unwrap_or(&raw_stderr);
    let stderr = typed
        .as_deref()
        .and_then(|typed| stderr.strip_prefix(typed))
        .unwrap_or(stderr)
        .replace("\r\n", "\n");
    let code = std::fs::read_to_string(&status)
        .ok()
        .and_then(|text| text.trim().parse().ok());
    PtyRun {
        code,
        stdout: std::fs::read_to_string(&out).unwrap_or_default(),
        stderr,
        raw_stderr,
    }
}

/// A fresh folder for one run's files, inside the test root's run folder.
fn tempfile_dir(root: &Path) -> std::path::PathBuf {
    let base = root.join("pty");
    std::fs::create_dir_all(&base).expect("the test root is writable");
    let mut n = 0;
    loop {
        let dir = base.join(n.to_string());
        if std::fs::create_dir(&dir).is_ok() {
            return dir;
        }
        n += 1;
    }
}
