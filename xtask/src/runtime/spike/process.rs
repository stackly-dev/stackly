//! Provider processes of the spike (spec 0003): each runs directly as a
//! child in its own process group, with an emptied environment, and every
//! group is killed on every way out of the worker.

use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::{Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

/// `kill`; process groups are signalled as `-<pgid>`.
const KILL: &str = "/bin/kill";
/// The most of each log a result keeps.
pub const LOG_TAIL_BYTES: u64 = 4 * 1024;
/// How long a group gets between TERM and KILL when it is torn down.
const GROUP_GRACE: Duration = Duration::from_secs(5);

/// Every process group the worker started and has not seen end.
static GROUPS: Mutex<Vec<u32>> = Mutex::new(Vec::new());

/// The emptied environment of every provider process (AC-6).
#[derive(Debug, Clone)]
pub struct CleanEnv {
    /// The working folder.
    pub cwd: PathBuf,
    /// `HOME`, a new folder in the run folder.
    pub home: PathBuf,
    /// `TMPDIR`, a new folder in the run folder.
    pub tmpdir: PathBuf,
}

impl CleanEnv {
    /// A command for `program` with only `PATH`, `HOME`, `TMPDIR`, and
    /// `LANG` set, no `DYLD_*` or Homebrew variable, and stdin closed.
    pub fn command(&self, program: &Path) -> Command {
        let mut command = Command::new(program);
        command
            .env_clear()
            .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
            .env("HOME", &self.home)
            .env("TMPDIR", &self.tmpdir)
            .env("LANG", "en_US.UTF-8")
            .current_dir(&self.cwd)
            .stdin(Stdio::null());
        command
    }
}

/// A long running provider process.
#[derive(Debug)]
pub struct Server {
    /// The name used in logs and the result, such as `server`.
    pub name: String,
    /// The child; its pid is also its process group ID.
    pub child: Child,
    /// Its stdout file.
    pub stdout: PathBuf,
    /// Its stderr file.
    pub stderr: PathBuf,
    /// When it was spawned.
    pub started: Instant,
}

impl Server {
    /// Spawns `command` in a new process group, with stdout and stderr
    /// going to `<logs>/<name>.stdout.log` and `.stderr.log`.
    pub fn spawn(name: &str, mut command: Command, logs: &Path) -> Result<Self> {
        let stdout = logs.join(format!("{name}.stdout.log"));
        let stderr = logs.join(format!("{name}.stderr.log"));
        let out =
            File::create(&stdout).with_context(|| format!("cannot create {}", stdout.display()))?;
        let err =
            File::create(&stderr).with_context(|| format!("cannot create {}", stderr.display()))?;
        let child = command
            .process_group(0)
            .stdout(out)
            .stderr(err)
            .spawn()
            .with_context(|| format!("cannot start {name}"))?;
        track(child.id());
        Ok(Self {
            name: name.to_owned(),
            child,
            stdout,
            stderr,
            started: Instant::now(),
        })
    }

    /// The pid, which is also the process group ID.
    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    /// The exit status when the process already ended.
    pub fn try_exit(&mut self) -> Result<Option<ExitStatus>> {
        let status = self
            .child
            .try_wait()
            .with_context(|| format!("cannot check {}", self.name))?;
        if status.is_some() {
            kill_group(self.pid());
        }
        Ok(status)
    }

    /// Waits at most `timeout` for the process to end.
    pub fn wait_for(&mut self, timeout: Duration) -> Result<Option<ExitStatus>> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(status) = self.try_exit()? {
                return Ok(Some(status));
            }
            if Instant::now() >= deadline {
                return Ok(None);
            }
            thread::sleep(Duration::from_millis(100));
        }
    }

    /// Sends `signal` (such as `TERM` or `INT`) to the process only.
    pub fn signal(&self, signal: &str) -> Result<()> {
        send(signal, &self.pid().to_string())
    }

    /// Kills the whole group and reaps the child.
    pub fn kill(&mut self) {
        terminate(&mut self.child);
    }
}

/// The exit code and signal of a status, for the result.
pub fn code_and_signal(status: ExitStatus) -> (Option<i32>, Option<i32>) {
    (status.code(), status.signal())
}

/// Describes a status as `exit 3` or `signal 9`.
pub fn describe(status: ExitStatus) -> String {
    match (status.code(), status.signal()) {
        (Some(code), _) => format!("exit {code}"),
        (None, Some(signal)) => format!("signal {signal}"),
        (None, None) => "an unknown status".to_owned(),
    }
}

