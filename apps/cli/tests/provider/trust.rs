//! Spec 0037 AC-14 and AC-11 (CLI part): `stackly provider import`,
//! `approve`, `revoke`, and `remove` against a scripted daemon that
//! answers with the fixtures of `protocols/fixtures/provider/`, plus the
//! `TRUST` column of `list` and the local rows of `inspect`. Every test
//! checks which methods reached the daemon, so a refused confirmation
//! provably sends no mutation.

use std::sync::{Arc, Mutex};

use stackly_testkit::TestRoot;
use stackly_testkit::evidence::{Kind, evidence};

use crate::common::pty::{PtyOptions, run_in_pty};
use crate::common::scripted::{Logged, ScriptedDaemon};
use crate::common::{detail, envelope, golden, run};
use crate::trust_data::{self, Script};

const STATEMENT: &str = "approval-statement.txt";

fn serving(script: Script) -> (TestRoot, ScriptedDaemon, Arc<Mutex<Script>>) {
    let root = TestRoot::new().expect("a test root is made");
    let script = Arc::new(Mutex::new(script));
    let daemon = ScriptedDaemon::serve(&root, trust_data::provider(Arc::clone(&script)));
    (root, daemon, script)
}

/// The methods the daemon read, without the handshakes.
fn calls(daemon: &ScriptedDaemon) -> Vec<String> {
    daemon
        .methods()
        .into_iter()
        .filter(|method| method != "daemon.handshake")
        .collect()
}

/// The logged request of `method`.
fn request(daemon: &ScriptedDaemon, method: &str) -> Logged {
    daemon
        .log()
        .into_iter()
        .find(|logged| logged.method == method)
        .unwrap_or_else(|| panic!("{method} was sent"))
}

fn statement() -> String {
    trust_data::read(STATEMENT)
        .trim_end_matches('\n')
        .to_owned()
}

/// AC-14.
#[test]
fn a_dry_run_previews_an_absolute_path_and_writes_the_preview() {
    let ev = evidence!(
        covers = ["REQ-028", "AC-028"],
        kind = Kind::Fixture,
        expected = "provider import <relative dir> --dry-run sends one provider.preview_import with the folder made absolute and without dot segments, writes the preview with publisher not verified, permissions, quarantine note, and import footnote, exits 0, and writes the ImportPreview itself with --json",
    );
    let (root, daemon, _) = serving(Script::default());
    let work = root.path().join("work");
    std::fs::create_dir_all(work.join("pkgs")).unwrap();

    let mut command = crate::common::stackly(root.path());
    command
        .current_dir(&work)
        .args(["provider", "import", "./pkgs/../acme-demo", "--dry-run"]);
    let text = crate::common::run_command(command);
    let json = run(
        root.path(),
        &[
            "provider",
            "import",
            "/srv/acme-demo",
            "--dry-run",
            "--json",
        ],
    );

    assert_eq!(text.code, Some(0), "{text:?}");
    assert_eq!(text.stderr, "");
    golden::check("provider_import_dry_run.txt", &text.stdout);
    assert!(text.stdout.contains("acme (not verified)"));
    let sent = request(&daemon, "provider.preview_import");
    let path = sent.raw["call"]["content"]["path"]
        .as_str()
        .expect("a path");
    assert!(
        path.starts_with('/') && path.ends_with("/work/acme-demo"),
        "{path}"
    );
    assert!(!path.contains("/./") && !path.contains(".."), "{path}");
    assert_eq!(json.code, Some(0), "{json:?}");
    let value = envelope(&json);
    assert_eq!(value["command"], "provider import");
    assert_eq!(
        value["result"],
        serde_json::to_value(trust_data::preview("pending")).unwrap()
    );
    assert_eq!(
        calls(&daemon),
        ["provider.preview_import", "provider.preview_import"]
    );
    ev.pass();
}

