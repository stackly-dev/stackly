//! The end of a login session and the return to it under a real launchd job
//! (spec 0031 AC-11 and AC-13, native part of AC-21): a temporary LaunchAgent
//! with a test label runs `stacklyd` in `gui/<uid>`. `launchctl kickstart -k`
//! and `bootout` then `bootstrap` send it `SIGTERM` with no `daemon.shutdown`
//! before it, which is a logout for the daemon. The instance that was
//! running stops (desired stays `running`), the next daemon finds the same
//! login session (`continued`) and starts it again, and an instance the
//! person stopped stays stopped.
//!
//! A real login, logout, and reboot are not part of it: they need the user,
//! and their records are `BLOCKED` in `verify.md`.
//!
//! Every test is native and loads a launchd job in your session, so it runs
//! only in a native run you start: `cargo xtask evidence run --kind native
//! -- cargo test -p stacklyd --test native_session -- --ignored
//! --test-threads 1`. Without a GUI login session a test ends `BLOCKED`. The
//! fixture boots the job out on drop.
#![cfg(target_os = "macos")]

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use common::{STACKLYD, daemon_paths, daemon_paths_at, runtime};
use serde_json::Value;
use stackly_protocol::daemon::{DaemonInfo, LaunchContextKind, SessionRotationKind};
use stackly_protocol::{Call, Reply};
use stackly_testkit::evidence::{Evidence, Kind, Source, evidence};
use stackly_testkit::{
    CliRunner, LaunchAgentFixture, ProcessTable, ReapGuard, TestRoot, cargo_bin, cargo_example,
    fake_server, plist_json,
};