/// What a short command left behind.
#[derive(Debug)]
pub struct Finished {
    /// `None` when it ran past its time and was killed.
    pub status: Option<ExitStatus>,
    /// Everything it printed on stdout.
    pub stdout: String,
    /// Everything it printed on stderr.
    pub stderr: String,
}

impl Finished {
    /// True when it exited with code 0.
    pub fn success(&self) -> bool {
        self.status.is_some_and(|status| status.success())
    }

    /// `stderr`, or `stdout` when stderr is empty, trimmed for a reason.
    pub fn complaint(&self) -> String {
        let text = if self.stderr.trim().is_empty() {
            &self.stdout
        } else {
            &self.stderr
        };
        let text = text.trim();
        let start = text.len().saturating_sub(400);
        let start = (start..text.len())
            .find(|i| text.is_char_boundary(*i))
            .unwrap_or(text.len());
        text[start..].to_owned()
    }
}

/// Runs a short command in its own process group and kills the group when
/// it takes longer than `timeout`.
pub fn run(mut command: Command, timeout: Duration) -> Result<Finished> {
    let program = command.get_program().to_string_lossy().into_owned();
    let mut child = command
        .process_group(0)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("cannot run {program}"))?;
    track(child.id());
    let stdout = reader(child.stdout.take());
    let stderr = reader(child.stderr.take());
    let deadline = Instant::now() + timeout;
    let status = loop {
        if let Some(status) = child
            .try_wait()
            .with_context(|| format!("cannot wait for {program}"))?
        {
            break Some(status);
        }
        if Instant::now() >= deadline {
            break None;
        }
        thread::sleep(Duration::from_millis(50));
    };
    if status.is_some() {
        kill_group(child.id());
    } else {
        terminate(&mut child);
    }
    Ok(Finished {
        status,
        stdout: stdout.join().unwrap_or_default(),
        stderr: stderr.join().unwrap_or_default(),
    })
}

fn reader(pipe: Option<impl Read + Send + 'static>) -> thread::JoinHandle<String> {
    thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(mut pipe) = pipe {
            // Ignored: a broken pipe keeps what was read.
            let _ = pipe.read_to_end(&mut bytes);
        }
        String::from_utf8_lossy(&bytes).into_owned()
    })
}

/// Sends TERM to the child's group, reaps the child once it ends (so its
/// zombie does not look alive), then kills what is left of the group.
fn terminate(child: &mut Child) {
    let pgid = child.id();
    // Ignored: the group may already be gone.
    let _ = send("TERM", &format!("-{pgid}"));
    let deadline = Instant::now() + GROUP_GRACE;
    while Instant::now() < deadline && !matches!(child.try_wait(), Ok(Some(_))) {
        thread::sleep(Duration::from_millis(50));
    }
    if !matches!(child.try_wait(), Ok(Some(_))) {
        let _ = send("KILL", &format!("-{pgid}"));
    }
    // Ignored: the child may already be reaped.
    let _ = child.wait();
    kill_group(pgid);
}

/// Remembers a process group so every way out of the worker kills it.
fn track(pgid: u32) {
    GROUPS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push(pgid);
}

/// TERM then, when needed, KILL for one process group.
pub fn kill_group(pgid: u32) {
    let group = format!("-{pgid}");
    if send("TERM", &group).is_err() {
        // The group is already gone.
        untrack(pgid);
        return;
    }
    let deadline = Instant::now() + GROUP_GRACE;
    while Instant::now() < deadline {
        if send("0", &group).is_err() {
            untrack(pgid);
            return;
        }
        thread::sleep(Duration::from_millis(100));
    }
    // Ignored: the group may end between the checks.
    let _ = send("KILL", &group);
    untrack(pgid);
}

/// Kills every group the worker started, for the signal handler, the
/// watchdog, and the normal exit.
pub fn kill_all_groups() {
    let groups: Vec<u32> = GROUPS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    for pgid in groups {
        kill_group(pgid);
    }
}

fn untrack(pgid: u32) {
    GROUPS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .retain(|tracked| *tracked != pgid);
}

/// Runs `kill -<signal> -- <target>`; `target` is a pid or `-<pgid>`.
/// Signal `0` only checks that the target exists.
pub fn send(signal: &str, target: &str) -> Result<()> {
    let status = Command::new(KILL)
        .arg(format!("-{signal}"))
        .arg("--")
        .arg(target)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .with_context(|| format!("cannot run {KILL}"))?;
    if !status.success() {
        bail!("kill -{signal} {target} failed");
    }
    Ok(())
}

