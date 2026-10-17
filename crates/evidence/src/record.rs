//! The evidence record `stackly.evidence/v1` and its rules.

use jiff::Timestamp;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::RunContext;
use crate::rules::{Violations, check_covers, is_exact_version, is_sha256_hex};
use crate::{RECORD_SCHEMA, RuleViolations};

/// `actual` of a Rust `PASS` when the test called `pass()`.
pub const RUST_DEFAULT_ACTUAL: &str = "all assertions passed";
/// `actual` of a Swift `PASS` when the test set no observed value.
pub const SWIFT_DEFAULT_ACTUAL: &str = "all expectations passed";
/// `actual` of a Rust `FAIL` whose test panicked.
pub const PANIC_ACTUAL: &str = "test panicked, see output.log";
/// `actual` of a Rust `FAIL` whose test ended without `pass()` or `blocked()`.
pub const UNFINISHED_ACTUAL: &str = "test ended without pass() or blocked()";

/// Whether a test needs only fakes and fixtures, or a real provider runtime
/// or something only a macOS 27 Mac has.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// Fakes, mocks, dummy servers, fake providers, or contract fixtures.
    Fixture,
    /// A real provider binary, or something only a macOS 27 Mac has.
    Native,
}

impl Kind {
    /// The value used on the wire and in `STACKLY_EVIDENCE_KIND`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fixture => "fixture",
            Self::Native => "native",
        }
    }

    /// Parses `fixture` or `native`.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "fixture" => Some(Self::Fixture),
            "native" => Some(Self::Native),
            _ => None,
        }
    }
}

/// Whether a record comes from a test run or from a reviewer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Method {
    /// Written by the Rust guard or the Swift trait during a test run.
    Automated,
    /// Written by `cargo xtask evidence new` and filled in by a reviewer.
    Manual,
}

/// The status of a record (PRD section 28).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Status {
    /// No result was observed yet. Every record starts here.
    NotRun,
    /// The test or review passed.
    Pass,
    /// The test or review failed.
    Fail,
    /// A prerequisite (binary, hardware, signing) was missing.
    Blocked,
    /// Manual only: the scope is outside the baseline.
    Deferred,
}

impl Status {
    /// Every status, in the order reports list them.
    pub const ALL: [Self; 5] = [
        Self::NotRun,
        Self::Pass,
        Self::Fail,
        Self::Blocked,
        Self::Deferred,
    ];

    /// True for every status except `NOT_RUN`.
    #[must_use]
    pub fn is_final(self) -> bool {
        self != Self::NotRun
    }

    /// The value used on the wire, for example `NOT_RUN`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotRun => "NOT_RUN",
            Self::Pass => "PASS",
            Self::Fail => "FAIL",
            Self::Blocked => "BLOCKED",
            Self::Deferred => "DEFERRED",
        }
    }
}

/// The language a test is written in, or `manual` for a review.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    /// A Rust `#[test]`.
    Rust,
    /// A Swift Testing `@Test`.
    Swift,
    /// A manual review.
    Manual,
}

/// The identity of a test: `(language, package, target, name)`, plus where
/// it lives.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TestIdentity {
    /// The language of the test.
    pub language: Language,
    /// Rust: `CARGO_PKG_NAME`. Swift and manual: null.
    #[serde(default)]
    pub package: Option<String>,
    /// Rust: `CARGO_CRATE_NAME`. Swift: the module name. Manual: null.
    #[serde(default)]
    pub target: Option<String>,
    /// Rust: the libtest test path. Swift: the test ID without module and
    /// location. Manual: the title.
    pub name: String,
    /// The source file, relative to the workspace root in `report.json`.
    #[serde(default)]
    pub file: Option<String>,
    /// The line of the guard or the test.
    #[serde(default)]
    pub line: Option<u32>,
}

impl TestIdentity {
    /// The key that must be unique within one run.
    #[must_use]
    pub fn key(&self) -> (Language, Option<&str>, Option<&str>, &str) {
        (
            self.language,
            self.package.as_deref(),
            self.target.as_deref(),
            &self.name,
        )
    }
}

