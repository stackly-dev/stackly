//! `stackly secret` against a scripted daemon (spec 0011 AC-17, AC-16 for
//! the CLI): the sources of a value and how they are read, the rules that
//! stop a value before the daemon sees it, the confirmation of a replace
//! that changes only the stored copy, and text and `--json` output that
//! never hold a value but `reveal`'s. Every value is a fixture credential.
#![cfg(target_os = "macos")]

mod common;

use std::io::Write;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::process::Stdio;
use std::time::Duration;

use serde_json::{Value, json};
use stackly_protocol::codes::secret as secret_codes;
use stackly_protocol::secret::{
    DeleteSecretReply, ListSecretsReply, RevealSecretReply, SecretEffectInfo, SecretEffectKind,
    SecretOwnerKind, SecretRefSummary, SecretStatus, SecretStatusReply, SecretStoreBackend,
    SecretStoreState, SecretStoreStatus, SecretStoreTarget, SecretText, SecretUseInfo,
    SecretValueOrigin, SecretWarning, SetSecretReply,
};
use stackly_protocol::{Call, ErrorBody, ErrorItem, Reply, ResourceKind, Revision, Timestamp};
use stackly_testkit::TestRoot;
use stackly_testkit::evidence::{FixtureCredential, Kind, evidence};

use common::pty::{PtyOptions, run_in_pty};
use common::scripted::{ScriptedDaemon, Step};
use common::{Run, detail, envelope, listing, paths, run, run_command, stackly};

const REFERENCE: &str = "local:user/api-token";
const OTHER: &str = "local:user/other";
const INSTANCE: &str = "0192f3a0-7c1e-7b2a-9d4e-5f6a7b8c9d0e";

fn at() -> Timestamp {
    "2026-10-18T03:04:05Z".parse().expect("a fixed time parses")
}

fn summary(reference: &str) -> SecretRefSummary {
    SecretRefSummary {
        reference: reference.to_owned(),
        owner_kind: SecretOwnerKind::User,
        owner_id: None,
        purpose: "user".to_owned(),
        status: SecretStatus::Present,
        status_observed_at: Some(at()),
        value_origin: Some(SecretValueOrigin::Entered),
        value_updated_at: Some(at()),
        used_by: Vec::new(),
        revision: Revision::new(1).expect("1 is in range"),
    }
}

fn store() -> SecretStoreStatus {
    SecretStoreStatus {
        backend: SecretStoreBackend::MacosKeychain,
        state: SecretStoreState::Available,
        target: Some(SecretStoreTarget::File),
    }
}

fn set_reply(reference: &str) -> SetSecretReply {
    SetSecretReply {
        summary: summary(reference),
        warnings: Vec::new(),
        effects: Vec::new(),
    }
}

/// A daemon that answers every secret method with a fixed reply and keeps
/// the requests it read.
fn serving(root: &TestRoot, credential: &FixtureCredential) -> ScriptedDaemon {
    let value = credential.value().to_owned();
    ScriptedDaemon::serve(root, move |call, _| match call {
        Call::SecretList(_) => vec![Step::Reply(Reply::SecretList(ListSecretsReply {
            items: vec![summary(REFERENCE), {
                let mut used = summary("local:instance/a/password");
                used.owner_kind = SecretOwnerKind::Instance;
                used.status = SecretStatus::Missing;
                used.value_origin = Some(SecretValueOrigin::Generated);
                used.used_by = vec![SecretUseInfo {
                    resource_kind: ResourceKind::Instance,
                    resource_id: INSTANCE.to_owned(),
                    field: "password".to_owned(),
                }];
                used
            }],
            store: store(),
        }))],
        Call::SecretStatus(params) => vec![Step::Reply(Reply::SecretStatus(Box::new(
            SecretStatusReply {
                summary: summary(&params.reference),
                store: store(),
            },
        )))],
        Call::SecretSet(params) => vec![Step::Reply(Reply::SecretSet(Box::new(set_reply(
            &params.reference,
        ))))],
        Call::SecretReveal(params) => vec![Step::Reply(Reply::SecretReveal(RevealSecretReply {
            reference: params.reference.clone(),
            value: SecretText::new(value.clone()),
        }))],
        Call::SecretDelete(params) => vec![Step::Reply(Reply::SecretDelete(DeleteSecretReply {
            reference: params.reference.clone(),
        }))],
        _ => Vec::new(),
    })
}

