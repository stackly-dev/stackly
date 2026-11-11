//! `stackly provider validate` against a real `stacklyd` (spec 0036 AC-5,
//! CLI part): the CLI makes the folder absolute, the daemon checks it, and
//! the CLI prints the lines of `cargo xtask provider check` with a count,
//! exiting 0 for a publishable package and 2 with `cli.provider_invalid`
//! and the result kept otherwise.

use std::path::{Path, PathBuf};

use stackly_testkit::evidence::{Kind, evidence};
use stackly_testkit::{ReapGuard, TestRoot};

use crate::common::stacklyd::start_daemon;
use crate::common::{detail, envelope, paths, run, run_command, stackly};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Copies a folder tree with `std::fs`, as test setup.
fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("the copy target can be created");
    for entry in std::fs::read_dir(from).expect("the folder can be listed") {
        let entry = entry.expect("the entry can be read");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("the entry has a type").is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).expect("the file can be copied");
        }
    }
}

#[test]
fn a_publishable_package_prints_its_ok_line_and_exits_0() {
    let ev = evidence!(
        covers = ["REQ-030"],
        kind = Kind::Fixture,
        expected = "stackly provider validate on official/nats prints ok with its id, revision, and digest and 0 error(s), 0 warning(s), exits 0, and writes the ProviderValidation as result with --json",
    );
    let root = TestRoot::new().expect("a test root");
    let _reap = ReapGuard::new(root.path(), &paths(&root).runtime);
    let _daemon = start_daemon(&root);
    let nats = repo().join("providers/nats");
    let nats = nats.to_str().expect("a UTF-8 path");

    let text = run(root.path(), &["provider", "validate", nats]);
    let json = run(root.path(), &["provider", "validate", nats, "--json"]);

    assert_eq!(text.code, Some(0), "{text:?}");
    let lines: Vec<&str> = text.stdout.lines().collect();
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(lines[0].starts_with("ok official/nats "), "{}", lines[0]);
    assert_eq!(lines[0].split(' ').nth(3).map(str::len), Some(64));
    assert_eq!(lines[1], "0 error(s), 0 warning(s)");
    assert_eq!(json.code, Some(0), "{json:?}");
    let value = envelope(&json);
    assert_eq!(value["command"], "provider validate");
    assert_eq!(value["result"]["publishable"], true);
    assert_eq!(value["result"]["identity"]["provider"], "official/nats");
    assert_eq!(value["result"]["folder"], "nats");
    ev.pass();
}

#[test]
fn a_package_that_is_not_publishable_exits_2_with_its_lines_and_the_result() {
    let ev = evidence!(
        covers = ["AC-030", "REQ-030"],
        kind = Kind::Fixture,
        expected = "stackly provider validate on a package with apiVersion stackly/v2, given as a relative path with dot segments, prints the one unsupported_api_version line and 1 error(s), 0 warning(s), and exits 2 with cli.provider_invalid, keeping the validation as result with --json",
    );
    let root = TestRoot::new().expect("a test root");
    let _reap = ReapGuard::new(root.path(), &paths(&root).runtime);
    let _daemon = start_daemon(&root);
    let work = root.path().join("work");
    let mytool = work.join("pkgs/mytool");
    copy_dir(&repo().join("providers/fake"), &mytool);
    let manifest = mytool.join("provider.yaml");
    let text = std::fs::read_to_string(&manifest).unwrap();
    std::fs::write(
        &manifest,
        text.replace("apiVersion: stackly/v1alpha1", "apiVersion: stackly/v2"),
    )
    .unwrap();
    let relative = "./pkgs/../pkgs/./mytool";

    let mut command = stackly(root.path());
    command
        .current_dir(&work)
        .args(["provider", "validate", relative]);
    let text = run_command(command);
    let mut command = stackly(root.path());
    command
        .current_dir(&work)
        .args(["provider", "validate", relative, "--json"]);
    let json = run_command(command);

    assert_eq!(text.code, Some(2), "{text:?}");
    assert_eq!(
        text.stdout,
        "mytool: provider.yaml apiVersion: unsupported_api_version: apiVersion \"stackly/v2\" is not supported; this Stackly supports: stackly/v1alpha1\n1 error(s), 0 warning(s)\n"
    );
    assert!(
        text.stderr.contains("cli.provider_invalid"),
        "{}",
        text.stderr
    );
    assert_eq!(json.code, Some(2), "{json:?}");
    let value = envelope(&json);
    assert_eq!(value["error"]["code"], "cli.provider_invalid");
    assert_eq!(value["error"]["category"], "invalid_input");
    assert_eq!(detail(&value["error"], "errors").as_deref(), Some("1"));
    assert_eq!(detail(&value["error"], "warnings").as_deref(), Some("0"));
    assert_eq!(value["result"]["loadable"], false);
    assert_eq!(value["result"]["folder"], "mytool");
    assert_eq!(
        value["result"]["findings"][0]["code"],
        "unsupported_api_version"
    );
    ev.pass();
}

#[test]
fn an_authoring_finding_is_an_error_line_and_a_missing_folder_is_a_finding() {
    let root = TestRoot::new().expect("a test root");
    let _reap = ReapGuard::new(root.path(), &paths(&root).runtime);
    let _daemon = start_daemon(&root);
    let fake_command = repo().join("crates/core/tests/fixtures/packages/fake-command");

    let authoring = run(
        root.path(),
        &[
            "provider",
            "validate",
            fake_command.to_str().expect("a UTF-8 path"),
        ],
    );
    let missing = run(
        root.path(),
        &[
            "provider",
            "validate",
            root.path().join("nowhere").to_str().expect("a UTF-8 path"),
        ],
    );

    assert_eq!(authoring.code, Some(2), "{authoring:?}");
    assert!(
        authoring
            .stdout
            .starts_with("fake-command: templates/peers.yaml.tmpl 2:7: token_not_allowed_here: "),
        "{}",
        authoring.stdout
    );
    assert!(authoring.stdout.ends_with("1 error(s), 0 warning(s)\n"));
    assert!(!authoring.stdout.contains("\nok "));
    assert_eq!(missing.code, Some(2), "{missing:?}");
    assert!(
        missing.stdout.contains(": missing_file: "),
        "{}",
        missing.stdout
    );
}
