//! `stackly provider import`, `list`, `inspect`, and `remove` against a
//! real `stacklyd` (spec 0037 AC-14): the daemon reads a copy of the
//! `acme-demo` fixture package in the test root, and the real `stackly`
//! binary previews, imports, lists, inspects, and removes it.

use std::path::{Path, PathBuf};

use stackly_testkit::evidence::{Kind, evidence};
use stackly_testkit::{ReapGuard, TestRoot};

use crate::common::stacklyd::start_daemon;
use crate::common::{envelope, listing, paths, run};

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