/// Where a provider or runtime under test came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    /// A runtime Stackly installs and owns.
    Managed,
    /// A runtime found on the system, such as a Homebrew one.
    External,
    /// A fake or fixture. Never used by a native record.
    Fixture,
}

/// One provider or runtime a test ran against.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Subject {
    /// The provider, for example `postgresql`.
    pub provider: String,
    /// The exact version, read from the running binary.
    pub version: String,
    /// The SHA-256 of the artifact as 64 lowercase hex characters, or null.
    #[serde(default)]
    pub digest: Option<String>,
    /// Where the runtime came from.
    pub source: Source,
}

/// One evidence record: one test function in one run, or one manual review.
///
/// The Rust types are the source of truth; `cargo xtask codegen` publishes
/// the JSON Schema to `schemas/evidence/`. Unknown fields are accepted, so
/// `v1` can grow with optional fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct EvidenceRecord {
    /// Always [`RECORD_SCHEMA`].
    pub schema: String,
    /// A UUID v7, also the name of the raw record file.
    pub id: Uuid,
    /// The PRD IDs this test proves.
    pub covers: Vec<String>,
    /// Fixture or native; equal to the kind of the run.
    pub kind: Kind,
    /// Automated or manual.
    pub method: Method,
    /// Which test this is.
    pub test: TestIdentity,
    /// The result.
    pub status: Status,
    /// The expected behavior in one sentence.
    pub expected: String,
    /// What was observed. Required for `PASS` and `FAIL`.
    #[serde(default)]
    pub actual: Option<String>,
    /// Why the record is `BLOCKED` or `DEFERRED`.
    #[serde(default)]
    pub reason: Option<String>,
    /// Who can unblock a `BLOCKED` record.
    #[serde(default)]
    pub owner: Option<String>,
    /// The providers or runtimes the test ran against.
    #[serde(default)]
    pub subjects: Vec<Subject>,
    /// Manual steps; always null for an automated record.
    #[serde(default)]
    pub steps: Option<Vec<String>>,
    /// The reviewer of a manual record.
    #[serde(default)]
    pub reviewer: Option<String>,
    /// When the guard or trait was created, or the manual record written.
    pub started_at: Timestamp,
    /// When the final result was written; null while `NOT_RUN`.
    #[serde(default)]
    pub finished_at: Option<Timestamp>,
    /// Automated: `output.log` or `artifacts/<id>/`, relative to the run
    /// folder. Manual: a path relative to the repository.
    pub evidence_path: String,
    /// The run context; present in `report.json` and manual records, absent
    /// from raw record files.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<RunContext>,
}

impl EvidenceRecord {
    /// Checks every rule of `stackly.evidence/v1` that the record can check
    /// on its own. Whether `context` must be present depends on where the
    /// record is read, so callers check that themselves.
    ///
    /// # Errors
    ///
    /// Returns every violated rule at once.
    pub fn validate(&self) -> Result<(), RuleViolations> {
        let mut v = Violations::default();
        v.check(self.schema == RECORD_SCHEMA, || {
            format!("schema must be `{RECORD_SCHEMA}`, found `{}`", self.schema)
        });
        check_covers(&self.covers, &mut v);
        v.check(!self.expected.trim().is_empty(), || {
            "expected must not be empty".to_owned()
        });
        self.check_identity(&mut v);
        self.check_status(&mut v);
        self.check_subjects(&mut v);
        self.check_evidence_path(&mut v);
        v.into_result()
    }

