//! `provider list` and `inspect` against a real `stacklyd` (spec 0017 AC-9):
//! the daemon serves the catalog `cargo xtask catalog build` writes from
//! `providers/` through `STACKLY_CATALOG_DIR`, and the real `stackly`
//! binary lists and inspects it. The assertions hold for whatever the
//! catalog holds, so a provider that moves from planned to available does
//! not break them.

use std::process::Command;
use std::time::{Duration, Instant};

use serde_json::Value;
use stackly_testkit::evidence::{Kind, evidence};
use stackly_testkit::{ReapGuard, SpawnCatalog, SpawnedDaemon, TestRoot, cargo_bin};

use crate::common::stacklyd::{closed_connections, handshakes};
use crate::common::{detail, envelope, paths, run};

/// Builds the catalog of `providers/` into a folder of `root`.
fn built_catalog(root: &TestRoot) -> std::path::PathBuf {
    let dir = root.path().join("catalog");
    let output = Command::new(cargo_bin("xtask", "xtask"))
        .current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
        .args(["catalog", "build", "--out"])
        .arg(&dir)
        .output()
        .expect("xtask runs");
    assert!(
        output.status.success(),
        "the catalog build failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    dir
}

/// Starts `stacklyd` with `catalog` and waits until it is ready and quiet,
/// like `common::stacklyd::start_daemon`, which always gives the empty
/// catalog.
fn start(root: &TestRoot, catalog: SpawnCatalog) -> SpawnedDaemon {
    let paths = paths(root);
    let closed_before = closed_connections(&paths.log);
    let daemon = SpawnedDaemon::start_with_catalog(
        &cargo_bin("stacklyd", "stacklyd"),
        root.path(),
        &paths.socket,
        &paths.runtime,
        catalog,
        |daemon| daemon.stderr().contains("registry_state") && handshakes(daemon.socket()),
    )
    .expect("stacklyd starts");
    let deadline = Instant::now() + Duration::from_secs(5);
    while closed_connections(&paths.log) <= closed_before {
        assert!(
            Instant::now() < deadline,
            "stacklyd did not log the close of the ready probe: {}",
            daemon.stderr()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    daemon
}

/// AC-9.
#[test]
fn a_real_daemon_lists_and_inspects_the_catalog_it_was_given() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "against a real stacklyd with the catalog of providers/ as STACKLY_CATALOG_DIR, provider list names a development catalog, --category keeps only that category, inspect of a listed ID returns the summary the list has, and a bare name or a missing ID exits 10 with the daemon's hint and suggestion",
    );
    let root = TestRoot::new().expect("a test root");
    let dir = built_catalog(&root);
    let _reap = ReapGuard::new(root.path(), &paths(&root).runtime);
    let daemon = start(&root, SpawnCatalog::Dir(dir));

    let list = run(root.path(), &["provider", "list", "--json"]);
    let text = run(root.path(), &["provider", "list"]);

    assert_eq!(list.code, Some(0), "{list:?}");
    let listed = envelope(&list);
    let catalog = &listed["result"]["catalog"];
    assert_eq!(catalog["source"], "development");
    assert_eq!(catalog["trust"], "development");
    assert_eq!(catalog["host"]["os"], "macos");
    let items = listed["result"]["items"].as_array().expect("items");
    assert!(!items.is_empty(), "the catalog of providers/ has entries");
    assert!(
        text.stdout.starts_with("Catalog: development, revision "),
        "{}",
        text.stdout
    );
    assert_eq!(
        text.stdout.lines().count(),
        items.len() + 2,
        "one catalog line, one header, one row per provider: {}",
        text.stdout
    );

    let first = &items[0];
    let id = first["provider"].as_str().expect("an ID");
    let category = first["category"].as_str().expect("a category");
    let filtered = envelope(&run(
        root.path(),
        &["provider", "list", "--category", category, "--json"],
    ));
    let expected: Vec<&Value> = items
        .iter()
        .filter(|item| item["category"] == first["category"])
        .collect();
    assert_eq!(
        filtered["result"]["items"]
            .as_array()
            .expect("items")
            .iter()
            .collect::<Vec<_>>(),
        expected
    );

    let inspected = run(root.path(), &["provider", "inspect", id, "--json"]);
    assert_eq!(inspected.code, Some(0), "{inspected:?}");
    assert_eq!(&envelope(&inspected)["result"]["summary"], first);
    let text = run(root.path(), &["provider", "inspect", id]).stdout;
    assert!(
        text.lines()
            .any(|line| line.starts_with("ID ") && line.ends_with(id)),
        "{text}"
    );

    let (namespace, name) = id.split_once('/').expect("an ID has a namespace");
    let bare = run(root.path(), &["provider", "inspect", name, "--json"]);
    assert_eq!(bare.code, Some(10), "{bare:?}");
    let bare = envelope(&bare);
    assert_eq!(bare["error"]["code"], "catalog.provider_not_found");
    assert_eq!(
        detail(&bare["error"], "suggestion").as_deref(),
        Some(id),
        "{namespace}"
    );
    assert_eq!(
        bare["error"]["hint"],
        "use the full provider ID, including its namespace"
    );
    let missing = run(
        root.path(),
        &["provider", "inspect", "official/no-such", "--json"],
    );
    assert_eq!(missing.code, Some(10), "{missing:?}");
    assert_eq!(
        envelope(&missing)["error"]["hint"],
        "run stackly provider list to see the catalog"
    );
    let invalid = run(
        root.path(),
        &["provider", "inspect", "Official/NATS", "--json"],
    );
    assert_eq!(invalid.code, Some(2), "{invalid:?}");
    assert_eq!(
        envelope(&invalid)["error"]["code"],
        "request.invalid_params"
    );
    assert_eq!(daemon.stop().unwrap().code(), Some(0));
    ev.pass();
}

/// AC-9: a daemon without a catalog answers `catalog.unavailable`, and the
/// CLI exits 6 for both verbs with the daemon's hint.
#[test]
fn a_real_daemon_without_a_catalog_exits_6_with_its_hint() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "against a real stacklyd started without STACKLY_CATALOG_DIR, provider list and provider inspect exit 6 with catalog.unavailable and the hint that names STACKLY_CATALOG_DIR",
    );
    let root = TestRoot::new().expect("a test root");
    let _reap = ReapGuard::new(root.path(), &paths(&root).runtime);
    let daemon = start(&root, SpawnCatalog::Unset);

    let list = run(root.path(), &["provider", "list", "--json"]);
    let inspect = run(
        root.path(),
        &["provider", "inspect", "official/nats", "--json"],
    );

    for (verb, run) in [("list", &list), ("inspect", &inspect)] {
        assert_eq!(run.code, Some(6), "{verb}: {run:?}");
        let value = envelope(run);
        assert_eq!(value["error"]["code"], "catalog.unavailable", "{verb}");
        assert_eq!(
            value["error"]["hint"], "run stacklyd from Stackly.app or set STACKLY_CATALOG_DIR",
            "{verb}"
        );
        assert_eq!(
            detail(&value["error"], "reason").as_deref(),
            Some("not_configured")
        );
    }
    assert_eq!(daemon.stop().unwrap().code(), Some(0));
    ev.pass();
}
