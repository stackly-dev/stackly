//! The shared `registry_conformance` suite on the SQLite registry, seeded
//! through a second connection to the same file, and the secret smoke test
//! over the still open registry file (spec 0005 step 13, AC-9, AC-12).

use std::fs;

use stackly_domain::{Registry, WriteOp};
use stackly_testkit::evidence::{FixtureCredential, Kind, evidence};
use stackly_testkit::registry_conformance;

use crate::common::{Harness, suffixed};

#[test]
fn the_sqlite_registry_passes_the_registry_conformance_suite() {
    let harness = Harness::new();

    registry_conformance(&harness.registry, &harness.seeder(), harness.clock());
}

#[test]
fn the_suite_also_passes_on_a_registry_that_already_holds_data() {
    let harness = Harness::new();
    let base = harness.base();
    let instance = harness.instance(&base, "main");
    harness.commit(vec![WriteOp::InsertInstance(instance.clone())]);

    registry_conformance(&harness.registry, &harness.seeder(), harness.clock());

    assert!(
        harness
            .registry
            .known_instance_ids()
            .unwrap()
            .value
            .contains(&instance.id())
    );
}

#[test]
fn registry_never_holds_secret_values() {
    let ev = evidence!(
        covers = ["BR-018"],
        kind = Kind::Fixture,
        expected = "after the conformance suite ran on the SQLite registry while the test held a \
                    fixture credential, neither stackly.sqlite nor its WAL, read before the \
                    registry closed, contains a fixture credential",
    );
    let credential = FixtureCredential::new();
    let harness = Harness::new();

    registry_conformance(&harness.registry, &harness.seeder(), harness.clock());

    let db = harness.layout.registry_db();
    for (path, what) in [
        (db.clone(), "stackly.sqlite"),
        (suffixed(&db, "-wal"), "stackly.sqlite-wal"),
    ] {
        let bytes = fs::read(&path).expect("the registry file is readable while it is open");
        credential.assert_absent(&String::from_utf8_lossy(&bytes), what);
    }
    drop(harness);
    ev.pass();
}
