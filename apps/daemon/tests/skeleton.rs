//! The walking skeleton through the real `stacklyd`, the real `stackly`,
//! and the real `stackly-fake-server` (spec 0014 AC-9 to AC-13): a happy
//! path, a failed init that is abandoned, and a daemon that restarts or
//! dies in the middle of a create. Every daemon and CLI run uses the file
//! keychain of the test root, so no Keychain dialog can appear.
//!
//! Each test seeds its own `TestRoot`. A [`Rig`] stops the daemon first, then
//! reaps every process whose argv names the root and removes the runtime
//! root, also when the test panics.
#![cfg(target_os = "macos")]
#![allow(
    dead_code,
    reason = "the restart tests of step 8 use the rest of the rig"
)]

mod common;

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use common::{daemon_paths, runtime, start_daemon};
use serde_json::Value;
use sha2::{Digest, Sha256};
use stackly_protocol::instance::ListInstancesParams;
use stackly_protocol::{Call, Reply};
use stackly_testkit::evidence::{FixtureCredential, Kind, evidence};
use stackly_testkit::{
    CliOutput, CliRunner, ProcessTable, ReapGuard, SpawnedDaemon, TestRoot, cargo_bin,
    cargo_example, fake_server,
};

const PROVIDER: &str = "testing/fake";
/// How long a state that should arrive soon gets.
const PATIENCE: Duration = Duration::from_secs(10);

/// A seeded root with a running daemon and a CLI runner. The fields drop
/// in order: the daemon first, then the guard that reaps what it left
/// behind, then the root.
struct Rig {
    daemon: Option<SpawnedDaemon>,
    _guard: ReapGuard,
    cli: CliRunner,
    runtime_id: String,
    revision: String,
    /// Everything every CLI run of this rig wrote to stdout and stderr.
    transcript: RefCell<String>,
    root: TestRoot,
}

impl Rig {
    fn new() -> Self {
        let root = TestRoot::new().expect("a test root");
        let (runtime_id, revision) = seed(&root);
        let paths = daemon_paths(&root);
        let guard = ReapGuard::new(root.path(), &paths.runtime);
        let cli = CliRunner::new(&cargo_bin("stackly-cli", "stackly"), root.path());
        let daemon = start_daemon(&root);
        Self {
            daemon: Some(daemon),
            _guard: guard,
            cli,
            runtime_id,
            revision,
            transcript: RefCell::new(String::new()),
            root,
        }
    }

    /// Runs the CLI and returns its output; a hung run fails the test.
    fn run(&self, args: &[&str]) -> CliOutput {
        let output = self.cli.run(args).expect("stackly runs to the end");
        self.record(&output);
        output
    }

    /// Runs the CLI with `stdin` and returns its output.
    fn run_with_stdin(&self, args: &[&str], stdin: &[u8]) -> CliOutput {
        let output = self
            .cli
            .run_with_stdin(args, stdin)
            .expect("stackly runs to the end");
        self.record(&output);
        output
    }

    fn record(&self, output: &CliOutput) {
        let mut transcript = self.transcript.borrow_mut();
        transcript.push_str(&output.stdout);
        transcript.push_str(&output.stderr);
    }

    /// Runs the CLI and returns the envelope.
    fn json(&self, args: &[&str]) -> (Option<i32>, Value) {
        let output = self.run(args);
        let envelope = output.json().expect("stackly writes one JSON envelope");
        (output.code(), envelope)
    }

    /// `instance create <name> ... --wait` with the extra arguments.
    fn create(&self, name: &str, extra: &[&str]) -> (Option<i32>, Value) {
        let mut args = vec!["--wait"];
        args.extend_from_slice(extra);
        self.create_with(name, &args)
    }

    /// `instance create <name> ...` with exactly the extra arguments.
    fn create_with(&self, name: &str, extra: &[&str]) -> (Option<i32>, Value) {
        self.create_piped(name, extra, None)
    }

