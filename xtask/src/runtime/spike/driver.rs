//! `cargo xtask runtime spike --id <id> --result <file> [--no-quarantine]`:
//! the driver of one spike (spec 0003, Mekanisme LaunchAgent).
//!
//! It takes the spike lock, cleans up what earlier runs left, downloads
//! and checks the artifact before its clock starts, writes the plan and
//! the plist into a fresh root, bootstraps the worker into `gui/<uid>`,
//! waits for `result.json`, and then removes the agent and the root on
//! every way out. Exit code 0 is PASS, 1 FAIL, 2 BLOCKED.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::sync::{Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use stackly_evidence::write_json_atomic;
use uuid::Uuid;

use super::launchd::{self, Agent, JobState};
use super::lock::SpikeLock;
use super::plan::{Layout, PLAN_SCHEMA, Plan, PlanArtifact, Redaction};
use super::result::{SpikeResult, SpikeStatus, redact};
use super::{DRIVER_TIMEOUT, ROOT_PREFIX, WORKER_TIMEOUT, cleanup};
use crate::runtime::cache::{Cache, TooLarge};
use crate::runtime::inventory::Inventory;

/// The root the signal handler tears down: `(root, domain, uid)`.
static ACTIVE: Mutex<Option<(PathBuf, String, u32)>> = Mutex::new(None);
/// Time between checks for the result.
const POLL: Duration = Duration::from_secs(1);
/// How long the worker gets to exit on its own after writing the result.
const EXIT_GRACE: Duration = Duration::from_secs(5);

/// Runs the spike of `id`. `no_quarantine` skips the quarantine probe for
/// the local run before a maintainer upload (AC-9, AC-19).
pub fn run(repo: &Path, id: &str, result_path: &Path, no_quarantine: bool) -> Result<ExitCode> {
    let inventory = Inventory::load(repo)?;
    let artifact = inventory
        .artifacts
        .iter()
        .find(|artifact| artifact.id.as_deref() == Some(id))
        .with_context(|| format!("the inventory has no artifact `{id}`"))?;
    let sha256 = artifact.sha256.clone().unwrap_or_default();
    let home_only = home_redactions();
    if artifact.status.as_deref() == Some("blocked") {
        let reason = artifact
            .blocked_reason
            .clone()
            .unwrap_or_else(|| "blocked in the inventory".to_owned());
        return end(
            result_path,
            SpikeResult::ended(id, &sha256, SpikeStatus::Blocked, "inventory", &reason),
            &home_only,
        );
    }
    if !cfg!(target_os = "macos") {
        return end(
            result_path,
            SpikeResult::ended(
                id,
                &sha256,
                SpikeStatus::Blocked,
                "launch",
                "runtime spike needs macOS",
            ),
            &home_only,
        );
    }
    let artifact = PlanArtifact::from_inventory(artifact)?;
    let cache = Cache::open()?;
    eprintln!("spike {id}: waiting for the spike lock");
    let _lock = SpikeLock::acquire(cache.dir())?;
    let uid = launchd::uid()?;
    let swept = cleanup::sweep(uid)?;
    if swept != cleanup::Swept::default() {
        eprintln!("spike {id}: cleaned up what an earlier run left: {swept:?}");
    }

    let cache_file = match fetch(&cache, &artifact) {
        Ok(path) => path,
        Err(reason) => {
            return end(
                result_path,
                SpikeResult::ended(id, &sha256, SpikeStatus::Fail, "download", &reason),
                &home_only,
            );
        }
    };
    let clock = Instant::now();
    if !launchd::has_gui_session(uid)? {
        eprintln!("spike {id}: no gui login session");
        return end(
            result_path,
            SpikeResult::ended(
                id,
                &sha256,
                SpikeStatus::Blocked,
                "launch",
                "no gui login session",
            ),
            &home_only,
        );
    }

    let run_id = Uuid::now_v7();
    let layout = Layout::new(std::env::temp_dir().join(format!("{ROOT_PREFIX}{run_id}")));
    let domain = launchd::gui_domain(uid);
    let guard = RootGuard::create(&layout, &domain, uid)?;
    let (quarantine, quarantine_recorded_in) = if no_quarantine {
        eprintln!("spike {id}: local run, the quarantine probe is skipped");
        (false, None)
    } else {
        quarantine_decision(repo, &artifact.sha256)?
    };
    let redactions = redactions(layout.root())?;
    let plan = Plan {
        schema: PLAN_SCHEMA.to_owned(),
        run_id,
        artifact,
        cache_file,
        root: layout.root().to_path_buf(),
        domain: domain.clone(),
        quarantine,
        quarantine_recorded_in,
        local_run: no_quarantine,
        worker_timeout_secs: WORKER_TIMEOUT.as_secs(),
        redactions: redactions.clone(),
    };
    write_json_atomic(&layout.plan(), &plan).context("cannot write plan.json")?;
    for dir in [layout.agent_home(), layout.agent_tmp()] {
        fs::create_dir_all(&dir).with_context(|| format!("cannot create {}", dir.display()))?;
    }
    let agent = Agent {
        program: std::env::current_exe().context("cannot find the xtask binary")?,
        plan: layout.plan(),
        root: layout.root().to_path_buf(),
        stdout: layout.worker_stdout(),
        stderr: layout.worker_stderr(),
        home: layout.agent_home(),
        tmpdir: layout.agent_tmp(),
    };
    fs::write(layout.plist(), launchd::render_plist(&agent))
        .context("cannot write the LaunchAgent plist")?;

    eprintln!("spike {id}: starting the worker in {domain}");
    let result = match launchd::bootstrap(&domain, &layout.plist()) {
        Ok(()) => wait_for_result(&layout, &domain, clock, id, &sha256)?,
        Err(_) if !launchd::has_gui_session(uid)? => SpikeResult::ended(
            id,
            &sha256,
            SpikeStatus::Blocked,
            "launch",
            "no gui login session",
        ),
        Err(err) => SpikeResult::ended(
            id,
            &sha256,
            SpikeStatus::Fail,
            "bootstrap",
            &format!("{err:#}"),
        ),
    };
    if result.status == SpikeStatus::Fail {
        let stderr = fs::read_to_string(layout.worker_stderr()).unwrap_or_default();
        if !stderr.trim().is_empty() {
            eprintln!(
                "spike {id}: worker stderr:\n{}",
                redact(stderr.trim(), &redactions)
            );
        }
    }
    drop(guard);
    end(result_path, result, &redactions)
}