/// AC-14.
#[test]
fn a_dry_run_of_a_package_that_cannot_be_imported_exits_2_and_keeps_the_preview() {
    let ev = evidence!(
        covers = ["REQ-028", "AC-028"],
        kind = Kind::Fixture,
        expected = "provider import --dry-run of the invalid preview writes the preview on stdout, the warning finding and the namespace_not_allowed issue on stderr, exits 2 with cli.provider_invalid (1 error, 1 warning), and keeps the ImportPreview as result with --json",
    );
    let (root, daemon, _) = serving(Script {
        preview: trust_data::preview("invalid"),
        ..Script::default()
    });

    let text = run(
        root.path(),
        &["provider", "import", "/srv/official-demo", "--dry-run"],
    );
    let json = run(
        root.path(),
        &[
            "provider",
            "import",
            "/srv/official-demo",
            "--dry-run",
            "--json",
        ],
    );

    assert_eq!(text.code, Some(2), "{text:?}");
    golden::check("provider_import_dry_run_invalid.txt", &text.stdout);
    golden::check("provider_import_dry_run_invalid_stderr.txt", &text.stderr);
    assert!(text.stderr.contains(
        "error: provider.yaml metadata.id: namespace_not_allowed: namespace official can't be imported"
    ));
    let value = envelope(&json);
    assert_eq!(value["error"]["code"], "cli.provider_invalid");
    assert_eq!(detail(&value["error"], "errors").as_deref(), Some("1"));
    assert_eq!(detail(&value["error"], "warnings").as_deref(), Some("1"));
    assert_eq!(value["result"]["verdict"]["importable"], false);
    assert_eq!(json.code, Some(2));
    assert_eq!(
        calls(&daemon),
        ["provider.preview_import", "provider.preview_import"]
    );
    ev.pass();
}

/// AC-14, AC-3.
#[test]
fn import_sends_the_previewed_digest_with_a_key_and_writes_the_entry() {
    let ev = evidence!(
        covers = ["REQ-028", "AC-028", "REQ-033"],
        kind = Kind::Fixture,
        expected = "provider import previews, writes the preview to stderr, sends provider.import with expectedPackageSha256 from the preview and an idempotency key, asks nothing, writes Imported <name>. Approve it before you install anything. with the entry rows, and the ProviderDetail with --json",
    );
    let (root, daemon, _) = serving(Script::default());

    let text = run(root.path(), &["provider", "import", "/srv/acme-demo"]);
    let json = run(
        root.path(),
        &["provider", "import", "/srv/acme-demo", "--json"],
    );

    assert_eq!(text.code, Some(0), "{text:?}");
    golden::check("provider_import.txt", &text.stdout);
    assert!(text.stderr.contains("Permissions:"), "{}", text.stderr);
    assert!(!text.stderr.contains("[y/N]"));
    let sent = request(&daemon, "provider.import");
    assert_eq!(
        sent.raw["call"]["content"]["expectedPackageSha256"],
        "f".repeat(64),
        "{:?}",
        sent.raw
    );
    assert_eq!(sent.raw["call"]["content"]["path"], "/srv/acme-demo");
    assert!(sent.idempotency_key.is_some(), "an idempotency key");
    assert_eq!(json.code, Some(0), "{json:?}");
    golden::check("provider_import.json", &json.stdout);
    let value = envelope(&json);
    assert_eq!(
        value["result"],
        serde_json::to_value(trust_data::local("pending")).unwrap()
    );
    assert_eq!(
        calls(&daemon),
        [
            "provider.preview_import",
            "provider.import",
            "provider.preview_import",
            "provider.import"
        ]
    );
    ev.pass();
}

/// AC-14, AC-3.
#[test]
fn importing_the_stored_bytes_again_says_already_imported() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "provider import of a preview with samePackage and storedCopyValid writes Already imported. and exits 0 without printing the preview",
    );
    let (root, _daemon, _) = serving(Script {
        preview: trust_data::preview_same(),
        imported: trust_data::local("approved"),
        ..Script::default()
    });

    let text = run(root.path(), &["provider", "import", "/srv/acme-demo"]);

    assert_eq!(text.code, Some(0), "{text:?}");
    assert_eq!(text.stdout.lines().next(), Some("Already imported."));
    assert_eq!(text.stderr, "");
    ev.pass();
}

