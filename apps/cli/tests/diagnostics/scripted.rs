//! AC-16 against a scripted daemon.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use stackly_protocol::codes::diagnostics;
use stackly_protocol::diagnostics::{BundlePreview, ExportedBundle};
use stackly_protocol::{Call, ErrorBody, Reply};
use stackly_testkit::TestRoot;
use stackly_testkit::evidence::{Kind, evidence};

use crate::common::pty::{PtyOptions, run_in_pty};
use crate::common::scripted::{ScriptedDaemon, Step};
use crate::common::{Run, envelope, golden, listing, run, run_command, stackly};

/// The suggested file name of the preview fixture.
const SUGGESTED: &str = "stackly-diagnostics-20261110T080000Z.zip";

/// The reply content of the fixture frame `name`.
fn fixture(name: &str) -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../protocols/fixtures/ipc")
        .join(name);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("fixture {} reads: {error}", path.display()));
    let frame: Value = serde_json::from_str(&text).expect("a fixture is JSON");
    frame["content"]["reply"]["content"].clone()
}

/// The error body of the fixture frame `name`.
fn fixture_error(name: &str) -> ErrorBody {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../protocols/fixtures/ipc")
        .join(name);
    let frame: Value =
        serde_json::from_str(&std::fs::read_to_string(&path).expect("the fixture reads"))
            .expect("a fixture is JSON");
    serde_json::from_value(frame["content"]["error"].clone()).expect("an error body decodes")
}

fn preview() -> BundlePreview {
    serde_json::from_value(fixture("frame-response-diagnostics-preview-bundle.json"))
        .expect("the preview fixture decodes")
}

fn exported() -> ExportedBundle {
    serde_json::from_value(fixture("frame-response-diagnostics-export-bundle.json"))
        .expect("the export fixture decodes")
}

/// A daemon that answers every preview with the fixture and every export
/// with `export`.
fn daemon(root: &TestRoot, export: Step) -> ScriptedDaemon {
    let preview = preview();
    ScriptedDaemon::serve(root, move |call, _| match call {
        Call::DiagnosticsPreviewBundle => {
            vec![Step::Reply(Reply::DiagnosticsPreviewBundle(Box::new(
                preview.clone(),
            )))]
        }
        Call::DiagnosticsExportBundle(_) => vec![export.clone()],
        _ => Vec::new(),
    })
}

/// A daemon whose exports succeed with the fixture.
fn exporting(root: &TestRoot) -> ScriptedDaemon {
    daemon(
        root,
        Step::Reply(Reply::DiagnosticsExportBundle(exported())),
    )
}

/// The params of every `diagnostics.export_bundle` the daemon got.
fn exports(daemon: &ScriptedDaemon) -> Vec<Value> {
    daemon
        .log()
        .into_iter()
        .filter(|logged| logged.method == "diagnostics.export_bundle")
        .map(|logged| logged.raw["call"]["content"].clone())
        .collect()
}

/// `stackly <args>` for `root`, run from `cwd`.
fn run_in(root: &TestRoot, cwd: &Path, args: &[&str]) -> Run {
    let mut command = stackly(root.path());
    command.args(args).current_dir(cwd);
    run_command(command)
}

/// A folder outside the test root, and its canonical path.
fn folder() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("a temporary folder");
    let canonical = std::fs::canonicalize(dir.path()).expect("the folder is canonical");
    (dir, canonical)
}

