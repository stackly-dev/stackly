//! A temporary LaunchAgent for native tests of the daemon under launchd
//! (spec 0015, milestone 2).
//!
//! The plist is the production golden file with three changes: `Label`
//! becomes a test label `dev.stackly.test.agent.<pid>-<n>`, `BundleProgram`
//! becomes `ProgramArguments` naming a `stacklyd` binary outside any
//! bundle, and `EnvironmentVariables` holds only `STACKLY_ROOT`. Every other
//! key stays exactly as the golden file has it. The plist lives in the test
//! root, never in `~/Library/LaunchAgents`, and the job lives in the user's
//! own `gui/<uid>` domain. Dropping the fixture boots the job out, also when
//! the test panicked; `cargo xtask runtime spike --cleanup` removes any job
//! a killed test run left behind.

use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use stackly_platform::bundle::{DAEMON_BUNDLE_PROGRAM, DAEMON_LABEL, TEST_AGENT_LABEL_PREFIX};

const LAUNCHCTL: &str = "/bin/launchctl";
const PLUTIL: &str = "/usr/bin/plutil";

/// The production LaunchAgent plist, the golden file `cargo xtask
/// build-app` is checked against.
pub const GOLDEN_DAEMON_PLIST: &str =
    include_str!("../../../xtask/tests/golden/com.ojiepermana.stackly.daemon.plist");

/// How long [`LaunchAgentFixture::bootout`] waits for launchd to forget
/// the job; longer than the golden `ExitTimeOut` of 30 seconds.
pub const BOOTOUT_TIMEOUT: Duration = Duration::from_secs(40);

/// The next number of a test label in this process.
static NEXT_LABEL: AtomicU32 = AtomicU32::new(0);

/// What `launchctl print gui/<uid>/<label>` says about a loaded job.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct AgentJob {
    /// Whether the job's process runs right now.
    pub running: bool,
    /// The PID of the running process.
    pub pid: Option<u32>,
    /// How many times launchd started the job.
    pub runs: Option<u32>,
    /// The text after `last exit code = `, when there is one.
    pub last_exit: Option<String>,
}

/// One temporary LaunchAgent in `gui/<uid>`, booted out on drop.
#[derive(Debug)]
pub struct LaunchAgentFixture {
    label: String,
    domain: String,
    plist: PathBuf,
    loaded: bool,
}

impl LaunchAgentFixture {
    /// Writes the derived plist for `daemon` and `stackly_root` into
    /// `folder` (a [`TestRoot`](crate::TestRoot)) under a new test label.
    /// Nothing is loaded yet.
    ///
    /// # Errors
    ///
    /// Returns the error of `id -u` or of the plist write.
    pub fn new(folder: &Path, daemon: &Path, stackly_root: &Path) -> io::Result<Self> {
        let label = next_label();
        let plist = folder.join(format!("{label}.plist"));
        std::fs::write(&plist, render_plist(&label, daemon, stackly_root))?;
        Ok(Self {
            domain: format!("gui/{}", uid()?),
            label,
            plist,
            loaded: false,
        })
    }

    /// Returns the job label.
    #[must_use]
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Returns the path of the plist in the test root.
    #[must_use]
    pub fn plist(&self) -> &Path {
        &self.plist
    }

    /// Returns `gui/<uid>/<label>`, the job's service target.
    #[must_use]
    pub fn target(&self) -> String {
        format!("{}/{}", self.domain, self.label)
    }

    /// Returns whether this user has a GUI login session, so `gui/<uid>`
    /// exists and a LaunchAgent can load.
    ///
    /// # Errors
    ///
    /// Returns the error of `id -u` or of running `launchctl`.
    pub fn has_gui_session() -> io::Result<bool> {
        let status = Command::new(LAUNCHCTL)
            .args(["print", &format!("gui/{}", uid()?)])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()?;
        Ok(status.success())
    }

    /// Loads the job; launchd starts it at once (`RunAtLoad`).
    ///
    /// # Errors
    ///
    /// Returns an error when `launchctl bootstrap` fails.
    pub fn bootstrap(&mut self) -> io::Result<()> {
        let mut command = Command::new(LAUNCHCTL);
        command.arg("bootstrap").arg(&self.domain).arg(&self.plist);
        run(command, "launchctl bootstrap")?;
        self.loaded = true;
        Ok(())
    }