/// AC-14, AC-2.
#[test]
fn a_package_that_cannot_be_imported_stops_after_the_preview() {
    let ev = evidence!(
        covers = ["REQ-028", "AC-028"],
        kind = Kind::Fixture,
        expected = "provider import of the invalid preview exits 2 with cli.provider_invalid and its lines on stderr, and of a preview the import limit blocks exits 3 with provider.import_limit and its hint; neither sends provider.import",
    );
    let (root, daemon, script) = serving(Script {
        preview: trust_data::preview("invalid"),
        ..Script::default()
    });

    let invalid = run(root.path(), &["provider", "import", "/srv/x"]);
    script.lock().unwrap().preview = trust_data::preview_limit();
    let limit = run(root.path(), &["provider", "import", "/srv/x", "--json"]);

    assert_eq!(invalid.code, Some(2), "{invalid:?}");
    assert!(invalid.stderr.contains("cli.provider_invalid"));
    assert!(invalid.stderr.contains("namespace_not_allowed"));
    assert_eq!(limit.code, Some(3), "{limit:?}");
    let value = envelope(&limit);
    assert_eq!(value["error"]["code"], "provider.import_limit");
    assert_eq!(value["error"]["hint"], "remove an imported provider first");
    assert_eq!(
        calls(&daemon),
        ["provider.preview_import", "provider.preview_import"]
    );
    ev.pass();
}

/// AC-14, K6.
#[test]
fn import_approve_needs_yes_before_the_import_is_sent() {
    let ev = evidence!(
        covers = ["REQ-028", "AC-028", "REQ-032"],
        kind = Kind::Fixture,
        expected = "provider import --approve without --yes and without a terminal, and with --json, exits 2 with cli.confirmation_required naming --yes after the preview and before provider.import is sent",
    );
    let (root, daemon, _) = serving(Script::default());

    let text = run(root.path(), &["provider", "import", "/srv/x", "--approve"]);
    let json = run(
        root.path(),
        &["provider", "import", "/srv/x", "--approve", "--json"],
    );

    assert_eq!(text.code, Some(2), "{text:?}");
    assert!(text.stderr.contains("cli.confirmation_required"));
    let value = envelope(&json);
    assert_eq!(value["error"]["code"], "cli.confirmation_required");
    assert_eq!(detail(&value["error"], "flag").as_deref(), Some("--yes"));
    assert_eq!(
        calls(&daemon),
        ["provider.preview_import", "provider.preview_import"]
    );
    ev.pass();
}

/// AC-14, K6.
#[test]
fn import_approve_yes_imports_then_approves_the_imported_bytes() {
    let ev = evidence!(
        covers = ["REQ-028", "AC-028", "REQ-032"],
        kind = Kind::Fixture,
        expected = "provider import --approve --yes --json sends provider.preview_import, provider.import, and provider.approve with the packageSha256 of the imported entry and its own idempotency key, and writes the approved ProviderDetail",
    );
    let (root, daemon, _) = serving(Script::default());

    let json = run(
        root.path(),
        &[
            "provider",
            "import",
            "/srv/x",
            "--approve",
            "--yes",
            "--json",
        ],
    );

    assert_eq!(json.code, Some(0), "{json:?}");
    golden::check("provider_import_approve.json", &json.stdout);
    let value = envelope(&json);
    assert_eq!(value["result"]["summary"]["approval"], "approved");
    assert_eq!(
        calls(&daemon),
        [
            "provider.preview_import",
            "provider.import",
            "provider.approve"
        ]
    );
    let import = request(&daemon, "provider.import");
    let approve = request(&daemon, "provider.approve");
    assert_eq!(approve.raw["call"]["content"]["provider"], "acme/demo");
    assert_eq!(
        approve.raw["call"]["content"]["expectedPackageSha256"],
        "f".repeat(64)
    );
    assert_ne!(import.idempotency_key, approve.idempotency_key);
    ev.pass();
}

/// AC-14, AC-11.
#[test]
fn approve_with_yes_prints_the_permissions_and_the_statement_exactly() {
    let ev = evidence!(
        covers = ["REQ-028", "AC-028", "REQ-032", "AC-061"],
        kind = Kind::Fixture,
        expected = "provider approve acme/demo --yes reads catalog.get_provider, writes the identity line, the permissions, the approval statement of approval-statement.txt exactly, and the footnote to stderr, sends provider.approve with local.packageSha256, and writes Approved <name>. with the entry rows",
    );
    let (root, daemon, _) = serving(Script::default());

    let text = run(root.path(), &["provider", "approve", "acme/demo", "--yes"]);

    assert_eq!(text.code, Some(0), "{text:?}");
    golden::check("provider_approve.txt", &text.stdout);
    golden::check("provider_approve_stderr.txt", &text.stderr);
    assert!(
        text.stderr.lines().any(|line| line == statement()),
        "the statement line in:\n{}",
        text.stderr
    );
    assert_eq!(calls(&daemon), ["catalog.get_provider", "provider.approve"]);
    let approve = request(&daemon, "provider.approve");
    assert_eq!(
        approve.raw["call"]["content"]["expectedPackageSha256"],
        "f".repeat(64)
    );
    ev.pass();
}