/// Writes the result where the caller asked, prints one line, and turns
/// the status into the exit code.
fn end(path: &Path, result: SpikeResult, redactions: &[Redaction]) -> Result<ExitCode> {
    result.write(path, redactions)?;
    match &result.failure {
        Some(failure) => eprintln!(
            "spike {}: {} at {}: {}",
            result.artifact_id,
            status_text(result.status),
            failure.step,
            redact(&failure.reason, redactions)
        ),
        None => eprintln!(
            "spike {}: PASS, version {}",
            result.artifact_id,
            result.observed_version.as_deref().unwrap_or("?")
        ),
    }
    Ok(ExitCode::from(result.status.exit_code()))
}

fn status_text(status: SpikeStatus) -> &'static str {
    match status {
        SpikeStatus::Pass => "PASS",
        SpikeStatus::Fail => "FAIL",
        SpikeStatus::Blocked => "BLOCKED",
    }
}

/// The cached download, checked against the pinned digest and size before
/// anything is extracted. A mismatch is the reason AC-3 names.
fn fetch(cache: &Cache, artifact: &PlanArtifact) -> Result<PathBuf, String> {
    if let Some(path) = cache
        .lookup(&artifact.sha256, artifact.size)
        .map_err(|err| format!("{err:#}"))?
    {
        return Ok(path);
    }
    let mut last_error = None;
    for url in &artifact.urls {
        eprintln!("spike {}: downloading {url}", artifact.id);
        match cache.download(url, artifact.size) {
            Ok(download) if download.size != artifact.size => {
                return Err(format!(
                    "size mismatch: {url} gave {} bytes, the inventory says {}",
                    download.size, artifact.size
                ));
            }
            Ok(download) if download.sha256 != artifact.sha256 => {
                return Err(format!(
                    "digest mismatch: {url} gave {}, the inventory says {}",
                    download.sha256, artifact.sha256
                ));
            }
            Ok(download) => return cache.store(download).map_err(|err| format!("{err:#}")),
            Err(err) if err.downcast_ref::<TooLarge>().is_some() => {
                return Err(format!(
                    "size mismatch: {url} is larger than the {} bytes in the inventory",
                    artifact.size
                ));
            }
            Err(err) => last_error = Some(format!("{err:#}")),
        }
    }
    Err(last_error.unwrap_or_else(|| "the inventory entry has no URL".to_owned()))
}

