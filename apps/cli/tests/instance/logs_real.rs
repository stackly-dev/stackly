//! `stackly instance logs` against a real `stacklyd` (spec 0027, AC-9 and
//! AC-10): a fake instance that writes many lines, read with `--tail`
//! across pages, filtered by stream and text, as text and as `--json`
//! items, followed until SIGINT, and the usage and not found refusals.

use std::io::{BufRead, BufReader};
use std::process::Stdio;
use std::time::{Duration, Instant};

use serde_json::Value;
use stackly_testkit::evidence::{Kind, evidence};
use stackly_testkit::{ReapGuard, TestRoot};

use crate::common::stacklyd::start_daemon;
use crate::common::{envelope, envelope_of, paths, run, stackly};
use crate::stacklyd::seed;

/// The JSON lines of a run: every `item` record, then the envelope.
fn records(stdout: &str) -> (Vec<Value>, Value) {
    let mut lines: Vec<Value> = stdout
        .lines()
        .map(|line| serde_json::from_str(line).expect("each line is json"))
        .collect();
    let envelope = lines.pop().expect("a result record");
    assert_eq!(envelope["type"], "result", "{envelope}");
    assert!(lines.iter().all(|line| line["type"] == "item"));
    (lines, envelope)
}

#[test]
fn logs_read_pages_filter_and_follow_until_sigint_against_a_real_daemon() {
    let ev = evidence!(
        covers = ["REQ-017"],
        kind = Kind::Fixture,
        expected = "spec 0027 AC-10: against a real stacklyd with a fake writing 1000 lines a second, instance logs --tail 1500 --json reads two pages into 1500 items with rising seq and a result with the last cursor, --stream stderr and --grep keep only matching text lines, --follow --json prints new items and exits 0 with a result on SIGINT, --follow --before exits 2, and an unknown name exits 10",
    );
    let root = TestRoot::new().expect("a test root");
    let runtime = seed(&root);
    let _reap = ReapGuard::new(root.path(), &paths(&root).runtime);
    let daemon = start_daemon(&root);
    let create = |name: &str, rate: &str| {
        run(
            root.path(),
            &[
                "instance",
                "create",
                name,
                "--runtime",
                &runtime,
                "--provider",
                "testing/fake",
                "--revision",
                "0.4.0",
                "--set",
                &format!("logLinesPerSecond={rate}"),
                "--start",
                "--wait",
                "--timeout",
                "60s",
                "--json",
            ],
        )
    };
    let created = create("chatty", "1000");
    assert_eq!(created.code, Some(0), "{created:?}");
    let id = envelope(&created)["result"]["instance"]["id"]
        .as_str()
        .expect("an id")
        .to_owned();
    // Enough lines for two pages of 1000: wait for what is stored, not for a
    // time, so a busy machine only makes the wait longer.
    let waiting = Instant::now();
    loop {
        let head = run(
            root.path(),
            &["instance", "logs", "chatty", "--tail", "1", "--json"],
        );
        let stored = if head.code == Some(0) {
            records(&head.stdout).1["result"]["last"]["seq"]
                .as_u64()
                .unwrap_or(0)
        } else {
            0
        };
        if stored >= 1600 {
            break;
        }
        assert!(
            waiting.elapsed() < Duration::from_secs(120),
            "the fake stored {stored} records in 120 seconds"
        );
        std::thread::sleep(Duration::from_millis(500));
    }

    let tail = run(
        root.path(),
        &["instance", "logs", "chatty", "--tail", "1500", "--json"],
    );
    assert_eq!(tail.code, Some(0), "{tail:?}");
    let (items, result) = records(&tail.stdout);
    assert_eq!(items.len(), 1500);
    let seqs: Vec<u64> = items
        .iter()
        .map(|item| item["item"]["content"]["seq"].as_u64().expect("a seq"))
        .collect();
    assert!(seqs.windows(2).all(|pair| pair[0] < pair[1]));
    assert_eq!(result["result"]["instance"]["name"], "chatty");
    assert_eq!(result["result"]["instance"]["id"], id.as_str());
    assert_eq!(
        result["result"]["last"]["seq"].as_u64(),
        seqs.last().copied()
    );
    assert_eq!(result["result"]["retention"]["segmentsPerStream"], 5);

    let stderr = run(
        root.path(),
        &[
            "instance", "logs", "chatty", "--tail", "5", "--stream", "stderr",
        ],
    );
    assert_eq!(stderr.code, Some(0), "{stderr:?}");
    let lines: Vec<&str> = stderr.stdout.lines().collect();
    assert_eq!(lines.len(), 5, "{stderr:?}");
    assert!(
        lines
            .iter()
            .all(|line| line.contains("  stderr   log line ")),
        "{lines:?}"
    );
    let grep = run(
        root.path(),
        &[
            "instance",
            "logs",
            "chatty",
            "--tail",
            "3",
            "--grep",
            "ON STDOUT",
        ],
    );
    assert_eq!(grep.code, Some(0), "{grep:?}");
    assert!(
        grep.stdout.lines().all(|line| line.contains("on stdout")),
        "{grep:?}"
    );

    let mut follow = stackly(root.path());
    follow
        .args([
            "instance", "logs", "chatty", "--tail", "1", "--follow", "--json",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = follow.spawn().expect("stackly starts");
    let mut stdout = BufReader::new(child.stdout.take().expect("stdout is piped"));
    let mut seen = Vec::new();
    let started = Instant::now();
    while seen.len() < 50 {
        let mut line = String::new();
        stdout.read_line(&mut line).expect("stdout reads");
        assert!(!line.is_empty(), "the follow ended early");
        seen.push(line);
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "the follow is too slow"
        );
    }
    let status = std::process::Command::new("/bin/kill")
        .args(["-INT", &child.id().to_string()])
        .status()
        .expect("kill runs");
    assert!(status.success());
    let mut rest = String::new();
    std::io::Read::read_to_string(&mut stdout, &mut rest).expect("stdout reads");
    let output = child.wait_with_output().expect("stackly ends");
    assert_eq!(
        output.status.code(),
        Some(0),
        "{:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    let last = rest.lines().last().expect("a result line");
    let ended = envelope_of(&format!("{last}\n"));
    assert_eq!(ended["type"], "result");
    assert!(ended["result"]["last"]["seq"].as_u64().is_some());

    let conflict = run(
        root.path(),
        &[
            "instance",
            "logs",
            "chatty",
            "--follow",
            "--before",
            "0192f3a5-0000-7000-8000-0000000000aa:5",
            "--json",
        ],
    );
    assert_eq!(conflict.code, Some(2), "{conflict:?}");
    let missing = run(root.path(), &["instance", "logs", "gamma", "--json"]);
    assert_eq!(missing.code, Some(10), "{missing:?}");
    assert_eq!(
        envelope(&missing)["error"]["code"],
        "cli.instance_not_found"
    );

    let stop = run(
        root.path(),
        &["instance", "stop", "chatty", "--wait", "--timeout", "60s"],
    );
    assert_eq!(stop.code, Some(0), "{stop:?}");
    assert_eq!(daemon.stop().unwrap().code(), Some(0));
    ev.pass();
}
