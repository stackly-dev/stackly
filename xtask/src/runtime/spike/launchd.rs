//! The temporary LaunchAgent of the spike (spec 0003): the user's own
//! `gui/<uid>` domain, a plist that lives in the spike root, and
//! `launchctl` to load, watch, and remove it. Never `sudo`.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use super::LABEL;

const LAUNCHCTL: &str = "/bin/launchctl";
const ID: &str = "/usr/bin/id";

/// What `launchctl print` says about the worker job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobState {
    /// The job is loaded and its process runs.
    Running,
    /// The job is loaded but its process ended; the text after
    /// `last exit code = ` when there is one.
    Exited(Option<String>),
    /// The job is not loaded.
    Missing,
}

/// The numeric user ID, from `id -u`.
pub fn uid() -> Result<u32> {
    let output = Command::new(ID)
        .arg("-u")
        .output()
        .with_context(|| format!("cannot run {ID}"))?;
    if !output.status.success() {
        bail!("`id -u` failed");
    }
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .context("`id -u` did not print a number")
}

/// `gui/<uid>`.
pub fn gui_domain(uid: u32) -> String {
    format!("gui/{uid}")
}

/// True when the user has a GUI login session, so `gui/<uid>` exists.
pub fn has_gui_session(uid: u32) -> Result<bool> {
    let status = Command::new(LAUNCHCTL)
        .args(["print", &gui_domain(uid)])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .with_context(|| format!("cannot run {LAUNCHCTL}"))?;
    Ok(status.success())
}

