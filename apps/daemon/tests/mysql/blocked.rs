//! The `BLOCKED` path of the native suite (spec 0042 AC-16): an artifact that
//! is not in the runtime cache ends a test as `BLOCKED` with its reason and
//! owner, and nothing is downloaded: once by the reason text, and once by
//! running the native suite itself against an empty cache.

use std::path::Path;
use std::process::Command;

use stackly_testkit::TestRoot;
use stackly_testkit::evidence::{EVIDENCE_DIR_ENV, EVIDENCE_KIND_ENV, Kind, evidence};

use crate::harness::{MY84, My, OWNER, Site};

/// An artifact ID the inventory never had, so the cache cannot hold it.
const MISSING: My = My {
    artifact: "mysql-0.0.0-macos-aarch64",
    line: "0.0",
    version: "0.0.0",
};

// covers: AC-16 (spec 0042)
#[test]
fn an_artifact_that_is_not_in_the_runtime_cache_is_named_without_a_download() {
    let ev = evidence!(
        covers = ["BR-025"],
        kind = Kind::Fixture,
        expected = "the reason and owner a native MySQL test records when its artifact is not in the runtime cache",
    );
    assert_eq!(OWNER, "@ojiepermana");
    assert_eq!(
        Site::blocked_reason(MISSING),
        "artifact mysql-0.0.0-macos-aarch64 is not in the runtime cache"
    );
    ev.pass();
}

/// Every native test of this binary, run in a child of the test binary with
/// `HOME` on an empty folder, so the runtime cache of `cargo xtask` is
/// empty. The child writes its records into a run folder of its own.
fn run_native_suite_with_an_empty_cache(home: &Path, run: &Path) -> std::process::Output {
    let exe = std::env::current_exe().expect("the test binary knows its path");
    Command::new(exe)
        .args(["--ignored", "--test-threads", "1", "mysql_8_4_"])
        .env_clear()
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
        .env("HOME", home)
        .env("TMPDIR", std::env::temp_dir())
        .env(EVIDENCE_DIR_ENV, run)
        .env(EVIDENCE_KIND_ENV, "native")
        .output()
        .expect("the test binary runs as a child")
}

// covers: AC-16 (spec 0042)
#[test]
fn every_native_test_ends_blocked_with_an_empty_runtime_cache_and_downloads_nothing() {
    let ev = evidence!(
        covers = ["BR-025"],
        kind = Kind::Fixture,
        expected = "every native MySQL 8.4 test of this binary, run with an empty runtime cache, ends BLOCKED with the reason artifact mysql-8.4.11-macos-aarch64 is not in the runtime cache and the owner @ojiepermana, none ends FAIL or PASS, and the cache folder is never created",
    );
    let home = TestRoot::new().expect("an empty home folder");
    let run = TestRoot::new().expect("a run folder for the child");
    let output = run_native_suite_with_an_empty_cache(home.path(), run.path());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "{stdout}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let ran = stdout
        .lines()
        .filter(|line| line.starts_with("test ") && line.ends_with(" ... ok"))
        .count();
    assert!(ran >= 10, "the child ran the native suite: {stdout}");

    let mut records = Vec::new();
    for entry in std::fs::read_dir(run.path().join("records")).expect("the child wrote records") {
        let path = entry.expect("a record entry").path();
        let text = std::fs::read_to_string(&path).expect("a record can be read");
        let record: serde_json::Value = serde_json::from_str(&text).expect("a record is JSON");
        records.push(record);
    }
    assert_eq!(records.len(), ran, "one record per test: {stdout}");
    for record in &records {
        let name = record["test"]["name"].as_str().unwrap_or_default();
        assert_eq!(record["kind"], "native", "{name}");
        assert_eq!(record["status"], "BLOCKED", "{name}: {record:#}");
        assert_eq!(
            record["reason"].as_str(),
            Some(Site::blocked_reason(MY84).as_str()),
            "{name}"
        );
        assert_eq!(record["owner"].as_str(), Some(OWNER), "{name}");
    }
    assert!(
        !home.path().join("Library/Caches").exists(),
        "a cache folder was created under the empty home"
    );
    ev.pass_with(&format!(
        "{} native MySQL 8.4 tests ended BLOCKED with the cache reason and owner",
        records.len()
    ));
}
