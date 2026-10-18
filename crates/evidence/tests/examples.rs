//! Checks the shared examples in `schemas/evidence/examples/` against the
//! Rust evidence types and rules. The Swift tests of `StacklyTestSupport`
//! read the same files.
//!
//! A file named `<type>-<case>.json` decodes, passes the rules, and encodes
//! back to the same JSON. A file named `invalid-<type>-<case>.json` decodes
//! and breaks exactly the rules listed in [`INVALID`]. `<type>` is the stem
//! of a schema in `schemas/evidence/`: `record`, `run-context`, `report`, or
//! `baseline-host`.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use stackly_evidence::{BaselineHost, EvidenceRecord, Method, RunContext, RunReport, Summary};

/// Every deliberately wrong example and the violations it must produce, in
/// the order the rules report them.
const INVALID: &[(&str, &[&str])] = &[
    (
        "invalid-baseline-host-virtualized.json",
        &["the baseline host is a real Mac, not a VM"],
    ),
    (
        "invalid-record-blocked-without-owner.json",
        &["a BLOCKED record needs reason and owner"],
    ),
    (
        "invalid-record-covers-not-prd-id.json",
        &[
            "covers entry `OQ-011` is not a PRD ID (AC, REQ, NFR, VAL, or BR plus three digits)",
            "covers entry `AC-11` is not a PRD ID (AC, REQ, NFR, VAL, or BR plus three digits)",
        ],
    ),
    (
        "invalid-record-final-manual-without-reviewer.json",
        &[
            "a final manual record needs steps",
            "a final manual record needs a reviewer",
        ],
    ),
    (
        "invalid-record-native-pass-default-actual.json",
        &[
            "a native PASS needs at least one subject",
            "a native PASS needs an observed actual, not the default text",
        ],
    ),
    (
        "invalid-record-native-subject-selector.json",
        &[
            "subject postgresql version `18` is not exact",
            "subject postgresql digest is not 64 lowercase hex characters",
            "native subject postgresql cannot have source fixture",
        ],
    ),
];

/// The file types an example can hold, keyed by the schema stem.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExampleType {
    Record,
    RunContext,
    Report,
    BaselineHost,
}

impl ExampleType {
    /// Reads the type from a file name such as `record-manual-not-run.json`,
    /// with or without the `invalid-` prefix.
    fn of(name: &str) -> Self {
        let name = name.strip_prefix("invalid-").unwrap_or(name);
        [
            ("record-", Self::Record),
            ("run-context-", Self::RunContext),
            ("report-", Self::Report),
            ("baseline-host-", Self::BaselineHost),
        ]
        .into_iter()
        .find_map(|(prefix, kind)| name.starts_with(prefix).then_some(kind))
        .unwrap_or_else(|| panic!("example `{name}` does not start with a schema stem"))
    }
}

fn examples_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/evidence/examples")
}

/// Every example file name, sorted.
fn example_names() -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(examples_dir())
        .expect("schemas/evidence/examples is readable")
        .map(|entry| {
            entry
                .expect("an examples entry is readable")
                .file_name()
                .into_string()
                .expect("example names are UTF-8")
        })
        .collect();
    names.sort();
    names
}

fn read_value(name: &str) -> Value {
    let path = examples_dir().join(name);
    let bytes =
        fs::read(&path).unwrap_or_else(|err| panic!("cannot read {}: {err}", path.display()));
    serde_json::from_slice(&bytes).unwrap_or_else(|err| panic!("{name} is not JSON: {err}"))
}

/// Decodes an example and checks that it encodes back to the same JSON, so
/// every field is spelled out and none is unknown.
fn decode_exact<T: DeserializeOwned + Serialize>(name: &str) -> T {
    let value = read_value(name);
    let decoded: T = serde_json::from_value(value.clone())
        .unwrap_or_else(|err| panic!("{name} does not decode: {err}"));
    let encoded = serde_json::to_value(&decoded).expect("an evidence type encodes to JSON");
    assert_eq!(
        encoded, value,
        "{name} does not encode back to the same JSON"
    );
    decoded
}

fn record_violations(name: &str) -> Vec<String> {
    let record: EvidenceRecord = decode_exact(name);
    record
        .validate()
        .err()
        .map(|e| e.violations)
        .unwrap_or_default()
}

fn baseline_violations(name: &str) -> Vec<String> {
    let baseline: BaselineHost = decode_exact(name);
    baseline
        .validate()
        .err()
        .map(|e| e.violations)
        .unwrap_or_default()
}

fn check_valid_report(name: &str) {
    let report: RunReport = decode_exact(name);
    assert_eq!(report.context.run_id, Some(report.run_id), "{name}");
    assert_eq!(report.context.run_kind, Some(report.kind), "{name}");
    assert_eq!(report.summary, Summary::of(&report.records), "{name}");
    for record in &report.records {
        assert_eq!(record.validate(), Ok(()), "{name}: record {}", record.id);
        assert_eq!(record.kind, report.kind, "{name}: record {}", record.id);
        assert_eq!(
            record.context.as_ref(),
            Some(&report.context),
            "{name}: every record in a report carries the run context"
        );
    }
}

#[test]
fn every_valid_example_decodes_passes_the_rules_and_encodes_back() {
    let valid: Vec<String> = example_names()
        .into_iter()
        .filter(|name| !name.starts_with("invalid-"))
        .collect();
    assert!(!valid.is_empty(), "there are valid examples");
    for name in &valid {
        match ExampleType::of(name) {
            ExampleType::Record => {
                assert_eq!(record_violations(name), Vec::<String>::new(), "{name}");
            }
            ExampleType::RunContext => {
                let _: RunContext = decode_exact(name);
            }
            ExampleType::Report => check_valid_report(name),
            ExampleType::BaselineHost => {
                assert_eq!(baseline_violations(name), Vec::<String>::new(), "{name}");
            }
        }
    }
}

#[test]
fn every_invalid_example_breaks_exactly_the_listed_rules() {
    let invalid: Vec<String> = example_names()
        .into_iter()
        .filter(|name| name.starts_with("invalid-"))
        .collect();
    let listed: Vec<&str> = INVALID.iter().map(|(name, _)| *name).collect();
    assert_eq!(invalid, listed, "INVALID lists every invalid example");

    for (name, expected) in INVALID {
        let violations = match ExampleType::of(name) {
            ExampleType::Record => record_violations(name),
            ExampleType::BaselineHost => baseline_violations(name),
            other => panic!("{name}: no rules exist for {other:?} examples"),
        };
        assert_eq!(violations, *expected, "{name}");
    }
}

#[test]
fn manual_examples_carry_a_context_without_run_values() {
    let manual: Vec<EvidenceRecord> = example_names()
        .iter()
        .filter(|name| !name.starts_with("invalid-") && name.starts_with("record-manual-"))
        .map(|name| decode_exact(name))
        .collect();
    assert!(!manual.is_empty(), "there are manual examples");
    for record in manual {
        assert_eq!(record.method, Method::Manual);
        let context = record.context.expect("a manual record carries its context");
        assert_eq!(context.run_id, None);
        assert_eq!(context.run_kind, None);
        assert_eq!(context.command, None);
        assert!(record.evidence_path.starts_with("evidence/manual/"));
    }
}