    fn check_identity(&self, v: &mut Violations) {
        let test = &self.test;
        let automated = self.method == Method::Automated;
        v.check(automated == (test.language != Language::Manual), || {
            "an automated record is rust or swift, and a manual record is manual".to_owned()
        });
        v.check(!test.name.trim().is_empty(), || {
            "test name must not be empty".to_owned()
        });
        let has_location = test.file.is_some() && test.line.is_some();
        match test.language {
            Language::Rust => v.check(
                test.package.is_some() && test.target.is_some() && has_location,
                || "a rust test identity needs package, target, file, and line".to_owned(),
            ),
            Language::Swift => v.check(
                test.package.is_none() && test.target.is_some() && has_location,
                || "a swift test identity needs target, file, and line, and no package".to_owned(),
            ),
            Language::Manual => v.check(
                test.package.is_none()
                    && test.target.is_none()
                    && test.file.is_none()
                    && test.line.is_none(),
                || "a manual test identity has only a name".to_owned(),
            ),
        }
    }

    fn check_status(&self, v: &mut Violations) {
        let status = self.status;
        let filled =
            |value: &Option<String>| value.as_deref().is_some_and(|s| !s.trim().is_empty());
        v.check(status.is_final() == self.finished_at.is_some(), || {
            "finishedAt is set exactly when the status is final".to_owned()
        });
        if matches!(status, Status::Pass | Status::Fail) {
            v.check(filled(&self.actual), || {
                format!("a {} record needs actual", status.as_str())
            });
        }
        if status == Status::Blocked {
            v.check(filled(&self.reason) && filled(&self.owner), || {
                "a BLOCKED record needs reason and owner".to_owned()
            });
        }
        if status == Status::Deferred {
            v.check(self.method == Method::Manual, || {
                "only a manual record may be DEFERRED".to_owned()
            });
            v.check(filled(&self.reason), || {
                "a DEFERRED record needs reason".to_owned()
            });
        }
        if self.kind == Kind::Native && status == Status::Pass {
            v.check(!self.subjects.is_empty(), || {
                "a native PASS needs at least one subject".to_owned()
            });
            let actual = self.actual.as_deref();
            v.check(
                actual != Some(RUST_DEFAULT_ACTUAL) && actual != Some(SWIFT_DEFAULT_ACTUAL),
                || "a native PASS needs an observed actual, not the default text".to_owned(),
            );
        }
        match self.method {
            Method::Automated => v.check(self.steps.is_none(), || {
                "an automated record has no steps; the command is in context".to_owned()
            }),
            Method::Manual if status.is_final() => {
                v.check(
                    self.steps.as_ref().is_some_and(|steps| !steps.is_empty()),
                    || "a final manual record needs steps".to_owned(),
                );
                v.check(filled(&self.reviewer), || {
                    "a final manual record needs a reviewer".to_owned()
                });
            }
            Method::Manual => {}
        }
    }

    fn check_subjects(&self, v: &mut Violations) {
        for subject in &self.subjects {
            let provider = &subject.provider;
            v.check(!provider.trim().is_empty(), || {
                "a subject needs a provider".to_owned()
            });
            v.check(is_exact_version(&subject.version), || {
                format!(
                    "subject {provider} version `{}` is not exact",
                    subject.version
                )
            });
            v.check(subject.digest.as_deref().is_none_or(is_sha256_hex), || {
                format!("subject {provider} digest is not 64 lowercase hex characters")
            });
            v.check(
                !(self.kind == Kind::Native && subject.source == Source::Fixture),
                || format!("native subject {provider} cannot have source fixture"),
            );
        }
    }