/// AC-14.
#[test]
fn approve_without_yes_and_without_a_terminal_sends_no_mutation() {
    let ev = evidence!(
        covers = ["REQ-028", "AC-028"],
        kind = Kind::Fixture,
        expected = "provider approve, revoke, and remove without --yes, noninteractive or with --json, exit 2 with cli.confirmation_required and send only catalog.get_provider",
    );
    let (root, daemon, script) = serving(Script::default());

    let approve = run(root.path(), &["provider", "approve", "acme/demo"]);
    let approve_json = run(root.path(), &["provider", "approve", "acme/demo", "--json"]);
    script.lock().unwrap().detail = trust_data::local("approved");
    let revoke = run(root.path(), &["provider", "revoke", "acme/demo"]);
    let remove = run(root.path(), &["provider", "remove", "acme/demo", "--json"]);

    for out in [&approve, &approve_json, &revoke, &remove] {
        assert_eq!(out.code, Some(2), "{out:?}");
    }
    assert!(approve.stderr.contains("cli.confirmation_required"));
    assert_eq!(
        envelope(&approve_json)["error"]["code"],
        "cli.confirmation_required"
    );
    assert_eq!(
        envelope(&remove)["error"]["code"],
        "cli.confirmation_required"
    );
    assert_eq!(calls(&daemon), ["catalog.get_provider"; 4]);
    ev.pass();
}

/// AC-14, AC-11.
#[test]
fn approve_at_a_terminal_asks_and_only_yes_approves() {
    let ev = evidence!(
        covers = ["REQ-028", "AC-028", "AC-061"],
        kind = Kind::Fixture,
        expected = "provider approve acme/demo at a terminal shows the statement and Approve acme/demo? [y/N]; y sends provider.approve and exits 0, n exits 9 with cli.confirmation_declined and sends no mutation",
    );
    let (root, daemon, _) = serving(Script::default());
    let options = |input| PtyOptions {
        stdin_tty: true,
        input: Some(input),
        ..PtyOptions::default()
    };

    let no = run_in_pty(
        root.path(),
        &["provider", "approve", "acme/demo"],
        &options("n\n"),
    );
    assert_eq!(calls(&daemon), ["catalog.get_provider"]);
    let yes = run_in_pty(
        root.path(),
        &["provider", "approve", "acme/demo"],
        &options("y\n"),
    );

    assert_eq!(no.code, Some(9), "{no:?}");
    assert!(
        no.stderr.contains("cli.confirmation_declined"),
        "{}",
        no.stderr
    );
    assert_eq!(yes.code, Some(0), "{yes:?}");
    for run in [&no, &yes] {
        assert!(run.stderr.contains(&statement()), "{}", run.stderr);
        assert!(
            run.stderr.contains("Approve acme/demo?\n[y/N] "),
            "{}",
            run.stderr
        );
    }
    assert_eq!(
        calls(&daemon),
        [
            "catalog.get_provider",
            "catalog.get_provider",
            "provider.approve"
        ]
    );
    ev.pass();
}

/// AC-14.
#[test]
fn an_entry_already_approved_for_its_bytes_needs_no_request() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "provider approve of an entry whose approval is approved writes <name> is already approved. and exits 0 without asking and without provider.approve",
    );
    let (root, daemon, _) = serving(Script {
        detail: trust_data::local("approved"),
        ..Script::default()
    });

    let text = run(root.path(), &["provider", "approve", "acme/demo"]);

    assert_eq!(text.code, Some(0), "{text:?}");
    assert_eq!(
        text.stdout.lines().next(),
        Some("Acme Demo is already approved.")
    );
    assert_eq!(calls(&daemon), ["catalog.get_provider"]);
    ev.pass();
}

