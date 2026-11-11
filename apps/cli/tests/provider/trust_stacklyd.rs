//! `stackly provider import`, `list`, `inspect`, and `remove` against a
//! real `stacklyd` (spec 0037 AC-14): the daemon reads a copy of the
//! `acme-demo` fixture package in the test root, and the real `stackly`
//! binary previews, imports, lists, inspects, and removes it.

use std::path::{Path, PathBuf};

use stackly_testkit::evidence::{Kind, evidence};
use stackly_testkit::{ReapGuard, TestRoot};

use crate::common::stacklyd::start_daemon;
use crate::common::{detail, envelope, listing, paths, run};

fn fixture_package() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/core/tests/fixtures/packages/acme-demo")
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

/// AC-14, AC-3, AC-9.
#[test]
fn a_real_daemon_imports_lists_inspects_and_removes_a_package_folder() {
    let ev = evidence!(
        covers = ["REQ-028", "AC-028", "REQ-033"],
        kind = Kind::Fixture,
        expected = "against a real stacklyd, provider import --dry-run of a copy of acme-demo exits 0 without writing, import exits 0 with a pending local entry, a second import writes Already imported., list shows acme/demo with TRUST local and STATE needs approval, inspect shows publisher acme (not verified) and the permissions, remove --yes removes it, and the package folder is the same before and after",
    );
    let root = TestRoot::new().expect("a test root");
    let _reap = ReapGuard::new(root.path(), &paths(&root).runtime);
    let _daemon = start_daemon(&root);
    let package = root.path().join("work/acme-demo");
    copy_dir(&fixture_package(), &package);
    let before = listing(&package);
    let dir = package.to_str().expect("a UTF-8 path");

    let dry = run(
        root.path(),
        &["provider", "import", dir, "--dry-run", "--json"],
    );
    let empty = run(root.path(), &["provider", "list", "--json"]);
    let imported = run(root.path(), &["provider", "import", dir, "--json"]);
    let again = run(root.path(), &["provider", "import", dir]);
    let list = run(root.path(), &["provider", "list"]);
    let inspect = run(root.path(), &["provider", "inspect", "acme/demo"]);
    let removed = run(
        root.path(),
        &["provider", "remove", "acme/demo", "--yes", "--json"],
    );
    let gone = run(root.path(), &["provider", "inspect", "acme/demo", "--json"]);

    assert_eq!(dry.code, Some(0), "{dry:?}");
    let preview = envelope(&dry);
    assert_eq!(preview["result"]["provider"], "acme/demo");
    assert_eq!(preview["result"]["verdict"]["importable"], true);
    assert_eq!(envelope(&empty)["result"]["catalog"]["localCount"], 0);
    assert_eq!(imported.code, Some(0), "{imported:?}");
    let detail = envelope(&imported);
    assert_eq!(detail["result"]["summary"]["trust"], "local");
    assert_eq!(detail["result"]["summary"]["approval"], "pending");
    assert_eq!(
        detail["result"]["local"]["packageSha256"],
        preview["result"]["packageSha256"]
    );
    assert_eq!(again.code, Some(0), "{again:?}");
    assert_eq!(again.stdout.lines().next(), Some("Already imported."));
    assert_eq!(list.code, Some(0), "{list:?}");
    let row = list
        .stdout
        .lines()
        .find(|line| line.starts_with("acme/demo "))
        .unwrap_or_else(|| panic!("acme/demo in:\n{}", list.stdout));
    assert!(
        row.contains("needs approval") && row.contains("local"),
        "{row}"
    );
    assert!(list.stdout.lines().next().unwrap().contains(", 1 imported"));
    assert_eq!(inspect.code, Some(0), "{inspect:?}");
    for needle in [
        "acme (not verified)",
        "local, not verified",
        "needs approval",
        "Permissions:",
    ] {
        assert!(
            inspect.stdout.contains(needle),
            "{needle} in:\n{}",
            inspect.stdout
        );
    }
    assert_eq!(removed.code, Some(0), "{removed:?}");
    assert_eq!(
        envelope(&removed)["result"],
        serde_json::json!({ "provider": "acme/demo" })
    );
    assert_eq!(gone.code, Some(10), "{gone:?}");
    assert_eq!(listing(&package), before, "the package folder is untouched");
    ev.pass();
}

/// A copy of the fake package under the `acme` namespace, in a folder named
/// after its ID, so it imports as a local entry of the software the seeded
/// fake runtime runs. A fixture artifact belongs only to `testing`, so the
/// copy lists an available one that no test downloads.
fn local_fake(root: &Path) -> PathBuf {
    let from = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../providers/fake");
    let to = root.join("work/acme/fake");
    copy_dir(&from, &to);
    let _ = std::fs::remove_file(to.join("README.md"));
    let manifest = to.join("provider.yaml");
    let text = std::fs::read_to_string(&manifest).expect("the manifest reads");
    let text = text
        .replace("id: testing/fake", "id: acme/fake")
        .replace("category: testing", "category: database")
        .replace("displayName: Fake Provider", "displayName: Acme Fake");
    std::fs::write(&manifest, text).expect("the manifest writes");
    std::fs::write(
        to.join("versions.json"),
        r#"{
  "schema": "stackly.provider-versions/v1",
  "software": "fake",
  "artifacts": [
    {
      "id": "fake-0.1.0-macos-aarch64",
      "line": "0.1",
      "version": "0.1.0",
      "os": "macos",
      "arch": "aarch64",
      "status": "available",
      "minOs": "13.0",
      "format": "tar_gz",
      "urls": ["https://downloads.example.test/fake/fake-0.1.0-macos-aarch64.tar.gz"],
      "sha256": "1111111111111111111111111111111111111111111111111111111111111111",
      "size": 1048576,
      "signingPolicy": "keep_or_adhoc",
      "license": "MIT",
      "licenseUrl": "https://downloads.example.test/fake/LICENSE",
      "executables": ["stackly-fake-server"],
      "stable": true
    }
  ]
}
"#,
    )
    .expect("the versions write");
    to
}

