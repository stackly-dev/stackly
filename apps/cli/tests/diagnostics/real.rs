//! `stackly diagnostics` against a real `stacklyd` (spec 0034, AC-13,
//! AC-14, AC-16): a fake instance learned a fixture credential when it
//! started and printed it into its log; the exported file is the previewed
//! zip byte for byte, a second export without `--force` leaves it alone,
//! and the credential never shows in the output of `stackly diagnostics`
//! or `stackly doctor`.

use serde_json::Value;
use sha2::{Digest, Sha256};
use stackly_evidence::FixtureCredential;
use stackly_testkit::evidence::{Kind, evidence};
use stackly_testkit::{ReapGuard, TestRoot};

use crate::common::stacklyd::{seed, start_daemon};
use crate::common::{Run, envelope, paths, run};

/// Every byte of `run` a person or a script sees.
fn all_output(run: &Run) -> String {
    format!("{}{}", run.stdout, run.stderr)
}

/// The sha256 of `path`, lowercase hex.
fn sha256(path: &std::path::Path) -> String {
    let bytes = std::fs::read(path).expect("the bundle reads");
    Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Spec 0034 AC-13, AC-14, AC-16 against a real `stacklyd`.
#[test]
fn a_real_export_writes_the_previewed_zip_never_replaces_without_force_and_leaks_no_secret() {
    let ev = evidence!(
        covers = ["REQ-028", "NFR-016", "AC-016"],
        kind = Kind::Fixture,
        expected = "against a real stacklyd with a running fake instance that learned a fixture credential at start and printed it into its log, stackly diagnostics export --yes writes a file whose sha256 and size match the reply and whose size is the preview's zip size; a second export to the same path without --force exits 3 with diagnostics.destination_exists and leaves the file as it was, and with --force replaces it; the credential appears in no output of diagnostics preview, diagnostics export, or doctor, as text or --json",
    );
    let root = TestRoot::new().expect("a test root");
    let runtime = seed(&root);
    let _reap = ReapGuard::new(root.path(), &paths(&root).runtime);
    let daemon = start_daemon(&root);
    let credential = FixtureCredential::new();
    let token = root.path().join("token.txt");
    std::fs::write(&token, credential.value()).expect("the token file is written");
    let secret = format!("authToken=file:{}", token.display());
    let created = run(
        root.path(),
        &[
            "instance",
            "create",
            "leaky",
            "--runtime",
            &runtime,
            "--provider",
            "testing/fake",
            "--revision",
            "0.4.0",
            "--set",
            "leakToken=true",
            "--secret",
            &secret,
            "--wait",
            "--timeout",
            "60s",
            "--json",
        ],
    );
    assert_eq!(created.code, Some(0), "{created:?}");
    let started = run(
        root.path(),
        &["instance", "start", "leaky", "--wait", "--timeout", "60s"],
    );
    assert_eq!(started.code, Some(0), "{started:?}");
    let folder = tempfile::tempdir().expect("a folder for the bundle");
    let canonical = std::fs::canonicalize(folder.path()).expect("the folder is canonical");
    let bundle = canonical.join("bundle.zip");
    let output = bundle.to_str().expect("a UTF-8 path");

    let preview_text = run(root.path(), &["diagnostics", "preview"]);
    let preview_json = run(root.path(), &["diagnostics", "preview", "--json"]);
    let export_json = run(
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
    let first = sha256(&bundle);
    let size = std::fs::metadata(&bundle)
        .expect("the bundle is there")
        .len();
    let again = run(
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
    let unchanged = sha256(&bundle);
    let forced = run(
        root.path(),
        &[
            "diagnostics",
            "export",
            "--yes",
            "--force",
            "--output",
            output,
        ],
    );
    let doctor_text = run(root.path(), &["doctor"]);
    let doctor_json = run(root.path(), &["doctor", "--json"]);

    assert_eq!(preview_text.code, Some(0), "{preview_text:?}");
    assert!(
        preview_text.stdout.contains("logs/instances/leaky.log"),
        "{}",
        preview_text.stdout
    );
    assert_eq!(preview_json.code, Some(0), "{preview_json:?}");

    assert_eq!(export_json.code, Some(0), "{export_json:?}");
    let value = envelope(&export_json);
    let exported = &value["result"]["exported"];
    assert_eq!(exported["path"], output);
    assert_eq!(exported["sha256"], first.as_str());
    assert_eq!(exported["bytes"], size);
    assert_eq!(value["result"]["preview"]["zipBytes"], size);

    assert_eq!(again.code, Some(3), "{again:?}");
    let refused = envelope(&again);
    assert_eq!(refused["error"]["code"], "diagnostics.destination_exists");
    assert_eq!(refused["result"]["exported"], Value::Null);
    assert_eq!(unchanged, first, "the refused export left the file alone");

    assert_eq!(forced.code, Some(0), "{forced:?}");
    assert!(
        forced.stdout.starts_with(&format!("Saved {output} (")),
        "{}",
        forced.stdout
    );
    assert!(bundle.is_file());
    let leftovers: Vec<_> = std::fs::read_dir(&canonical)
        .expect("the folder lists")
        .map(|entry| entry.expect("an entry").file_name())
        .collect();
    assert_eq!(leftovers, ["bundle.zip"], "no partial file is left");

    assert!(
        doctor_text.code == Some(0) || doctor_text.code == Some(7),
        "{doctor_text:?}"
    );
    for (out, what) in [
        (all_output(&preview_text), "diagnostics preview"),
        (all_output(&preview_json), "diagnostics preview --json"),
        (all_output(&export_json), "diagnostics export --json"),
        (all_output(&again), "a refused diagnostics export"),
        (all_output(&forced), "diagnostics export --force"),
        (all_output(&doctor_text), "doctor"),
        (all_output(&doctor_json), "doctor --json"),
    ] {
        credential.assert_absent(&out, what);
    }
    assert_eq!(daemon.stop().unwrap().code(), Some(0));
    ev.pass();
}