/// Polls for `result.json` until the worker writes it, the worker ends
/// without one, or the driver's time runs out.
fn wait_for_result(
    layout: &Layout,
    domain: &str,
    clock: Instant,
    id: &str,
    sha256: &str,
) -> Result<SpikeResult> {
    let fail =
        |step: &str, reason: &str| SpikeResult::ended(id, sha256, SpikeStatus::Fail, step, reason);
    loop {
        if layout.result().is_file() {
            let result = SpikeResult::read(&layout.result())?;
            let grace = Instant::now() + EXIT_GRACE;
            while Instant::now() < grace && launchd::job_state(domain)? == JobState::Running {
                thread::sleep(Duration::from_millis(200));
            }
            return Ok(result);
        }
        match launchd::job_state(domain)? {
            JobState::Running => {}
            JobState::Exited(code)
                if code
                    .as_deref()
                    .is_none_or(|code| code.starts_with("(never")) => {}
            JobState::Exited(code) => {
                thread::sleep(Duration::from_millis(500));
                if layout.result().is_file() {
                    continue;
                }
                return Ok(fail(
                    "worker",
                    &format!("worker exited {}", code.unwrap_or_default()),
                ));
            }
            JobState::Missing => {
                return Ok(fail(
                    "worker",
                    "the agent vanished before the worker wrote a result",
                ));
            }
        }
        if clock.elapsed() >= DRIVER_TIMEOUT {
            return Ok(fail(
                "driver",
                &format!(
                    "the worker did not finish within {} minutes",
                    DRIVER_TIMEOUT.as_secs() / 60
                ),
            ));
        }
        thread::sleep(POLL);
    }
}

/// Owns the spike root: it is created with the owner marker, and the agent
/// and every leftover are removed when the guard drops or the driver gets
/// SIGINT or SIGTERM.
struct RootGuard;

impl RootGuard {
    fn create(layout: &Layout, domain: &str, uid: u32) -> Result<Self> {
        fs::create_dir_all(layout.root())
            .with_context(|| format!("cannot create {}", layout.root().display()))?;
        fs::write(layout.owner_marker(), format!("{uid}\n"))
            .context("cannot write the owner marker")?;
        *ACTIVE.lock().unwrap_or_else(PoisonError::into_inner) =
            Some((layout.root().to_path_buf(), domain.to_owned(), uid));
        ctrlc::set_handler(|| {
            teardown();
            std::process::exit(130);
        })
        .context("cannot install the signal handler")?;
        Ok(Self)
    }
}

impl Drop for RootGuard {
    fn drop(&mut self) {
        teardown();
    }
}

fn teardown() {
    let active = ACTIVE.lock().unwrap_or_else(PoisonError::into_inner).take();
    if let Some((root, domain, uid)) = active {
        launchd::bootout(&domain);
        if let Err(err) = cleanup::sweep(uid) {
            eprintln!("spike: cleanup failed: {err:#}");
        }
        match fs::remove_dir_all(&root) {
            Err(err) if err.kind() != ErrorKind::NotFound => {
                eprintln!("spike: cannot delete {}: {err}", root.display());
            }
            _ => {}
        }
    }
}