    /// [`create_with`](Self::create_with) with bytes on stdin.
    fn create_piped(
        &self,
        name: &str,
        extra: &[&str],
        stdin: Option<&[u8]>,
    ) -> (Option<i32>, Value) {
        let mut args = vec![
            "instance",
            "create",
            name,
            "--runtime",
            &self.runtime_id,
            "--provider",
            PROVIDER,
            "--revision",
            &self.revision,
            "--port",
            "http=auto",
        ];
        args.extend_from_slice(extra);
        let Some(stdin) = stdin else {
            return self.json(&args);
        };
        let output = self.run_with_stdin(&args, stdin);
        let envelope = output.json().expect("stackly writes one JSON envelope");
        (output.code(), envelope)
    }

    fn get(&self, id: &str) -> Value {
        let (code, envelope) = self.json(&["instance", "get", id]);
        assert_eq!(code, Some(0), "{envelope}");
        envelope["result"]["instance"].clone()
    }

    /// Polls `get` until `done` accepts the instance or [`PATIENCE`] ends.
    fn get_until(&self, id: &str, done: impl Fn(&Value) -> bool) -> Value {
        let deadline = Instant::now() + PATIENCE;
        loop {
            let instance = self.get(id);
            if done(&instance) {
                return instance;
            }
            assert!(Instant::now() < deadline, "not in time: {instance}");
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    fn data_dir(&self, id: &str) -> PathBuf {
        daemon_paths(&self.root)
            .layout
            .instances_dir()
            .join(id)
            .join("data")
    }

    /// The `secrets/` folder of the instance in the runtime root, where the
    /// sensitive fake config lives while the instance runs (spec 0009
    /// AC-20).
    fn secrets_dir(&self, id: &str) -> PathBuf {
        let id: stackly_domain::InstanceId = id.parse().expect("the instance ID parses");
        daemon_paths(&self.root)
            .layout
            .instance_secrets_dir(id.short_key())
    }

    /// Fake servers in `serve` mode for the instance, by its data folder.
    fn servers(&self, id: &str) -> Vec<u32> {
        let data = self.data_dir(id).display().to_string();
        ProcessTable::snapshot()
            .expect("the process table reads")
            .containing_all(&[&data, "stackly-fake-server serve"])
            .iter()
            .map(|row| row.pid)
            .collect()
    }

    /// Every process whose argv names the test root.
    fn processes_under_root(&self) -> Vec<String> {
        let root = self.root.path().display().to_string();
        ProcessTable::snapshot()
            .expect("the process table reads")
            .containing(&root)
            .iter()
            .map(|row| row.args.clone())
            .collect()
    }

    /// The names of the instances `instance.list` returns, all pages.
    fn listed_names(&self) -> Vec<String> {
        let socket = self
            .daemon
            .as_ref()
            .expect("a daemon runs")
            .socket()
            .to_path_buf();
        std::thread::spawn(move || {
            runtime().block_on(async move {
                let (client, _) = common::client(&socket).await;
                let mut names = Vec::new();
                let mut before = None;
                loop {
                    let Ok(Reply::InstanceList(page)) = client
                        .call(Call::InstanceList(ListInstancesParams {
                            limit: Some(200),
                            before: before.take(),
                        }))
                        .await
                    else {
                        panic!("instance.list failed");
                    };
                    names.extend(page.items.iter().map(|item| item.name.clone()));
                    match page.next {
                        Some(next) => before = Some(next),
                        None => return names,
                    }
                }
            })
        })
        .join()
        .expect("the list thread ends")
    }

    /// Stops the daemon the way `how` says, starts a new one on the same
    /// root, and waits until its supervisor finished the reconciliation.
    fn restart(&mut self, how: Stop) {
        let daemon = self.daemon.take().expect("a daemon runs");
        match how {
            Stop::Kill => daemon.kill().expect("the daemon is killed"),
            Stop::Term => daemon.stop().expect("the daemon stops"),
        };
        self.daemon = Some(start_daemon(&self.root));
        let deadline = Instant::now() + PATIENCE;
        loop {
            let (_, status) = self.json(&["daemon", "status"]);
            let supervisor_ok = status["result"]["health"]["subsystems"]
                .as_array()
                .is_some_and(|all| {
                    all.iter()
                        .any(|part| part["name"] == "supervisor" && part["status"] == "ok")
                });
            if supervisor_ok {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "the supervisor did not settle: {status}"
            );
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    /// Every file a secret value must stay out of, as lossy text: the
    /// daemon log and stderr, the registry with its `-wal` and `-shm`, the
    /// runtime root (spool files, and any `secrets/` folder, which must be
    /// gone once the instance has stopped), and every file under
    /// `instances/<id>/` in the data root (the instance manifest, `config/`,
    /// `data/`, and `temp/`).
    fn scanned_files(&self) -> Vec<(String, String)> {
        let paths = daemon_paths(&self.root);
        let layout = &paths.layout;
        let mut files = vec![layout.daemon_log_dir().join("stacklyd.log")];
        let registry = layout.registry_db();
        for suffix in ["", "-wal", "-shm"] {
            let mut name = registry.clone().into_os_string();
            name.push(suffix);
            files.push(PathBuf::from(name));
        }
        files.extend(self.stderr_files());
        let mut pending = vec![paths.runtime.clone()];
        while let Some(dir) = pending.pop() {
            for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    pending.push(path);
                } else {
                    files.push(path);
                }
            }
        }
        let mut pending = vec![layout.instances_dir()];
        while let Some(dir) = pending.pop() {
            for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    pending.push(path);
                } else {
                    files.push(path);
                }
            }
        }
        let log_dir = layout.daemon_log_dir();
        for entry in std::fs::read_dir(&log_dir).into_iter().flatten().flatten() {
            files.push(entry.path());
        }
        files.sort();
        files.dedup();
        files
            .into_iter()
            .filter(|path| path.is_file())
            .map(|path| {
                let bytes = std::fs::read(&path).unwrap_or_default();
                (
                    path.display().to_string(),
                    String::from_utf8_lossy(&bytes).into_owned(),
                )
            })
            .collect()
    }