    /// `launchctl kickstart -k`: kills the running process and starts the
    /// job again.
    ///
    /// # Errors
    ///
    /// Returns an error when `launchctl kickstart` fails.
    pub fn kickstart_kill(&self) -> io::Result<()> {
        let mut command = Command::new(LAUNCHCTL);
        command.args(["kickstart", "-k", &self.target()]);
        run(command, "launchctl kickstart -k")
    }

    /// Unloads the job and waits up to [`BOOTOUT_TIMEOUT`] until launchd no
    /// longer knows it. launchd sends the process SIGTERM, then SIGKILL
    /// after `ExitTimeOut`.
    ///
    /// # Errors
    ///
    /// Returns an error when the job is still known after the wait.
    pub fn bootout(&mut self) -> io::Result<()> {
        // Ignored: the job may already be gone; the wait below decides.
        let _ = Command::new(LAUNCHCTL)
            .args(["bootout", &self.target()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        let deadline = Instant::now() + BOOTOUT_TIMEOUT;
        while self.job()?.is_some() {
            if Instant::now() >= deadline {
                return Err(io::Error::other(format!(
                    "{} is still loaded after bootout",
                    self.target()
                )));
            }
            thread::sleep(Duration::from_millis(200));
        }
        self.loaded = false;
        Ok(())
    }

    /// Reads the job from `launchctl print`; `None` when it is not loaded.
    ///
    /// # Errors
    ///
    /// Returns the error of running `launchctl`.
    pub fn job(&self) -> io::Result<Option<AgentJob>> {
        let output = Command::new(LAUNCHCTL)
            .args(["print", &self.target()])
            .stderr(Stdio::null())
            .output()?;
        if !output.status.success() {
            return Ok(None);
        }
        Ok(Some(parse_print(&String::from_utf8_lossy(&output.stdout))))
    }
}

impl Drop for LaunchAgentFixture {
    fn drop(&mut self) {
        if self.loaded {
            // Ignored: a drop cannot fail; `runtime spike --cleanup` removes
            // a job that outlived its test.
            let _ = self.bootout();
        }
    }
}

/// The plist of a test agent: the golden file with `label`, `daemon` as
/// `ProgramArguments`, and `STACKLY_ROOT=stackly_root` as the only
/// environment variable.
#[must_use]
pub fn render_plist(label: &str, daemon: &Path, stackly_root: &Path) -> String {
    let production_label = format!("\t<key>Label</key>\n\t<string>{DAEMON_LABEL}</string>\n");
    let bundle_program =
        format!("\t<key>BundleProgram</key>\n\t<string>{DAEMON_BUNDLE_PROGRAM}</string>\n");
    let end = "</dict>\n</plist>\n";
    assert!(
        GOLDEN_DAEMON_PLIST.contains(&production_label)
            && GOLDEN_DAEMON_PLIST.contains(&bundle_program)
            && GOLDEN_DAEMON_PLIST.ends_with(end),
        "the golden plist keeps its Label, BundleProgram, and closing lines"
    );
    let program = xml_escape(&daemon.to_string_lossy());
    let root = xml_escape(&stackly_root.to_string_lossy());
    GOLDEN_DAEMON_PLIST
        .replace(
            &production_label,
            &format!(
                "\t<key>Label</key>\n\t<string>{}</string>\n",
                xml_escape(label)
            ),
        )
        .replace(
            &bundle_program,
            &format!(
                "\t<key>ProgramArguments</key>\n\t<array>\n\t\t<string>{program}</string>\n\t</array>\n"
            ),
        )
        .replace(
            end,
            &format!(
                "\t<key>EnvironmentVariables</key>\n\t<dict>\n\t\t<key>STACKLY_ROOT</key>\n\t\t<string>{root}</string>\n\t</dict>\n{end}"
            ),
        )
}

/// Converts the plist at `path` to JSON text with `plutil`, which also
/// rejects a plist that does not lint.
///
/// # Errors
///
/// Returns an error when `plutil` fails.
pub fn plist_json(path: &Path) -> io::Result<String> {
    let mut command = Command::new(PLUTIL);
    command
        .args(["-convert", "json", "-o", "-"])
        .arg(path)
        .stderr(Stdio::piped());
    let output = command.output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "plutil -convert json {} failed: {}",
            path.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    String::from_utf8(output.stdout).map_err(io::Error::other)
}

/// Reads a job from the text of `launchctl print <domain>/<label>`. Only
/// the job's own lines count: one tab deep, not those of nested sections.
#[must_use]
pub fn parse_print(text: &str) -> AgentJob {
    let mut job = AgentJob::default();
    for line in text.lines() {
        let Some(own) = line.strip_prefix('\t') else {
            continue;
        };
        if own.starts_with('\t') {
            continue;
        }
        let Some((key, value)) = own.split_once(" = ") else {
            continue;
        };
        let value = value.trim();
        match key {
            "state" => job.running = value == "running",
            "pid" => job.pid = value.parse().ok(),
            "runs" => job.runs = value.parse().ok(),
            "last exit code" => job.last_exit = Some(value.to_owned()),
            _ => {}
        }
    }
    job
}

fn next_label() -> String {
    let n = NEXT_LABEL.fetch_add(1, Ordering::Relaxed);
    format!("{TEST_AGENT_LABEL_PREFIX}{}-{n}", std::process::id())
}

fn uid() -> io::Result<u32> {
    let output = Command::new("/usr/bin/id").arg("-u").output()?;
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .map_err(|_| io::Error::other("`id -u` did not print a number"))
}

fn run(mut command: Command, what: &str) -> io::Result<()> {
    let output = command.stdout(Stdio::null()).output()?;
    if output.status.success() {
        return Ok(());
    }
    Err(io::Error::other(format!(
        "{what} failed: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    )))
}

fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn json(text: &str) -> Value {
        serde_json::from_str(text).unwrap()
    }

    #[test]
    fn the_derived_plist_lints_and_differs_from_the_golden_file_only_in_the_three_test_keys() {
        let dir = tempfile::tempdir().unwrap();
        let golden = dir.path().join("golden.plist");
        std::fs::write(&golden, GOLDEN_DAEMON_PLIST).unwrap();
        let derived = dir.path().join("derived.plist");
        let text = render_plist(
            "dev.stackly.test.agent.1-0",
            Path::new("/tmp/a & b/stacklyd"),
            Path::new("/tmp/root"),
        );
        std::fs::write(&derived, &text).unwrap();

        let mut golden = json(&plist_json(&golden).unwrap());
        let mut derived = json(&plist_json(&derived).unwrap());
        assert_eq!(derived["Label"], "dev.stackly.test.agent.1-0");
        assert_eq!(
            derived["ProgramArguments"],
            serde_json::json!(["/tmp/a & b/stacklyd"])
        );
        assert_eq!(
            derived["EnvironmentVariables"],
            serde_json::json!({ "STACKLY_ROOT": "/tmp/root" })
        );
        let golden = golden.as_object_mut().unwrap();
        golden.remove("Label");
        golden.remove("BundleProgram");
        let derived = derived.as_object_mut().unwrap();
        for key in ["Label", "ProgramArguments", "EnvironmentVariables"] {
            derived.remove(key);
        }
        assert_eq!(derived, golden);
        assert!(!text.contains("AbandonProcessGroup"));
    }

    #[test]
    fn a_test_label_carries_the_prefix_the_pid_and_a_new_number_each_time() {
        let first = next_label();
        let second = next_label();
        let prefix = format!("{TEST_AGENT_LABEL_PREFIX}{}-", std::process::id());
        assert!(first.starts_with(&prefix), "{first}");
        assert!(second.starts_with(&prefix), "{second}");
        assert_ne!(first, second);
    }

    #[test]
    fn print_parsing_reads_only_the_jobs_own_lines() {
        let text = "gui/501/dev.stackly.test.agent.1-0 = {\n\tactive count = 1\n\tstate = running\n\tpid = 4242\n\truns = 3\n\tlast exit code = 3\n\tendpoints = {\n\t\tstate = waiting\n\t\tpid = 1\n\t}\n}\n";
        let job = parse_print(text);
        assert!(job.running);
        assert_eq!(job.pid, Some(4242));
        assert_eq!(job.runs, Some(3));
        assert_eq!(job.last_exit.as_deref(), Some("3"));

        let stopped = parse_print("x = {\n\tstate = not running\n\truns = 1\n}\n");
        assert!(!stopped.running);
        assert_eq!(stopped.pid, None);
        assert_eq!(stopped.last_exit, None);
    }
}