/// How long a first start of the agent gets.
const FIRST_START: Duration = Duration::from_secs(30);
/// How long the end of a session and the next daemon get, throttle included.
const RESTART_BOUND: Duration = Duration::from_secs(60);
/// How long a state that should arrive soon gets.
const PATIENCE: Duration = Duration::from_secs(15);
/// How long an instance the next daemon starts gets.
const SLOW: Duration = Duration::from_secs(60);
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
    fn new(root: TestRoot) -> Self {
        let path = root.path().to_path_buf();
        let paths = daemon_paths_at(&path);
        let guard = ReapGuard::new(&path, &paths.runtime);
        let agent = LaunchAgentFixture::new(&path, Path::new(STACKLYD), &path)
            .expect("the agent plist is written");
        Self {
            agent,
            _guard: guard,
            socket: paths.socket,
            root,
        }
    }

    /// Checks the derived plist, loads the job, and waits for its daemon.
    fn bootstrap(&mut self) -> DaemonInfo {
        let json: Value =
            serde_json::from_str(&plist_json(self.agent.plist()).expect("the derived plist lints"))
                .expect("plutil writes JSON");
        assert_eq!(json["Label"], self.agent.label());
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
        let deadline = Instant::now() + bound;
        loop {
            if let Some(info) = info(&self.socket).filter(|info| {
                info.launch.as_ref().is_some_and(|launch| {
                    launch.context == LaunchContextKind::Launchd
                        && launch.label.as_deref() == Some(label.as_str())
                }) && accept(info)
            }) {
                return Some(info);
            }
            if Instant::now() >= deadline {
                return None;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
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

/// Polls `instance get` until `done` accepts the instance or `bound` ends.
fn get_until(cli: &CliRunner, id: &str, bound: Duration, done: impl Fn(&Value) -> bool) -> Value {
    let deadline = Instant::now() + bound;
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

/// Waits until no fake server runs on the instance.
fn wait_no_server(root: &TestRoot, id: &str, when: &str) {
    let deadline = Instant::now() + RESTART_BOUND;
    while !servers(root, id).is_empty() {
        assert!(Instant::now() < deadline, "a server still runs {when}");
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// What the scenario starts from: one instance that keeps running, one that
/// the person stopped, and the pid of the first.
struct Setup {
    keep: String,
    user: String,
    keep_pid: u32,
}

/// Creates two instances under the agent's daemon: `keep` runs, `user` ran
/// and was stopped by the person.
fn prepare(cli: &CliRunner, root: &TestRoot, runtime_id: &str, revision: &str) -> Setup {
    let create = |name: &str| -> String {
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
    };
    let start = |id: &str| {
        let (code, started) = json(cli, &["instance", "start", id, "--wait"]);
        assert_eq!(code, Some(0), "{started}");
        get_until(cli, id, PATIENCE, |instance| {
            instance["readiness"]["state"] == "ready"
        })
    };
    let keep = create("keep");
    let user = create("user");
    let running = start(&keep);
    let keep_pid =
        u32::try_from(running["process"]["pid"].as_u64().expect("a pid")).expect("a pid fits");
    start(&user);
    let (code, stopped) = json(cli, &["instance", "stop", &user, "--wait"]);
    assert_eq!(code, Some(0), "{stopped}");
    assert_eq!(servers(root, &keep), [keep_pid]);
    assert!(servers(root, &user).is_empty());
    Setup {
        keep,
        user,
        keep_pid,
    }
}

/// The instance `keep` runs again with another process, and `user` is still
/// stopped, with no process.
fn assert_returned(cli: &CliRunner, root: &TestRoot, setup: &Setup) -> u32 {
    let again = get_until(cli, &setup.keep, SLOW, |instance| {
        instance["readiness"]["state"] == "ready"
            && instance["process"]["pid"].as_u64() != Some(u64::from(setup.keep_pid))
    });
    let pid = u32::try_from(again["process"]["pid"].as_u64().expect("a pid")).expect("a pid fits");
    assert_eq!(
        again["desired"], "running",
        "the end of a session left desired alone"
    );
    assert_eq!(servers(root, &setup.keep), [pid]);
    let user = get_until(cli, &setup.user, PATIENCE, |_| true);
    assert_eq!(user["lifecycle"], "stopped", "{user}");
    assert!(
        servers(root, &setup.user).is_empty(),
        "the instance the person stopped did not start"
    );
    pid
}

/// Spec 0031 AC-11 and AC-13 under `launchctl kickstart -k`.
#[test]
#[ignore = "native: launchd gui domain"]
fn kickstart_ends_the_session_and_the_next_daemon_starts_the_instance_again() {
    let ev = evidence!(
        covers = ["REQ-014", "AC-069"],
        kind = Kind::Native,
        expected = "with a temporary LaunchAgent, launchctl kickstart -k ends the session: the running instance stops with desired running and the job's daemon exits without SIGKILL, the next daemon reports rotation continued in the same session ID, starts that instance again with another process, and leaves the instance the person stopped stopped",
    );
    if !gui_session(&ev) {
        return;
    }
    let root = TestRoot::new().expect("a test root");
    let (runtime_id, revision) = seed(&root);
    let mut rig = Rig::new(root);
    let cli = CliRunner::new(&cargo_bin("stackly-cli", "stackly"), rig.root.path());

    let first = rig.bootstrap();
    let session = first
        .session
        .clone()
        .expect("a launchd daemon knows its session");
    assert_eq!(session.rotation, SessionRotationKind::First);
    let setup = prepare(&cli, &rig.root, &runtime_id, &revision);

    rig.agent.kickstart_kill().expect("kickstart -k runs");
    let next = rig
        .wait_launchd(RESTART_BOUND, |info| {
            info.server_instance_id != first.server_instance_id
        })
        .expect("the job runs a new daemon after kickstart -k");
    let again = next
        .session
        .clone()
        .expect("the next daemon knows its session");
    assert_eq!(again.id, session.id, "the same login session");
    assert_eq!(again.rotation, SessionRotationKind::Continued);
    let pid = assert_returned(&cli, &rig.root, &setup);
    assert_ne!(pid, setup.keep_pid);
    let job = rig
        .agent
        .job()
        .expect("launchctl runs")
        .expect("the job stays loaded");
    assert_ne!(
        job.last_exit.as_deref(),
        Some("9"),
        "launchd did not need SIGKILL: {job:?}"
    );

    let (code, stopped) = json(&cli, &["instance", "stop", &setup.keep, "--wait"]);
    assert_eq!(code, Some(0), "{stopped}");
    ev.pass_with(&format!(
        "session {} went first then continued; instance keep moved from pid {} to {pid}; instance user stayed stopped; last exit {:?}",
        session.id, setup.keep_pid, job.last_exit
    ));
}

/// Spec 0031 AC-11 and AC-13 under `bootout` then `bootstrap`.
#[test]
#[ignore = "native: launchd gui domain"]
fn bootout_and_bootstrap_end_the_session_and_the_next_daemon_starts_the_instance_again() {
    let ev = evidence!(
        covers = ["REQ-014", "AC-069"],
        kind = Kind::Native,
        expected = "with a temporary LaunchAgent, launchctl bootout ends the session: no daemon and no instance process remain, desired stays running, and after bootstrap the next daemon reports rotation continued in the same session ID and starts the instance again, while the instance the person stopped stays stopped",
    );
    if !gui_session(&ev) {
        return;
    }
    let root = TestRoot::new().expect("a test root");
    let (runtime_id, revision) = seed(&root);
    let mut rig = Rig::new(root);
    let cli = CliRunner::new(&cargo_bin("stackly-cli", "stackly"), rig.root.path());

    let first = rig.bootstrap();
    let session = first
        .session
        .clone()
        .expect("a launchd daemon knows its session");
    assert_eq!(session.rotation, SessionRotationKind::First);
    let setup = prepare(&cli, &rig.root, &runtime_id, &revision);

    rig.agent.bootout().expect("the job boots out");
    assert!(info(&rig.socket).is_none(), "no daemon after bootout");
    wait_no_server(&rig.root, &setup.keep, "after bootout");

    let next = rig.bootstrap();
    let again = next
        .session
        .clone()
        .expect("the next daemon knows its session");
    assert_eq!(again.id, session.id, "the same login session");
    assert_eq!(again.rotation, SessionRotationKind::Continued);
    let pid = assert_returned(&cli, &rig.root, &setup);
    assert_ne!(pid, setup.keep_pid);

    let (code, stopped) = json(&cli, &["instance", "stop", &setup.keep, "--wait"]);
    assert_eq!(code, Some(0), "{stopped}");
    ev.pass_with(&format!(
        "session {} went first then continued; instance keep moved from pid {} to {pid} through bootout and bootstrap; instance user stayed stopped",
        session.id, setup.keep_pid
    ));
}
