//! AC-9: `provider list` calls `catalog.list_providers`, writes the catalog
//! line and the table, filters with `--category` on the client, and writes
//! `{ catalog, items }` with `--json`.

use stackly_testkit::TestRoot;
use stackly_testkit::evidence::{Kind, evidence};

use crate::common::scripted::{ScriptedDaemon, Step, error};
use crate::common::{envelope, golden, run};
use crate::data;

fn serving(reply: stackly_protocol::catalog::ListProvidersReply) -> (TestRoot, ScriptedDaemon) {
    let root = TestRoot::new().expect("a test root is made");
    let daemon = ScriptedDaemon::serve(&root, data::catalog(reply, Vec::new()));
    (root, daemon)
}

fn ids(value: &serde_json::Value) -> Vec<&str> {
    value["items"]
        .as_array()
        .expect("items is an array")
        .iter()
        .map(|item| item["provider"].as_str().expect("a provider ID"))
        .collect()
}

/// AC-9.
#[test]
fn list_writes_the_catalog_line_the_table_and_the_json_result() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "provider list over list-providers.json writes the catalog line and the PROVIDER to LICENSE table in the daemon's order, and { catalog, items } with --json, from one catalog.list_providers call each",
    );
    let (root, daemon) = serving(data::list());

    let text = run(root.path(), &["provider", "list"]);
    let json = run(root.path(), &["provider", "list", "--json"]);

    assert_eq!(text.code, Some(0), "{text:?}");
    assert_eq!(text.stderr, "");
    golden::check("provider_list.txt", &text.stdout);
    assert_eq!(
        text.stdout.lines().next(),
        Some("Catalog: official, bundled, revision 1, published 2026-10-20 01:26 +00:00")
    );
    assert_eq!(json.code, Some(0), "{json:?}");
    golden::check("provider_list.json", &json.stdout);
    let value = envelope(&json);
    assert_eq!(value["command"], "provider list");
    let result = &value["result"];
    assert_eq!(
        result
            .as_object()
            .expect("an object")
            .keys()
            .collect::<Vec<_>>(),
        ["catalog", "items"]
    );
    assert_eq!(result["catalog"]["revision"], 1);
    assert_eq!(result["items"].as_array().expect("items").len(), 10);
    assert_eq!(
        daemon.methods(),
        [
            "daemon.handshake",
            "catalog.list_providers",
            "daemon.handshake",
            "catalog.list_providers"
        ]
    );
    ev.pass();
}

/// AC-9.
#[test]
fn category_filters_the_table_and_the_json_items_but_not_the_catalog() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "provider list --category messaging keeps the three messaging entries in text and JSON, never matches an entry without a category, and an empty category writes No providers in category <c> with exit 0",
    );
    let (root, _daemon) = serving(data::list());

    let text = run(
        root.path(),
        &["provider", "list", "--category", "messaging"],
    );
    let json = run(
        root.path(),
        &["provider", "list", "--category", "messaging", "--json"],
    );
    let none = run(root.path(), &["provider", "list", "--category", "testing"]);
    let none_json = run(
        root.path(),
        &["provider", "list", "--category", "testing", "--json"],
    );

    assert_eq!(text.code, Some(0), "{text:?}");
    golden::check("provider_list_messaging.txt", &text.stdout);
    let value = envelope(&json);
    assert_eq!(
        ids(&value["result"]),
        [
            "official/blockedhost",
            "official/intelonly",
            "official/nats"
        ]
    );
    assert_eq!(value["result"]["catalog"]["invalidCount"], 2);
    assert_eq!(none.code, Some(0), "{none:?}");
    golden::check("provider_list_category_empty.txt", &none.stdout);
    let value = envelope(&none_json);
    assert_eq!(value["result"]["items"], serde_json::json!([]));
    ev.pass();
}

/// AC-9, AC-8.
#[test]
fn a_development_catalog_older_than_180_days_says_so_in_its_line() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "the catalog line of a development catalog names development instead of official and bundled, and an outdated one adds the older than 180 days note, with no claim that a newer catalog exists",
    );
    let (root, _daemon) = serving(data::list_development_outdated());

    let text = run(root.path(), &["provider", "list"]);

    assert_eq!(text.code, Some(0), "{text:?}");
    golden::check("provider_list_development_outdated.txt", &text.stdout);
    let line = text.stdout.lines().next().expect("a catalog line");
    assert!(
        line.starts_with("Catalog: development, revision 1"),
        "{line}"
    );
    assert!(
        line.ends_with(", older than 180 days; updating Stackly may bring a newer catalog"),
        "{line}"
    );
    ev.pass();
}

/// AC-9.
#[test]
fn a_catalog_with_no_providers_says_so_after_its_line() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "provider list of a catalog with no entries writes the catalog line and No providers in the catalog with exit 0",
    );
    let (root, _daemon) = serving(data::list_empty());

    let text = run(root.path(), &["provider", "list"]);

    assert_eq!(text.code, Some(0), "{text:?}");
    golden::check("provider_list_empty.txt", &text.stdout);
    ev.pass();
}

/// AC-9: exit 6 for a catalog that is not available, exit 2 for a category
/// clap refuses, exit 6 for a daemon that is not running.
#[test]
fn list_exits_6_without_a_catalog_or_a_daemon_and_2_for_a_wrong_category() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "provider list exits 6 with catalog.unavailable and its hint, 6 with no daemon, and 2 with cli.usage for a category that does not exist",
    );
    let root = TestRoot::new().expect("a test root is made");
    {
        let _daemon = ScriptedDaemon::serve(&root, |call, _| match call {
            stackly_protocol::Call::CatalogListProviders => vec![Step::Fail(
                error(
                    "catalog.unavailable",
                    "unavailable",
                    "the provider catalog is not available",
                )
                .with_hint("run stacklyd from Stackly.app or set STACKLY_CATALOG_DIR"),
            )],
            _ => Vec::new(),
        });
        let unavailable = run(root.path(), &["provider", "list", "--json"]);
        let text = run(root.path(), &["provider", "list"]);
        let wrong = run(
            root.path(),
            &["provider", "list", "--category", "games", "--json"],
        );

        assert_eq!(unavailable.code, Some(6), "{unavailable:?}");
        let value = envelope(&unavailable);
        assert_eq!(value["error"]["code"], "catalog.unavailable");
        assert_eq!(
            value["error"]["hint"],
            "run stacklyd from Stackly.app or set STACKLY_CATALOG_DIR"
        );
        assert_eq!(text.code, Some(6), "{text:?}");
        assert!(
            text.stderr.contains("run stacklyd from Stackly.app"),
            "{text:?}"
        );
        assert_eq!(wrong.code, Some(2), "{wrong:?}");
        assert_eq!(envelope(&wrong)["error"]["code"], "cli.usage");
    }

    let nobody = run(root.path(), &["provider", "list", "--json"]);

    assert_eq!(nobody.code, Some(6), "{nobody:?}");
    assert_eq!(envelope(&nobody)["error"]["category"], "unavailable");
    ev.pass();
}
