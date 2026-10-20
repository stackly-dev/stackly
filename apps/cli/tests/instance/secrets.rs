//! AC-17: `instance create --secret <field>=<source>` reads each value with
//! the rules of `secret_input` (spec 0011), sends it only in the request
//! params, leaves out every field it is not given, and never prints it.

use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use serde_json::{Value, json};
use stackly_protocol::Reply;
use stackly_protocol::operation::{OperationKind, OperationState};
use stackly_testkit::TestRoot;
use stackly_testkit::evidence::{FixtureCredential, Kind, evidence};

use crate::common::scripted::ScriptedDaemon;
use crate::common::{Run, detail, envelope, run, stackly};
use crate::data::{self, RUNTIME};

const KEY: &str = "create-secrets-0001";

fn serve(root: &TestRoot) -> ScriptedDaemon {
    ScriptedDaemon::serve(
        root,
        data::scripted(
            Reply::InstanceCreate(Box::new(data::accepted(
                data::operation(OperationKind::CreateInstance, OperationState::Queued, 1),
                false,
            ))),
            Vec::new(),
            None,
        ),
    )
}

fn base() -> Vec<&'static str> {
    vec![
        "instance",
        "create",
        "demo",
        "--runtime",
        RUNTIME,
        "--provider",
        "fake",
        "--revision",
        "0.2.0",
        "--idempotency-key",
        KEY,
    ]
}

/// `stackly <args>` with `input` on stdin and `env` set, to the end.
fn run_piped(root: &Path, args: &[&str], input: &[u8], env: &[(&str, &str)]) -> Run {
    let mut command = stackly(root);
    command.args(args).stdin(Stdio::piped());
    for (name, value) in env {
        command.env(name, value);
    }
    let started = std::time::Instant::now();
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("stackly runs");
    let mut stdin = child.stdin.take().expect("stdin is piped");
    let bytes = input.to_vec();
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(&bytes);
    });
    let output = child.wait_with_output().expect("stackly ends");
    writer.join().expect("the writer ends");
    Run {
        code: output.status.code(),
        stdout: String::from_utf8(output.stdout).expect("stdout is utf-8"),
        stderr: String::from_utf8(output.stderr).expect("stderr is utf-8"),
        elapsed: started.elapsed(),
    }
}

fn secrets_sent(daemon: &ScriptedDaemon) -> Vec<Value> {
    daemon
        .log()
        .into_iter()
        .filter(|logged| logged.method == "instance.create")
        .map(|logged| logged.raw["call"]["content"]["secrets"].clone())
        .collect()
}

fn calls(daemon: &ScriptedDaemon) -> usize {
    daemon
        .log()
        .iter()
        .filter(|logged| logged.method == "instance.create")
        .count()
}

/// Spec 0014 AC-17.
#[test]
fn every_source_maps_to_a_secret_input_and_the_last_flag_for_a_field_wins() {
    let ev = evidence!(
        covers = ["REQ-016", "AC-016"],
        kind = Kind::Fixture,
        expected = "instance create --secret sends generate, stdin, file, env, copy, and bind sources as secrets[] in the order the fields first appear, the last flag for a field wins, a field left out is not sent, and neither stdout nor stderr holds a value",
    );
    let root = TestRoot::new().expect("a test root is made");
    let daemon = serve(&root);
    let credential = FixtureCredential::new();
    let from_file = FixtureCredential::new();
    let from_env = FixtureCredential::new();
    let dir = tempfile::tempdir().expect("a temp dir is made");
    let file = dir.path().join("value");
    std::fs::write(&file, format!("{}\n", from_file.value())).expect("the value file is written");
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600))
        .expect("the file is private");
    let file_flag = format!("fromFile=file:{}", file.display());
    let mut args = base();
    args.extend([
        "--secret",
        "authToken=stdin",
        "--secret",
        "generated=prompt",
        "--secret",
        "generated=generate",
        "--secret",
        &file_flag,
        "--secret",
        "fromEnv=env:STACKLY_TEST_SECRET",
        "--secret",
        "copied=copy:local:user/source",
        "--secret",
        "bound=bind:local:user/existing",
        "--json",
    ]);
    let mut piped = credential.value().as_bytes().to_vec();
    piped.push(b'\n');

    let json = run_piped(
        root.path(),
        &args,
        &piped,
        &[("STACKLY_TEST_SECRET", from_env.value())],
    );

    assert_eq!(json.code, Some(0), "{json:?}");
    assert_eq!(calls(&daemon), 1);
    assert_eq!(
        secrets_sent(&daemon)[0],
        json!([
            { "field": "authToken", "input": { "type": "value", "content": credential.value() } },
            { "field": "generated", "input": { "type": "generate" } },
            { "field": "fromFile", "input": { "type": "value", "content": from_file.value() } },
            { "field": "fromEnv", "input": { "type": "value", "content": from_env.value() } },
            { "field": "copied", "input": { "type": "copyFrom", "content": "local:user/source" } },
            { "field": "bound", "input": { "type": "bind", "content": "local:user/existing" } },
        ])
    );
    for value in [&credential, &from_file, &from_env] {
        value.assert_absent(&json.stdout, "stdout");
        value.assert_absent(&json.stderr, "stderr");
    }
    assert_eq!(envelope(&json)["result"]["idempotencyKey"], KEY);
    ev.pass();
}

