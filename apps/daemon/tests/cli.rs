//! `stackly operation` against stacklyd (spec 0008 build step 12, the real
//! daemon part): the server, engine, and registry run in process on the
//! layout `STACKLY_ROOT` gives a test root, the test's method starts the
//! operations (spec 0007 build step 15; only this crate's tests may
//! register it), and the real `stackly` binary lists, inspects, waits for,
//! cancels, resumes, and abandons them over the socket.
#![cfg(target_os = "macos")]

#[allow(
    dead_code,
    reason = "this binary uses a part of the operation test helpers"
)]
#[path = "operations/support.rs"]
mod support;

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use stackly_core::operation::testing::{HoldHandle, ScriptedWork};
use stackly_core::{PreparedLayout, prepare_layout};
use stackly_domain::OperationKind;
use stackly_ipc::{Client, ConnectPolicy};
use stackly_platform::{Clock, Platform, SystemClock, UserId};
use stackly_platform_macos::MacosPlatform;
use stackly_protocol::Timestamp;
use stackly_protocol::events::ShutdownReason;
use stackly_protocol::handshake::{ClientInfo, ClientKind};
use stackly_protocol::operation::OperationState;
use stackly_testkit::TestRoot;
use stackly_testkit::cargo_bin;
use stackly_testkit::evidence::{Kind, evidence};
use stacklyd::dispatch::Handlers;
use stacklyd::ipc::operation::TestStartingMethod;
use stacklyd::lock::{DaemonLock, LockOwner};
use stacklyd::log_sink::{LogSink, LogSinkConfig};
use stacklyd::map::StartFacts;
use stacklyd::server::{self, DaemonDeps, DaemonHandle};
use uuid::Uuid;

use support::{PATIENCE, Raw, Script, Scripts, accepted, get, key, wait_state};

/// The default handlers plus the test's starting method.
struct WithScripts(Scripts);

impl Handlers for WithScripts {
    fn test_starting_method(&self) -> Option<Arc<dyn TestStartingMethod>> {
        Some(Arc::new(self.0.clone()))
    }
}

/// A test root laid out as `STACKLY_ROOT` lays it out, so `stackly` with
/// that override finds the socket, with the daemon lock held. Its runtime
/// root sits in the real user temporary directory, so the site removes it
/// again when it is empty or holds only an empty `i/`, as
/// `SpawnedDaemon` does.
struct Site {
    root: TestRoot,
    platform: Platform,
    prepared: PreparedLayout,
    runtime: PathBuf,
    _lock: DaemonLock,
}

impl Site {
    fn new() -> Self {
        support::install_logs();
        let root = TestRoot::new().expect("a test root can be created");
        let platform =
            MacosPlatform::new(Some(root.path().into())).expect("a test root is a valid override");
        let prepared = prepare_layout(&platform).expect("the test root layout can be prepared");
        let runtime = prepared.layout().roots().runtime().to_path_buf();
        let owner = LockOwner::new(
            std::process::id(),
            Uuid::now_v7(),
            Timestamp::from_jiff(SystemClock.now()),
            "0.1.0",
        );
        let lock = DaemonLock::acquire(&prepared.layout().daemon_lock(), owner)
            .expect("the test root's lock is free");
        Self {
            root,
            platform,
            prepared,
            runtime,
            _lock: lock,
        }
    }

    fn socket(&self) -> PathBuf {
        self.prepared.layout().daemon_socket()
    }

    fn uid(&self) -> u32 {
        match self
            .platform
            .identity()
            .current_user()
            .expect("the current user can be read")
        {
            UserId::Unix(uid) => uid,
            other => panic!("not a unix user: {other:?}"),
        }
    }

    /// Starts a daemon serving `scripts` and waits for its start slot.
    async fn start(&self, scripts: &Scripts) -> DaemonHandle {
        let log = LogSink::start(Box::new(std::io::sink()), LogSinkConfig::DAEMON)
            .expect("the log writer starts");
        let facts = StartFacts::new(
            Uuid::now_v7(),
            Timestamp::from_jiff(SystemClock.now()),
            true,
            std::process::id(),
        );
        let deps = DaemonDeps::new(
            self.platform.clone(),
            self.prepared.clone(),
            facts,
            Arc::new(log),
        )
        .with_handlers(WithScripts(scripts.clone()));
        let handle = server::start(deps).await.expect("the server starts");
        handle.started().await;
        handle
    }

