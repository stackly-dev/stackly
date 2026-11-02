//! AC-12 of spec 0028: `instance metrics` writes the newest sample for the
//! main process and the tree with their scope labels, `--json` writes the
//! reply of `instance.get_metrics` as it is, an instance that is not running
//! says so and exits 0, and `--watch` asks again with the cursor it holds.

use std::io::{BufRead, BufReader};
use std::process::Stdio;
use std::time::{Duration, Instant};

use stackly_protocol::instance::MetricsReply;
use stackly_protocol::{Call, Reply, ServerFrame};
use stackly_testkit::TestRoot;
use stackly_testkit::evidence::{Kind, evidence};

use crate::common::scripted::{ScriptedDaemon, Step};
use crate::common::{detail, envelope, golden, run, stackly};
use crate::data;

const ID: &str = "0192f3a5-0000-7000-8000-000000000001";

fn fixture(name: &str) -> MetricsReply {
    let path = format!(
        "{}/../../protocols/fixtures/ipc/frame-response-instance-get-metrics-{name}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let text = std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("{path}: {err}"));
    let ServerFrame::Response(response) = serde_json::from_str(&text).expect("a fixture frame")
    else {
        panic!("{path}: expected a response");
    };
    let Reply::InstanceGetMetrics(reply) = response.reply else {
        panic!("{path}: expected get_metrics");
    };
    *reply
}

fn serving(reply: MetricsReply) -> (TestRoot, ScriptedDaemon) {
    let root = TestRoot::new().expect("a test root is made");
    let listed = data::list_mixed().items;
    let daemon = ScriptedDaemon::serve(&root, move |call, _| match call {
        Call::InstanceGetMetrics(_) => vec![Step::Reply(Reply::InstanceGetMetrics(Box::new(
            reply.clone(),
        )))],
        Call::InstanceList(_) => vec![Step::Reply(Reply::InstanceList(Box::new(
            stackly_protocol::instance::InstancePage {
                items: listed.clone(),
                next: None,
            },
        )))],
        _ => Vec::new(),
    });
    (root, daemon)
}

/// Spec 0028 AC-12.
#[test]
fn metrics_writes_both_scopes_with_labels_and_the_reply_as_json() {
    let ev = evidence!(
        covers = ["REQ-018"],
        kind = Kind::Fixture,
        expected = "instance metrics asks instance.get_metrics once and writes the newest sample for the main process and the process tree with their scope labels, unavailable (reason) for what was not measured, and the restarts, last failure, health with latency, data size, and process list; --json writes the reply as it is",
    );
    let (root, daemon) = serving(fixture("live"));

    let text = run(root.path(), &["instance", "metrics", ID]);
    let json = run(root.path(), &["instance", "metrics", ID, "--json"]);

    assert_eq!(text.code, Some(0), "{text:?}");
    assert_eq!(text.stderr, "");
    golden::check("instance_metrics.txt", &text.stdout);
    assert!(text.stdout.contains("main process (pid 4242)"), "{text:?}");
    assert!(
        text.stdout
            .contains("process tree (6 processes, summed per process), 5 measured")
            || text
                .stdout
                .contains("process tree (6 processes, summed per process)"),
        "{text:?}"
    );
    assert!(text.stdout.contains("2 since created"), "{text:?}");
    assert_eq!(json.code, Some(0), "{json:?}");
    let value = envelope(&json);
    assert_eq!(value["command"], "instance metrics");
    let expected = serde_json::to_value(fixture("live")).expect("encodes");
    assert_eq!(value["result"], expected);
    assert_eq!(
        daemon.methods(),
        [
            "daemon.handshake",
            "instance.get_metrics",
            "daemon.handshake",
            "instance.get_metrics"
        ]
    );
    assert_eq!(daemon.log()[1].raw["call"]["content"]["instanceId"], ID);
    ev.pass();
}

/// Spec 0028 AC-12, AC-3.
#[test]
fn a_number_that_was_not_measured_is_written_unavailable_with_its_reason() {
    let (root, _daemon) = serving(fixture("live"));

    let text = run(root.path(), &["instance", "metrics", ID]);

    assert!(
        text.stdout.contains("unavailable (not_permitted)")
            || text.stdout.contains("unavailable (warming_up)"),
        "{text:?}"
    );
}

/// Spec 0028 AC-12.
#[test]
fn an_instance_that_is_not_running_says_so_and_exits_0() {
    let (root, _daemon) = serving(fixture("not-running"));

    let text = run(root.path(), &["instance", "metrics", ID]);
    let json = run(root.path(), &["instance", "metrics", ID, "--json"]);

    assert_eq!(text.code, Some(0), "{text:?}");
    assert_eq!(text.stdout, "not running (stopped)\n");
    assert_eq!(json.code, Some(0), "{json:?}");
    assert_eq!(envelope(&json)["result"]["state"], "not_running");
}

/// Spec 0028 AC-12.
#[test]
fn an_unknown_name_exits_10_with_instance_not_found() {
    let (root, daemon) = serving(fixture("live"));

    let text = run(root.path(), &["instance", "metrics", "Cache"]);
    let json = run(root.path(), &["instance", "metrics", "Cache", "--json"]);

    assert_eq!(text.code, Some(10), "{text:?}");
    assert!(text.stderr.contains("cli.instance_not_found"), "{text:?}");
    let value = envelope(&json);
    assert_eq!(value["error"]["code"], "cli.instance_not_found");
    assert_eq!(detail(&value["error"], "name").as_deref(), Some("Cache"));
    assert!(
        !daemon
            .methods()
            .contains(&"instance.get_metrics".to_owned())
    );
}

fn watching(root: &TestRoot) -> ScriptedDaemon {
    let live = {
        let mut reply = fixture("live");
        reply.interval_seconds = 1;
        reply
    };
    ScriptedDaemon::serve(root, move |call, ctx| match call {
        Call::InstanceGetMetrics(_) => {
            let mut reply = live.clone();
            match ctx.nth {
                0 => {}
                1 => {
                    // One new sample after the cursor.
                    let mut next = reply.samples[2];
                    next.seq = stackly_protocol::Sequence::new(4).expect("a sequence");
                    reply.samples = vec![next];
                    reply.history_reset = false;
                }
                _ => {
                    // The process changed: the client starts over.
                    reply.history_id = Some("9999-1760950900.000000".to_owned());
                    reply.samples.truncate(1);
                    reply.history_reset = true;
                }
            }
            vec![Step::Reply(Reply::InstanceGetMetrics(Box::new(reply)))]
        }
        _ => Vec::new(),
    })
}

/// Spec 0028 AC-12.
#[test]
fn watch_writes_one_line_per_new_sample_with_the_cursor_and_a_line_for_a_reset() {
    let ev = evidence!(
        covers = ["REQ-018"],
        kind = Kind::Fixture,
        expected = "instance metrics --watch writes the newest sample at once, asks again with the historyId and the afterSeq it holds, writes one line per new sample and history reset when the daemon says so, and Ctrl+C ends it with exit 0",
    );
    let root = TestRoot::new().expect("a test root is made");
    let daemon = watching(&root);
    let mut command = stackly(root.path());
    command
        .args(["instance", "metrics", ID, "--watch"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().expect("stackly starts");
    let mut stdout = BufReader::new(child.stdout.take().expect("stdout is piped"));
    let started = Instant::now();
    let mut lines = Vec::new();
    while lines.iter().all(|line: &String| line != "history reset") {
        let mut line = String::new();
        let read = stdout.read_line(&mut line).expect("stdout can be read");
        assert!(read > 0, "stackly ended early: {lines:?}");
        lines.push(line.trim_end().to_owned());
        assert!(started.elapsed() < Duration::from_secs(20), "{lines:?}");
    }
    let mut after_reset = String::new();
    stdout.read_line(&mut after_reset).expect("a sample line");
    let status = std::process::Command::new("/bin/kill")
        .args(["-INT", &child.id().to_string()])
        .status()
        .expect("kill runs");
    assert!(status.success());
    let output = child.wait_with_output().expect("stackly ends");

    assert_eq!(output.status.code(), Some(0), "{lines:?}");
    assert_eq!(lines.len(), 3, "{lines:?}");
    assert!(
        lines[0].contains("main cpu") && lines[0].contains("tree cpu"),
        "{lines:?}"
    );
    assert!(after_reset.contains("main cpu"), "{after_reset:?}");
    let requests: Vec<_> = daemon
        .log()
        .into_iter()
        .filter(|logged| logged.method == "instance.get_metrics")
        .collect();
    assert!(requests.len() >= 3, "{requests:?}");
    assert!(requests[0].raw["call"]["content"]["historyId"].is_null());
    assert_eq!(
        requests[1].raw["call"]["content"]["historyId"],
        "4242-1760950802.000000"
    );
    assert_eq!(requests[1].raw["call"]["content"]["afterSeq"], 3);
    ev.pass();
}