/// The last `max` bytes of a file, cut at a character boundary.
pub fn tail(path: &Path, max: u64) -> String {
    let Ok(mut file) = File::open(path) else {
        return String::new();
    };
    let len = file.metadata().map(|meta| meta.len()).unwrap_or(0);
    if file.seek(SeekFrom::Start(len.saturating_sub(max))).is_err() {
        return String::new();
    }
    let mut bytes = Vec::new();
    if file.read_to_end(&mut bytes).is_err() {
        return String::new();
    }
    let text = String::from_utf8_lossy(&bytes);
    text.trim_start_matches('\u{fffd}').to_owned()
}

/// A minimal HTTP/1.0 GET to `127.0.0.1:<port>`: the status code and the
/// body. Never through a proxy.
pub fn http_get(port: u16, path: &str, timeout: Duration) -> Result<(u16, String)> {
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let mut stream = TcpStream::connect_timeout(&address, timeout)
        .with_context(|| format!("cannot connect to 127.0.0.1:{port}"))?;
    stream
        .set_read_timeout(Some(timeout))
        .context("cannot set a read timeout")?;
    stream
        .set_write_timeout(Some(timeout))
        .context("cannot set a write timeout")?;
    write!(
        stream,
        "GET {path} HTTP/1.0\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
    )
    .with_context(|| format!("cannot send a request to 127.0.0.1:{port}"))?;
    let mut response = Vec::new();
    match stream.read_to_end(&mut response) {
        Ok(_) => {}
        Err(err) if err.kind() == io::ErrorKind::ConnectionReset && !response.is_empty() => {}
        Err(err) => return Err(err).with_context(|| format!("cannot read from 127.0.0.1:{port}")),
    }
    parse_response(&String::from_utf8_lossy(&response))
}

fn parse_response(text: &str) -> Result<(u16, String)> {
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((text, ""));
    let status = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse().ok())
        .with_context(|| {
            format!(
                "not an HTTP response: `{}`",
                head.lines().next().unwrap_or_default()
            )
        })?;
    Ok((status, body.to_owned()))
}

/// Creates a folder and its parents.
pub fn create_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path).with_context(|| format!("cannot create {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env() -> (tempfile::TempDir, CleanEnv) {
        let dir = tempfile::tempdir().unwrap();
        let env = CleanEnv {
            cwd: dir.path().to_path_buf(),
            home: dir.path().join("home"),
            tmpdir: dir.path().join("tmp"),
        };
        (dir, env)
    }

    #[test]
    fn a_provider_process_sees_only_the_clean_environment() {
        let (_dir, env) = env();
        let mut command = env.command(Path::new("/usr/bin/env"));
        command.env_remove("NOTHING");
        let finished = run(command, Duration::from_secs(10)).unwrap();
        assert!(finished.success());
        let mut names: Vec<&str> = finished
            .stdout
            .lines()
            .filter_map(|line| line.split_once('=').map(|(name, _)| name))
            .collect();
        names.sort_unstable();
        assert_eq!(names, ["HOME", "LANG", "PATH", "TMPDIR"]);
    }

    #[test]
    fn a_command_past_its_time_is_killed_with_its_group() {
        let (_dir, env) = env();
        let mut command = env.command(Path::new("/bin/sh"));
        command.args(["-c", "sleep 30 & sleep 30"]);
        let started = Instant::now();
        let finished = run(command, Duration::from_millis(300)).unwrap();
        assert!(finished.status.is_none());
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn a_server_stops_on_term_and_reports_its_status() {
        let (dir, env) = env();
        let mut command = env.command(Path::new("/bin/sleep"));
        command.arg("30");
        let mut server = Server::spawn("server", command, dir.path()).unwrap();
        assert!(server.try_exit().unwrap().is_none());
        server.signal("TERM").unwrap();
        let status = server.wait_for(Duration::from_secs(10)).unwrap().unwrap();
        assert_eq!(code_and_signal(status), (None, Some(15)));
        assert_eq!(describe(status), "signal 15");
    }

    #[test]
    fn tail_keeps_the_last_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("log");
        fs::write(&path, "abcdef").unwrap();
        assert_eq!(tail(&path, 3), "def");
        assert_eq!(tail(&path, 100), "abcdef");
        assert_eq!(tail(&dir.path().join("none"), 3), "");
    }

    #[test]
    fn an_http_response_gives_its_status_and_body() {
        assert_eq!(
            parse_response("HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\nOk.").unwrap(),
            (200, "Ok.".to_owned())
        );
        assert!(parse_response("garbage").is_err());
    }
}