    /// The `.stderr` files `SpawnedDaemon` left in the test root.
    fn stderr_files(&self) -> Vec<PathBuf> {
        std::fs::read_dir(self.root.path())
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "stderr"))
            .collect()
    }

    fn keychain_file(&self) -> PathBuf {
        daemon_paths(&self.root)
            .layout
            .keychain_dir()
            .join("stackly.keychain-db")
    }
}

/// How a daemon ends before the next one starts.
#[derive(Clone, Copy)]
enum Stop {
    /// `SIGKILL`: no chance to clean up.
    Kill,
    /// `SIGTERM`: a normal stop, which leaves instances running.
    Term,
}

/// Seeds `root` with the example and returns the runtime ID and revision.
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

/// Name and SHA-256 of every file under `data`.
fn data_digest(data: &Path) -> BTreeMap<String, String> {
    let mut digest = BTreeMap::new();
    let mut pending = vec![data.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(&dir).expect("the data folder reads") {
            let path = entry.expect("a data folder entry").path();
            if path.is_dir() {
                pending.push(path);
            } else {
                let bytes = std::fs::read(&path).expect("a data file reads");
                let relative = path
                    .strip_prefix(data)
                    .expect("the file is under the data folder")
                    .display()
                    .to_string();
                digest.insert(
                    relative,
                    Sha256::digest(&bytes)
                        .iter()
                        .map(|byte| format!("{byte:02x}"))
                        .collect(),
                );
            }
        }
    }
    digest
}

fn instance_id(envelope: &Value) -> String {
    envelope["result"]["instance"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("the envelope names an instance: {envelope}"))
        .to_owned()
}

