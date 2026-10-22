//! AC-9: `provider inspect` calls `catalog.get_provider` once, writes the
//! identity block and the capability, matrix, and artifact tables, and
//! writes the `ProviderDetail` itself with `--json`.

use stackly_testkit::TestRoot;
use stackly_testkit::evidence::{Kind, evidence};

use crate::common::scripted::{ScriptedDaemon, Step, error};
use crate::common::{envelope, golden, run};
use crate::data;

fn serving() -> (TestRoot, ScriptedDaemon) {
    let root = TestRoot::new().expect("a test root is made");
    let daemon = ScriptedDaemon::serve(&root, data::catalog(data::list(), data::all_details()));
    (root, daemon)
}

/// AC-9.
#[test]
fn inspect_writes_the_identity_capabilities_matrix_and_artifacts() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "provider inspect official/nats writes the identity block with the full manifest digest, the baseline line, the capability table, the version matrix with the install verdict, and the artifact table with decimal sizes, and the ProviderDetail itself with --json",
    );
    let (root, daemon) = serving();

    let text = run(root.path(), &["provider", "inspect", "official/nats"]);
    let json = run(
        root.path(),
        &["provider", "inspect", "official/nats", "--json"],
    );

    assert_eq!(text.code, Some(0), "{text:?}");
    assert_eq!(text.stderr, "");
    golden::check("provider_inspect_nats.txt", &text.stdout);
    for needle in [
        "sha256:91d3c6da2a0c43639d45a354e5f0dc9ddc492398b09b9350b99a25b1689e9235",
        "lifecycle, readiness probe, logs",
        "Reload without restart  not supported",
        "6.8 MB",
    ] {
        assert!(
            text.stdout.contains(needle),
            "{needle} in:\n{}",
            text.stdout
        );
    }
    assert_eq!(json.code, Some(0), "{json:?}");
    golden::check("provider_inspect_nats.json", &json.stdout);
    let value = envelope(&json);
    assert_eq!(value["command"], "provider inspect");
    assert_eq!(value["result"]["summary"]["provider"], "official/nats");
    assert_eq!(
        value["result"],
        serde_json::to_value(data::detail("nats")).expect("a detail encodes")
    );
    assert_eq!(
        daemon.methods(),
        [
            "daemon.handshake",
            "catalog.get_provider",
            "daemon.handshake",
            "catalog.get_provider"
        ]
    );
    assert_eq!(
        daemon.log()[1].raw["call"]["content"]["provider"],
        "official/nats"
    );
    ev.pass();
}

/// AC-9, AC-6: the status, reason, and verdict on screen are the DTO's.
#[test]
fn every_kind_of_entry_writes_its_own_state_and_verdict() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "inspect of an Intel only, a blocked, a planned, and two invalid entries writes the state with its reason, the no (reason) install verdict, the planned note, and the issue table, with a dash for a missing category",
    );
    let (root, _daemon) = serving();

    let intel = run(root.path(), &["provider", "inspect", "official/intelonly"]);
    let blocked = run(
        root.path(),
        &["provider", "inspect", "official/blockedhost"],
    );
    let planned = run(root.path(), &["provider", "inspect", "official/postgresql"]);
    let broken = run(root.path(), &["provider", "inspect", "official/broken"]);
    let zenith = run(root.path(), &["provider", "inspect", "official/zenith"]);

    for (name, run) in [
        ("intelonly", &intel),
        ("blockedhost", &blocked),
        ("postgresql", &planned),
        ("broken", &broken),
        ("zenith", &zenith),
    ] {
        assert_eq!(run.code, Some(0), "{name}: {run:?}");
        golden::check(&format!("provider_inspect_{name}.txt"), &run.stdout);
    }
    assert!(
        intel.stdout.contains("no (no_build_for_arch)"),
        "{}",
        intel.stdout
    );
    assert!(
        blocked
            .stdout
            .contains("blocked (the upstream build fails the closure audit)"),
        "{}",
        blocked.stdout
    );
    assert!(planned.stdout.contains("Planned for a later release."));
    assert!(!planned.stdout.contains("CAPABILITY"));
    assert!(broken.stdout.contains("catalog_digest_mismatch"));
    assert!(
        zenith
            .stdout
            .lines()
            .any(|line| line.starts_with("Category") && line.ends_with("  -")),
        "{}",
        zenith.stdout
    );
    ev.pass();
}

/// AC-9: exit 10 for a provider that is not there, exit 2 for an ID the
/// daemon refuses, exit 6 for a catalog that is not available.
#[test]
fn inspect_exits_10_2_and_6_by_the_error_category_of_the_daemon() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "provider inspect exits 10 for catalog.provider_not_found with the daemon's hint, 2 for request.invalid_params, and 6 for catalog.unavailable, and exits 2 with cli.usage without an argument",
    );
    let root = TestRoot::new().expect("a test root is made");
    let _daemon = ScriptedDaemon::serve(&root, |call, _| match call {
        stackly_protocol::Call::CatalogGetProvider(params) => {
            let failure = match params.provider.as_str() {
                "official/postgres" => error(
                    "catalog.provider_not_found",
                    "not_found",
                    "no provider official/postgres in the catalog",
                )
                .with_hint("run stackly provider list to see the catalog"),
                "official/offline" => error(
                    "catalog.unavailable",
                    "unavailable",
                    "the provider catalog is not available",
                ),
                _ => error(
                    "request.invalid_params",
                    "invalid_input",
                    "the request params are not valid",
                ),
            };
            vec![Step::Fail(failure)]
        }
        _ => Vec::new(),
    });

    let missing = run(
        root.path(),
        &["provider", "inspect", "official/postgres", "--json"],
    );
    let invalid = run(
        root.path(),
        &["provider", "inspect", "Official/NATS", "--json"],
    );
    let offline = run(
        root.path(),
        &["provider", "inspect", "official/offline", "--json"],
    );
    let usage = run(root.path(), &["provider", "inspect", "--json"]);

    assert_eq!(missing.code, Some(10), "{missing:?}");
    let value = envelope(&missing);
    assert_eq!(value["error"]["code"], "catalog.provider_not_found");
    assert_eq!(
        value["error"]["hint"],
        "run stackly provider list to see the catalog"
    );
    assert_eq!(invalid.code, Some(2), "{invalid:?}");
    assert_eq!(offline.code, Some(6), "{offline:?}");
    assert_eq!(usage.code, Some(2), "{usage:?}");
    assert_eq!(envelope(&usage)["error"]["code"], "cli.usage");
    ev.pass();
}
