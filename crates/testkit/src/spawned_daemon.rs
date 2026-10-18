//! Runs the real `stacklyd` binary against a test root (spec 0006,
//! *Testkit*).

use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// How long [`SpawnedDaemon::start`] waits for the daemon to be ready.
pub const DAEMON_START_TIMEOUT: Duration = Duration::from_secs(10);
/// How long [`SpawnedDaemon::stop`] waits after SIGTERM before it kills.
pub const DAEMON_STOP_TIMEOUT: Duration = Duration::from_secs(5);
/// How often a wait checks the process and the ready condition.
const POLL: Duration = Duration::from_millis(10);

/// A `stacklyd` process started for one test, which never outlives it.
///
/// The daemon runs with `STACKLY_ROOT=<root>` and `STACKLY_LOG=debug`, its
/// stdout goes nowhere and its stderr to a fresh file inside the root. On
/// drop the process gets SIGTERM (through `/bin/kill`, so testkit needs no
/// libc), then is killed if it still runs after
/// [`DAEMON_STOP_TIMEOUT`]. Once it stopped, the runtime root of the run is
/// removed, but only when it is empty or holds nothing but an empty `i/`;
/// `STACKLY_ROOT` places the runtime root in the real user temporary
/// directory, so nothing else there is ever touched.
///
/// Tests take `socket` and `runtime_root` from
/// `MacosPlatform::new(Some(root))`, so the paths are the ones the daemon
/// computes. Testkit does not depend on `stackly-ipc`: a test that needs a
/// verified connection passes it in the ready check.
#[derive(Debug)]
pub struct SpawnedDaemon {
    child: Child,
    status: Option<ExitStatus>,
    stderr: PathBuf,
    socket: PathBuf,
    runtime_root: PathBuf,
}

impl SpawnedDaemon {
    /// Starts `bin` and waits up to [`DAEMON_START_TIMEOUT`] for `ready`
    /// to return true. `ready` gets the daemon, so it can read
    /// [`stderr`](Self::stderr) or connect to [`socket`](Self::socket).
    ///
    /// # Errors
    ///
    /// Returns the I/O error when the process cannot start, and an error
    /// that quotes the daemon's stderr when it exits before it is ready
    /// or is not ready in time (it is stopped first).
    pub fn start(
        bin: &Path,
        root: &Path,
        socket: &Path,
        runtime_root: &Path,
        ready: impl FnMut(&Self) -> bool,
    ) -> io::Result<Self> {
        let mut daemon = Self::spawn(bin, root, socket, runtime_root, &[])?;
        daemon.wait_ready(ready)?;
        Ok(daemon)
    }