/// AC-14.
#[test]
fn a_catalog_entry_has_no_approval_to_change() {
    let ev = evidence!(
        covers = ["REQ-028", "AC-028"],
        kind = Kind::Fixture,
        expected = "provider approve, revoke, and remove of official/nats exit 10 with provider.import_not_found reason not_local and its hint, before asking and without a mutation",
    );
    let (root, daemon, _) = serving(Script::default());

    for verb in ["approve", "revoke", "remove"] {
        let json = run(
            root.path(),
            &["provider", verb, "official/nats", "--yes", "--json"],
        );
        assert_eq!(json.code, Some(10), "{verb}: {json:?}");
        let value = envelope(&json);
        assert_eq!(value["error"]["code"], "provider.import_not_found");
        assert_eq!(
            detail(&value["error"], "reason").as_deref(),
            Some("not_local")
        );
        assert_eq!(
            value["error"]["hint"],
            "this provider comes from the catalog and has no approval to change"
        );
    }
    assert_eq!(calls(&daemon), ["catalog.get_provider"; 3]);
    ev.pass();
}

/// AC-14, AC-7.
#[test]
fn revoke_with_yes_prints_what_it_does_and_writes_the_revoked_entry() {
    let ev = evidence!(
        covers = ["REQ-028", "AC-028", "AC-072"],
        kind = Kind::Fixture,
        expected = "provider revoke acme/demo --yes writes the revoke message to stderr, sends provider.revoke with an idempotency key, writes Revoked the approval for <name>. Existing instances can still start. with approval revoked, and the revoked ProviderDetail with --json",
    );
    let (root, daemon, _) = serving(Script {
        detail: trust_data::local("approved"),
        ..Script::default()
    });

    let text = run(root.path(), &["provider", "revoke", "acme/demo", "--yes"]);
    let json = run(
        root.path(),
        &["provider", "revoke", "acme/demo", "--yes", "--json"],
    );

    assert_eq!(text.code, Some(0), "{text:?}");
    golden::check("provider_revoke.txt", &text.stdout);
    assert!(
        text.stderr
            .contains("Existing instances, their data, and installed runtimes stay as they are")
    );
    assert_eq!(json.code, Some(0), "{json:?}");
    assert_eq!(json.stderr, "");
    golden::check("provider_revoke.json", &json.stdout);
    assert!(
        request(&daemon, "provider.revoke")
            .idempotency_key
            .is_some()
    );
    assert_eq!(
        calls(&daemon),
        [
            "catalog.get_provider",
            "provider.revoke",
            "catalog.get_provider",
            "provider.revoke"
        ]
    );
    ev.pass();
}

/// AC-14, AC-9.
#[test]
fn remove_at_a_terminal_asks_with_its_consequence_and_writes_the_removed_id() {
    let ev = evidence!(
        covers = ["REQ-028", "AC-028"],
        kind = Kind::Fixture,
        expected = "provider remove acme/demo at a terminal asks Remove “Acme Demo” from the catalog? with the remove message, and on y sends provider.remove; with --yes --json it writes the RemovedProvider; an invalid local entry can be removed too",
    );
    let (root, daemon, script) = serving(Script::default());

    let asked = run_in_pty(
        root.path(),
        &["provider", "remove", "acme/demo"],
        &PtyOptions {
            stdin_tty: true,
            input: Some("yes\n"),
            ..PtyOptions::default()
        },
    );
    script.lock().unwrap().detail = trust_data::local_invalid();
    let json = run(
        root.path(),
        &["provider", "remove", "acme/demo", "--yes", "--json"],
    );

    assert_eq!(asked.code, Some(0), "{asked:?}");
    assert!(
        asked
            .stderr
            .contains("Remove “Acme Demo” from the catalog?")
    );
    assert!(
        asked
            .stderr
            .contains("The folder you imported from is not touched.")
    );
    golden::check("provider_remove.txt", &asked.stdout);
    assert_eq!(json.code, Some(0), "{json:?}");
    golden::check("provider_remove.json", &json.stdout);
    assert_eq!(
        envelope(&json)["result"],
        serde_json::json!({ "provider": "acme/demo" })
    );
    assert_eq!(
        calls(&daemon),
        [
            "catalog.get_provider",
            "provider.remove",
            "catalog.get_provider",
            "provider.remove"
        ]
    );
    ev.pass();
}

