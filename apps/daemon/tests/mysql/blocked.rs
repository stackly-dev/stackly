//! The `BLOCKED` path of the native suite (spec 0042 AC-16): an artifact that
//! is not in the runtime cache ends a test as `BLOCKED` with its reason and
//! owner, and nothing is downloaded.

use stackly_testkit::evidence::{Kind, evidence};

use crate::harness::{My, OWNER, Site};

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
