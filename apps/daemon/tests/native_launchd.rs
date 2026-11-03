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
#[path = "nats/harness.rs"]
mod nats;
#[path = "postgresql/harness.rs"]
mod pg;
#[path = "supervisor/support.rs"]
mod support;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use common::{STACKLYD, daemon_paths, daemon_paths_at, runtime, start_daemon};
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
    root: Option<TestRoot>,
}

impl Rig {
    fn new() -> Self {
        let root = TestRoot::new().expect("a test root");
        Self::on(root)
    }

    fn on(root: TestRoot) -> Self {
        let path = root.path().to_path_buf();
        Self::at(&path, &path, Some(root))
    }

    /// An agent whose plist lies in `folder` and whose daemon serves the
    /// root override `stackly_root`; `root` is kept alive with the rig.
    fn at(folder: &Path, stackly_root: &Path, root: Option<TestRoot>) -> Self {
        let paths = daemon_paths_at(stackly_root);
        let guard = ReapGuard::new(folder, &paths.runtime);
        let agent = LaunchAgentFixture::new(folder, Path::new(STACKLYD), stackly_root)
            .expect("the agent plist is written");
        Self {
            agent,
            _guard: guard,
            socket: paths.socket,
            root,
        }
    }

    /// The test root of a rig made with [`Rig::on`].
    fn root(&self) -> &TestRoot {
        self.root.as_ref().expect("this rig owns a test root")
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
            // A restart starts an operation, so it needs an idempotency key
            // (spec 0030); a stop does not.
            let key = uuid::Uuid::now_v7().to_string();
            // The supervisor serves a moment after the socket answers, and a
            // restart waits for it (`daemon.starting`); the same key is sent
            // again until it does.
            let deadline = Instant::now() + PATIENCE;
            loop {
                let reply = client
                    .call_detailed(
                        Call::DaemonShutdown(ShutdownParams::new(mode)),
                        stackly_ipc::CallOptions::default().with_idempotency_key(&key),
                    )
                    .await;
                match &reply {
                    Err(error)
                        if error.code() == "daemon.starting" && Instant::now() < deadline =>
                    {
                        tokio::time::sleep(Duration::from_millis(250)).await;
                    }
                    _ => return reply,
                }
            }
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
    let manual = start_daemon(rig.root());
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

/// Spec 0015 AC-16 and spec 0031 AC-11 and AC-13: an instance outlives a
/// crash of the agent's daemon, and the end of a session stops it and the
/// next daemon starts it again.
#[test]
#[ignore = "native: launchd gui domain"]
fn a_fake_instance_survives_sigkill_and_returns_after_kickstart_and_bootout() {
    let ev = evidence!(
        covers = ["REQ-013", "AC-013"],
        kind = Kind::Native,
        expected = "a running fake instance keeps its pid and start time when the agent's daemon is killed with SIGKILL and the next launchd daemon adopts it as ready with the same process, with no AbandonProcessGroup in the plist; launchctl kickstart -k and launchctl bootout then end the session, so the instance stops, and the next daemon starts it again with another process",
    );
    if !gui_session(&ev) {
        return;
    }
    let root = TestRoot::new().expect("a test root");
    let (runtime_id, revision) = seed(&root);
    let mut rig = Rig::on(root);
    let cli = CliRunner::new(&cargo_bin("stackly-cli", "stackly"), rig.root().path());

    // The instance starts under a manual daemon, which a SIGTERM ends
    // without stopping it.
    let manual = start_daemon(rig.root());
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
    assert_eq!(servers(rig.root(), &id), [pid]);

    let same_process = |instance: &Value| {
        instance["readiness"]["state"] == "ready"
            && instance["process"]["pid"] == process["pid"]
            && instance["process"]["startedAt"] == process["startedAt"]
            && instance["process"]["adopted"] == true
    };
    let other_process = |than: u32| {
        move |instance: &Value| {
            instance["readiness"]["state"] == "ready"
                && instance["process"]["pid"].as_u64() != Some(u64::from(than))
        }
    };

    let first = rig.bootstrap();
    get_until(&cli, &id, same_process);

    // A crash of the daemon leaves the instance alone.
    let status = Command::new("/bin/kill")
        .args(["-KILL", &first.pid.to_string()])
        .status()
        .expect("kill runs");
    assert!(status.success());
    let second = rig
        .wait_launchd(RESTART_BOUND, |info| {
            info.server_instance_id != first.server_instance_id
        })
        .expect("the job runs a new daemon after SIGKILL");
    get_until(&cli, &id, same_process);
    assert_eq!(servers(rig.root(), &id), [pid], "after SIGKILL");

    // kickstart -k ends the session: the instance stops, and the next
    // daemon starts it again.
    rig.agent.kickstart_kill().expect("kickstart -k runs");
    rig.wait_launchd(RESTART_BOUND, |info| {
        info.server_instance_id != second.server_instance_id
    })
    .expect("the job runs a new daemon after kickstart -k");
    let restarted = get_until(&cli, &id, other_process(pid));
    let restarted_pid =
        u32::try_from(restarted["process"]["pid"].as_u64().expect("a pid")).expect("a pid fits");
    assert!(!support::alive(pid), "the old process stopped");
    assert_eq!(servers(rig.root(), &id), [restarted_pid]);

    // bootout ends it again; bootstrap brings it back.
    rig.agent.bootout().expect("the job boots out");
    assert!(info(&rig.socket).is_none(), "no daemon after bootout");
    wait_no_server(rig.root(), &id, "after bootout");
    rig.bootstrap();
    let returned = get_until(&cli, &id, other_process(restarted_pid));
    let returned_pid =
        u32::try_from(returned["process"]["pid"].as_u64().expect("a pid")).expect("a pid fits");

    let (code, stopped) = json(&cli, &["instance", "stop", &id, "--wait"]);
    assert_eq!(code, Some(0), "{stopped}");
    assert!(servers(rig.root(), &id).is_empty());
    ev.pass_with(&format!(
        "fake server pid {pid} started at {} stayed the adopted, ready process through SIGKILL; kickstart -k moved it to pid {restarted_pid} and bootout with bootstrap to pid {returned_pid}",
        process["startedAt"]
    ));
}

/// Waits until no fake server runs on the instance.
fn wait_no_server(root: &TestRoot, id: &str, when: &str) {
    let deadline = Instant::now() + SLOW;
    while !servers(root, id).is_empty() {
        assert!(Instant::now() < deadline, "a server still runs {when}");
        std::thread::sleep(Duration::from_millis(200));
    }
}

// Spec 0030 AC-8: PostgreSQL 17, PostgreSQL 18, and NATS outlive the daemon
// and its restarts. Each test starts the instance under a manual daemon,
// hands the root to the agent, and checks that the same process, with the
// same PID and start time, is adopted as ready after every kind of end.

/// How long a state that needs a whole daemon start gets.
const SLOW: Duration = Duration::from_secs(60);

/// The process an instance runs, as the first daemon showed it.
struct Seen {
    pid: u32,
    started_at: stackly_protocol::Timestamp,
}

/// Runs `work` on a thread of its own with its own runtime, so a sync test
/// can use the async client.
fn call<T: Send + 'static>(work: impl std::future::Future<Output = T> + Send + 'static) -> T {
    std::thread::spawn(move || runtime().block_on(work))
        .join()
        .expect("the client thread ends")
}

/// A typed client of the agent's daemon, once its supervisor serves.
async fn served(socket: &Path, runtime_id: uuid::Uuid) -> Option<support::Api> {
    let (client, _) = tokio::time::timeout(Duration::from_secs(5), common::client_try(socket))
        .await
        .ok()?
        .ok()?;
    let api = support::Api::new(client, runtime_id);
    let ready = tokio::time::timeout(
        Duration::from_secs(10),
        api.client.call(Call::InstanceList(
            stackly_protocol::instance::ListInstancesParams::default(),
        )),
    )
    .await;
    matches!(ready, Ok(Ok(_))).then_some(api)
}

/// The instance as the agent's daemon shows it right now.
fn summary_of(
    socket: &Path,
    runtime_id: uuid::Uuid,
    id: uuid::Uuid,
) -> Option<stackly_protocol::instance::InstanceSummary> {
    let socket = socket.to_path_buf();
    call(async move { served(&socket, runtime_id).await?.get(id).await.ok() })
}

/// Waits until the instance is the process `seen`, adopted and ready.
fn wait_same(rig: &Rig, runtime_id: uuid::Uuid, id: uuid::Uuid, seen: &Seen, when: &str) {
    let deadline = Instant::now() + SLOW;
    loop {
        let same = summary_of(&rig.socket, runtime_id, id).is_some_and(|summary| {
            summary.readiness.state == stackly_protocol::instance::InstanceReadinessState::Ready
                && summary.process.as_ref().is_some_and(|process| {
                    process.pid == seen.pid
                        && process.started_at == seen.started_at
                        && process.adopted
                })
        });
        if same {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "{when}: pid {} is not the adopted ready process of the instance",
            seen.pid
        );
        std::thread::sleep(Duration::from_millis(250));
    }
}

/// Starts `id` and returns the process it runs.
async fn started(api: &support::Api, id: uuid::Uuid) -> Seen {
    let summary = api.start_ready(id).await;
    let process = summary.process.expect("a ready instance has a process");
    Seen {
        pid: process.pid,
        started_at: process.started_at,
    }
}

/// `daemon.restart_preview` and then the confirmed `daemon.shutdown
/// restart`, on the agent's daemon. Returns what the plan decided for `id`.
fn planned_restart(
    socket: &Path,
    id: uuid::Uuid,
) -> stackly_protocol::restart::RestartPreviewInstance {
    let socket = socket.to_path_buf();
    call(async move {
        let (client, _) = common::client(&socket).await;
        let preview = match client.call(Call::DaemonRestartPreview).await {
            Ok(Reply::DaemonRestartPreview(preview)) => preview,
            other => panic!("expected a preview, got {other:?}"),
        };
        let entry = preview
            .instances
            .iter()
            .find(|entry| entry.instance_id == id)
            .cloned()
            .expect("the running instance is in the plan");
        let key = uuid::Uuid::now_v7().to_string();
        let reply = client
            .call_detailed(
                Call::DaemonShutdown(ShutdownParams {
                    mode: ShutdownMode::Restart,
                    expected_plan_digest: Some(preview.digest),
                    force: false,
                }),
                stackly_ipc::CallOptions::default().with_idempotency_key(&key),
            )
            .await;
        match reply {
            Ok(Reply::DaemonShutdown(accepted)) => assert!(accepted.plan_id.is_some()),
            other => panic!("expected an accepted restart, got {other:?}"),
        }
        entry
    })
}

/// Waits until the instance is ready with a process other than `old`, which
/// the daemon started itself, and returns it.
fn wait_restarted(
    rig: &Rig,
    runtime_id: uuid::Uuid,
    id: uuid::Uuid,
    old: &Seen,
    when: &str,
) -> Seen {
    let deadline = Instant::now() + SLOW;
    loop {
        let process = summary_of(&rig.socket, runtime_id, id).and_then(|summary| {
            (summary.readiness.state == stackly_protocol::instance::InstanceReadinessState::Ready)
                .then_some(summary.process)
                .flatten()
        });
        if let Some(process) = process.filter(|process| process.pid != old.pid) {
            assert!(
                !support::alive(old.pid),
                "{when}: the old process {} still runs",
                old.pid
            );
            return Seen {
                pid: process.pid,
                started_at: process.started_at,
            };
        }
        assert!(
            Instant::now() < deadline,
            "{when}: the instance did not run again with another process"
        );
        std::thread::sleep(Duration::from_millis(250));
    }
}

/// The whole sequence for one instance that already runs under a manual
/// daemon that has ended. The agent adopts it; a planned restart and a
/// `SIGKILL` of the daemon leave the process alone; `kickstart -k` and
/// `bootout` end the session, so the instance stops and the next daemon
/// starts it again with another process. Returns the last process.
fn survives_every_end(rig: &mut Rig, runtime_id: uuid::Uuid, id: uuid::Uuid, seen: &Seen) -> Seen {
    let first = rig.bootstrap();
    wait_same(rig, runtime_id, id, seen, "after the first daemon");

    let decision = planned_restart(&rig.socket, id);
    assert_eq!(
        (decision.decision, decision.reason),
        (
            stackly_protocol::restart::PlanDecision::Keep,
            stackly_protocol::restart::PlanReason::HandoverProven
        ),
        "the provider is in HANDOVER_PROVEN only when this test passes"
    );
    let second = rig
        .wait_launchd(RESTART_BOUND, |info| {
            info.server_instance_id != first.server_instance_id
        })
        .expect("the job runs a new daemon after the planned restart");
    wait_same(rig, runtime_id, id, seen, "after the planned restart");
    let plan = wait_info(&rig.socket, SLOW, |info| {
        info.restart
            .as_ref()
            .is_some_and(|plan| plan.state == stackly_protocol::restart::PlanState::Finished)
    })
    .and_then(|info| info.restart)
    .expect("the next daemon finishes the plan");
    let outcome = plan
        .instances
        .iter()
        .find(|entry| entry.instance_id == id)
        .and_then(|entry| entry.outcome);
    assert_eq!(
        outcome,
        Some(stackly_protocol::restart::PlanOutcome::Adopted)
    );

    let status = Command::new("/bin/kill")
        .args(["-KILL", &second.pid.to_string()])
        .status()
        .expect("kill runs");
    assert!(status.success());
    let third = rig
        .wait_launchd(RESTART_BOUND, |info| {
            info.server_instance_id != second.server_instance_id
        })
        .expect("the job runs a new daemon after SIGKILL");
    wait_same(rig, runtime_id, id, seen, "after SIGKILL of the daemon");

    rig.agent.kickstart_kill().expect("kickstart -k runs");
    rig.wait_launchd(RESTART_BOUND, |info| {
        info.server_instance_id != third.server_instance_id
    })
    .expect("the job runs a new daemon after kickstart -k");
    let restarted = wait_restarted(rig, runtime_id, id, seen, "after kickstart -k");

    rig.agent.bootout().expect("the job boots out");
    assert!(info(&rig.socket).is_none(), "no daemon after bootout");
    let deadline = Instant::now() + PATIENCE;
    while support::alive(restarted.pid) {
        assert!(Instant::now() < deadline, "the process outlived bootout");
        std::thread::sleep(Duration::from_millis(200));
    }
    rig.bootstrap();
    wait_restarted(
        rig,
        runtime_id,
        id,
        &restarted,
        "after bootout and bootstrap",
    )
}

/// Stops the instance through the agent's daemon and checks its process
/// ended.
fn stop_and_check(rig: &Rig, runtime_id: uuid::Uuid, id: uuid::Uuid, seen: &Seen) {
    let socket = rig.socket.clone();
    call(async move {
        let api = served(&socket, runtime_id)
            .await
            .expect("the daemon serves");
        let accepted = api
            .stop_when_free(id)
            .await
            .accepted
            .expect("a stop starts an operation");
        api.wait_state(
            accepted.operation.id,
            stackly_protocol::operation::OperationState::Succeeded,
        )
        .await;
    });
    let deadline = Instant::now() + PATIENCE;
    while support::alive(seen.pid) {
        assert!(Instant::now() < deadline, "pid {} still runs", seen.pid);
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// The PostgreSQL sequence for `line`, recorded under `ev` of the test that
/// calls it.
fn postgresql_survives(ev: &Evidence, line: pg::Pg) {
    if !gui_session(ev) {
        return;
    }
    let Some(site) = pg::Site::open(ev, line) else {
        return;
    };
    let manual = site.binary(&[]);
    let (id, seen) = runtime().block_on(async {
        let api = site.binary_api(&manual).await;
        let made = pg::create_instance(&site, &api, "survivor").await;
        let seen = started(&api, made.id).await;
        (made.id, seen)
    });
    manual.stop();
    let mut rig = Rig::at(site.root.path(), site.root.path(), None);
    let last = survives_every_end(&mut rig, site.runtime_id, id, &seen);
    stop_and_check(&rig, site.runtime_id, id, &last);
    ev.pass_with(&format!(
        "postgres pid {} started at {:?} stayed the adopted, ready process through a planned restart and SIGKILL of the daemon; kickstart -k and bootout ended the session and the instance ran again as pid {}",
        seen.pid, seen.started_at, last.pid
    ));
}

/// Spec 0030 AC-8 and spec 0031 AC-11 and AC-13, PostgreSQL 17.
#[test]
#[ignore = "native: launchd gui domain"]
fn postgresql_17_survives_planned_restart_and_sigkill_and_returns_after_kickstart_and_bootout() {
    let ev = evidence!(
        covers = ["REQ-013", "AC-013"],
        kind = Kind::Native,
        expected = "a running PostgreSQL instance keeps its pid and start time through a planned daemon restart and a SIGKILL of the daemon, and every next daemon adopts it as ready with a verified identity; launchctl kickstart -k and launchctl bootout then end the session, so the instance stops, and the next daemon starts it again with another process",
    );
    postgresql_survives(&ev, pg::PG17);
}

/// Spec 0030 AC-8 and spec 0031 AC-11 and AC-13, PostgreSQL 18.
#[test]
#[ignore = "native: launchd gui domain"]
fn postgresql_18_survives_planned_restart_and_sigkill_and_returns_after_kickstart_and_bootout() {
    let ev = evidence!(
        covers = ["REQ-013", "AC-013"],
        kind = Kind::Native,
        expected = "a running PostgreSQL instance keeps its pid and start time through a planned daemon restart and a SIGKILL of the daemon, and every next daemon adopts it as ready with a verified identity; launchctl kickstart -k and launchctl bootout then end the session, so the instance stops, and the next daemon starts it again with another process",
    );
    postgresql_survives(&ev, pg::PG18);
}

/// Spec 0030 AC-8 and spec 0031 AC-11 and AC-13, NATS.
#[test]
#[ignore = "native: launchd gui domain"]
fn nats_survives_planned_restart_and_sigkill_and_returns_after_kickstart_and_bootout() {
    let ev = evidence!(
        covers = ["REQ-013", "AC-013"],
        kind = Kind::Native,
        expected = "a running NATS instance keeps its pid and start time through a planned daemon restart and a SIGKILL of the daemon, and every next daemon adopts it as ready with a verified identity; launchctl kickstart -k and launchctl bootout then end the session, so the instance stops, and the next daemon starts it again with another process",
    );
    if !gui_session(&ev) {
        return;
    }
    let Some(site) = nats::Site::open(&ev, nats::Serve::Binary) else {
        return;
    };
    let manual = site.spawn();
    let (id, seen) = runtime().block_on(async {
        let api = site.binary_api(&manual).await;
        let id = nats::create_instance(&site, &api, "survivor", serde_json::json!({})).await;
        let seen = started(&api, id).await;
        (id, seen)
    });
    manual.stop().expect("the manual daemon stops");
    let mut rig = Rig::at(site.root.path(), &site.data_root, None);
    let last = survives_every_end(&mut rig, site.runtime_id, id, &seen);
    stop_and_check(&rig, site.runtime_id, id, &last);
    ev.pass_with(&format!(
        "nats-server pid {} started at {:?} stayed the adopted, ready process through a planned restart and SIGKILL of the daemon; kickstart -k and bootout ended the session and the instance ran again as pid {}",
        seen.pid, seen.started_at, last.pid
    ));
}

/// `instance create` on the agent's daemon; returns the new instance's ID.
fn create_stopped(cli: &CliRunner, name: &str, runtime_id: &str, revision: &str) -> String {
    let (code, created) = json(
        cli,
        &[
            "instance",
            "create",
            name,
            "--runtime",
            runtime_id,
            "--provider",
            PROVIDER,
            "--revision",
            revision,
            "--port",
            "http=auto",
            "--wait",
        ],
    );
    assert_eq!(code, Some(0), "{created}");
    created["result"]["instance"]["id"]
        .as_str()
        .expect("the envelope names an instance")
        .to_owned()
}

/// `stackly daemon restart --yes` against the agent's daemon: it exits 0,
/// the next launchd daemon serves, and the plan it prints is `finished`
/// with `members` entries. Returns the new daemon's info.
fn restart_to_a_finished_plan(
    rig: &Rig,
    cli: &CliRunner,
    before: &DaemonInfo,
    members: usize,
) -> DaemonInfo {
    let (code, restarted) = json(cli, &["daemon", "restart", "--yes"]);
    assert_eq!(code, Some(0), "{restarted}");
    let plan = &restarted["result"]["plan"];
    assert_eq!(plan["state"], "finished", "{restarted}");
    assert_eq!(
        plan["instances"].as_array().map(Vec::len),
        Some(members),
        "{restarted}"
    );
    let after = rig
        .wait_launchd(RESTART_BOUND, |info| {
            info.server_instance_id != before.server_instance_id
        })
        .expect("the job runs a new daemon after the restart");
    let settled = wait_info(&rig.socket, SLOW, |info| {
        info.restart
            .as_ref()
            .is_some_and(|plan| plan.state == stackly_protocol::restart::PlanState::Finished)
    })
    .and_then(|info| info.restart)
    .expect("the new daemon reports the plan as finished");
    assert_eq!(settled.instances.len(), members);
    after
}

/// Spec 0030 AC-8 and AC-9: a planned restart whose plan has no instance to
/// keep or stop still ends `finished` under a real launchd job, and the new
/// daemon accepts mutations.
#[test]
#[ignore = "native: launchd gui domain"]
fn a_planned_restart_with_an_empty_plan_finishes_under_launchd_and_the_next_mutation_is_accepted() {
    let ev = evidence!(
        covers = ["REQ-013", "AC-013"],
        kind = Kind::Native,
        expected = "stackly daemon restart --yes against a launchd daemon with no running instance exits 0 with a finished plan, once with no instance at all and once with only a stopped instance, the next launchd daemon reports the plan as finished, and an instance create after each restart is accepted",
    );
    if !gui_session(&ev) {
        return;
    }
    let root = TestRoot::new().expect("a test root");
    let (runtime_id, revision) = seed(&root);
    let mut rig = Rig::on(root);
    let cli = CliRunner::new(&cargo_bin("stackly-cli", "stackly"), rig.root().path());

    let first = rig.bootstrap();
    let second = restart_to_a_finished_plan(&rig, &cli, &first, 0);
    let stopped = create_stopped(&cli, "after-empty", &runtime_id, &revision);

    let third = restart_to_a_finished_plan(&rig, &cli, &second, 0);
    assert_ne!(third.server_instance_id, second.server_instance_id);
    let instance = get_until(&cli, &stopped, |instance| {
        instance["lifecycle"] == "stopped" || instance["process"].is_null()
    });
    assert!(servers(rig.root(), &stopped).is_empty(), "{instance}");
    create_stopped(&cli, "after-stopped", &runtime_id, &revision);
    ev.pass_with(&format!(
        "daemon restart --yes ended finished with an empty plan twice, once with no instance and once with only the stopped instance {stopped}, and instance create was accepted after each"
    ));
}

/// Spec 0030 AC-17: the `daemonGap` of a planned restart with ten running
/// PostgreSQL instances that all stay (`keep`), recorded as the figure the
/// CLI prints. The spec sets no bound on it.
#[test]
#[ignore = "native: launchd gui domain"]
fn ten_postgresql_instances_all_kept_record_the_daemon_gap_of_a_planned_restart() {
    let ev = evidence!(
        covers = ["NFR-007"],
        kind = Kind::Native,
        expected = "stackly daemon restart --yes against a launchd daemon with ten running PostgreSQL instances, five of 17 and five of 18, keeps all ten as adopted with their old pid and start time and prints a durations.daemonGap, which the test records as an observed figure without a bound",
    );
    if !gui_session(&ev) {
        return;
    }
    let Some(site) = pg::Site::open(&ev, pg::PG17) else {
        return;
    };
    let Some(other) = site.install_other(&ev, pg::PG18) else {
        return;
    };
    let manual = site.binary(&[]);
    let running = runtime().block_on(async {
        let api = site.binary_api(&manual).await;
        pg::ten_running_instances(&site, &api, (other.0.as_uuid(), pg::PG18.major)).await
    });
    manual.stop();
    let seen: Vec<(uuid::Uuid, Seen)> = running
        .iter()
        .map(|(made, summary)| {
            let process = summary.process.as_ref().expect("a running instance");
            (
                made.id,
                Seen {
                    pid: process.pid,
                    started_at: process.started_at,
                },
            )
        })
        .collect();

    let mut rig = Rig::at(site.root.path(), site.root.path(), None);
    let cli = CliRunner::new(&cargo_bin("stackly-cli", "stackly"), site.root.path());
    let first = rig.bootstrap();
    for (id, seen) in &seen {
        wait_same(&rig, site.runtime_id, *id, seen, "after the first daemon");
    }

    let (code, restarted) = json(&cli, &["daemon", "restart", "--yes"]);
    assert_eq!(code, Some(0), "{restarted}");
    let plan = &restarted["result"]["plan"];
    assert_eq!(plan["state"], "finished", "{restarted}");
    let entries = plan["instances"].as_array().expect("plan entries");
    assert_eq!(entries.len(), 10, "{restarted}");
    assert!(
        entries
            .iter()
            .all(|entry| entry["decision"] == "keep" && entry["outcome"] == "adopted"),
        "{restarted}"
    );
    let durations = &restarted["result"]["durations"];
    let gap = durations["daemonGap"]
        .as_u64()
        .unwrap_or_else(|| panic!("a daemonGap in {restarted}"));
    rig.wait_launchd(RESTART_BOUND, |info| {
        info.server_instance_id != first.server_instance_id
    })
    .expect("the job runs a new daemon after the restart");
    for (id, seen) in &seen {
        wait_same(
            &rig,
            site.runtime_id,
            *id,
            seen,
            "after the planned restart",
        );
    }
    for (id, seen) in &seen {
        stop_and_check(&rig, site.runtime_id, *id, seen);
    }
    ev.pass_with(&format!(
        "planned restart with ten kept PostgreSQL instances ({} and {}): daemonGap {gap} ms, stopping {} ms, restoring {} ms; all ten adopted with their old pid and start time",
        pg::PG17.version,
        pg::PG18.version,
        durations["stopping"],
        durations["restoring"],
    ));
}
