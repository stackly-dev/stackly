//! `stackly secret` against the real `stacklyd` (spec 0011 build step 8,
//! AC-16 and AC-17): the sentinel suite through the CLI. The daemon runs
//! with `STACKLY_ROOT` on a test root, so it opens a file keychain of its
//! own and never touches the login Keychain. The only credential is a
//! fixture credential, and its prefix must appear nowhere but in the
//! request that carries it and in `stackly secret reveal`.
#![cfg(target_os = "macos")]

mod common;

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use common::{daemon_paths, start_daemon};
use serde_json::Value;
use stackly_testkit::TestRoot;
use stackly_testkit::cargo_bin;
use stackly_testkit::evidence::{FixtureCredential, Kind, evidence};

const REFERENCE: &str = "local:user/sentinel";
const COPY: &str = "local:user/sentinel-copy";

/// What one run of `stackly` left.
struct Run {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

impl Run {
    fn envelope(&self) -> Value {
        serde_json::from_str(self.stdout.trim_end()).expect("--json writes one envelope line")
    }

    fn error_code(&self) -> String {
        self.envelope()["error"]["code"]
            .as_str()
            .expect("the run failed with a code")
            .to_owned()
    }
}

/// `stackly <args>` against the daemon of `root`, with `input` on stdin.
fn stackly(root: &Path, args: &[&str], input: &[u8]) -> Run {
    let mut child = Command::new(cargo_bin("stackly-cli", "stackly"))
        .args(args)
        .env("STACKLY_ROOT", root)
        .env("TZ", "UTC")
        .env_remove("NO_COLOR")
        .env_remove("CLICOLOR_FORCE")
        .env_remove("STACKLY_NO_INPUT")
        .stdin(Stdio::piped())
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
    }
}

