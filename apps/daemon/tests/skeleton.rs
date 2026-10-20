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

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use common::{daemon_paths, runtime, start_daemon};
use serde_json::Value;
use sha2::{Digest, Sha256};
use stackly_protocol::instance::ListInstancesParams;
use stackly_protocol::{Call, Reply};
use stackly_testkit::evidence::{Kind, evidence};
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
            root,
        }
    }

    /// Runs the CLI and returns its output; a hung run fails the test.
    fn run(&self, args: &[&str]) -> CliOutput {
        self.cli.run(args).expect("stackly runs to the end")
    }

    /// Runs the CLI and returns the envelope.
    fn json(&self, args: &[&str]) -> (Option<i32>, Value) {
        let output = self.run(args);
        let envelope = output.json().expect("stackly writes one JSON envelope");
        (output.code(), envelope)
    }

    /// `instance create <name> ... --wait` with the extra arguments.
    fn create(&self, name: &str, extra: &[&str]) -> (Option<i32>, Value) {
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
            "--wait",
        ];
        args.extend_from_slice(extra);
        self.json(&args)
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

    fn keychain_file(&self) -> PathBuf {
        daemon_paths(&self.root)
            .layout
            .keychain_dir()
            .join("stackly.keychain-db")
    }
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
