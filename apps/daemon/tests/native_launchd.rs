//! The daemon under a real launchd job, without signing (spec 0015
//! milestone 2, AC-15 and AC-16): a temporary LaunchAgent in `gui/<uid>`
//! whose plist is the production golden file with a test label, the
//! `stacklyd` cargo built as `ProgramArguments`, and the test root as the
//! only environment variable.
//!
//! Every test is native and loads a launchd job in your session, so it runs
//! only in a native run you start: `cargo xtask evidence run --kind native
//! -- cargo test -p stacklyd --test native_launchd -- --ignored
//! --test-threads 1`. Without a GUI login session a test ends `BLOCKED`. The
//! fixture boots the job out on drop; `cargo xtask runtime spike --cleanup`
//! removes any `dev.stackly.test.agent.*` job a killed run left behind.
#![cfg(target_os = "macos")]

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use common::{STACKLYD, daemon_paths, runtime, start_daemon};
use serde_json::Value;
use stackly_protocol::daemon::{DaemonInfo, LaunchContextKind, ShutdownMode, ShutdownParams};
use stackly_protocol::{Call, Reply};
use stackly_testkit::evidence::{Evidence, Kind, Source, evidence};
use stackly_testkit::{
    CliRunner, LaunchAgentFixture, ProcessTable, ReapGuard, TestRoot, cargo_bin, cargo_example,
    fake_server, plist_json,
};

/// How long a first start of the agent gets.
const FIRST_START: Duration = Duration::from_secs(30);
/// AC-15: a restart request brings a new daemon within this time.
const RESTART_BOUND: Duration = Duration::from_secs(30);
/// AC-15: a stopped agent stays down at least this long.
const STAYS_DOWN: Duration = Duration::from_secs(20);
/// AC-15: SIGKILL brings a new daemon, and a lock takeover happens,
/// within this time.
const RESPAWN_BOUND: Duration = Duration::from_secs(20);
/// How long a state that should arrive soon gets.
const PATIENCE: Duration = Duration::from_secs(15);
/// Who can give the GUI session a blocked test needs.
const GUI_OWNER: &str = "the user of this Mac, logged in to a GUI session";
const PROVIDER: &str = "testing/fake";

/// A test root with a derived agent. Fields drop in order: the agent is
/// booted out first, then the guard reaps what is left, then the root goes.
struct Rig {
    agent: LaunchAgentFixture,
    _guard: ReapGuard,
    socket: PathBuf,
    root: TestRoot,
}

impl Rig {
    fn new() -> Self {
        let root = TestRoot::new().expect("a test root");
        Self::on(root)
    }

    fn on(root: TestRoot) -> Self {
        let paths = daemon_paths(&root);
        let guard = ReapGuard::new(root.path(), &paths.runtime);
        let agent = LaunchAgentFixture::new(root.path(), Path::new(STACKLYD), root.path())
            .expect("the agent plist is written");
        Self {
            agent,
            _guard: guard,
            socket: paths.socket,
            root,
        }
    }

    /// Checks the derived plist against the golden file once more, then
    /// loads the job and waits for its first daemon.
    fn bootstrap(&mut self) -> DaemonInfo {
        let json: Value =
            serde_json::from_str(&plist_json(self.agent.plist()).expect("the derived plist lints"))
                .expect("plutil writes JSON");
        assert_eq!(json["Label"], self.agent.label());
        assert_eq!(json["ProgramArguments"], serde_json::json!([STACKLYD]));
        assert!(json.get("AbandonProcessGroup").is_none(), "{json}");
        self.agent.bootstrap().expect("the agent loads");
        self.wait_launchd(FIRST_START, |_| true)
            .expect("the agent's daemon answers")
    }

    /// Waits up to `bound` for a launchd started daemon of this agent that
    /// `accept` takes.
    fn wait_launchd(
        &self,
        bound: Duration,
        accept: impl Fn(&DaemonInfo) -> bool,
    ) -> Option<DaemonInfo> {
        let label = self.agent.label().to_owned();
        wait_info(&self.socket, bound, |info| {
            info.launch.as_ref().is_some_and(|launch| {
                launch.context == LaunchContextKind::Launchd
                    && launch.label.as_deref() == Some(label.as_str())
            }) && accept(info)
        })
    }
}

/// `daemon.get_info` on `socket`, or `None` when no daemon answers within
/// five seconds.
fn info(socket: &Path) -> Option<DaemonInfo> {
    let socket = socket.to_path_buf();
    std::thread::spawn(move || {
        runtime().block_on(async {
            tokio::time::timeout(Duration::from_secs(5), async {
                let (client, _) = common::client_try(&socket).await.ok()?;
                match client.call(Call::DaemonGetInfo).await {
                    Ok(Reply::DaemonGetInfo(info)) => Some(info),
                    _ => None,
                }
            })
            .await
            .ok()
            .flatten()
        })
    })
    .join()
    .ok()
    .flatten()
}