    /// A typed client, for what the test checks beside the CLI.
    async fn client(&self) -> Client {
        Client::connect(
            &self.socket(),
            ConnectPolicy::same_user(self.uid()),
            ClientInfo {
                kind: ClientKind::Gui,
                version: "0.1.0".to_owned(),
                pid: Some(std::process::id()),
                interactive: Some(false),
            },
        )
        .await
        .expect("the client connects")
        .0
    }

    /// The root folder of an instance.
    fn instance_root(&self, id: &str) -> PathBuf {
        let id = id.parse().expect("an instance ID");
        self.prepared.layout().instance_paths(id).root
    }
}

impl Drop for Site {
    fn drop(&mut self) {
        let Ok(entries) = std::fs::read_dir(&self.runtime) else {
            return;
        };
        let names: Vec<_> = entries
            .filter_map(Result::ok)
            .map(|entry| entry.file_name())
            .collect();
        let runs = self.runtime.join("i");
        let only_empty_runs = names.len() == 1
            && names[0] == "i"
            && std::fs::read_dir(&runs).is_ok_and(|mut entries| entries.next().is_none());
        if only_empty_runs {
            let _ = std::fs::remove_dir(&runs);
        }
        if names.is_empty() || only_empty_runs {
            let _ = std::fs::remove_dir(&self.runtime);
        }
    }
}

async fn stop(daemon: DaemonHandle) {
    daemon
        .shutdown(ShutdownReason::Requested, std::future::pending())
        .await;
}