/// AC-14: the exit code of every provider code comes from its category.
#[test]
fn every_provider_code_exits_with_the_code_of_its_category() {
    let ev = evidence!(
        covers = ["REQ-028", "AC-028"],
        kind = Kind::Fixture,
        expected = "provider.approval_required and provider.revoked exit 4; provider.import_changed, provider.import_limit, and provider.not_approved exit 3; manifest.invalid and provider.import_unreadable exit 2; provider.store_failed exits 7; provider.import_not_found exits 10, each from the daemon's category",
    );
    let (root, _daemon, script) = serving(Script::default());

    for (code, category, exit) in [
        ("provider.approval_required", "unauthorized", 4),
        ("provider.revoked", "unauthorized", 4),
        ("provider.import_changed", "conflict", 3),
        ("provider.import_limit", "conflict", 3),
        ("provider.not_approved", "conflict", 3),
        ("manifest.invalid", "invalid_input", 2),
        ("provider.import_unreadable", "invalid_input", 2),
        ("provider.store_failed", "failed", 7),
        ("provider.import_not_found", "not_found", 10),
    ] {
        script.lock().unwrap().fail = Some(trust_data::daemon_error(code, category));
        let import = run(root.path(), &["provider", "import", "/srv/x", "--json"]);
        let revoke = run(
            root.path(),
            &["provider", "revoke", "acme/demo", "--yes", "--json"],
        );
        for out in [&import, &revoke] {
            assert_eq!(out.code, Some(exit), "{code}: {out:?}");
            assert_eq!(envelope(out)["error"]["code"], code);
        }
    }
    ev.pass();
}

/// AC-14, AC-11.
#[test]
fn list_shows_trust_approval_imported_count_and_catalog_revocations() {
    let ev = evidence!(
        covers = ["REQ-028", "AC-061", "AC-072"],
        kind = Kind::Fixture,
        expected = "provider list with three local entries writes , 3 imported in the catalog line, a TRUST column with official and local, STATE needs approval, available, and revoked for the pending, approved, and revoked entries, and the Revoked by this catalog (revision 1, published 2026-10-20) section; --json adds revocations",
    );
    let (root, _daemon, _) = serving(Script::default());

    let text = run(root.path(), &["provider", "list"]);
    let json = run(root.path(), &["provider", "list", "--json"]);

    assert_eq!(text.code, Some(0), "{text:?}");
    golden::check("provider_list_local.txt", &text.stdout);
    assert!(text.stdout.lines().next().unwrap().contains(", 3 imported"));
    assert!(
        text.stdout
            .contains("Revoked by this catalog (revision 1, published 2026-10-20):")
    );
    let value = envelope(&json);
    assert_eq!(
        value["result"]["revocations"][0]["provider"],
        "official/nats"
    );
    assert_eq!(value["result"]["catalog"]["localCount"], 3);
    ev.pass();
}

/// AC-14, AC-11, AC-17.
#[test]
fn inspect_of_a_local_entry_shows_trust_approval_import_and_permissions() {
    let ev = evidence!(
        covers = ["REQ-028", "AC-061", "REQ-032"],
        kind = Kind::Fixture,
        expected = "provider inspect of a local entry writes publisher <ns> (not verified), trust local, not verified, the approval with its decision time, the import time and folder, the package digest, the permissions block, and Changes since your decision when pending after a decision; a revoked entry adds the revocation; an invalid one says the permissions are not available; --json is the ProviderDetail itself",
    );
    let (root, _daemon, script) = serving(Script::default());
    let mut outputs = Vec::new();
    for (name, detail) in [
        ("pending", trust_data::local("pending")),
        ("changed", trust_data::local_changed()),
        ("approved", trust_data::local("approved")),
        ("revoked", trust_data::local("revoked")),
        ("invalid", trust_data::local_invalid()),
    ] {
        script.lock().unwrap().detail = detail.clone();
        let text = run(root.path(), &["provider", "inspect", "acme/demo"]);
        assert_eq!(text.code, Some(0), "{name}: {text:?}");
        golden::check(&format!("provider_inspect_local_{name}.txt"), &text.stdout);
        outputs.push((name, text.stdout));
        let json = run(root.path(), &["provider", "inspect", "acme/demo", "--json"]);
        assert_eq!(
            envelope(&json)["result"],
            serde_json::to_value(&detail).unwrap(),
            "{name}"
        );
    }
    for (name, stdout) in &outputs {
        assert!(stdout.contains("acme (not verified)"), "{name}");
        assert!(stdout.contains("local, not verified"), "{name}");
    }
    assert!(
        outputs[1]
            .1
            .contains("Changes since your decision: revision, files it downloads")
    );
    assert!(outputs[3].1.contains("by you on"));
    assert!(outputs[4].1.contains("Permissions: not available"));
    ev.pass();
}