/// Polls [`info`] until `accept` takes the reply or `bound` ends.
fn wait_info(
    socket: &Path,
    bound: Duration,
    accept: impl Fn(&DaemonInfo) -> bool,
) -> Option<DaemonInfo> {
    let deadline = Instant::now() + bound;
    loop {
        if let Some(info) = info(socket).filter(|info| accept(info)) {
            return Some(info);
        }
        if Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

/// Sends `daemon.shutdown` with `mode` and expects it accepted.
fn shutdown(socket: &Path, mode: ShutdownMode) {
    let socket = socket.to_path_buf();
    let reply = std::thread::spawn(move || {
        runtime().block_on(async {
            let (client, _) = common::client(&socket).await;
            client
                .call(Call::DaemonShutdown(ShutdownParams { mode }))
                .await
        })
    })
    .join()
    .expect("the shutdown thread ends");
    match reply {
        Ok(Reply::DaemonShutdown(accepted)) => assert_eq!(accepted.mode, mode),
        other => panic!("expected an accepted shutdown, got {other:?}"),
    }
}

/// True when the test may run, with launchd recorded as the subject;
/// otherwise the evidence is `BLOCKED`.
fn gui_session(ev: &Evidence) -> bool {
    if LaunchAgentFixture::has_gui_session().expect("launchctl runs") {
        ev.subject("launchd", &launchd_version(), None, Source::External);
        return true;
    }
    ev.blocked(
        "no GUI login session, so gui/<uid> does not exist",
        GUI_OWNER,
    );
    false
}

/// The launchd version, from `Darwin Bootstrapper Version <x.y.z>: ...` in
/// `launchctl version`.
fn launchd_version() -> String {
    let output = Command::new("/bin/launchctl")
        .arg("version")
        .output()
        .expect("launchctl runs");
    let text = String::from_utf8_lossy(&output.stdout);
    text.split_once("Version ")
        .and_then(|(_, rest)| rest.split(':').next())
        .map(|version| version.trim().to_owned())
        .unwrap_or_else(|| panic!("launchctl version names no version: {text}"))
}

/// Spec 0015 AC-15: the launch context, and a restart request.
#[test]
#[ignore = "native: launchd gui domain"]
fn a_restart_request_to_the_agent_brings_a_new_launchd_daemon_within_30_seconds() {
    let ev = evidence!(
        covers = ["REQ-014", "REQ-028"],
        kind = Kind::Native,
        expected = "a temporary LaunchAgent derived from the golden plist starts stacklyd with launch.context launchd and the test label, and daemon.shutdown restart brings a daemon with another serverInstanceId under the same job within 30 seconds",
    );
    if !gui_session(&ev) {
        return;
    }
    let mut rig = Rig::new();
    let first = rig.bootstrap();

    let asked = Instant::now();
    shutdown(&rig.socket, ShutdownMode::Restart);
    let next = rig
        .wait_launchd(RESTART_BOUND, |info| {
            info.server_instance_id != first.server_instance_id
        })
        .expect("a new daemon answers within 30 seconds");
    assert_ne!(next.pid, first.pid);
    ev.pass_with(&format!(
        "new daemon after {} ms",
        asked.elapsed().as_millis()
    ));
}

/// Spec 0015 AC-15: a stop request keeps the agent down.
#[test]
#[ignore = "native: launchd gui domain"]
fn a_stop_request_keeps_the_agent_down_for_longer_than_the_throttle_interval() {
    let ev = evidence!(
        covers = ["REQ-014", "REQ-028"],
        kind = Kind::Native,
        expected = "after daemon.shutdown stop the agent's daemon exits 0, and for at least 20 seconds launchd starts no new process and no daemon answers on the socket",
    );
    if !gui_session(&ev) {
        return;
    }
    let mut rig = Rig::new();
    rig.bootstrap();

    shutdown(&rig.socket, ShutdownMode::Stop);
    let deadline = Instant::now() + PATIENCE;
    while rig
        .agent
        .job()
        .expect("launchctl runs")
        .is_some_and(|job| job.running)
    {
        assert!(Instant::now() < deadline, "the daemon did not exit");
        std::thread::sleep(Duration::from_millis(200));
    }
    let job = rig
        .agent
        .job()
        .expect("launchctl runs")
        .expect("the job stays loaded");
    assert_eq!(job.last_exit.as_deref(), Some("0"), "{job:?}");
    let runs = job.runs;

    let down_since = Instant::now();
    while down_since.elapsed() < STAYS_DOWN {
        let job = rig
            .agent
            .job()
            .expect("launchctl runs")
            .expect("the job stays loaded");
        assert!(!job.running, "launchd started the agent again: {job:?}");
        assert_eq!(job.runs, runs, "{job:?}");
        assert!(info(&rig.socket).is_none(), "a daemon answers after stop");
        std::thread::sleep(Duration::from_millis(500));
    }
    ev.pass_with(&format!(
        "last exit code 0, runs {runs:?}, no process and no answer for {} ms",
        down_since.elapsed().as_millis()
    ));
}

/// Spec 0015 AC-15: a crash brings a new daemon.
#[test]
#[ignore = "native: launchd gui domain"]
fn sigkill_of_the_agents_daemon_brings_a_new_one_within_20_seconds() {
    let ev = evidence!(
        covers = ["REQ-014", "REQ-028"],
        kind = Kind::Native,
        expected = "after SIGKILL of the agent's daemon launchd starts a new daemon, with another pid and serverInstanceId, that answers within 20 seconds",
    );
    if !gui_session(&ev) {
        return;
    }
    let mut rig = Rig::new();
    let first = rig.bootstrap();

    let killed = Instant::now();
    let status = Command::new("/bin/kill")
        .args(["-KILL", &first.pid.to_string()])
        .status()
        .expect("kill runs");
    assert!(status.success());
    let next = rig
        .wait_launchd(RESPAWN_BOUND, |info| {
            info.server_instance_id != first.server_instance_id
        })
        .expect("a new daemon answers within 20 seconds");
    assert_ne!(next.pid, first.pid);
    ev.pass_with(&format!(
        "new daemon after {} ms",
        killed.elapsed().as_millis()
    ));
}

/// Spec 0015 AC-15: the agent waits out a manual daemon that holds the lock.
#[test]
#[ignore = "native: launchd gui domain"]
fn an_agent_blocked_by_a_manual_daemons_lock_exits_3_until_it_takes_over() {
    let ev = evidence!(
        covers = ["REQ-014", "REQ-028"],
        kind = Kind::Native,
        expected = "while a manual stacklyd holds the lock of the root the agent's daemon exits 3 on every run and the manual daemon keeps serving, and at most 20 seconds after the manual daemon stops a launchd daemon of the agent answers",
    );
    if !gui_session(&ev) {
        return;
    }
    let mut rig = Rig::new();
    let manual = start_daemon(&rig.root);
    rig.agent.bootstrap().expect("the agent loads");

    let deadline = Instant::now() + FIRST_START;
    let blocked_runs;
    loop {
        let job = rig
            .agent
            .job()
            .expect("launchctl runs")
            .expect("the job is loaded");
        if job.runs.is_some_and(|runs| runs >= 2) && job.last_exit.as_deref() == Some("3") {
            blocked_runs = job.runs;
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the agent did not exit 3 twice: {job:?}"
        );
        std::thread::sleep(Duration::from_millis(250));
    }
    let served = info(&rig.socket).expect("the manual daemon still serves");
    assert_eq!(
        served.launch.map(|launch| launch.context),
        Some(LaunchContextKind::Manual)
    );

    manual.stop().expect("the manual daemon stops");
    let stopped = Instant::now();
    rig.wait_launchd(RESPAWN_BOUND, |_| true)
        .expect("the agent takes over within 20 seconds");
    ev.pass_with(&format!(
        "{blocked_runs:?} runs ending with exit code 3, takeover {} ms after the manual daemon stopped",
        stopped.elapsed().as_millis()
    ));
}

/// Seeds `root` with the skeleton example and returns the runtime ID and
/// revision.
fn seed(root: &TestRoot) -> (String, String) {
    let output = Command::new(cargo_example("stacklyd", "skeleton_seed"))
        .arg("--root")
        .arg(root.path())
        .arg("--fake-server")
        .arg(fake_server::binary())
        .arg("--json")
        .output()
        .expect("skeleton_seed runs");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let seeded: Value = serde_json::from_slice(&output.stdout).expect("the seed prints JSON");
    (
        seeded["runtimeId"]
            .as_str()
            .expect("a runtime ID")
            .to_owned(),
        seeded["provider"]["revision"]
            .as_str()
            .expect("a revision")
            .to_owned(),
    )
}

/// `stackly <args>` on the root, as its JSON envelope.
fn json(cli: &CliRunner, args: &[&str]) -> (Option<i32>, Value) {
    let output = cli.run(args).expect("stackly runs to the end");
    let envelope = output.json().expect("stackly writes one JSON envelope");
    (output.code(), envelope)
}

/// Polls `instance get` until `done` accepts the instance or [`PATIENCE`]
/// ends.
fn get_until(cli: &CliRunner, id: &str, done: impl Fn(&Value) -> bool) -> Value {
    let deadline = Instant::now() + PATIENCE;
    loop {
        let (code, envelope) = json(cli, &["instance", "get", id]);
        let instance = envelope["result"]["instance"].clone();
        if code == Some(0) && done(&instance) {
            return instance;
        }
        assert!(Instant::now() < deadline, "not in time: {envelope}");
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// The PIDs of the fake servers in `serve` mode on the instance's data.
fn servers(root: &TestRoot, id: &str) -> Vec<u32> {
    let data = daemon_paths(root)
        .layout
        .instances_dir()
        .join(id)
        .join("data")
        .display()
        .to_string();
    ProcessTable::snapshot()
        .expect("the process table reads")
        .containing_all(&[&data, "stackly-fake-server serve"])
        .iter()
        .map(|row| row.pid)
        .collect()
}

/// Spec 0015 AC-16: an instance outlives the agent's daemon.
#[test]
#[ignore = "native: launchd gui domain"]
fn a_fake_instance_survives_kickstart_and_bootout_and_the_next_daemon_adopts_it() {
    let ev = evidence!(
        covers = ["REQ-013", "AC-013"],
        kind = Kind::Native,
        expected = "a running fake instance keeps its pid and start time when the agent's job is restarted with launchctl kickstart -k and when it is booted out, with no AbandonProcessGroup in the plist, and the next launchd daemon adopts it as ready with the same process",
    );
    if !gui_session(&ev) {
        return;
    }
    let root = TestRoot::new().expect("a test root");
    let (runtime_id, revision) = seed(&root);
    let mut rig = Rig::on(root);
    let cli = CliRunner::new(&cargo_bin("stackly-cli", "stackly"), rig.root.path());

    // The instance starts under a manual daemon, which a SIGTERM ends
    // without stopping it.
    let manual = start_daemon(&rig.root);
    let (code, created) = json(
        &cli,
        &[
            "instance",
            "create",
            "demo",
            "--runtime",
            &runtime_id,
            "--provider",
            PROVIDER,
            "--revision",
            &revision,
            "--port",
            "http=auto",
            "--wait",
        ],
    );
    assert_eq!(code, Some(0), "{created}");
    let id = created["result"]["instance"]["id"]
        .as_str()
        .expect("the envelope names an instance")
        .to_owned();
    let (code, started) = json(&cli, &["instance", "start", &id, "--wait"]);
    assert_eq!(code, Some(0), "{started}");
    let process = get_until(&cli, &id, |instance| {
        instance["readiness"]["state"] == "ready"
    })["process"]
        .clone();
    let pid = u32::try_from(process["pid"].as_u64().expect("a pid")).expect("a pid fits");
    manual.stop().expect("the manual daemon stops");
    assert_eq!(servers(&rig.root, &id), [pid]);

    let same_process = |instance: &Value| {
        instance["readiness"]["state"] == "ready"
            && instance["process"]["pid"] == process["pid"]
            && instance["process"]["startedAt"] == process["startedAt"]
            && instance["process"]["adopted"] == true
    };

    let first = rig.bootstrap();
    get_until(&cli, &id, same_process);

    rig.agent.kickstart_kill().expect("kickstart -k runs");
    rig.wait_launchd(RESTART_BOUND, |info| {
        info.server_instance_id != first.server_instance_id
    })
    .expect("the job runs a new daemon after kickstart -k");
    get_until(&cli, &id, same_process);
    assert_eq!(servers(&rig.root, &id), [pid], "after kickstart -k");

    rig.agent.bootout().expect("the job boots out");
    assert!(info(&rig.socket).is_none(), "no daemon after bootout");
    assert_eq!(servers(&rig.root, &id), [pid], "after bootout");

    rig.bootstrap();
    get_until(&cli, &id, same_process);
    assert_eq!(servers(&rig.root, &id), [pid], "after the next daemon");

    let (code, stopped) = json(&cli, &["instance", "stop", &id, "--wait"]);
    assert_eq!(code, Some(0), "{stopped}");
    assert!(servers(&rig.root, &id).is_empty());
    ev.pass_with(&format!(
        "fake server pid {pid} started at {} stayed the adopted, ready process through kickstart -k, bootout, and the next daemon",
        process["startedAt"]
    ));
}