/// Whether to probe quarantine: only when no file tracked by git under
/// `evidence/runtime/` holds a quarantine result for this digest. When one
/// does, its folder is returned for the `skipped` detail.
fn quarantine_decision(repo: &Path, sha256: &str) -> Result<(bool, Option<String>)> {
    let output = Command::new("git")
        .current_dir(repo)
        .args([
            "ls-files",
            "-z",
            "--",
            "evidence/runtime/*/artifacts/*/spike-result.json",
        ])
        .output()
        .context("cannot run git")?;
    if !output.status.success() {
        bail!(
            "git ls-files failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    for path in String::from_utf8_lossy(&output.stdout)
        .split('\0')
        .filter(|p| !p.is_empty())
    {
        let Ok(bytes) = fs::read(repo.join(path)) else {
            continue;
        };
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
            continue;
        };
        let outcome = value["quarantine"]["outcome"].as_str().unwrap_or("skipped");
        if value["sha256"].as_str() == Some(sha256) && outcome != "skipped" {
            let folder = Path::new(path)
                .parent()
                .map(|p| format!("{}/", p.display()))
                .unwrap_or_default();
            return Ok((false, Some(folder)));
        }
    }
    Ok((true, None))
}

/// What every string of the result is cleaned of: the root and its real
/// path, the home folder, the host name, and the user name.
fn redactions(root: &Path) -> Result<Vec<Redaction>> {
    let real = root
        .canonicalize()
        .with_context(|| format!("cannot resolve {}", root.display()))?;
    let mut out = vec![
        plain(&real.to_string_lossy(), "$ROOT"),
        plain(&root.to_string_lossy(), "$ROOT"),
    ];
    out.extend(home_redactions());
    if let Ok(output) = Command::new("/bin/hostname").output() {
        let host = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        let short = host.split('.').next().unwrap_or_default().to_owned();
        for name in [host, short] {
            if name.len() >= 3 && !out.iter().any(|r| r.from == name) {
                out.push(word(&name, "$HOST"));
            }
        }
    }
    let user = std::env::var("USER").unwrap_or_default();
    if user.len() >= 3 {
        out.push(word(&user, "$USER"));
    }
    Ok(out)
}

fn home_redactions() -> Vec<Redaction> {
    std::env::var("HOME")
        .ok()
        .filter(|home| home.len() > 1)
        .map(|home| vec![plain(&home, "~")])
        .unwrap_or_default()
}

fn plain(from: &str, to: &str) -> Redaction {
    Redaction {
        from: from.to_owned(),
        to: to.to_owned(),
        whole_word: false,
    }
}

fn word(from: &str, to: &str) -> Redaction {
    Redaction {
        from: from.to_owned(),
        to: to.to_owned(),
        whole_word: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The SHA-256 of the three bytes `abc`.
    const ABC_SHA256: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    /// Not https, so a download stops before curl runs.
    const LOCAL_URL: &str = "http://127.0.0.1:9/abc.tar.gz";

    /// A plan artifact pinned to the bytes `abc`.
    fn abc_artifact() -> PlanArtifact {
        serde_json::from_value(serde_json::json!({
            "id": "nats-2.15.1-macos-aarch64",
            "provider": "nats",
            "line": "2.15",
            "version": "2.15.1",
            "origin": "upstream",
            "format": "tar_gz",
            "executables": ["nats-server"],
            "signingPolicy": "keep_or_adhoc",
            "sha256": ABC_SHA256,
            "size": 3,
            "urls": [LOCAL_URL],
            "dependencies": []
        }))
        .unwrap()
    }

    // covers: AC-3
    #[test]
    fn a_cached_file_with_the_pinned_digest_and_size_is_used_without_a_download() {
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::at(dir.path().to_path_buf()).unwrap();
        fs::write(cache.path(ABC_SHA256), "abc").unwrap();
        assert_eq!(fetch(&cache, &abc_artifact()), Ok(cache.path(ABC_SHA256)));
    }

    // covers: AC-3 (Critical test scenarios, Digest salah: a broken cache entry is deleted and downloaded again)
    #[test]
    fn a_cached_file_with_one_byte_changed_or_one_byte_more_is_deleted_and_downloaded_again() {
        for bytes in ["abd", "abcd"] {
            let dir = tempfile::tempdir().unwrap();
            let cache = Cache::at(dir.path().to_path_buf()).unwrap();
            fs::write(cache.path(ABC_SHA256), bytes).unwrap();
            // The download is the next step; its URL stops it before curl.
            assert_eq!(
                fetch(&cache, &abc_artifact()),
                Err(format!("url must be an https:// URL, found `{LOCAL_URL}`"))
            );
            assert!(!cache.path(ABC_SHA256).exists(), "{bytes}");
        }
    }
}