/// Spec 0014 AC-17.
#[test]
fn a_create_without_secret_flags_sends_no_secret_field() {
    let root = TestRoot::new().expect("a test root is made");
    let daemon = serve(&root);

    let text = run(root.path(), &base());

    assert_eq!(text.code, Some(0), "{text:?}");
    assert_eq!(secrets_sent(&daemon), [json!([])]);
}

/// Spec 0014 AC-17.
#[test]
fn the_text_output_holds_no_value_either() {
    let root = TestRoot::new().expect("a test root is made");
    let _daemon = serve(&root);
    let credential = FixtureCredential::new();
    let mut args = base();
    args.extend(["--secret", "authToken=stdin"]);

    let text = run_piped(
        root.path(),
        &args,
        format!("{}\n", credential.value()).as_bytes(),
        &[],
    );

    assert_eq!(text.code, Some(0), "{text:?}");
    credential.assert_absent(&text.stdout, "stdout");
    credential.assert_absent(&text.stderr, "stderr");
}

fn refused(root: &TestRoot, extra: &[&str], input: &[u8], kind: &str) {
    let daemon = serve(root);
    let mut args = base();
    args.extend(extra);
    args.push("--json");

    let refused = run_piped(root.path(), &args, input, &[("STACKLY_NO_INPUT", "1")]);

    assert_eq!(refused.code, Some(2), "{extra:?}: {refused:?}");
    let value = envelope(&refused);
    assert_eq!(value["error"]["code"], "cli.usage", "{extra:?}");
    if !kind.is_empty() {
        assert_eq!(
            detail(&value["error"], "kind").as_deref(),
            Some(kind),
            "{extra:?}"
        );
    }
    assert_eq!(calls(&daemon), 0, "{extra:?}: nothing reached the daemon");
}

/// Spec 0014 AC-17.
#[test]
fn a_secret_flag_the_cli_refuses_stops_before_the_daemon() {
    let ev = evidence!(
        covers = ["REQ-016", "AC-016"],
        kind = Kind::Fixture,
        expected = "instance create refuses a malformed --secret, stdin for two fields, a missing variable, and an empty stdin with cli.usage and exit 2 before any instance.create",
    );
    let root = TestRoot::new().expect("a test root is made");
    refused(&root, &["--secret", "authToken"], b"", "");
    refused(&root, &["--secret", "=generate"], b"", "");
    refused(&root, &["--secret", "authToken=literal-value"], b"", "");
    refused(&root, &["--secret", "authToken=file:"], b"", "");
    refused(
        &root,
        &["--secret", "a=stdin", "--secret", "b=stdin"],
        b"one\n",
        "secret_stdin_twice",
    );
    refused(
        &root,
        &["--secret", "a=env:STACKLY_TEST_UNSET_SECRET"],
        b"",
        "secret_env_missing",
    );
    refused(
        &root,
        &["--secret", "a=stdin"],
        b"\n",
        "secret_value_invalid",
    );
    refused(
        &root,
        &["--secret", "a=file:/nonexistent/stackly/secret"],
        b"",
        "secret_file_unreadable",
    );
    ev.pass();
}

/// Spec 0014 AC-17.
#[test]
fn the_prompt_source_without_a_terminal_fails_at_once() {
    let root = TestRoot::new().expect("a test root is made");
    let daemon = serve(&root);
    let mut args = base();
    args.extend(["--secret", "authToken=prompt", "--json"]);

    let started = std::time::Instant::now();
    let refused = run_piped(root.path(), &args, b"", &[("STACKLY_NO_INPUT", "1")]);

    assert!(started.elapsed() < Duration::from_secs(5));
    assert_eq!(refused.code, Some(2), "{refused:?}");
    assert_eq!(envelope(&refused)["error"]["code"], "cli.input_required");
    assert_eq!(calls(&daemon), 0);
}

/// Spec 0014 AC-17.
#[test]
fn help_lists_the_secret_flag_with_its_sources() {
    let root = TestRoot::new().expect("a test root is made");

    let help = run(root.path(), &["instance", "create", "--help"]);

    assert_eq!(help.code, Some(0), "{help:?}");
    assert!(
        help.stdout.contains("--secret <FIELD=SOURCE>"),
        "{}",
        help.stdout
    );
    assert!(help.stdout.contains("file:PATH"), "{}", help.stdout);
}
