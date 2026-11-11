//! The compatibility corpus (spec 0036 AC-6, REQ-030): every frozen
//! revision in `tests/fixtures/compat/<name>/<revision>/` still loads with
//! `NameCheck::Skip`, keeps its `manifest_sha256`, builds its init and run
//! plans in the frozen context, and gives the same start argv, start env
//! keys, and rendered config digests that `frozen.json` recorded. A change
//! to the engine that breaks this broke a package pinned in a user's root.

use std::fs;
use std::path::{Path, PathBuf};

use stackly_core::provider::testing::{FROZEN_SCHEMA, FrozenFacts, frozen_facts};
use stackly_core::provider::{
    NameCheck, Scenario, Scenarios, ValidatedPackage, read_package, read_scenarios,
};
use stackly_evidence::{Kind, evidence};
use stackly_testkit::{FakePlatform, TestRoot};

fn corpus() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/compat")
}

/// Every `<name>/<revision>` folder of the corpus, sorted.
fn entries() -> Vec<PathBuf> {
    let mut entries = Vec::new();
    for name in fs::read_dir(corpus()).expect("the corpus folder exists") {
        let name = name.expect("a corpus entry reads").path();
        if !name.is_dir() {
            continue;
        }
        for revision in fs::read_dir(&name).expect("a provider folder reads") {
            let revision = revision.expect("a revision entry reads").path();
            if revision.is_dir() {
                entries.push(revision);
            }
        }
    }
    entries.sort();
    entries
}

/// Copies a folder tree with `std::fs`, as test setup.
fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).expect("the copy target can be created");
    for entry in fs::read_dir(from).expect("the folder can be listed") {
        let entry = entry.expect("the entry can be read");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("the entry has a type").is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target).expect("the file can be copied");
        }
    }
}

fn recorded(entry: &Path) -> FrozenFacts {
    let text = fs::read_to_string(entry.join("frozen.json")).expect("frozen.json reads");
    serde_json::from_str(&text).expect("frozen.json holds frozen facts")
}

/// Loads a copy of an entry's package through the fake filesystem in a
/// test root, as a pinned copy is loaded, with its scenarios.
fn load(entry: &Path) -> (ValidatedPackage, Vec<Scenario>) {
    let root = TestRoot::new().expect("a test root");
    let platform = FakePlatform::new(&root);
    let dir = root.path().join("package");
    copy_dir(&entry.join("package"), &dir);
    let files = read_package(platform.fs(), &dir)
        .unwrap_or_else(|err| panic!("{}: {:#?}", entry.display(), err.issues));
    let package = ValidatedPackage::from_files(files, NameCheck::Skip)
        .unwrap_or_else(|err| panic!("{}: {:#?}", entry.display(), err.issues));
    let scenarios = match read_scenarios(platform.fs(), &dir).expect("the scenarios file reads") {
        Some(text) => Scenarios::parse(&text).expect("the frozen scenarios parse"),
        None => vec![Scenario::implied_default()],
    };
    (package, scenarios)
}

// covers: AC-6 (spec 0036)
#[test]
fn every_frozen_revision_loads_with_its_digest_and_the_same_plans() {
    let ev = evidence!(
        covers = ["REQ-030"],
        kind = Kind::Fixture,
        expected = "every corpus entry loads with NameCheck::Skip, keeps the manifest_sha256 of frozen.json, and gives the start argv, start env keys, and rendered config digests frozen.json recorded",
    );
    let entries = entries();
    assert!(!entries.is_empty(), "the corpus holds at least one entry");
    for entry in &entries {
        let facts = recorded(entry);
        let (package, scenarios) = load(entry);

        let now = frozen_facts(&package, &scenarios)
            .unwrap_or_else(|err| panic!("{}: {err}", entry.display()));

        assert_eq!(facts.schema, FROZEN_SCHEMA, "{}", entry.display());
        assert_eq!(
            package.key().manifest_sha256().to_string(),
            facts.manifest_sha256,
            "{}: the digest changed",
            entry.display()
        );
        assert_eq!(now, facts, "{}: the frozen facts changed", entry.display());
        let revision = entry.file_name().unwrap().to_string_lossy();
        let name = entry
            .parent()
            .unwrap()
            .file_name()
            .unwrap()
            .to_string_lossy();
        assert_eq!(facts.revision, revision, "{}", entry.display());
        assert_eq!(
            facts.provider,
            format!("official/{name}"),
            "{}",
            entry.display()
        );
    }
    ev.pass();
}

// covers: AC-6 (spec 0036)
#[test]
fn the_corpus_holds_nats_and_postgresql() {
    let names: Vec<String> = entries()
        .iter()
        .map(|entry| {
            entry
                .parent()
                .unwrap()
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    assert!(names.contains(&"nats".to_owned()), "{names:?}");
    assert!(names.contains(&"postgresql".to_owned()), "{names:?}");
}

// covers: AC-6 (spec 0036)
#[test]
fn every_entry_holds_only_its_package_and_frozen_json() {
    for entry in entries() {
        let mut names: Vec<String> = fs::read_dir(&entry)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names, ["frozen.json", "package"], "{}", entry.display());
    }
}