/// `stackly <args>` for `root` with a clean environment: `STACKLY_ROOT`
/// set, the color and input variables unset, `TZ=UTC`, stdin closed (so it
/// runs noninteractive), and stdout and stderr piped.
fn command(root: &Path, args: &[String]) -> Command {
    let mut command = Command::new(cargo_bin("stackly-cli", "stackly"));
    command
        .args(args)
        .env("STACKLY_ROOT", root)
        .env("TZ", "UTC")
        .env_remove("NO_COLOR")
        .env_remove("CLICOLOR")
        .env_remove("CLICOLOR_FORCE")
        .env_remove("STACKLY_NO_INPUT")
        .env_remove("CI")
        .env("TERM", "xterm-256color")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

/// Runs `stackly` with stderr on a pty (through `/usr/bin/script`), where
/// progress is the live line, and returns the exit code and what the pty
/// showed. `script` ties stdout to the pty too, so the text is both.
async fn on_a_pty(site: &Site, args: &[&str]) -> (Option<i32>, String) {
    let root = site.root.path().to_path_buf();
    let args = owned(args);
    tokio::task::spawn_blocking(move || {
        let mut command = Command::new("/usr/bin/script");
        command
            .args(["-q", "/dev/null"])
            .arg(cargo_bin("stackly-cli", "stackly"))
            .args(&args)
            .env("STACKLY_ROOT", &root)
            .env("TZ", "UTC")
            .env("TERM", "xterm-256color")
            .env_remove("NO_COLOR")
            .env_remove("CLICOLOR")
            .env_remove("CLICOLOR_FORCE")
            .env_remove("STACKLY_NO_INPUT")
            .env_remove("CI")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let output = command.output().expect("script runs");
        (
            output.status.code(),
            String::from_utf8_lossy(&output.stdout).into_owned(),
        )
    })
    .await
    .expect("the pty run's thread ends")
}

fn owned(args: &[&str]) -> Vec<String> {
    args.iter().map(|arg| (*arg).to_owned()).collect()
}

/// What one run of `stackly` left.
#[derive(Debug)]
struct Run {
    code: Option<i32>,
    stdout: String,
    stderr: String,
    elapsed: Duration,
}

impl Run {
    /// The one envelope line on stdout, with the exit rule: `error` is
    /// null exactly when the exit code is 0.
    fn envelope(&self) -> Value {
        let line = self
            .stdout
            .strip_suffix('\n')
            .unwrap_or_else(|| panic!("one line on stdout: {self:?}"));
        assert!(!line.contains('\n'), "one line on stdout: {self:?}");
        let value: Value = serde_json::from_str(line).expect("the line is JSON");
        assert_eq!(value["schemaVersion"], 2);
        assert_eq!(value["type"], "result");
        assert_eq!(
            value["error"].is_null(),
            self.code == Some(0),
            "error is null exactly when the exit code is 0: {self:?}"
        );
        value
    }
}

/// Runs `stackly <args>` to its end, off the runtime's thread so the
/// daemon in process keeps serving.
async fn stackly(site: &Site, args: &[&str]) -> Run {
    let root = site.root.path().to_path_buf();
    let args = owned(args);
    tokio::task::spawn_blocking(move || {
        let started = Instant::now();
        let output = command(&root, &args).output().expect("stackly runs");
        Run {
            code: output.status.code(),
            stdout: String::from_utf8(output.stdout).expect("stdout is utf-8"),
            stderr: String::from_utf8(output.stderr).expect("stderr is utf-8"),
            elapsed: started.elapsed(),
        }
    })
    .await
    .expect("the run's thread ends")
}

/// A `stackly` run in the background whose stderr the test reads while it
/// runs; dropped, the process is killed and reaped.
struct Waiting {
    child: Option<Child>,
    stderr: Arc<Mutex<String>>,
    reader: Option<std::thread::JoinHandle<()>>,
    started: Instant,
}

impl Waiting {
    async fn start(site: &Site, args: &[&str]) -> Self {
        let root = site.root.path().to_path_buf();
        let args = owned(args);
        let started = Instant::now();
        let mut child = tokio::task::spawn_blocking(move || {
            command(&root, &args).spawn().expect("stackly starts")
        })
        .await
        .expect("the spawn's thread ends");
        let mut pipe = child.stderr.take().expect("stderr is piped");
        let stderr = Arc::new(Mutex::new(String::new()));
        let sink = Arc::clone(&stderr);
        let reader = std::thread::spawn(move || {
            let mut buffer = [0_u8; 4096];
            while let Ok(read) = pipe.read(&mut buffer) {
                if read == 0 {
                    break;
                }
                sink.lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push_str(&String::from_utf8_lossy(&buffer[..read]));
            }
        });
        Self {
            child: Some(child),
            stderr,
            reader: Some(reader),
            started,
        }
    }

    fn stderr(&self) -> String {
        self.stderr
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Waits until stderr holds `text`.
    async fn until(&self, text: &str) {
        let deadline = Instant::now() + PATIENCE;
        while !self.stderr().contains(text) {
            assert!(
                Instant::now() < deadline,
                "stackly did not write {text:?} to stderr: {:?}",
                self.stderr()
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    /// Sends SIGINT, as Ctrl+C does.
    fn interrupt(&self) {
        let pid = self.child.as_ref().expect("the run is going").id();
        let status = Command::new("/bin/kill")
            .args(["-INT", &pid.to_string()])
            .status()
            .expect("kill runs");
        assert!(status.success(), "kill -INT {pid} failed");
    }

    /// Waits for the run to end.
    async fn finish(mut self) -> Run {
        let child = self.child.take().expect("the run is going");
        let reader = self.reader.take().expect("the reader runs");
        let output = tokio::task::spawn_blocking(move || {
            let output = child.wait_with_output().expect("stackly ends");
            reader.join().expect("the stderr reader ends");
            output
        })
        .await
        .expect("the wait's thread ends");
        Run {
            code: output.status.code(),
            stdout: String::from_utf8(output.stdout).expect("stdout is utf-8"),
            stderr: self.stderr(),
            elapsed: self.started.elapsed(),
        }
    }
}

impl Drop for Waiting {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// The methods of the requests the daemon logged for operation `id`.
fn requests_for(id: Uuid) -> Vec<String> {
    let id = id.to_string();
    support::log_lines()
        .iter()
        .filter(|line| {
            line["fields"]["message"] == "request" && line["fields"]["operation_id"] == id.as_str()
        })
        .map(|line| {
            line["fields"]["method"]
                .as_str()
                .unwrap_or_default()
                .to_owned()
        })
        .collect()
}

/// The value of detail `key` of a JSON error body.
fn detail(error: &Value, key: &str) -> Option<String> {
    error["details"]
        .as_array()?
        .iter()
        .find(|detail| detail["key"] == key)
        .and_then(|detail| detail["value"].as_str().map(str::to_owned))
}

/// The IDs of a `stackly operation list --json` result.
fn listed(run: &Run) -> Vec<String> {
    assert_eq!(run.code, Some(0), "{run:?}");
    run.envelope()["result"]["items"]
        .as_array()
        .expect("items")
        .iter()
        .map(|item| item["id"].as_str().expect("an ID").to_owned())
        .collect()
}

#[tokio::test]
async fn a_running_operation_is_listed_inspected_and_waited_for_and_stopping_the_wait_never_cancels_it()
 {
    let ev = evidence!(
        covers = ["REQ-028", "REQ-054", "AC-054", "AC-028"],
        kind = Kind::Fixture,
        expected = "against stacklyd with a held operation, stackly operation list and inspect show it running, inspect --wait --timeout 1s exits 8 and Ctrl+C exits 130 with the operation still running and no operation.cancel sent, inspect --wait exits 0 once it succeeds, AC-8: counted progress 1/2 shows in the list, the table, and the live wait line on a pty, and a cancel after that exits 3 with cli.cancel_not_applied",
    );
    let site = Site::new();
    let scripts = Scripts::new();
    let hold = HoldHandle::new();
    scripts.add(
        "held",
        Script::new(
            OperationKind::UpdateManager,
            ScriptedWork::new()
                .step("prepare")
                .step("wait")
                .hold(&hold)
                .progress(1, 2),
        ),
    );
    let daemon = site.start(&scripts).await;
    let client = site.client().await;
    let mut raw = Raw::ready(&site.socket()).await;
    let id = accepted(&raw.start(Some(&key()), json!({ "script": "held" })).await)
        .operation
        .id;
    let id_text = id.to_string();
    tokio::time::timeout(PATIENCE, hold.started())
        .await
        .unwrap();

    let list = stackly(&site, &["operation", "list", "--json"]).await;
    let active = stackly(&site, &["operation", "list", "--active", "--json"]).await;
    let table = stackly(&site, &["operation", "list"]).await;
    let inspect = stackly(&site, &["operation", "inspect", &id_text, "--json"]).await;
    let inspect_text = stackly(&site, &["operation", "inspect", &id_text]).await;

    assert_eq!(listed(&list), std::slice::from_ref(&id_text));
    let item = &list.envelope()["result"]["items"][0];
    assert_eq!(item["state"], "running");
    // A stage comes only from the work's `set_stage` (spec 0007), which a
    // scripted work never calls: a step name is not a stage.
    assert!(item["stage"].is_null(), "{item}");
    assert_eq!(item["kind"], "update_manager");
    assert_eq!(listed(&active), std::slice::from_ref(&id_text));
    assert_eq!(table.code, Some(0), "{table:?}");
    let rows: Vec<&str> = table.stdout.lines().collect();
    assert_eq!(rows.len(), 2, "{}", table.stdout);
    assert!(rows[0].starts_with("ID "), "{}", table.stdout);
    assert!(rows[1].starts_with(&id_text), "{}", table.stdout);
    assert!(
        rows[1].contains("update_manager") && rows[1].contains("running"),
        "{}",
        table.stdout
    );
    assert_eq!(inspect.code, Some(0), "{inspect:?}");
    let result = &inspect.envelope()["result"];
    assert_eq!(result["operation"]["id"], id_text.as_str());
    assert_eq!(result["operation"]["state"], "running");
    let steps: Vec<(&str, &str)> = result["steps"]
        .as_array()
        .expect("steps")
        .iter()
        .map(|step| {
            (
                step["name"].as_str().expect("a name"),
                step["state"].as_str().expect("a state"),
            )
        })
        .collect();
    assert_eq!(steps, [("prepare", "done"), ("wait", "running")]);
    assert_eq!(inspect_text.code, Some(0), "{inspect_text:?}");
    assert!(
        inspect_text
            .stdout
            .lines()
            .any(|line| line.starts_with("State ") && line.ends_with(" running")),
        "{}",
        inspect_text.stdout
    );
    assert!(inspect_text.stdout.contains("\nSteps:\n"));

    let timed_out = stackly(
        &site,
        &[
            "operation",
            "inspect",
            &id_text,
            "--wait",
            "--timeout",
            "1s",
            "--json",
        ],
    )
    .await;

    assert_eq!(timed_out.code, Some(8), "{timed_out:?}");
    assert!(timed_out.elapsed < Duration::from_secs(5), "{timed_out:?}");
    let value = timed_out.envelope();
    assert_eq!(value["error"]["code"], "cli.wait_timeout");
    assert_eq!(value["error"]["operationId"], id_text.as_str());
    assert_eq!(detail(&value["error"], "state").as_deref(), Some("running"));
    assert_eq!(
        detail(&value["error"], "timeoutSeconds").as_deref(),
        Some("1")
    );
    assert_eq!(value["result"]["operation"]["state"], "running");

    // AC-8: counted progress reaches the wait line. Off a terminal a wait
    // line has no count (spec 0008, Progress), so the count is read from
    // the table and from the live line on a pty.
    assert_eq!(
        item["progress"],
        json!({ "type": "counted", "content": { "done": 1, "total": 2 } }),
        "{item}"
    );
    assert!(
        rows[1].contains(" 1/2 "),
        "the table shows the count: {}",
        table.stdout
    );
    let (live_code, live) = on_a_pty(
        &site,
        &[
            "operation",
            "inspect",
            &id_text,
            "--wait",
            "--timeout",
            "2s",
        ],
    )
    .await;
    assert_eq!(live_code, Some(8), "{live:?}");
    assert!(
        live.contains("\r\u{1b}[Krunning 1/2 ("),
        "the live wait line carries the count: {live:?}"
    );

    let interrupted = Waiting::start(
        &site,
        &["operation", "inspect", &id_text, "--wait", "--json"],
    )
    .await;
    interrupted.until(" running\n").await;
    interrupted.interrupt();
    let interrupted = interrupted.finish().await;

    assert_eq!(interrupted.code, Some(130), "{interrupted:?}");
    let value = interrupted.envelope();
    assert_eq!(value["error"]["code"], "cli.interrupted");
    assert_eq!(detail(&value["error"], "state").as_deref(), Some("running"));
    assert_eq!(
        get(&client, id).await.operation.state,
        OperationState::Running,
        "stopping the wait left the operation running"
    );
    assert!(
        !requests_for(id).contains(&"operation.cancel".to_owned()),
        "a stopped wait sent operation.cancel: {:?}",
        requests_for(id)
    );

    let waiting = Waiting::start(
        &site,
        &["operation", "inspect", &id_text, "--wait", "--json"],
    )
    .await;
    waiting.until(" running\n").await;
    hold.release();
    let waited = waiting.finish().await;

    assert_eq!(waited.code, Some(0), "{waited:?}");
    assert!(
        waited
            .stderr
            .starts_with(&format!("waiting for operation {id_text} (")),
        "{}",
        waited.stderr
    );
    assert!(
        waited
            .stderr
            .lines()
            .last()
            .is_some_and(|line| line.ends_with(" succeeded")),
        "{}",
        waited.stderr
    );
    assert_eq!(
        waited.envelope()["result"]["operation"]["state"],
        "succeeded"
    );

    let late = stackly(&site, &["operation", "cancel", &id_text, "--json"]).await;
    let late_text = stackly(&site, &["operation", "cancel", &id_text]).await;
    let none_active = stackly(&site, &["operation", "list", "--active", "--json"]).await;

    assert_eq!(late.code, Some(3), "{late:?}");
    let value = late.envelope();
    assert_eq!(value["error"]["code"], "cli.cancel_not_applied");
    assert_eq!(
        detail(&value["error"], "state").as_deref(),
        Some("succeeded")
    );
    assert_eq!(value["result"]["disposition"], "already_final");
    assert_eq!(late_text.code, Some(3), "{late_text:?}");
    assert_eq!(
        late_text.stdout,
        format!("operation {id_text} already ended (state succeeded)\n")
    );
    assert!(listed(&none_active).is_empty());
    stop(daemon).await;
    ev.pass();
}

#[tokio::test]
async fn cancel_wait_undoes_an_operation_before_its_point_of_no_return_and_exits_0() {
    let ev = evidence!(
        covers = ["REQ-028", "REQ-054", "AC-054"],
        kind = Kind::Fixture,
        expected = "against stacklyd, stackly operation cancel --wait of a running operation before its point of no return exits 0 once it is cancelled and its instance tree is undone, and inspect --wait of the cancelled operation exits 9 with cli.operation_cancelled",
    );
    let site = Site::new();
    let scripts = Scripts::new();
    let hold = HoldHandle::new();
    scripts.add(
        "undoable",
        Script::new(
            OperationKind::CreateInstance,
            ScriptedWork::new()
                .step("reserve")
                .creates_instance_tree()
                .step("wait")
                .hold(&hold),
        )
        .creating(),
    );
    let daemon = site.start(&scripts).await;
    let client = site.client().await;
    let mut raw = Raw::ready(&site.socket()).await;
    let started = accepted(
        &raw.start(Some(&key()), json!({ "script": "undoable" }))
            .await,
    );
    let id = started.operation.id;
    let id_text = id.to_string();
    let resource = started
        .operation
        .resource
        .expect("a create names its instance");
    tokio::time::timeout(PATIENCE, hold.started())
        .await
        .unwrap();
    let tree = site.instance_root(&get(&client, id).await.created[0].reference);
    assert!(tree.exists());

    let by_resource = stackly(
        &site,
        &[
            "operation",
            "list",
            "--resource",
            &format!("instance:{}", resource.id),
            "--json",
        ],
    )
    .await;
    let other_resource = stackly(
        &site,
        &[
            "operation",
            "list",
            "--resource",
            &format!("instance:{}", Uuid::now_v7()),
            "--json",
        ],
    )
    .await;
    let cancelled = stackly(
        &site,
        &["operation", "cancel", &id_text, "--wait", "--json"],
    )
    .await;
    let observed = stackly(
        &site,
        &["operation", "inspect", &id_text, "--wait", "--json"],
    )
    .await;

    assert_eq!(listed(&by_resource), std::slice::from_ref(&id_text));
    assert!(listed(&other_resource).is_empty());
    assert_eq!(cancelled.code, Some(0), "{cancelled:?}");
    let value = cancelled.envelope();
    assert_eq!(value["command"], "operation cancel");
    assert_eq!(value["result"]["disposition"], "requested");
    assert_eq!(value["result"]["operation"]["state"], "cancelled");
    assert!(!tree.exists(), "the cancel undid the instance tree");
    assert_eq!(observed.code, Some(9), "{observed:?}");
    let value = observed.envelope();
    assert_eq!(value["error"]["code"], "cli.operation_cancelled");
    assert_eq!(value["error"]["operationId"], id_text.as_str());
    assert_eq!(value["result"]["operation"]["state"], "cancelled");
    stop(daemon).await;
    ev.pass();
}

#[tokio::test]
async fn cancel_wait_past_the_point_of_no_return_ends_succeeded_and_exits_3() {
    let ev = evidence!(
        covers = ["REQ-028", "REQ-054", "AC-054"],
        kind = Kind::Fixture,
        expected = "against stacklyd, stackly operation cancel --wait of an operation inside its irreversible step answers continuing_past_point_of_no_return, waits while the step runs to its end, and exits 3 with cli.cancel_not_applied and the succeeded state",
    );
    let site = Site::new();
    let scripts = Scripts::new();
    let hold = HoldHandle::new();
    scripts.add(
        "committed",
        Script::new(
            OperationKind::RestoreInstance,
            ScriptedWork::new()
                .step("commit")
                .irreversible()
                .hold(&hold),
        )
        .creating(),
    );
    let daemon = site.start(&scripts).await;
    let mut raw = Raw::ready(&site.socket()).await;
    let id = accepted(
        &raw.start(Some(&key()), json!({ "script": "committed" }))
            .await,
    )
    .operation
    .id;
    let id_text = id.to_string();
    tokio::time::timeout(PATIENCE, hold.started())
        .await
        .unwrap();

    let waiting = Waiting::start(
        &site,
        &["operation", "cancel", &id_text, "--wait", "--json"],
    )
    .await;
    waiting.until(" cancel_requested\n").await;
    hold.release();
    let run = waiting.finish().await;

    assert_eq!(run.code, Some(3), "{run:?}");
    let value = run.envelope();
    assert_eq!(value["error"]["code"], "cli.cancel_not_applied");
    assert_eq!(value["error"]["operationId"], id_text.as_str());
    assert_eq!(
        detail(&value["error"], "state").as_deref(),
        Some("succeeded")
    );
    assert_eq!(
        value["result"]["disposition"],
        "continuing_past_point_of_no_return"
    );
    assert_eq!(value["result"]["operation"]["state"], "succeeded");
    assert!(!value["result"]["operation"]["cancelRequestedAt"].is_null());
    stop(daemon).await;
    ev.pass();
}

#[tokio::test]
async fn a_failed_operation_exits_0_to_inspect_and_by_its_errors_category_to_inspect_wait() {
    let ev = evidence!(
        covers = ["REQ-028", "AC-028"],
        kind = Kind::Fixture,
        expected = "against stacklyd, stackly operation inspect of a failed operation exits 0, and inspect --wait exits 7 from the failed category of the operation's own error, which the envelope carries unchanged",
    );
    let site = Site::new();
    let scripts = Scripts::new();
    scripts.add(
        "fails",
        Script::new(
            OperationKind::UpdateManager,
            ScriptedWork::new()
                .step("render")
                .fail("provider.render_failed", false),
        ),
    );
    let daemon = site.start(&scripts).await;
    let client = site.client().await;
    let mut raw = Raw::ready(&site.socket()).await;
    let id = accepted(&raw.start(Some(&key()), json!({ "script": "fails" })).await)
        .operation
        .id;
    let id_text = id.to_string();
    let failed = wait_state(&client, id, OperationState::Failed).await;
    let error = failed.operation.failure.expect("a failure").error;

    let inspect = stackly(&site, &["operation", "inspect", &id_text, "--json"]).await;
    let waited = stackly(
        &site,
        &["operation", "inspect", &id_text, "--wait", "--json"],
    )
    .await;

    assert_eq!(inspect.code, Some(0), "{inspect:?}");
    assert_eq!(inspect.envelope()["result"]["operation"]["state"], "failed");
    assert_eq!(waited.code, Some(7), "{waited:?}");
    let value = waited.envelope();
    assert_eq!(
        value["error"],
        serde_json::to_value(&error).expect("an error body serializes")
    );
    assert_eq!(value["error"]["code"], "provider.render_failed");
    assert_eq!(value["result"]["operation"]["state"], "failed");
    stop(daemon).await;
    ev.pass();
}

#[tokio::test]
async fn resume_sends_the_revision_it_read_and_waits_for_the_resumed_operation() {
    let ev = evidence!(
        covers = ["REQ-028", "REQ-054", "AC-054"],
        kind = Kind::Fixture,
        expected = "against stacklyd, inspect --wait of an operation that needs recovery exits by the category of its recovery error, stackly operation resume --wait sends the revision operation.get gave and exits 0 once the resumed operation succeeds, and a second resume exits 3 with operation.not_resumable",
    );
    let site = Site::new();
    let scripts = Scripts::new();
    scripts.add(
        "flaky",
        Script::new(
            OperationKind::InstallRuntime,
            ScriptedWork::new()
                .step("download")
                .repeatable()
                .irreversible()
                .fail("internal.unexpected", false),
        )
        .rebuilt_as(
            ScriptedWork::new()
                .step("download")
                .repeatable()
                .irreversible(),
        ),
    );
    let daemon = site.start(&scripts).await;
    let client = site.client().await;
    let mut raw = Raw::ready(&site.socket()).await;
    let id = accepted(&raw.start(Some(&key()), json!({ "script": "flaky" })).await)
        .operation
        .id;
    let id_text = id.to_string();
    wait_state(&client, id, OperationState::NeedsRecovery).await;

    let stuck = stackly(
        &site,
        &["operation", "inspect", &id_text, "--wait", "--json"],
    )
    .await;
    let stuck_text = stackly(&site, &["operation", "inspect", &id_text]).await;
    let resumed = stackly(
        &site,
        &["operation", "resume", &id_text, "--wait", "--json"],
    )
    .await;
    let again = stackly(&site, &["operation", "resume", &id_text, "--json"]).await;

    assert_eq!(stuck.code, Some(1), "{stuck:?}");
    let value = stuck.envelope();
    assert_eq!(value["error"]["code"], "internal.unexpected");
    assert_eq!(value["error"]["operationId"], id_text.as_str());
    assert_eq!(value["result"]["operation"]["state"], "needs_recovery");
    assert_eq!(stuck_text.code, Some(0), "{stuck_text:?}");
    assert!(
        stuck_text.stdout.lines().any(
            |line| line.starts_with("Recovery ") && line.ends_with(" resume after step_failed")
        ),
        "{}",
        stuck_text.stdout
    );
    assert!(
        stuck_text
            .stdout
            .lines()
            .any(|line| line.starts_with("Actions ") && line.ends_with(" resume, abandon")),
        "{}",
        stuck_text.stdout
    );
    assert_eq!(resumed.code, Some(0), "{resumed:?}");
    let value = resumed.envelope();
    assert_eq!(value["command"], "operation resume");
    assert_eq!(value["result"]["state"], "succeeded");
    assert_eq!(get(&client, id).await.steps[0].attempts, 2);
    assert_eq!(again.code, Some(3), "{again:?}");
    let value = again.envelope();
    assert_eq!(value["error"]["code"], "operation.not_resumable");
    assert_eq!(
        detail(&value["error"], "reason").as_deref(),
        Some("wrong_state")
    );
    stop(daemon).await;
    ev.pass();
}

#[tokio::test]
async fn abandon_needs_yes_noninteractive_then_closes_the_operation_with_its_leftovers() {
    let ev = evidence!(
        covers = ["REQ-028", "REQ-054", "AC-054"],
        kind = Kind::Fixture,
        expected = "against stacklyd, stackly operation abandon of an operation that needs recovery exits 2 with cli.confirmation_required without --yes and sends nothing, resume exits 3 with operation.not_resumable, cancel exits by the category of its recovery error, abandon --yes exits 0 with the operation failed with operation.abandoned and its leftovers, and a second abandon exits 3",
    );
    let site = Site::new();
    let scripts = Scripts::new();
    scripts.add(
        "broken",
        Script::new(
            OperationKind::CreateInstance,
            ScriptedWork::new()
                .step("reserve")
                .creates_instance_tree()
                .step("commit")
                .irreversible()
                .fail("internal.unexpected", false),
        )
        .creating(),
    );
    let daemon = site.start(&scripts).await;
    let client = site.client().await;
    let mut raw = Raw::ready(&site.socket()).await;
    let id = accepted(&raw.start(Some(&key()), json!({ "script": "broken" })).await)
        .operation
        .id;
    let id_text = id.to_string();
    let stuck = wait_state(&client, id, OperationState::NeedsRecovery).await;

    let refused = stackly(&site, &["operation", "abandon", &id_text, "--json"]).await;

    assert_eq!(refused.code, Some(2), "{refused:?}");
    let value = refused.envelope();
    assert_eq!(value["error"]["code"], "cli.confirmation_required");
    assert_eq!(detail(&value["error"], "flag").as_deref(), Some("--yes"));
    let still = get(&client, id).await;
    assert_eq!(still.operation.state, OperationState::NeedsRecovery);
    assert_eq!(still.operation.revision, stuck.operation.revision);
    assert!(
        !requests_for(id).contains(&"operation.abandon".to_owned()),
        "{:?}",
        requests_for(id)
    );

    let resume = stackly(&site, &["operation", "resume", &id_text, "--json"]).await;
    let cancel = stackly(&site, &["operation", "cancel", &id_text, "--json"]).await;
    let abandoned = stackly(
        &site,
        &["operation", "abandon", &id_text, "--yes", "--json"],
    )
    .await;
    let again = stackly(
        &site,
        &["operation", "abandon", &id_text, "--yes", "--json"],
    )
    .await;
    let observed = stackly(
        &site,
        &["operation", "inspect", &id_text, "--wait", "--json"],
    )
    .await;

    assert_eq!(resume.code, Some(3), "{resume:?}");
    assert_eq!(
        resume.envelope()["error"]["code"],
        "operation.not_resumable"
    );
    assert_eq!(cancel.code, Some(1), "{cancel:?}");
    let value = cancel.envelope();
    assert_eq!(value["result"]["disposition"], "recovery_required");
    assert_eq!(value["error"]["code"], "internal.unexpected");
    assert_eq!(abandoned.code, Some(0), "{abandoned:?}");
    let value = abandoned.envelope();
    assert_eq!(value["command"], "operation abandon");
    let operation = &value["result"]["operation"];
    assert_eq!(operation["state"], "failed");
    assert_eq!(operation["failure"]["reason"], "abandoned");
    assert_eq!(operation["failure"]["error"]["code"], "operation.abandoned");
    assert!(value["result"]["leftovers"].is_array(), "{value}");
    assert_eq!(again.code, Some(3), "{again:?}");
    let value = again.envelope();
    assert_eq!(value["error"]["code"], "operation.not_abandonable");
    assert_eq!(detail(&value["error"], "state").as_deref(), Some("failed"));
    assert_eq!(observed.code, Some(7), "{observed:?}");
    assert_eq!(observed.envelope()["error"]["code"], "operation.abandoned");
    stop(daemon).await;
    ev.pass();
}