    fn check_evidence_path(&self, v: &mut Violations) {
        let path = &self.evidence_path;
        v.check(
            !path.is_empty()
                && !path.starts_with('/')
                && !path.split(['/', '\\']).any(|part| part == ".."),
            || format!("evidencePath `{path}` must be a relative path without `..`"),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(kind: Kind, status: Status) -> EvidenceRecord {
        EvidenceRecord {
            schema: RECORD_SCHEMA.to_owned(),
            id: Uuid::now_v7(),
            covers: vec!["AC-011".to_owned()],
            kind,
            method: Method::Automated,
            test: TestIdentity {
                language: Language::Rust,
                package: Some("stacklyd".to_owned()),
                target: Some("stacklyd".to_owned()),
                name: "tests::stop_never_sends_sigkill".to_owned(),
                file: Some("apps/daemon/src/lib.rs".to_owned()),
                line: Some(10),
            },
            status,
            expected: "a normal stop ends the dummy server with SIGTERM only".to_owned(),
            actual: None,
            reason: None,
            owner: None,
            subjects: Vec::new(),
            steps: None,
            reviewer: None,
            started_at: Timestamp::now(),
            finished_at: status.is_final().then(Timestamp::now),
            evidence_path: "output.log".to_owned(),
            context: None,
        }
    }

    fn violations(record: &EvidenceRecord) -> Vec<String> {
        record
            .validate()
            .err()
            .map(|err| err.violations)
            .unwrap_or_default()
    }

    #[test]
    fn a_new_fixture_record_is_valid() {
        assert_eq!(
            violations(&record(Kind::Fixture, Status::NotRun)),
            Vec::<String>::new()
        );
    }

    #[test]
    fn pass_and_fail_need_actual() {
        let mut pass = record(Kind::Fixture, Status::Pass);
        assert_eq!(violations(&pass).len(), 1);
        pass.actual = Some(RUST_DEFAULT_ACTUAL.to_owned());
        assert!(pass.validate().is_ok());

        let mut fail = record(Kind::Fixture, Status::Fail);
        assert_eq!(violations(&fail).len(), 1);
        fail.actual = Some(PANIC_ACTUAL.to_owned());
        assert!(fail.validate().is_ok());
    }

    #[test]
    fn native_pass_needs_a_subject_and_an_observed_actual() {
        let mut pass = record(Kind::Native, Status::Pass);
        pass.actual = Some(RUST_DEFAULT_ACTUAL.to_owned());
        assert_eq!(violations(&pass).len(), 2);

        pass.actual = Some("ready after 1.2s on port 54001".to_owned());
        pass.subjects.push(Subject {
            provider: "postgresql".to_owned(),
            version: "18.1".to_owned(),
            digest: Some("0".repeat(64)),
            source: Source::Managed,
        });
        assert!(pass.validate().is_ok());
    }

    #[test]
    fn native_subjects_reject_fixture_source_selectors_and_bad_digests() {
        let mut blocked = record(Kind::Native, Status::Blocked);
        blocked.reason = Some("no managed PostgreSQL 18 artifact".to_owned());
        blocked.owner = Some("@ojiepermana".to_owned());
        blocked.subjects.push(Subject {
            provider: "postgresql".to_owned(),
            version: "18".to_owned(),
            digest: Some("sha256:abc".to_owned()),
            source: Source::Fixture,
        });
        assert_eq!(violations(&blocked).len(), 3);
    }

    #[test]
    fn blocked_needs_reason_and_owner() {
        let mut blocked = record(Kind::Fixture, Status::Blocked);
        blocked.reason = Some("no signing identity".to_owned());
        assert_eq!(violations(&blocked).len(), 1);
    }

    #[test]
    fn only_manual_records_may_be_deferred() {
        let mut deferred = record(Kind::Fixture, Status::Deferred);
        deferred.reason = Some("outside the baseline".to_owned());
        assert_eq!(violations(&deferred).len(), 1);
    }

    #[test]
    fn evidence_path_must_stay_relative() {
        let mut escaping = record(Kind::Fixture, Status::NotRun);
        escaping.evidence_path = "../outside".to_owned();
        assert_eq!(violations(&escaping).len(), 1);
    }

    #[test]
    fn record_round_trips_through_camel_case_json() {
        let original = record(Kind::Fixture, Status::NotRun);
        let json = serde_json::to_value(&original).unwrap();
        assert_eq!(json["status"], "NOT_RUN");
        assert_eq!(json["evidencePath"], "output.log");
        assert!(json.get("context").is_none());
        let decoded: EvidenceRecord = serde_json::from_value(json).unwrap();
        assert_eq!(decoded, original);
    }
}