/// Spec 0014 AC-9, AC-13.
#[test]
fn an_instance_is_created_started_read_and_stopped_through_the_real_daemon() {
    let ev = evidence!(
        covers = ["AC-029"],
        kind = Kind::Fixture,
        expected = "a provider package the engine accepts is created, started, read, and stopped through stackly against the real daemon with exactly one fake server process while it runs and none after the stop, and no Keychain dialog",
    );
    let rig = Rig::new();

    let (code, created) = rig.create("demo", &[]);
    assert_eq!(code, Some(0), "{created}");
    assert_eq!(created["result"]["operation"]["state"], "succeeded");
    let id = instance_id(&created);
    let instance = &created["result"]["instance"];
    assert_eq!(instance["lifecycle"], "stopped");
    assert_eq!(instance["initialized"], true);
    assert!(rig.servers(&id).is_empty(), "nothing runs after a create");
    assert!(
        rig.keychain_file().is_file(),
        "the secrets went to the file keychain of the root"
    );

    let (code, started) = rig.json(&["instance", "start", &id, "--wait"]);
    assert_eq!(code, Some(0), "{started}");
    assert_eq!(started["result"]["operation"]["state"], "succeeded");
    assert_eq!(started["result"]["instance"]["lifecycle"], "running");
    assert_eq!(started["result"]["instance"]["readiness"]["state"], "ready");

    let read = rig.get(&id);
    assert_eq!(read["lifecycle"], "running");
    assert_eq!(read["desired"], "running");
    assert_eq!(read["readiness"]["state"], "ready");
    assert!(read["health"]["state"].is_string(), "{read}");
    let pid = read["process"]["pid"].as_u64().expect("a running process");
    assert_eq!(rig.servers(&id), [u32::try_from(pid).expect("a pid")]);

    let (code, stopped) = rig.json(&["instance", "stop", &id, "--wait"]);
    assert_eq!(code, Some(0), "{stopped}");
    assert_eq!(stopped["result"]["instance"]["lifecycle"], "stopped");
    assert!(rig.servers(&id).is_empty(), "no process after the stop");
    ev.pass();
}

/// Spec 0014 AC-10.
#[test]
fn a_failed_init_needs_recovery_and_abandoning_it_leaves_no_instance_to_start() {
    let rig = Rig::new();

    let (code, created) = rig.create("bad", &["--set-json", "failInit=true"]);

    assert_ne!(code, Some(0), "{created}");
    assert_eq!(created["error"]["code"], "supervisor.init_failed");
    assert_eq!(created["result"]["instance"]["lifecycle"], "failed");
    let operation = &created["result"]["operation"];
    assert_eq!(operation["kind"], "create_instance");
    assert_eq!(operation["state"], "needs_recovery");
    assert_eq!(operation["recovery"]["actions"][0], "abandon");
    let id = instance_id(&created);
    let operation_id = operation["id"].as_str().expect("an operation ID");

    let (code, abandoned) = rig.json(&["operation", "abandon", operation_id, "--yes"]);

    assert_eq!(code, Some(0), "{abandoned}");
    let operation = &abandoned["result"]["operation"];
    assert_eq!(operation["state"], "failed");
    assert_eq!(operation["failure"]["error"]["code"], "operation.abandoned");
    let (code, start) = rig.json(&["instance", "start", &id, "--wait"]);
    assert_ne!(code, Some(0), "{start}");
    assert_eq!(start["error"]["code"], "supervisor.not_initialized");
    assert!(rig.servers(&id).is_empty());
    assert!(
        rig.processes_under_root().is_empty(),
        "{:?}",
        rig.processes_under_root()
    );
}

