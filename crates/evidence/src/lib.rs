//! Test evidence for Stackly (spec 0002).
//!
//! Every test that proves a PRD ID (AC, REQ, NFR, VAL, or BR) writes one
//! evidence record in the `stackly.evidence/v1` format, labelled `fixture` or
//! `native`, that starts as `NOT_RUN` and is replaced by its real result.
//! `cargo xtask evidence run` is the only way a test run becomes evidence: it
//! sets `STACKLY_EVIDENCE_DIR`, runs the tests, collects the records, and
//! writes `report.json`.
//!
//! This is a leaf crate: it depends on no other Stackly crate, so
//! `stackly-domain` and `stackly-protocol` can take it as a dev dependency.
//! `stackly-testkit` re-exports it as `stackly_testkit::evidence`.
//!
//! ```no_run
//! use stackly_evidence::{Kind, evidence};
//!
//! #[test]
//! fn stop_never_sends_sigkill() {
//!     let ev = evidence!(
//!         covers = ["AC-011"],
//!         kind = Kind::Fixture,
//!         expected = "a normal stop ends the dummy server with SIGTERM only",
//!     );
//!     // arrange, act, assert as usual
//!     ev.pass();
//! }
//! ```

mod baseline;
mod context;
mod credential;
mod guard;
mod record;
mod report;
mod rules;

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::Path;

use serde::Serialize;

pub use baseline::{BaselineHost, Storage};
pub use context::{BaselineMatch, Host, RunContext, Toolchain};
pub use credential::{FIXTURE_CREDENTIAL_PREFIX, FixtureCredential};
pub use guard::{EVIDENCE_DIR_ENV, EVIDENCE_KIND_ENV, Evidence, TestSite};
pub use record::{
    EvidenceRecord, Kind, Language, Method, PANIC_ACTUAL, RUST_DEFAULT_ACTUAL,
    SWIFT_DEFAULT_ACTUAL, Source, Status, Subject, TestIdentity, UNFINISHED_ACTUAL,
};
pub use report::{Problem, ProblemCode, RunReport, Summary};
pub use rules::{RuleViolations, is_exact_version, is_prd_id, is_sha256_hex};

/// The `schema` value of an evidence record.
pub const RECORD_SCHEMA: &str = "stackly.evidence/v1";
/// The `schema` value of a run report.
pub const REPORT_SCHEMA: &str = "stackly.evidence-report/v1";
/// The `schema` value of the baseline host file.
pub const BASELINE_SCHEMA: &str = "stackly.evidence-baseline/v1";

/// Returns the JSON Schema of every evidence file type, keyed by file stem.
///
/// `cargo xtask codegen` writes each one to
/// `schemas/evidence/<stem>.schema.json`.
#[must_use]
pub fn json_schemas() -> Vec<(&'static str, schemars::Schema)> {
    vec![
        ("record", schemars::schema_for!(EvidenceRecord)),
        ("run-context", schemars::schema_for!(RunContext)),
        ("report", schemars::schema_for!(RunReport)),
        ("baseline-host", schemars::schema_for!(BaselineHost)),
    ]
}

/// Writes `value` as pretty JSON with a final newline, atomically: it goes
/// to `<name>.tmp` in the same folder first and is then renamed, so a reader
/// never sees a half written file.
///
/// # Errors
///
/// Returns the I/O error when the value cannot be serialized, or the
/// temporary file cannot be written or renamed.
pub fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    let mut contents = serde_json::to_vec_pretty(value).map_err(io::Error::other)?;
    contents.push(b'\n');
    let file_name = path.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "the target path has no file name",
        )
    })?;
    let mut temporary_name = file_name.to_owned();
    temporary_name.push(".tmp");
    let temporary = path.with_file_name(temporary_name);
    let mut file = File::create(&temporary)?;
    file.write_all(&contents)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&temporary, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_evidence_file_type_has_a_schema() {
        let stems: Vec<&str> = json_schemas().into_iter().map(|(stem, _)| stem).collect();
        assert_eq!(stems, ["record", "run-context", "report", "baseline-host"]);
    }

    #[test]
    fn atomic_write_leaves_no_temporary_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("run.json");
        write_json_atomic(&path, &serde_json::json!({ "ok": true })).unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "{\n  \"ok\": true\n}\n");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}