/// `stackly <args>` with `input` on stdin, to the end.
fn run_with_stdin(root: &TestRoot, args: &[&str], input: &[u8]) -> Run {
    let mut command = stackly(root.path());
    command.args(args).stdin(Stdio::piped());
    let started = std::time::Instant::now();
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("stackly runs");
    let mut stdin = child.stdin.take().expect("stdin is piped");
    let bytes = input.to_vec();
    let writer = std::thread::spawn(move || {
        // The CLI may stop reading at its limit, so a broken pipe is fine.
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

/// The `secret.set` requests the daemon read.
fn sets(daemon: &ScriptedDaemon) -> Vec<Value> {
    daemon
        .log()
        .into_iter()
        .filter(|logged| logged.method == "secret.set")
        .map(|logged| logged.raw["call"]["content"].clone())
        .collect()
}

fn error_code(run: &Run) -> String {
    envelope(run)["error"]["code"]
        .as_str()
        .expect("the run failed with a code")
        .to_owned()
}

fn kind(run: &Run) -> Option<String> {
    detail(&envelope(run)["error"], "kind")
}

#[test]
fn without_a_terminal_and_without_a_source_set_fails_at_once_and_waits_for_nothing() {
    let ev = evidence!(
        covers = ["BR-018", "REQ-028", "AC-028"],
        kind = Kind::Fixture,
        expected = "stackly secret set with no source flag and no terminal exits 2 with cli.input_required at once, never reads stdin, and never reaches a daemon",
    );
    let root = TestRoot::new().unwrap();
    let before = listing(root.path());
    for args in [
        &["secret", "set", REFERENCE, "--json"][..],
        &["secret", "set", REFERENCE, "--json", "--no-input"],
    ] {
        let mut command = stackly(root.path());
        command.args(args).stdin(Stdio::piped());
        let started = std::time::Instant::now();
        let child = command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("stackly runs");
        // stdin stays open and silent: a CLI that waited for it would hang.
        let output = child.wait_with_output().expect("stackly ends");
        assert_eq!(output.status.code(), Some(2), "{output:?}");
        assert!(started.elapsed() < Duration::from_secs(5));
        let value: Value = serde_json::from_slice(&output.stdout).expect("one envelope");
        assert_eq!(value["error"]["code"], "cli.input_required");
    }
    assert_eq!(listing(root.path()), before, "no socket was touched");
    ev.pass();
}

#[test]
fn two_sources_are_refused_and_no_flag_takes_a_value() {
    let ev = evidence!(
        covers = ["BR-018", "REQ-028"],
        kind = Kind::Fixture,
        expected = "stackly secret set refuses two source flags, a flag that would carry a value, and a malformed reference with cli.usage and exit 2, before any daemon request",
    );
    let root = TestRoot::new().unwrap();
    let credential = FixtureCredential::new();
    let daemon = serving(&root, &credential);
    for (args, expected) in [
        (
            vec!["secret", "set", REFERENCE, "--stdin", "--generate"],
            "argument_conflict",
        ),
        (
            vec![
                "secret",
                "set",
                REFERENCE,
                "--from-env",
                "A",
                "--from-file",
                "b",
            ],
            "argument_conflict",
        ),
        (
            vec![
                "secret",
                "set",
                REFERENCE,
                "--generate",
                "--copy-from",
                OTHER,
            ],
            "argument_conflict",
        ),
        (
            vec![
                "secret",
                "set",
                REFERENCE,
                "--replace",
                "--upsert",
                "--generate",
            ],
            "argument_conflict",
        ),
        (
            vec!["secret", "set", REFERENCE, "--password", credential.value()],
            "unknown_argument",
        ),
        (
            vec!["secret", "set", REFERENCE, "--value", credential.value()],
            "unknown_argument",
        ),
        (
            vec!["secret", "set", "not-a-reference", "--generate"],
            "value_validation",
        ),
        (
            vec![
                "secret",
                "set",
                REFERENCE,
                "--generate",
                "--expected-revision",
                "x",
            ],
            "value_validation",
        ),
    ] {
        let mut full = args.clone();
        full.push("--json");
        let refused = run(root.path(), &full);
        assert_eq!(refused.code, Some(2), "{args:?}: {refused:?}");
        assert_eq!(error_code(&refused), "cli.usage", "{args:?}");
        assert_eq!(kind(&refused).as_deref(), Some(expected), "{args:?}");
        credential.assert_absent(&refused.stdout, "stdout");
        credential.assert_absent(&refused.stderr, "stderr");
    }
    assert!(sets(&daemon).is_empty());
    ev.pass();
}

#[test]
fn stdin_sets_the_value_without_its_newline_and_no_output_holds_it() {
    let ev = evidence!(
        covers = ["AC-016", "REQ-016", "BR-018", "REQ-028"],
        kind = Kind::Fixture,
        expected = "stackly secret set --stdin sends the value once in the request, without one trailing CRLF, and neither the text nor the --json output holds it",
    );
    let root = TestRoot::new().unwrap();
    let credential = FixtureCredential::new();
    let daemon = serving(&root, &credential);
    let mut piped = credential.value().as_bytes().to_vec();
    piped.extend_from_slice(b"\r\n");

    let json = run_with_stdin(
        &root,
        &["secret", "set", REFERENCE, "--stdin", "--json"],
        &piped,
    );
    let text = run_with_stdin(&root, &["secret", "set", REFERENCE, "--stdin"], &piped);

    assert_eq!(json.code, Some(0), "{json:?}");
    assert_eq!(text.code, Some(0), "{text:?}");
    let requests = sets(&daemon);
    assert_eq!(requests.len(), 2);
    for request in &requests {
        assert_eq!(
            request["input"],
            json!({"type": "value", "content": credential.value()})
        );
        assert_eq!(request["reference"], REFERENCE);
        assert_eq!(request["mode"], "create");
        assert_eq!(request["confirmStoredOnly"], false);
        assert!(request["expectedRevision"].is_null());
    }
    let value = envelope(&json);
    assert_eq!(value["command"], "secret set");
    assert_eq!(value["result"]["summary"]["reference"], REFERENCE);
    assert_eq!(value["result"]["summary"]["status"], "present");
    assert!(
        text.stdout.contains(&format!("Reference  {REFERENCE}")),
        "{text:?}"
    );
    for output in [&json, &text] {
        credential.assert_absent(&output.stdout, "stdout");
        credential.assert_absent(&output.stderr, "stderr");
    }
    ev.pass();
}

#[test]
fn a_value_the_cli_refuses_never_reaches_the_daemon() {
    let ev = evidence!(
        covers = ["BR-018", "REQ-028"],
        kind = Kind::Fixture,
        expected = "an empty, NUL, oversize, or non UTF-8 value, an unset or empty variable, and a symlink or missing file are refused with exit 2 and a plain kind, and no request is sent",
    );
    let root = TestRoot::new().unwrap();
    let credential = FixtureCredential::new();
    let daemon = serving(&root, &credential);
    let dir = tempfile::tempdir().unwrap();
    let real = dir.path().join("real");
    std::fs::write(&real, credential.value()).unwrap();
    let link = dir.path().join("link");
    symlink(&real, &link).unwrap();
    let missing = dir.path().join("missing");

    let mut over = vec![b'a'; 65_537];
    over.push(b'\n');
    let mut cases: Vec<(Vec<&str>, Vec<u8>, &str)> = vec![
        (vec!["--stdin"], Vec::new(), "secret_value_invalid"),
        (vec!["--stdin"], b"\n".to_vec(), "secret_value_invalid"),
        (vec!["--stdin"], b"ab\0cd".to_vec(), "secret_value_invalid"),
        (
            vec!["--stdin"],
            b"\xff\xfe\n".to_vec(),
            "secret_value_not_utf8",
        ),
        (vec!["--stdin"], over.clone(), "secret_value_too_long"),
        (
            vec!["--from-env", "STACKLY_TEST_UNSET"],
            Vec::new(),
            "secret_env_missing",
        ),
        (
            vec!["--from-env", "STACKLY_TEST_EMPTY"],
            Vec::new(),
            "secret_env_missing",
        ),
    ];
    let link_arg = link.to_str().unwrap().to_owned();
    let missing_arg = missing.to_str().unwrap().to_owned();
    let dir_arg = dir.path().to_str().unwrap().to_owned();
    cases.push((
        vec!["--from-file", &link_arg],
        Vec::new(),
        "secret_file_symlink",
    ));
    cases.push((
        vec!["--from-file", &missing_arg],
        Vec::new(),
        "secret_file_unreadable",
    ));
    cases.push((
        vec!["--from-file", &dir_arg],
        Vec::new(),
        "secret_file_not_regular",
    ));
    for (flags, input, expected) in cases {
        let mut command = stackly(root.path());
        command
            .args(["secret", "set", REFERENCE, "--json"])
            .args(&flags)
            .env("STACKLY_TEST_EMPTY", "")
            .env_remove("STACKLY_TEST_UNSET")
            .stdin(Stdio::piped());
        let mut child = command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut stdin = child.stdin.take().unwrap();
        let bytes = input.clone();
        let writer = std::thread::spawn(move || {
            let _ = stdin.write_all(&bytes);
        });
        let output = child.wait_with_output().unwrap();
        writer.join().unwrap();
        let refused = Run {
            code: output.status.code(),
            stdout: String::from_utf8(output.stdout).unwrap(),
            stderr: String::from_utf8(output.stderr).unwrap(),
            elapsed: Duration::ZERO,
        };
        assert_eq!(refused.code, Some(2), "{flags:?}: {refused:?}");
        assert_eq!(error_code(&refused), "cli.usage", "{flags:?}");
        assert_eq!(kind(&refused).as_deref(), Some(expected), "{flags:?}");
        credential.assert_absent(&refused.stdout, "stdout");
        credential.assert_absent(&refused.stderr, "stderr");
    }
    assert!(sets(&daemon).is_empty(), "{:?}", daemon.methods());
    ev.pass();
}

#[test]
fn a_file_and_a_variable_give_their_value_and_a_loose_file_warns() {
    let ev = evidence!(
        covers = ["BR-018", "REQ-028"],
        kind = Kind::Fixture,
        expected = "--from-file drops one trailing newline and warns about a file that group or others can read, --from-env sends the variable as it is, and 7 byte values pass",
    );
    let root = TestRoot::new().unwrap();
    let credential = FixtureCredential::new();
    let daemon = serving(&root, &credential);
    let dir = tempfile::tempdir().unwrap();
    let tight = dir.path().join("tight");
    std::fs::write(&tight, format!("{}\n", credential.value())).unwrap();
    std::fs::set_permissions(&tight, std::fs::Permissions::from_mode(0o600)).unwrap();
    let loose = dir.path().join("loose");
    std::fs::write(&loose, "short77\n").unwrap();
    std::fs::set_permissions(&loose, std::fs::Permissions::from_mode(0o644)).unwrap();

    let from_tight = run(
        root.path(),
        &[
            "secret",
            "set",
            REFERENCE,
            "--from-file",
            tight.to_str().unwrap(),
            "--json",
        ],
    );
    let from_loose = run(
        root.path(),
        &[
            "secret",
            "set",
            OTHER,
            "--from-file",
            loose.to_str().unwrap(),
            "--json",
        ],
    );
    let mut command = stackly(root.path());
    command
        .args([
            "secret",
            "set",
            "local:user/third",
            "--from-env",
            "STACKLY_TEST_VALUE",
            "--json",
        ])
        .env("STACKLY_TEST_VALUE", format!("  {}\n", credential.value()));
    let from_env = run_command(command);

    for run in [&from_tight, &from_loose, &from_env] {
        assert_eq!(run.code, Some(0), "{run:?}");
        credential.assert_absent(&run.stdout, "stdout");
        credential.assert_absent(&run.stderr, "stderr");
    }
    assert_eq!(envelope(&from_tight)["warnings"], json!([]));
    let warnings = &envelope(&from_loose)["warnings"];
    assert_eq!(warnings[0]["code"], "cli.secret_file_permissions");
    let requests = sets(&daemon);
    assert_eq!(requests[0]["input"]["content"], credential.value());
    assert_eq!(requests[1]["input"]["content"], "short77");
    assert_eq!(
        requests[2]["input"]["content"],
        format!("  {}\n", credential.value())
    );
    ev.pass();
}

#[test]
fn generate_copy_from_and_the_mode_and_revision_flags_reach_the_request() {
    let ev = evidence!(
        covers = ["BR-018", "REQ-028"],
        kind = Kind::Fixture,
        expected = "--generate and --copy-from send a source without any value, and --replace, --upsert, --expected-revision, and --confirm-stored-only fill the request",
    );
    let root = TestRoot::new().unwrap();
    let credential = FixtureCredential::new();
    let daemon = serving(&root, &credential);
    let call = |args: &[&str]| {
        let mut full = vec!["secret", "set", REFERENCE, "--json"];
        full.extend_from_slice(args);
        let ran = run(root.path(), &full);
        assert_eq!(ran.code, Some(0), "{args:?}: {ran:?}");
    };
    call(&["--generate"]);
    call(&[
        "--copy-from",
        OTHER,
        "--replace",
        "--expected-revision",
        "7",
    ]);
    call(&["--generate", "--upsert", "--confirm-stored-only"]);

    let requests = sets(&daemon);
    assert_eq!(requests[0]["input"], json!({"type": "generate"}));
    assert_eq!(requests[0]["mode"], "create");
    assert_eq!(
        requests[1]["input"],
        json!({"type": "copyFrom", "content": OTHER})
    );
    assert_eq!(requests[1]["mode"], "replace");
    assert_eq!(requests[1]["expectedRevision"], 7);
    assert_eq!(requests[2]["mode"], "upsert");
    assert_eq!(requests[2]["confirmStoredOnly"], true);
    ev.pass();
}

#[test]
fn reveal_prints_the_value_alone_on_stdout_and_json_carries_reference_and_value() {
    let ev = evidence!(
        covers = ["AC-016", "REQ-016", "BR-018"],
        kind = Kind::Fixture,
        expected = "stackly secret reveal writes the value and one newline on stdout and nothing else, and --json puts reference and value in result",
    );
    let root = TestRoot::new().unwrap();
    let credential = FixtureCredential::new();
    let daemon = serving(&root, &credential);

    let text = run(root.path(), &["secret", "reveal", REFERENCE]);
    let json = run(root.path(), &["secret", "reveal", REFERENCE, "--json"]);

    assert_eq!(text.code, Some(0), "{text:?}");
    assert_eq!(text.stdout, format!("{}\n", credential.value()));
    assert_eq!(text.stderr, "");
    let value = envelope(&json);
    assert_eq!(
        value["result"],
        json!({"reference": REFERENCE, "value": credential.value()})
    );
    let reveals: Vec<_> = daemon
        .log()
        .into_iter()
        .filter(|logged| logged.method == "secret.reveal")
        .collect();
    assert_eq!(reveals.len(), 2);
    assert_eq!(reveals[0].raw["call"]["content"]["purpose"], "show");
    ev.pass();
}

#[test]
fn list_status_and_delete_write_no_value_in_text_or_json() {
    let ev = evidence!(
        covers = ["AC-016", "REQ-016", "BR-018"],
        kind = Kind::Fixture,
        expected = "list, status, and delete show references, state, uses, and the store in text and --json, never a value",
    );
    let root = TestRoot::new().unwrap();
    let credential = FixtureCredential::new();
    let daemon = serving(&root, &credential);

    let list = run(root.path(), &["secret", "list"]);
    let list_json = run(root.path(), &["secret", "list", "--json"]);
    let status = run(root.path(), &["secret", "status", REFERENCE]);
    let status_json = run(root.path(), &["secret", "status", REFERENCE, "--json"]);
    let delete = run(
        root.path(),
        &["secret", "delete", REFERENCE, "--expected-revision", "1"],
    );
    let delete_json = run(root.path(), &["secret", "delete", REFERENCE, "--json"]);

    for output in [
        &list,
        &list_json,
        &status,
        &status_json,
        &delete,
        &delete_json,
    ] {
        assert_eq!(output.code, Some(0), "{output:?}");
        credential.assert_absent(&output.stdout, "stdout");
        credential.assert_absent(&output.stderr, "stderr");
    }
    assert!(list.stdout.starts_with("REFERENCE"), "{}", list.stdout);
    assert!(list.stdout.contains(REFERENCE));
    assert!(list.stdout.contains("local:instance/a/password"));
    assert!(
        list.stdout
            .contains("Store  macos_keychain, available (file)"),
        "{}",
        list.stdout
    );
    let items = &envelope(&list_json)["result"]["items"];
    assert_eq!(items.as_array().unwrap().len(), 2);
    assert_eq!(items[1]["usedBy"][0]["field"], "password");
    assert_eq!(envelope(&list_json)["result"]["store"]["target"], "file");

    assert!(
        status.stdout.contains(&format!("Reference  {REFERENCE}")),
        "{}",
        status.stdout
    );
    assert!(status.stdout.contains("Status"));
    assert_eq!(
        envelope(&status_json)["result"]["summary"]["reference"],
        REFERENCE
    );

    assert_eq!(delete.stdout, format!("Deleted  {REFERENCE}\n"));
    assert_eq!(
        envelope(&delete_json)["result"],
        json!({"reference": REFERENCE})
    );
    let deletes: Vec<_> = daemon
        .log()
        .into_iter()
        .filter(|logged| logged.method == "secret.delete")
        .collect();
    assert_eq!(deletes[0].raw["call"]["content"]["expectedRevision"], 1);
    ev.pass();
}

/// A daemon that wants `confirmStoredOnly` for a replace.
fn needing_confirmation(root: &TestRoot) -> ScriptedDaemon {
    ScriptedDaemon::serve(root, |call, _| match call {
        Call::SecretSet(params) if !params.confirm_stored_only => {
            vec![Step::Fail(
                ErrorBody::new(
                    &secret_codes::CONFIRMATION_REQUIRED,
                    "replacing this secret changes only the stored copy",
                )
                .with_detail("reference", params.reference.clone())
                .with_items([ErrorItem::new("effect")
                    .with_field("resourceKind", "instance")
                    .with_field("resourceId", INSTANCE)
                    .with_field("field", "password")
                    .with_field("effect", "stored_only")]),
            )]
        }
        Call::SecretSet(params) => {
            let mut reply = set_reply(&params.reference);
            reply.effects = vec![SecretEffectInfo {
                resource_kind: ResourceKind::Instance,
                resource_id: INSTANCE.to_owned(),
                field: "password".to_owned(),
                effect: SecretEffectKind::StoredOnly,
            }];
            reply.warnings = vec![SecretWarning::ShortValueNotScrubbed];
            vec![Step::Reply(Reply::SecretSet(Box::new(reply)))]
        }
        _ => Vec::new(),
    })
}

#[test]
fn a_stored_only_replace_noninteractive_is_left_to_the_daemon_and_the_flag_confirms_it() {
    let ev = evidence!(
        covers = ["BR-018", "REQ-028"],
        kind = Kind::Fixture,
        expected = "noninteractive without --confirm-stored-only the daemon's secret.confirmation_required ends the command with exit 2 after one request, and with the flag the one request carries confirmStoredOnly and the effects and warning are shown",
    );
    let root = TestRoot::new().unwrap();
    let daemon = needing_confirmation(&root);

    let refused = run(
        root.path(),
        &[
            "secret",
            "set",
            REFERENCE,
            "--generate",
            "--replace",
            "--json",
        ],
    );
    assert_eq!(refused.code, Some(2), "{refused:?}");
    assert_eq!(error_code(&refused), "secret.confirmation_required");
    assert_eq!(sets(&daemon).len(), 1);
    assert_eq!(sets(&daemon)[0]["confirmStoredOnly"], false);

    let confirmed = run(
        root.path(),
        &[
            "secret",
            "set",
            REFERENCE,
            "--generate",
            "--replace",
            "--confirm-stored-only",
        ],
    );
    assert_eq!(confirmed.code, Some(0), "{confirmed:?}");
    assert_eq!(sets(&daemon).len(), 2);
    assert_eq!(sets(&daemon)[1]["confirmStoredOnly"], true);
    assert!(
        confirmed.stdout.contains(&format!(
            "Effects    instance {INSTANCE} password: stored_only"
        )),
        "{}",
        confirmed.stdout
    );
    assert!(
        confirmed.stdout.contains("Warnings"),
        "{}",
        confirmed.stdout
    );
    ev.pass();
}

#[test]
fn an_interactive_stored_only_replace_lists_the_effects_and_asks_with_no_as_the_default() {
    let ev = evidence!(
        covers = ["BR-018", "REQ-028"],
        kind = Kind::Fixture,
        expected = "on a terminal the stored only effects are listed and y sends the request again with confirmStoredOnly, while anything else ends with cli.confirmation_declined and exit 9",
    );
    let root = TestRoot::new().unwrap();
    let daemon = needing_confirmation(&root);
    let tty = |input| PtyOptions {
        stdin_tty: true,
        input: Some(input),
        ..PtyOptions::default()
    };
    let args = ["secret", "set", REFERENCE, "--generate", "--replace"];

    let declined = run_in_pty(root.path(), &args, &tty("\n"));
    assert_eq!(declined.code, Some(9), "{declined:?}");
    assert_eq!(sets(&daemon).len(), 1);
    assert!(
        declined
            .stderr
            .contains(&format!("instance {INSTANCE}, field password")),
        "{}",
        declined.stderr
    );
    assert!(
        declined.stderr.contains(&format!("replace {REFERENCE}?")),
        "{}",
        declined.stderr
    );
    assert!(declined.stderr.contains("[y/N] "));

    let confirmed = run_in_pty(root.path(), &args, &tty("y\n"));
    assert_eq!(confirmed.code, Some(0), "{confirmed:?}");
    let requests = sets(&daemon);
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[1]["confirmStoredOnly"], false);
    assert_eq!(requests[2]["confirmStoredOnly"], true);
    ev.pass();
}

#[test]
fn the_terminal_prompt_asks_twice_and_gives_up_after_three_mismatching_pairs() {
    let ev = evidence!(
        covers = ["BR-018", "REQ-028", "AC-028"],
        kind = Kind::Fixture,
        expected = "on a terminal stackly secret set reads the value twice from /dev/tty and sends it once when both match, and three differing pairs end with cli.usage and exit 2 with no request",
    );
    let root = TestRoot::new().unwrap();
    let credential = FixtureCredential::new();
    let daemon = serving(&root, &credential);
    let tty = |input| PtyOptions {
        stdin_tty: true,
        input: Some(input),
        ..PtyOptions::default()
    };

    let typed = format!("{0}\n{0}\n", credential.value());
    let stored = run_in_pty(root.path(), &["secret", "set", REFERENCE], &tty(&typed));
    assert_eq!(stored.code, Some(0), "{stored:?}");
    let requests = sets(&daemon);
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0]["input"],
        json!({"type": "value", "content": credential.value()})
    );
    assert!(
        stored.stderr.contains(&format!("Value for {REFERENCE}: ")),
        "{}",
        stored.stderr
    );
    assert!(stored.stderr.contains("Repeat the value: "));
    credential.assert_absent(&stored.stdout, "stdout");

    let differing = "one-1\ntwo-2\nthree-3\nfour-4\nfive-5\nsix-6\n";
    let gave_up = run_in_pty(root.path(), &["secret", "set", OTHER], &tty(differing));
    assert_eq!(gave_up.code, Some(2), "{gave_up:?}");
    assert_eq!(sets(&daemon).len(), 1, "no request after the mismatches");
    assert!(
        gave_up
            .stderr
            .contains("the two entries differed three times"),
        "{}",
        gave_up.stderr
    );
    ev.pass();
}

#[test]
fn the_secret_commands_are_listed_in_help_and_say_what_they_do() {
    let root = TestRoot::new().unwrap();
    let help = run(root.path(), &["secret", "set", "--help"]);
    assert_eq!(help.code, Some(0), "{help:?}");
    for needle in [
        "--stdin",
        "--from-file",
        "--from-env",
        "--generate",
        "--copy-from",
        "--replace",
        "--upsert",
        "--expected-revision",
        "--confirm-stored-only",
        "Examples:",
    ] {
        assert!(
            help.stdout.contains(needle),
            "{needle} missing from:\n{}",
            help.stdout
        );
    }
    for flag in ["--password", "--value", "--secret-value"] {
        assert!(!help.stdout.contains(flag), "{flag} must not exist");
    }
    let paths = paths(&root);
    assert!(!paths.socket.exists());
}