/// Loads the agent from `plist` into `domain`; launchd starts it at once.
pub fn bootstrap(domain: &str, plist: &Path) -> Result<()> {
    let output = Command::new(LAUNCHCTL)
        .arg("bootstrap")
        .arg(domain)
        .arg(plist)
        .output()
        .with_context(|| format!("cannot run {LAUNCHCTL}"))?;
    if !output.status.success() {
        bail!(
            "launchctl bootstrap {domain} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

/// Removes the agent when it is loaded. launchd sends the worker SIGTERM,
/// then SIGKILL after `ExitTimeOut`.
pub fn bootout(domain: &str) {
    if job_state(domain).is_ok_and(|state| state != JobState::Missing) {
        // Ignored: a job that just ended can vanish between the two calls.
        let _ = Command::new(LAUNCHCTL)
            .arg("bootout")
            .arg(format!("{domain}/{LABEL}"))
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
}

/// Asks launchd about the worker job.
pub fn job_state(domain: &str) -> Result<JobState> {
    let output = Command::new(LAUNCHCTL)
        .arg("print")
        .arg(format!("{domain}/{LABEL}"))
        .output()
        .with_context(|| format!("cannot run {LAUNCHCTL}"))?;
    if !output.status.success() {
        return Ok(JobState::Missing);
    }
    Ok(parse_print(&String::from_utf8_lossy(&output.stdout)))
}

/// Reads the job state from `launchctl print <domain>/<label>`. Only the
/// job's own lines count: one tab deep, not those of nested sections.
pub fn parse_print(text: &str) -> JobState {
    let mut running = false;
    let mut last_exit = None;
    for line in text.lines() {
        let Some(own) = line.strip_prefix('\t') else {
            continue;
        };
        if own.starts_with('\t') {
            continue;
        }
        if let Some(state) = own.strip_prefix("state = ") {
            running = state.trim() == "running";
        } else if let Some(code) = own.strip_prefix("last exit code = ") {
            last_exit = Some(code.trim().to_owned());
        }
    }
    if running {
        JobState::Running
    } else {
        JobState::Exited(last_exit)
    }
}

/// What the plist runs and where its files go.
#[derive(Debug, Clone)]
pub struct Agent {
    /// The xtask binary itself, never `cargo`.
    pub program: PathBuf,
    /// The absolute path of `plan.json`.
    pub plan: PathBuf,
    /// The spike root, also the working folder.
    pub root: PathBuf,
    /// The worker's stdout file.
    pub stdout: PathBuf,
    /// The worker's stderr file.
    pub stderr: PathBuf,
    /// `HOME` of the worker.
    pub home: PathBuf,
    /// `TMPDIR` of the worker.
    pub tmpdir: PathBuf,
}

/// The LaunchAgent property list of the worker.
pub fn render_plist(agent: &Agent) -> String {
    let string =
        |value: &Path| format!("<string>{}</string>", xml_escape(&value.to_string_lossy()));
    let program = [
        string(&agent.program),
        "<string>runtime</string>".to_owned(),
        "<string>spike-worker</string>".to_owned(),
        "<string>--plan</string>".to_owned(),
        string(&agent.plan),
    ]
    .join("\n\t\t");
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>Label</key>
	<string>{LABEL}</string>
	<key>ProgramArguments</key>
	<array>
		{program}
	</array>
	<key>RunAtLoad</key>
	<true/>
	<key>KeepAlive</key>
	<false/>
	<key>WorkingDirectory</key>
	{root}
	<key>StandardOutPath</key>
	{stdout}
	<key>StandardErrorPath</key>
	{stderr}
	<key>ExitTimeOut</key>
	<integer>30</integer>
	<key>EnvironmentVariables</key>
	<dict>
		<key>PATH</key>
		<string>/usr/bin:/bin:/usr/sbin:/sbin</string>
		<key>HOME</key>
		{home}
		<key>TMPDIR</key>
		{tmpdir}
		<key>LANG</key>
		<string>en_US.UTF-8</string>
	</dict>
</dict>
</plist>
"#,
        root = string(&agent.root),
        stdout = string(&agent.stdout),
        stderr = string(&agent.stderr),
        home = string(&agent.home),
        tmpdir = string(&agent.tmpdir),
    )
}

/// Escapes text for an XML element.
fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    const PRINT_RUNNING: &str = "\
gui/502/dev.stackly.xtask.runtime-spike = {
\tactive count = 1
\tpath = /var/t/stackly-runtime-spike-1/agent.plist
\ttype = LaunchAgent
\tstate = running

\tprogram = /repo/target/debug/xtask
\targuments = {
\t\t/repo/target/debug/xtask
\t}

\truns = 1
\tpid = 85333
\tlast exit code = (never exited)
\tendpoints = {
\t}
\tspawn type = daemon (3)
\tjetsam properties = {
\t\tstate = active
\t}
}
";

    #[test]
    fn print_output_tells_running_from_exited() {
        assert_eq!(parse_print(PRINT_RUNNING), JobState::Running);
        let exited = PRINT_RUNNING
            .replace("\tstate = running", "\tstate = not running")
            .replace("(never exited)", "3");
        assert_eq!(parse_print(&exited), JobState::Exited(Some("3".to_owned())));
    }

    // covers: AC-4 (no GUI login session means BLOCKED, not FAIL)
    #[cfg(target_os = "macos")]
    #[test]
    fn a_user_without_a_gui_login_session_has_no_gui_domain() {
        // `nobody` (uid -2) never logs in to the GUI.
        let nobody = u32::MAX - 1;
        assert_eq!(gui_domain(nobody), "gui/4294967294");
        assert!(!has_gui_session(nobody).unwrap());
    }

    #[test]
    fn the_plist_runs_the_worker_with_a_minimal_environment() {
        let agent = Agent {
            program: PathBuf::from("/repo/target/debug/xtask"),
            plan: PathBuf::from("/t/stackly-runtime-spike-1/plan.json"),
            root: PathBuf::from("/t/stackly-runtime-spike-1"),
            stdout: PathBuf::from("/t/stackly-runtime-spike-1/worker.out.log"),
            stderr: PathBuf::from("/t/stackly-runtime-spike-1/worker.err.log"),
            home: PathBuf::from("/t/stackly-runtime-spike-1/agent-home"),
            tmpdir: PathBuf::from("/t/a&b<c>"),
        };
        let plist = render_plist(&agent);
        assert!(plist.contains("<string>dev.stackly.xtask.runtime-spike</string>"));
        assert!(plist.contains(
            "<string>/repo/target/debug/xtask</string>\n\t\t<string>runtime</string>\n\t\t<string>spike-worker</string>\n\t\t<string>--plan</string>\n\t\t<string>/t/stackly-runtime-spike-1/plan.json</string>"
        ));
        assert!(plist.contains("<key>KeepAlive</key>\n\t<false/>"));
        assert!(plist.contains("<string>/t/a&amp;b&lt;c&gt;</string>"));
        assert!(!plist.contains("cargo"));
    }
}