/// Spec 0014 AC-13.
#[test]
fn secrets_go_to_the_file_keychain_of_the_root_without_any_prompt() {
    let rig = Rig::new();
    let keychain = rig.keychain_file();

    // The CLI has no terminal and STACKLY_NO_INPUT=1, so it is
    // noninteractive and the daemon reads and writes with interaction
    // `Never`: a dialog would hang the create and trip the CLI limit.
    let (code, created) = rig.create("quiet", &[]);

    assert_eq!(code, Some(0), "{created}");
    assert!(
        std::fs::metadata(&keychain)
            .expect("the file keychain exists")
            .len()
            > 0,
        "the keychain file holds the secret of the create"
    );
    let (code, started) = rig.json(&["instance", "start", &instance_id(&created), "--wait"]);
    assert_eq!(code, Some(0), "{started}");
}

/// Spec 0014 AC-11.
#[test]
fn a_running_instance_survives_sigkill_and_sigterm_of_the_daemon_with_the_same_process() {
    let ev = evidence!(
        covers = ["AC-013"],
        kind = Kind::Fixture,
        expected = "after a killed and a stopped stacklyd the instance is the same adopted process, there is one instance and one fake server, its data is unchanged, and a start answers alreadyInState",
    );
    let mut rig = Rig::new();
    let (code, created) = rig.create("demo", &[]);
    assert_eq!(code, Some(0), "{created}");
    let id = instance_id(&created);
    let (code, started) = rig.json(&["instance", "start", &id, "--wait"]);
    assert_eq!(code, Some(0), "{started}");
    let before = rig.get(&id);
    let process = before["process"].clone();
    let pid = u32::try_from(process["pid"].as_u64().expect("a pid")).expect("a pid fits");
    let data = data_digest(&rig.data_dir(&id));
    assert!(data.contains_key("token.sha256"), "{data:?}");

    for how in [Stop::Kill, Stop::Term] {
        rig.restart(how);

        let after = rig.get_until(&id, |instance| instance["readiness"]["state"] == "ready");
        assert_eq!(after["lifecycle"], "running");
        assert_eq!(after["process"]["pid"], process["pid"]);
        assert_eq!(after["process"]["startedAt"], process["startedAt"]);
        assert_eq!(after["process"]["adopted"], true);
        assert_eq!(rig.listed_names(), ["demo"]);
        assert_eq!(rig.servers(&id), [pid]);
        assert_eq!(data_digest(&rig.data_dir(&id)), data);
        let (code, again) = rig.json(&["instance", "start", &id, "--wait"]);
        assert_eq!(code, Some(0), "{again}");
        assert!(again["result"]["operation"].is_null(), "{again}");
        assert_eq!(rig.servers(&id), [pid], "a start spawns nothing new");
    }

    let (code, stopped) = rig.json(&["instance", "stop", &id, "--wait"]);
    assert_eq!(code, Some(0), "{stopped}");
    assert!(rig.servers(&id).is_empty());
    assert!(
        rig.processes_under_root().is_empty(),
        "{:?}",
        rig.processes_under_root()
    );
    ev.pass();
}