/// Spec 0034 AC-16.
#[test]
fn preview_lists_every_entry_with_its_size_and_the_redaction_note() {
    let ev = evidence!(
        covers = ["REQ-028", "NFR-016"],
        kind = Kind::Fixture,
        expected = "stackly diagnostics preview against a scripted daemon that answers the preview fixture writes one row per entry with its path, size, truncation, and description, the totals, and the redaction note, exits 0, sends no export, and --json holds the preview as the daemon sent it",
    );
    let root = TestRoot::new().unwrap();
    let daemon = exporting(&root);
    let before = listing(root.path());

    let text = run(root.path(), &["diagnostics", "preview"]);
    let json = run(root.path(), &["diagnostics", "preview", "--json"]);

    assert_eq!(text.code, Some(0), "{text:?}");
    golden::check("diagnostics_preview.txt", &text.stdout);
    assert_eq!(text.stderr, "Preparing the bundle…\n");
    assert_eq!(json.code, Some(0), "{json:?}");
    let value = envelope(&json);
    assert_eq!(value["command"], "diagnostics preview");
    assert_eq!(
        value["result"],
        fixture("frame-response-diagnostics-preview-bundle.json")
    );
    golden::check("diagnostics_preview.json", &json.stdout);
    assert!(exports(&daemon).is_empty());
    assert_eq!(listing(root.path()), before, "the preview created a path");
    ev.pass();
}

/// Spec 0034 AC-16.
#[test]
fn export_with_yes_lists_the_entries_on_stderr_then_exports_that_preview_and_says_where() {
    let ev = evidence!(
        covers = ["REQ-028", "NFR-016"],
        kind = Kind::Fixture,
        expected = "stackly diagnostics export --yes --output <absolute path> writes the progress line and the listing with the redaction note to stderr, sends one export of the previewed ID to that path with replace false, writes Saved <path> (<size>) to stdout, exits 0, and --json holds the preview and the exported file",
    );
    let root = TestRoot::new().unwrap();
    let daemon = exporting(&root);
    let (_dir, folder) = folder();
    let output = folder.join("bundle.zip");
    let output = output.to_str().unwrap();

    let text = run(
        root.path(),
        &["diagnostics", "export", "--yes", "--output", output],
    );
    let json = run(
        root.path(),
        &[
            "diagnostics",
            "export",
            "--yes",
            "--output",
            output,
            "--json",
        ],
    );

    assert_eq!(text.code, Some(0), "{text:?}");
    golden::check("diagnostics_export.txt", &text.stdout);
    assert!(
        text.stderr.starts_with("Preparing the bundle…\nPATH "),
        "{}",
        text.stderr
    );
    assert!(
        text.stderr.ends_with(
            "Secrets and connection strings are redacted. Paths in your home folder are shown as ~. Nothing is uploaded.\n"
        ),
        "{}",
        text.stderr
    );
    assert!(!text.stderr.contains("Save this bundle"), "{}", text.stderr);
    assert_eq!(json.code, Some(0), "{json:?}");
    let value = envelope(&json);
    assert_eq!(value["command"], "diagnostics export");
    assert_eq!(
        value["result"]["preview"],
        fixture("frame-response-diagnostics-preview-bundle.json")
    );
    assert_eq!(
        value["result"]["exported"],
        fixture("frame-response-diagnostics-export-bundle.json")
    );
    golden::check("diagnostics_export.json", &json.stdout);
    assert_eq!(json.stderr, "Preparing the bundle…\n");
    let expected = json!({
        "previewId": "0192f3a8-0000-7000-8000-0000000000b1",
        "destination": output,
        "replace": false,
    });
    assert_eq!(exports(&daemon), [expected.clone(), expected]);
    ev.pass();
}

/// Spec 0034 AC-16, spec 0008 `Confirm`: noninteractive without `--yes`
/// fails before the export.
#[test]
fn export_without_yes_and_without_a_terminal_fails_before_anything_is_written() {
    let root = TestRoot::new().unwrap();
    let daemon = exporting(&root);
    let (_dir, folder) = folder();
    let output = folder.join("bundle.zip");

    let json = run(
        root.path(),
        &[
            "diagnostics",
            "export",
            "--output",
            output.to_str().unwrap(),
            "--json",
        ],
    );

    assert_eq!(json.code, Some(2), "{json:?}");
    let value = envelope(&json);
    assert_eq!(value["error"]["code"], "cli.confirmation_required");
    assert_eq!(
        value["result"]["preview"],
        fixture("frame-response-diagnostics-preview-bundle.json")
    );
    assert_eq!(value["result"]["exported"], Value::Null);
    assert!(exports(&daemon).is_empty());
}