    /// Starts `bin` with `args` and returns at once, for a daemon that is
    /// expected to exit on its own (see [`wait_exit`](Self::wait_exit)).
    ///
    /// # Errors
    ///
    /// Returns the I/O error when the stderr file cannot be created or the
    /// process cannot start.
    pub fn spawn(
        bin: &Path,
        root: &Path,
        socket: &Path,
        runtime_root: &Path,
        args: &[&str],
    ) -> io::Result<Self> {
        let (file, stderr) = tempfile::Builder::new()
            .prefix("stacklyd-")
            .suffix(".stderr")
            .tempfile_in(root)?
            .keep()
            .map_err(|error| error.error)?;
        let child = Command::new(bin)
            .args(args)
            .env("STACKLY_ROOT", root)
            .env("STACKLY_LOG", "debug")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::from(file))
            .spawn()?;
        Ok(Self {
            child,
            status: None,
            stderr,
            socket: socket.to_path_buf(),
            runtime_root: runtime_root.to_path_buf(),
        })
    }

    /// Waits up to [`DAEMON_START_TIMEOUT`] for `ready` to return true.
    ///
    /// # Errors
    ///
    /// Returns an error that quotes the daemon's stderr when it exits
    /// first or the time runs out.
    pub fn wait_ready(&mut self, mut ready: impl FnMut(&Self) -> bool) -> io::Result<()> {
        let deadline = Instant::now() + DAEMON_START_TIMEOUT;
        loop {
            if ready(self) {
                return Ok(());
            }
            if let Some(status) = self.try_status()? {
                return Err(self.failure(&format!(
                    "stacklyd exited with {status} before it was ready"
                )));
            }
            if Instant::now() >= deadline {
                return Err(self.failure("stacklyd was not ready in time"));
            }
            thread::sleep(POLL);
        }
    }

    /// Returns the daemon's process ID.
    #[must_use]
    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    /// Returns the socket path the test passed in.
    #[must_use]
    pub fn socket(&self) -> &Path {
        &self.socket
    }

    /// Returns the file that holds the daemon's stderr.
    #[must_use]
    pub fn stderr_path(&self) -> &Path {
        &self.stderr
    }

    /// Returns what the daemon wrote to stderr so far (empty when the file
    /// cannot be read).
    #[must_use]
    pub fn stderr(&self) -> String {
        std::fs::read(&self.stderr)
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .unwrap_or_default()
    }

    /// Returns the exit status once the process ended, without waiting.
    ///
    /// # Errors
    ///
    /// Returns the I/O error of the wait call.
    pub fn try_status(&mut self) -> io::Result<Option<ExitStatus>> {
        if self.status.is_none() {
            self.status = self.child.try_wait()?;
        }
        Ok(self.status)
    }

    /// Sends SIGTERM, waits up to [`DAEMON_STOP_TIMEOUT`], kills the process
    /// if it still runs, and returns how it ended.
    ///
    /// # Errors
    ///
    /// Returns the I/O error of a signal or wait call.
    pub fn stop(mut self) -> io::Result<ExitStatus> {
        self.terminate()
    }

    /// Waits up to `timeout` for a daemon that should exit on its own, and
    /// returns its exit status.
    ///
    /// # Errors
    ///
    /// Returns an error that quotes the daemon's stderr when it still runs
    /// after `timeout` (it is stopped first).
    pub fn wait_exit(mut self, timeout: Duration) -> io::Result<ExitStatus> {
        match self.wait_for(timeout)? {
            Some(status) => Ok(status),
            None => Err(self.failure("stacklyd did not exit in time")),
        }
    }

    fn wait_for(&mut self, timeout: Duration) -> io::Result<Option<ExitStatus>> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(status) = self.try_status()? {
                return Ok(Some(status));
            }
            if Instant::now() >= deadline {
                return Ok(None);
            }
            thread::sleep(POLL);
        }
    }

    fn terminate(&mut self) -> io::Result<ExitStatus> {
        if let Some(status) = self.try_status()? {
            return Ok(status);
        }
        let _ = Command::new("/bin/kill")
            .arg("-TERM")
            .arg(self.pid().to_string())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        if let Some(status) = self.wait_for(DAEMON_STOP_TIMEOUT)? {
            return Ok(status);
        }
        let _ = self.child.kill();
        let status = self.child.wait()?;
        self.status = Some(status);
        Ok(status)
    }

    /// Stops the daemon, then builds an error quoting its stderr.
    fn failure(&mut self, what: &str) -> io::Error {
        let _ = self.terminate();
        io::Error::other(format!("{what}; stderr:\n{}", self.stderr()))
    }

    /// Removes the run's runtime root when it is empty or holds nothing
    /// but an empty `i/`. Anything else stays.
    fn remove_runtime_root(&self) {
        let Ok(entries) = std::fs::read_dir(&self.runtime_root) else {
            return;
        };
        let names: Vec<_> = entries
            .filter_map(Result::ok)
            .map(|entry| entry.file_name())
            .collect();
        let runs = self.runtime_root.join("i");
        let only_empty_runs = names.len() == 1
            && names[0] == "i"
            && std::fs::symlink_metadata(&runs).is_ok_and(|info| info.is_dir())
            && std::fs::read_dir(&runs).is_ok_and(|mut entries| entries.next().is_none());
        if only_empty_runs {
            let _ = std::fs::remove_dir(&runs);
        }
        if names.is_empty() || only_empty_runs {
            let _ = std::fs::remove_dir(&self.runtime_root);
        }
    }
}

impl Drop for SpawnedDaemon {
    fn drop(&mut self) {
        let _ = self.terminate();
        self.remove_runtime_root();
    }
}