#[test]
fn the_sentinel_value_set_through_the_cli_appears_only_in_reveal() {
    let ev = evidence!(
        covers = ["AC-016", "REQ-016", "BR-018"],
        kind = Kind::Fixture,
        expected = "a fixture credential set with stackly secret set --stdin, --from-file, --from-env, and --copy-from against a real stacklyd on a test root is read back by reveal, and its prefix is in no other CLI output, not in the daemon log, and not in the registry bytes including the WAL",
    );
    let root = TestRoot::new().unwrap();
    let daemon = start_daemon(&root);
    let paths = daemon_paths(&root);
    let credential = FixtureCredential::new();
    let rooted = root.path();
    let mut outputs: Vec<(String, Run)> = Vec::new();
    let mut record = |what: &str, run: Run| -> Run {
        assert!(!run.stdout.is_empty() || !run.stderr.is_empty() || run.code == Some(0));
        outputs.push((
            what.to_owned(),
            Run {
                code: run.code,
                stdout: run.stdout.clone(),
                stderr: run.stderr.clone(),
            },
        ));
        run
    };

    // Set through every source that carries a value.
    let piped = format!("{}\r\n", credential.value());
    let set = record(
        "set --stdin",
        stackly(
            rooted,
            &["secret", "set", REFERENCE, "--stdin", "--json"],
            piped.as_bytes(),
        ),
    );
    assert_eq!(set.code, Some(0), "{:?}", set.stderr);
    let summary = &set.envelope()["result"]["summary"];
    assert_eq!(summary["reference"], REFERENCE);
    assert_eq!(summary["status"], "present");
    assert_eq!(summary["valueOrigin"], "entered");
    assert_eq!(summary["revision"], 1);

    let inputs = rooted.join("inputs");
    std::fs::create_dir(&inputs).unwrap();
    let file = inputs.join("value");
    std::fs::write(&file, format!("{}\n", credential.value())).unwrap();
    std::fs::set_permissions(&file, std::os::unix::fs::PermissionsExt::from_mode(0o600)).unwrap();
    let from_file = record(
        "set --from-file",
        stackly(
            rooted,
            &[
                "secret",
                "set",
                REFERENCE,
                "--from-file",
                file.to_str().unwrap(),
                "--replace",
                "--json",
            ],
            b"",
        ),
    );
    assert_eq!(from_file.code, Some(0), "{:?}", from_file.stderr);
    assert_eq!(from_file.envelope()["result"]["summary"]["revision"], 2);

    let mut command = Command::new(cargo_bin("stackly-cli", "stackly"));
    command
        .args([
            "secret",
            "set",
            "local:user/from-env",
            "--from-env",
            "STACKLY_TEST_SENTINEL",
            "--json",
        ])
        .env("STACKLY_ROOT", rooted)
        .env("STACKLY_TEST_SENTINEL", credential.value())
        .stdin(Stdio::null());
    let output = command.output().expect("stackly runs");
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    credential.assert_absent(
        &String::from_utf8_lossy(&output.stdout),
        "set --from-env stdout",
    );
    credential.assert_absent(
        &String::from_utf8_lossy(&output.stderr),
        "set --from-env stderr",
    );

    let copied = record(
        "set --copy-from",
        stackly(
            rooted,
            &["secret", "set", COPY, "--copy-from", REFERENCE, "--json"],
            b"",
        ),
    );
    assert_eq!(copied.code, Some(0), "{:?}", copied.stderr);
    assert_eq!(
        copied.envelope()["result"]["summary"]["valueOrigin"],
        "copied"
    );

    let generated = record(
        "set --generate",
        stackly(
            rooted,
            &[
                "secret",
                "set",
                "local:user/generated",
                "--generate",
                "--json",
            ],
            b"",
        ),
    );
    assert_eq!(generated.code, Some(0), "{:?}", generated.stderr);
    assert_eq!(
        generated.envelope()["result"]["summary"]["valueOrigin"],
        "generated"
    );

    // List and status show state, never a value.
    let list = record("list", stackly(rooted, &["secret", "list"], b""));
    let list_json = record(
        "list --json",
        stackly(rooted, &["secret", "list", "--json"], b""),
    );
    let status = record(
        "status",
        stackly(rooted, &["secret", "status", REFERENCE], b""),
    );
    let status_json = record(
        "status --json",
        stackly(rooted, &["secret", "status", REFERENCE, "--json"], b""),
    );
    for run in [&list, &list_json, &status, &status_json] {
        assert_eq!(run.code, Some(0), "{:?}", run.stderr);
    }
    let items = list_json.envelope()["result"]["items"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(items.len(), 4, "{items:?}");
    assert_eq!(
        list_json.envelope()["result"]["store"]["backend"],
        "macos_keychain"
    );
    assert_eq!(list_json.envelope()["result"]["store"]["target"], "file");
    assert!(
        status.stdout.contains("Status     present"),
        "{}",
        status.stdout
    );
    assert_eq!(
        status_json.envelope()["result"]["summary"]["status"],
        "present"
    );

    // Reveal is the one command that shows the value.
    let reveal = stackly(rooted, &["secret", "reveal", REFERENCE], b"");
    assert_eq!(reveal.code, Some(0), "{:?}", reveal.stderr);
    assert_eq!(reveal.stdout, format!("{}\n", credential.value()));
    assert_eq!(reveal.stderr, "");
    let reveal_copy = stackly(rooted, &["secret", "reveal", COPY, "--json"], b"");
    assert_eq!(reveal_copy.code, Some(0), "{:?}", reveal_copy.stderr);
    assert_eq!(
        reveal_copy.envelope()["result"]["value"],
        credential.value()
    );

    // Errors the daemon answers, each with its exit code and no value.
    let again = record(
        "set create again",
        stackly(
            rooted,
            &["secret", "set", REFERENCE, "--stdin", "--json"],
            piped.as_bytes(),
        ),
    );
    assert_eq!(
        (again.code, again.error_code().as_str()),
        (Some(3), "secret.conflict")
    );
    let stale = record(
        "set stale revision",
        stackly(
            rooted,
            &[
                "secret",
                "set",
                REFERENCE,
                "--stdin",
                "--replace",
                "--expected-revision",
                "1",
                "--json",
            ],
            piped.as_bytes(),
        ),
    );
    assert_eq!(stale.code, Some(3), "{:?}", stale.stdout);
    let missing = record(
        "reveal missing",
        stackly(
            rooted,
            &["secret", "reveal", "local:user/nothing", "--json"],
            b"",
        ),
    );
    assert_eq!(
        (missing.code, missing.error_code().as_str()),
        (Some(10), "secret.not_found")
    );
    let replace_missing = record(
        "replace missing",
        stackly(
            rooted,
            &[
                "secret",
                "set",
                "local:user/nothing",
                "--stdin",
                "--replace",
                "--json",
            ],
            piped.as_bytes(),
        ),
    );
    assert_eq!(
        replace_missing.code,
        Some(10),
        "{:?}",
        replace_missing.stdout
    );

    // Delete, and a second delete finds nothing.
    let delete = record(
        "delete",
        stackly(rooted, &["secret", "delete", COPY, "--json"], b""),
    );
    assert_eq!(delete.code, Some(0), "{:?}", delete.stderr);
    assert_eq!(delete.envelope()["result"]["reference"], COPY);
    let delete_again = record(
        "delete again",
        stackly(rooted, &["secret", "delete", COPY, "--json"], b""),
    );
    assert_eq!(
        (delete_again.code, delete_again.error_code().as_str()),
        (Some(10), "secret.not_found")
    );

    for (what, run) in &outputs {
        credential.assert_absent(&run.stdout, &format!("{what} stdout"));
        credential.assert_absent(&run.stderr, &format!("{what} stderr"));
    }
    assert_eq!(daemon.stop().unwrap().code(), Some(0));
    credential.assert_absent(
        &std::fs::read_to_string(paths.layout.daemon_log_dir().join("stacklyd.log"))
            .unwrap_or_default(),
        "the daemon log",
    );
    let registry = paths.layout.registry_db();
    for suffix in ["", "-wal", "-shm"] {
        let mut name = registry.clone().into_os_string();
        name.push(suffix);
        if let Ok(bytes) = std::fs::read(&name) {
            credential.assert_absent(
                &String::from_utf8_lossy(&bytes),
                &format!("registry{suffix}"),
            );
        }
    }
    ev.pass();
}