/// Spec 0034 AC-16, spec 0008 `Confirm`: the question on a terminal, where
/// only yes exports.
#[test]
fn on_a_terminal_export_asks_where_it_saves_and_only_yes_exports() {
    let root = TestRoot::new().unwrap();
    let daemon = exporting(&root);
    let (_dir, folder) = folder();
    let output = folder.join("bundle.zip");
    let output = output.to_str().unwrap();
    let args = ["diagnostics", "export", "--output", output];

    let yes = run_in_pty(
        root.path(),
        &args,
        &PtyOptions {
            stdin_tty: true,
            input: Some("y\n"),
            ..PtyOptions::default()
        },
    );
    let declined = run_in_pty(
        root.path(),
        &args,
        &PtyOptions {
            stdin_tty: true,
            input: Some("\n"),
            ..PtyOptions::default()
        },
    );

    assert_eq!(yes.code, Some(0), "{yes:?}");
    assert!(
        yes.stderr.contains(&format!(
            "Save this bundle to {output}?\nthe zip is written only to this Mac; nothing is uploaded\n[y/N] "
        )),
        "{}",
        yes.stderr
    );
    assert!(yes.stderr.contains("logs/stacklyd.log"), "{}", yes.stderr);
    assert!(yes.stdout.starts_with("Saved "), "{}", yes.stdout);
    assert_eq!(declined.code, Some(9), "{declined:?}");
    assert!(
        declined.stderr.contains("code: cli.confirmation_declined"),
        "{}",
        declined.stderr
    );
    assert_eq!(exports(&daemon).len(), 1, "only the yes exported");
}

/// Spec 0034 AC-16: a relative `--output` and the default name start in the
/// current folder, and `--force` sends `replace`.
#[test]
fn a_relative_or_missing_output_is_made_absolute_in_the_current_folder() {
    let root = TestRoot::new().unwrap();
    let daemon = exporting(&root);
    let (_dir, folder) = folder();
    std::fs::create_dir(folder.join("sub")).unwrap();

    for args in [
        vec!["diagnostics", "export", "--yes", "--output", "b.zip"],
        vec![
            "diagnostics",
            "export",
            "--yes",
            "--output",
            "./sub/../b.zip",
        ],
        vec!["diagnostics", "export", "--yes"],
        vec![
            "diagnostics",
            "export",
            "--yes",
            "--force",
            "--output",
            "sub/./c.zip",
        ],
    ] {
        let done = run_in(&root, &folder, &args);
        assert_eq!(done.code, Some(0), "{args:?}: {done:?}");
    }

    let sent: Vec<(String, bool)> = exports(&daemon)
        .iter()
        .map(|params| {
            (
                params["destination"].as_str().unwrap().to_owned(),
                params["replace"].as_bool().unwrap(),
            )
        })
        .collect();
    let at = |name: &str| folder.join(name).to_str().unwrap().to_owned();
    assert_eq!(
        sent,
        [
            (at("b.zip"), false),
            (at("b.zip"), false),
            (at(SUGGESTED), false),
            (at("sub/c.zip"), true),
        ]
    );
}