/// Spec 0014 AC-12.
#[test]
fn a_create_cut_off_by_sigkill_ends_needing_recovery_with_one_instance_and_no_stray_process() {
    let ev = evidence!(
        covers = ["AC-013"],
        kind = Kind::Fixture,
        expected = "a stacklyd killed during the init of a create comes back with the operation in needs_recovery, one failed instance with that name, the same operation for the same key, name_taken for another key, an init helper nobody signalled, and no process left after the abandon",
    );
    let mut rig = Rig::new();
    let args = [
        "--set-json",
        "initDelayMs=15000",
        "--idempotency-key",
        "cut-off-key",
    ];
    let (code, accepted) = rig.create_with("demo", &args);
    assert_eq!(code, Some(0), "{accepted}");
    let operation_id = accepted["result"]["operation"]["id"]
        .as_str()
        .expect("an operation ID")
        .to_owned();
    let id = accepted["result"]["operation"]["resource"]["id"]
        .as_str()
        .expect("the operation names the instance")
        .to_owned();
    let data = rig.data_dir(&id).display().to_string();
    let helper = [data.as_str(), "stackly-fake-server init"];
    let started = ProcessTable::wait_for(&helper, Duration::from_secs(20), |rows| !rows.is_empty())
        .expect("the init helper starts");
    assert!(
        started
            .iter()
            .all(|row| row.args.contains("--delay-ms=15000")),
        "the delay of the setting reaches init (AC-4): {started:?}"
    );

    rig.restart(Stop::Kill);

    let alive_before = !ProcessTable::snapshot()
        .expect("the process table reads")
        .containing_all(&helper)
        .is_empty();
    let instance = rig.get(&id);
    let alive_after = !ProcessTable::snapshot()
        .expect("the process table reads")
        .containing_all(&helper)
        .is_empty();
    assert_eq!(instance["lifecycle"], "failed", "{instance}");
    assert_eq!(instance["lastFailure"]["code"], "interrupted", "{instance}");
    if alive_before && alive_after {
        assert_eq!(instance["blocker"]["type"], "dataInUse", "{instance}");
    }
    let (_, inspected) = rig.json(&["operation", "inspect", &operation_id]);
    assert_eq!(inspected["result"]["operation"]["state"], "needs_recovery");
    assert_eq!(rig.listed_names(), ["demo"]);
    if alive_after {
        assert!(
            !ProcessTable::snapshot()
                .expect("the process table reads")
                .containing_all(&helper)
                .is_empty(),
            "the new daemon never signals the helper while it is checking"
        );
    }

    let (code, replay) = rig.create_with("demo", &args);
    assert_eq!(code, Some(0), "{replay}");
    assert_eq!(replay["result"]["operation"]["id"], operation_id.as_str());
    let (code, other) = rig.create_with("demo", &["--idempotency-key", "another-key"]);
    assert_ne!(code, Some(0), "{other}");
    assert_eq!(other["error"]["code"], "instance.name_taken");
    assert!(other["result"].is_null() || other["result"]["operation"].is_null());
    assert_eq!(rig.listed_names(), ["demo"]);

    let (code, abandoned) = rig.json(&["operation", "abandon", &operation_id, "--yes"]);
    assert_eq!(code, Some(0), "{abandoned}");
    assert_eq!(abandoned["result"]["operation"]["state"], "failed");
    assert_eq!(
        abandoned["result"]["operation"]["failure"]["error"]["code"],
        "operation.abandoned"
    );
    let (code, start) = rig.json(&["instance", "start", &id, "--wait"]);
    assert_ne!(code, Some(0), "{start}");
    assert_eq!(start["error"]["code"], "supervisor.not_initialized");
    ProcessTable::wait_for(&helper, Duration::from_secs(25), |rows| rows.is_empty())
        .expect("the helper ends on its own after its delay");
    assert!(
        rig.processes_under_root().is_empty(),
        "{:?}",
        rig.processes_under_root()
    );
    ev.pass();
}

