//! A snapshot of the process table, for tests that count and reap the
//! processes a daemon started (spec 0014, *Suite ujung ke ujung*).

use std::io;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// How often a wait takes a new snapshot.
const POLL: Duration = Duration::from_millis(50);
/// How long [`ProcessTable::reap_under`] gives a process after SIGKILL.
const KILL_GRACE: Duration = Duration::from_secs(5);

/// One row of `ps`: a process ID and its whole argument vector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessEntry {
    /// The process ID.
    pub pid: u32,
    /// The arguments joined by single spaces, as `ps` prints them.
    pub args: String,
}

/// The processes of this Mac at one moment, from
/// `/bin/ps -axww -o pid=,args=`.
///
/// Every query leaves out the test process itself. A test finds the
/// processes of one instance by a path that appears in their arguments,
/// for example the data folder of the instance or the whole test root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessTable {
    entries: Vec<ProcessEntry>,
}

impl ProcessTable {
    /// Takes a snapshot.
    ///
    /// # Errors
    ///
    /// Returns the I/O error when `/bin/ps` cannot run, and an error that
    /// quotes its stderr when it fails.
    pub fn snapshot() -> io::Result<Self> {
        let output = Command::new("/bin/ps")
            .args(["-axww", "-o", "pid=,args="])
            .stdin(Stdio::null())
            .output()?;
        if !output.status.success() {
            return Err(io::Error::other(format!(
                "ps failed with {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        let own = std::process::id();
        let entries = String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(parse_line)
            .filter(|entry| entry.pid != own)
            .collect();
        Ok(Self { entries })
    }

    /// Returns every row of the snapshot.
    #[must_use]
    pub fn entries(&self) -> &[ProcessEntry] {
        &self.entries
    }

    /// Returns the rows whose arguments contain `needle`.
    #[must_use]
    pub fn containing(&self, needle: &str) -> Vec<&ProcessEntry> {
        self.entries
            .iter()
            .filter(|entry| entry.args.contains(needle))
            .collect()
    }

    /// Returns the rows whose arguments contain every one of `needles`.
    #[must_use]
    pub fn containing_all(&self, needles: &[&str]) -> Vec<&ProcessEntry> {
        self.entries
            .iter()
            .filter(|entry| needles.iter().all(|needle| entry.args.contains(needle)))
            .collect()
    }

    /// Returns the rows whose arguments name `path` or something under it:
    /// the path stands at the start, after a space, `=`, or `:`, and ends
    /// the arguments or is followed by a space or `/`. Another root whose
    /// name only starts with the same letters never matches.
    #[must_use]
    pub fn under(&self, path: &Path) -> Vec<&ProcessEntry> {
        let needle = path.to_string_lossy();
        self.entries
            .iter()
            .filter(|entry| names_path(&entry.args, &needle))
            .collect()
    }

    /// Takes snapshots until `done` accepts the rows that contain every
    /// one of `needles`, and returns those rows.
    ///
    /// # Errors
    ///
    /// Returns an error that lists the matching rows when `timeout` ran
    /// out, and the error of a failed snapshot.
    pub fn wait_for(
        needles: &[&str],
        timeout: Duration,
        done: impl Fn(&[&ProcessEntry]) -> bool,
    ) -> io::Result<Vec<ProcessEntry>> {
        let deadline = Instant::now() + timeout;
        loop {
            let table = Self::snapshot()?;
            let rows = table.containing_all(needles);
            if done(&rows) {
                return Ok(rows.into_iter().cloned().collect());
            }
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!(
                        "no matching process state for {needles:?} within {timeout:?}; rows: {rows:?}"
                    ),
                ));
            }
            thread::sleep(POLL);
        }
    }

    /// Stops every process whose arguments name `path` or something under
    /// it (see [`under`](Self::under)): SIGTERM first
    /// (through `/bin/kill`, so testkit needs no libc), then SIGKILL for
    /// the ones still there after `grace`. Returns how many processes it
    /// found.
    ///
    /// # Errors
    ///
    /// Returns an error that lists the survivors when a process outlives
    /// SIGKILL, and the error of a failed snapshot.
    pub fn reap_under(path: &Path, grace: Duration) -> io::Result<usize> {
        let needle = path.to_string_lossy().into_owned();
        let found = Self::snapshot()?;
        let first = found.under(path);
        if first.is_empty() {
            return Ok(0);
        }
        let count = first.len();
        signal("-TERM", &first);
        if Self::remaining(path, grace)?.is_empty() {
            return Ok(count);
        }
        let table = Self::snapshot()?;
        signal("-KILL", &table.under(path));
        let left = Self::remaining(path, KILL_GRACE)?;
        if left.is_empty() {
            Ok(count)
        } else {
            Err(io::Error::other(format!(
                "processes under {needle} outlived SIGKILL: {left:?}"
            )))
        }
    }

    /// Waits up to `limit` for no process to name `path`, and returns the
    /// ones that are still there.
    fn remaining(path: &Path, limit: Duration) -> io::Result<Vec<ProcessEntry>> {
        let deadline = Instant::now() + limit;
        loop {
            let table = Self::snapshot()?;
            let rows: Vec<ProcessEntry> = table.under(path).into_iter().cloned().collect();
            if rows.is_empty() || Instant::now() >= deadline {
                return Ok(rows);
            }
            thread::sleep(POLL);
        }
    }
}

/// Whether `args` holds `needle` as a whole path: bounded on the left by
/// the start, a space, `=`, or `:`, and on the right by the end, a space,
/// or `/`.
fn names_path(args: &str, needle: &str) -> bool {
    args.match_indices(needle).any(|(at, _)| {
        let before = args[..at].chars().next_back();
        let after = args[at + needle.len()..].chars().next();
        before.is_none_or(|c| matches!(c, ' ' | '=' | ':'))
            && after.is_none_or(|c| c == ' ' || c == '/')
    })
}

fn parse_line(line: &str) -> Option<ProcessEntry> {
    let line = line.trim_start();
    let (pid, args) = line.split_once(char::is_whitespace)?;
    Some(ProcessEntry {
        pid: pid.parse().ok()?,
        args: args.trim_start().to_owned(),
    })
}

fn signal(name: &str, rows: &[&ProcessEntry]) {
    for row in rows {
        let _ = Command::new("/bin/kill")
            .arg(name)
            .arg(row.pid.to_string())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}