/// Spec 0034 AC-13, AC-16: each diagnostics error exits by its category,
/// keeps the preview, and shows the daemon's hint.
#[test]
fn each_diagnostics_error_exits_by_its_category_with_the_preview_kept() {
    let (_dir, folder) = folder();
    let output = folder.join("bundle.zip");
    let output = output.to_str().unwrap();
    for (frame, code, exit) in [
        (
            "frame-error-diagnostics-destination-exists.json",
            "diagnostics.destination_exists",
            3,
        ),
        (
            "frame-error-diagnostics-preview-not-found.json",
            "diagnostics.preview_not_found",
            10,
        ),
        (
            "frame-error-diagnostics-destination-invalid.json",
            "diagnostics.destination_invalid",
            2,
        ),
        (
            "frame-error-diagnostics-write-failed.json",
            "diagnostics.write_failed",
            7,
        ),
    ] {
        let root = TestRoot::new().unwrap();
        let body = fixture_error(frame);
        let hint = body.hint().map(str::to_owned);
        let _daemon = daemon(&root, Step::Fail(body));

        let json = run(
            root.path(),
            &[
                "diagnostics",
                "export",
                "--yes",
                "--output",
                output,
                "--json",
            ],
        );
        let text = run(
            root.path(),
            &["diagnostics", "export", "--yes", "--output", output],
        );

        assert_eq!(json.code, Some(exit), "{code}: {json:?}");
        let value = envelope(&json);
        assert_eq!(value["error"]["code"], code);
        assert_eq!(value["result"]["exported"], Value::Null);
        assert_eq!(
            value["result"]["preview"]["previewId"],
            "0192f3a8-0000-7000-8000-0000000000b1"
        );
        assert_eq!(text.code, Some(exit), "{code}: {text:?}");
        assert_eq!(text.stdout, "", "{code}");
        if let Some(hint) = hint {
            assert!(
                text.stderr.contains(&format!("hint: {hint}")),
                "{code}: {}",
                text.stderr
            );
        }
    }

    let root = TestRoot::new().unwrap();
    let failed = fixture_error("frame-error-diagnostics-bundle-failed.json");
    let _daemon = ScriptedDaemon::serve(&root, move |call, _| match call {
        Call::DiagnosticsPreviewBundle => vec![Step::Fail(failed.clone())],
        _ => Vec::new(),
    });
    for args in [
        vec!["diagnostics", "preview", "--json"],
        vec!["diagnostics", "export", "--yes", "--json"],
    ] {
        let json = run(root.path(), &args);
        assert_eq!(json.code, Some(7), "{args:?}: {json:?}");
        let value = envelope(&json);
        assert_eq!(value["error"]["code"], diagnostics::BUNDLE_FAILED.code);
        assert_eq!(value["result"], Value::Null);
    }
}

/// Spec 0034 AC-16: without a daemon, the connection error and its exit.
#[test]
fn without_a_daemon_both_verbs_exit_6() {
    let root = TestRoot::new().unwrap();

    let preview = run(root.path(), &["diagnostics", "preview"]);
    let export = run(root.path(), &["diagnostics", "export", "--yes", "--json"]);

    assert_eq!(preview.code, Some(6), "{preview:?}");
    assert!(
        preview.stderr.contains("code: ipc.endpoint_missing"),
        "{}",
        preview.stderr
    );
    assert_eq!(export.code, Some(6), "{export:?}");
    assert_eq!(envelope(&export)["result"], Value::Null);
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}

/// Daemon text in a listing is escaped.
#[test]
fn daemon_text_in_the_listing_is_escaped() {
    let root = TestRoot::new().unwrap();
    let mut hostile = preview();
    hostile.entries[0].path = "README\u{1b}[2J.txt".to_owned();
    hostile.entries[0].description = "line\nbreak \u{202e}".to_owned();
    let _daemon = ScriptedDaemon::serve(&root, move |call, _| match call {
        Call::DiagnosticsPreviewBundle => vec![Step::Reply(Reply::DiagnosticsPreviewBundle(
            Box::new(hostile.clone()),
        ))],
        _ => Vec::new(),
    });

    let text = run(root.path(), &["diagnostics", "preview"]);

    assert_eq!(text.code, Some(0), "{text:?}");
    assert!(
        text.stdout.contains("README\\u{1b}[2J.txt"),
        "{}",
        text.stdout
    );
    assert!(
        text.stdout.contains("line\\u{a}break \\u{202e}"),
        "{}",
        text.stdout
    );
    assert!(!text.stdout.contains('\u{1b}'), "{}", text.stdout);
}