/// Spec 0014 AC-18.
#[test]
fn the_fixture_credential_reaches_the_init_helper_and_leaves_no_trace_anywhere_else() {
    let ev = evidence!(
        covers = ["AC-016", "REQ-016"],
        kind = Kind::Fixture,
        expected = "a fixture credential sent through --secret authToken=stdin on create reaches the init helper (token.sha256 is its SHA-256) and its prefix is in no daemon log, stderr file, registry file with its WAL, spool, file under the instance folder of the data root, or CLI output of the run, and after stop the secrets folder of the instance in the runtime root is gone",
    );
    let rig = Rig::new();
    let credential = FixtureCredential::new();
    let mut piped = credential.value().as_bytes().to_vec();
    piped.push(b'\n');

    let (code, created) = rig.create_piped(
        "secret",
        &["--wait", "--secret", "authToken=stdin"],
        Some(&piped),
    );

    assert_eq!(code, Some(0), "{created}");
    let id = instance_id(&created);
    let expected: String = Sha256::digest(credential.value().as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let recorded = std::fs::read_to_string(rig.data_dir(&id).join("token.sha256"))
        .expect("the init helper wrote token.sha256");
    assert_eq!(
        recorded.trim(),
        expected,
        "the init helper recorded the digest of the token it was given"
    );
    let (code, started) = rig.json(&["instance", "start", &id, "--wait"]);
    assert_eq!(code, Some(0), "{started}");
    let secret_config = rig.secrets_dir(&id).join("fake.json");
    let rendered = std::fs::read_to_string(&secret_config)
        .expect("the running instance has its sensitive config in the secrets folder");
    assert!(
        rendered.contains(credential.value()),
        "the sensitive config holds the token while the instance runs"
    );
    let (code, stopped) = rig.json(&["instance", "stop", &id, "--wait"]);
    assert_eq!(code, Some(0), "{stopped}");
    let secrets_dir = rig.secrets_dir(&id);
    assert!(
        !secrets_dir.exists(),
        "the stop left the secrets folder of the instance at {}",
        secrets_dir.display()
    );

    credential.assert_absent(&rig.transcript.borrow(), "the CLI output");
    let files = rig.scanned_files();
    assert!(
        files.iter().any(|(name, _)| name.ends_with("stacklyd.log")),
        "the daemon log is scanned: {:?}",
        files.iter().map(|(name, _)| name).collect::<Vec<_>>()
    );
    assert!(
        files
            .iter()
            .any(|(name, _)| name.ends_with("stackly.sqlite"))
    );
    for (name, text) in &files {
        credential.assert_absent(text, name);
    }
    ev.pass_with(&format!("{} files scanned", files.len()));
}

/// `/usr/bin/security lock-keychain <file>`.
fn lock_keychain(file: &Path) {
    let output = Command::new("/usr/bin/security")
        .arg("lock-keychain")
        .arg(file)
        .output()
        .expect("security runs");
    assert!(
        output.status.success(),
        "security lock-keychain failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Spec 0014 AC-19.
#[test]
fn a_locked_file_keychain_blocks_the_start_with_needs_secret_until_the_daemon_restarts() {
    let ev = evidence!(
        covers = ["AC-016", "REQ-016"],
        kind = Kind::Fixture,
        expected = "with the file keychain of the root locked by security lock-keychain, a start through stackly without a terminal fails secret.needs_secret without a dialog, the instance stays stopped and get shows a needsSecret blocker, and after the daemon restarts the start succeeds and the blocker is gone",
    );
    let manager = Command::new("launchctl")
        .arg("managername")
        .output()
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .unwrap_or_default();
    if manager != "Aqua" {
        ev.blocked(
            &format!(
                "no GUI login session (launchd manager `{manager}`), which locking a keychain needs"
            ),
            "the user of this Mac, logged in to a GUI session",
        );
        return;
    }
    let mut rig = Rig::new();
    let (code, created) = rig.create("locked", &[]);
    assert_eq!(code, Some(0), "{created}");
    let id = instance_id(&created);
    lock_keychain(&rig.keychain_file());

    let (code, blocked) = rig.json(&["instance", "start", &id, "--wait"]);

    assert_ne!(code, Some(0), "{blocked}");
    assert_eq!(blocked["error"]["code"], "secret.needs_secret", "{blocked}");
    let failure = &blocked["error"];
    let items = failure["items"]
        .as_array()
        .unwrap_or_else(|| panic!("the failure lists the references: {blocked}"));
    assert!(!items.is_empty(), "{blocked}");
    let field = |item: &serde_json::Value, key: &str| -> Option<String> {
        item["fields"]
            .as_array()?
            .iter()
            .find(|field| field["key"] == key)
            .and_then(|field| field["value"].as_str().map(str::to_owned))
    };
    assert!(
        items.iter().all(|item| item["kind"] == "reference"
            && field(item, "reason").as_deref() == Some("store_locked")
            && field(item, "reference").is_some_and(|r| r.starts_with("local:instance/"))),
        "{blocked}"
    );
    assert!(
        failure["hint"]
            .as_str()
            .is_some_and(|hint| hint.contains("stackly secret set")),
        "{blocked}"
    );
    let instance = rig.get(&id);
    assert_eq!(instance["lifecycle"], "stopped", "{instance}");
    assert_eq!(instance["blocker"]["type"], "needsSecret", "{instance}");
    assert!(rig.servers(&id).is_empty());

    rig.restart(Stop::Term);

    let (code, started) = rig.json(&["instance", "start", &id, "--wait"]);
    assert_eq!(code, Some(0), "{started}");
    assert_eq!(started["result"]["instance"]["lifecycle"], "running");
    let instance = rig.get(&id);
    assert!(instance["blocker"].is_null(), "{instance}");
    let (code, stopped) = rig.json(&["instance", "stop", &id, "--wait"]);
    assert_eq!(code, Some(0), "{stopped}");
    ev.pass();
}

/// Spec 0014 AC-6.
#[test]
fn a_start_of_a_running_instance_and_a_stop_of_a_stopped_one_answer_already_in_state() {
    let rig = Rig::new();
    let (code, created) = rig.create("demo", &[]);
    assert_eq!(code, Some(0), "{created}");
    let id = instance_id(&created);

    let (code, stopped_again) = rig.json(&["instance", "stop", &id, "--wait"]);
    assert_eq!(code, Some(0), "{stopped_again}");
    assert_eq!(stopped_again["result"]["instance"]["lifecycle"], "stopped");
    assert!(
        rig.servers(&id).is_empty(),
        "a stop of a stopped instance starts nothing"
    );

    let (code, started) = rig.json(&["instance", "start", &id, "--wait"]);
    assert_eq!(code, Some(0), "{started}");
    let pid = rig.servers(&id);
    assert_eq!(pid.len(), 1);
    let (code, started_again) = rig.json(&["instance", "start", &id, "--wait"]);

    assert_eq!(code, Some(0), "{started_again}");
    assert_eq!(started_again["result"]["instance"]["lifecycle"], "running");
    assert_eq!(rig.servers(&id), pid, "a second start spawns nothing");
    let operation = &started_again["result"]["operation"];
    assert!(
        operation.is_null() || operation["state"] == "succeeded",
        "an answer that was already in state leaves no operation to wait for: {operation}"
    );
    let (code, stopped) = rig.json(&["instance", "stop", &id, "--wait"]);
    assert_eq!(code, Some(0), "{stopped}");
    assert!(rig.servers(&id).is_empty());
}

/// Spec 0014 AC-8.
#[test]
fn a_create_repeated_with_the_same_key_returns_the_same_operation_and_one_instance() {
    let rig = Rig::new();
    let args = ["--wait", "--idempotency-key", "same-key"];

    let (code, first) = rig.create_with("demo", &args);
    let (second_code, second) = rig.create_with("demo", &args);

    assert_eq!(code, Some(0), "{first}");
    assert_eq!(second_code, Some(0), "{second}");
    assert_eq!(first["result"]["idempotencyKey"], "same-key");
    assert_eq!(second["result"]["idempotencyKey"], "same-key");
    assert_eq!(
        second["result"]["operation"]["id"], first["result"]["operation"]["id"],
        "the key names the first operation again"
    );
    assert_eq!(instance_id(&second), instance_id(&first));
    assert_eq!(rig.listed_names(), ["demo"]);
}

/// Spec 0014 AC-7.
#[test]
fn an_instance_the_real_daemon_does_not_know_fails_not_found_with_exit_10() {
    let rig = Rig::new();

    for verb in ["get", "start", "stop"] {
        let (code, envelope) =
            rig.json(&["instance", verb, "0192f3a5-0000-7000-8000-000000000001"]);

        assert_eq!(code, Some(10), "{verb}: {envelope}");
        assert_eq!(
            envelope["error"]["code"], "resource.not_found",
            "{verb}: {envelope}"
        );
    }
    assert!(rig.listed_names().is_empty());
}