/// AC-14, AC-5, AC-7.
#[test]
fn a_real_daemon_approves_and_revokes_and_a_revoke_closes_create_but_not_start() {
    let ev = evidence!(
        covers = ["REQ-028", "AC-028", "REQ-032", "AC-072"],
        kind = Kind::Fixture,
        expected = "against a real stacklyd, revoke of a pending acme/fake exits 3 with provider.not_approved, approve --yes makes it approved, an instance created from it starts, revoke --yes makes it revoked, a new instance create exits 4 with provider.revoked, the existing instance still starts, and import --approve --yes of acme-demo ends approved in one command",
    );
    let root = TestRoot::new().expect("a test root");
    let runtime = crate::common::stacklyd::seed(&root);
    let _reap = ReapGuard::new(root.path(), &paths(&root).runtime);
    let daemon = start_daemon(&root);
    let fake = local_fake(root.path());
    let fake = fake.to_str().expect("a UTF-8 path");
    let create = |name: &str| {
        run(
            root.path(),
            &[
                "instance",
                "create",
                name,
                "--runtime",
                &runtime,
                "--provider",
                "acme/fake",
                "--revision",
                "0.4.0",
                "--wait",
                "--timeout",
                "60s",
                "--json",
            ],
        )
    };

    let imported = run(root.path(), &["provider", "import", fake, "--json"]);
    assert_eq!(imported.code, Some(0), "{imported:?}");
    assert_eq!(
        envelope(&imported)["result"]["summary"]["approval"],
        "pending"
    );

    let unapproved = run(
        root.path(),
        &["provider", "revoke", "acme/fake", "--yes", "--json"],
    );
    assert_eq!(unapproved.code, Some(3), "{unapproved:?}");
    assert_eq!(
        envelope(&unapproved)["error"]["code"],
        "provider.not_approved"
    );

    let pending_create = create("early");
    assert_eq!(pending_create.code, Some(4), "{pending_create:?}");
    assert_eq!(
        envelope(&pending_create)["error"]["code"],
        "provider.approval_required"
    );

    let approved = run(
        root.path(),
        &["provider", "approve", "acme/fake", "--yes", "--json"],
    );
    assert_eq!(approved.code, Some(0), "{approved:?}");
    assert_eq!(
        envelope(&approved)["result"]["summary"]["approval"],
        "approved"
    );

    let created = create("before");
    assert_eq!(created.code, Some(0), "{created:?}");

    let revoked = run(
        root.path(),
        &["provider", "revoke", "acme/fake", "--yes", "--json"],
    );
    assert_eq!(revoked.code, Some(0), "{revoked:?}");
    assert_eq!(
        envelope(&revoked)["result"]["summary"]["approval"],
        "revoked"
    );

    let refused = create("after");
    assert_eq!(refused.code, Some(4), "{refused:?}");
    let error = &envelope(&refused)["error"];
    assert_eq!(error["code"], "provider.revoked");
    assert_eq!(detail(error, "by").as_deref(), Some("user"));

    let started = run(
        root.path(),
        &["instance", "start", "before", "--wait", "--timeout", "60s"],
    );
    assert_eq!(started.code, Some(0), "{started:?}");
    let stopped = run(
        root.path(),
        &["instance", "stop", "before", "--wait", "--timeout", "60s"],
    );
    assert_eq!(stopped.code, Some(0), "{stopped:?}");

    let demo = root.path().join("work/acme-demo");
    copy_dir(&fixture_package(), &demo);
    let chained = run(
        root.path(),
        &[
            "provider",
            "import",
            demo.to_str().expect("a UTF-8 path"),
            "--approve",
            "--yes",
            "--json",
        ],
    );
    assert_eq!(chained.code, Some(0), "{chained:?}");
    assert_eq!(
        envelope(&chained)["result"]["summary"]["approval"],
        "approved"
    );

    let listed = run(root.path(), &["instance", "list", "--json"]);
    let names: Vec<_> = envelope(&listed)["result"]["items"]
        .as_array()
        .expect("the list has items")
        .iter()
        .map(|item| item["name"].clone())
        .collect();
    assert_eq!(
        names,
        [serde_json::json!("before")],
        "no refused create ran"
    );
    assert_eq!(daemon.stop().unwrap().code(), Some(0));
    ev.pass();
}
