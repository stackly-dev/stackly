//! The shared agent state fixture of spec 0015 (AC-4): `classify` in
//! `stackly-platform::agent` and the Swift classifier in `StacklyUI` give the
//! same answer for every row of `protocols/fixtures/agent/states.json`.

use std::path::{Path, PathBuf};

use serde_json::Value;
use stackly_platform::{AgentInput, AgentProbe, AgentRegistration, ProbeOutcome, classify};

/// Reads `protocols/fixtures/agent/states.json`.
fn fixture() -> Value {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../protocols/fixtures/agent/states.json");
    let text = std::fs::read_to_string(&path).expect("the agent state fixture is readable");
    serde_json::from_str(&text).expect("the agent state fixture is JSON")
}

/// Builds the probe a row names; the bundle and executable paths are fixed
/// because `classify` never reads them.
fn probe(row: &Value) -> ProbeOutcome {
    let probe = &row["probe"];
    match probe["type"].as_str().expect("probe.type is a string") {
        "not_checked" => ProbeOutcome::NotChecked,
        "unsupported" => ProbeOutcome::Unsupported,
        "failed" => ProbeOutcome::Failed {
            code: probe["code"].as_i64(),
        },
        "not_bundled" => ProbeOutcome::Probe(AgentProbe::NotBundled {
            executable: PathBuf::from("/work/target/debug/stackly"),
        }),
        "helper_missing" => ProbeOutcome::Probe(AgentProbe::HelperMissing {
            bundle: PathBuf::from("/DerivedData/Stackly.app"),
        }),
        "registration" => {
            let registration = match probe["registration"]
                .as_str()
                .expect("probe.registration is a string")
            {
                "enabled" => AgentRegistration::Enabled,
                "requires_approval" => AgentRegistration::RequiresApproval,
                "not_registered" => AgentRegistration::NotRegistered,
                "not_found" => AgentRegistration::NotFound,
                "unrecognized" => AgentRegistration::Unrecognized(
                    probe["raw"].as_i64().expect("unrecognized rows carry raw"),
                ),
                other => panic!("unknown registration {other}"),
            };
            ProbeOutcome::Probe(AgentProbe::Registration {
                registration,
                bundle: PathBuf::from("/Applications/Stackly.app"),
            })
        }
        other => panic!("unknown probe type {other}"),
    }
}

#[test]
fn every_fixture_row_classifies_as_expected() {
    let fixture = fixture();
    assert_eq!(fixture["schema"], 1);
    let rows = fixture["rows"].as_array().expect("rows is an array");
    for row in rows {
        let name = row["name"].as_str().expect("name is a string");
        let input = AgentInput {
            root_override: row["rootOverride"].as_bool().expect("rootOverride"),
            probe: probe(row),
            launch_record: row["launchRecord"].as_bool().expect("launchRecord"),
            just_registered: row["justRegistered"].as_bool().expect("justRegistered"),
        };
        let assessment = classify(&input);
        let expect = &row["expect"];
        assert_eq!(
            assessment.state.as_code(),
            expect["state"].as_str().expect("expect.state"),
            "state of {name}"
        );
        assert_eq!(
            assessment.reason.map(|reason| reason.as_code()),
            expect["reason"].as_str(),
            "reason of {name}"
        );
        assert_eq!(
            assessment.recovery.as_code(),
            expect["recovery"].as_str().expect("expect.recovery"),
            "recovery of {name}"
        );
    }
}

#[test]
fn the_fixture_holds_the_rows_the_spec_requires() {
    let fixture = fixture();
    let rows = fixture["rows"].as_array().expect("rows is an array");
    let registrations = rows
        .iter()
        .filter(|row| row["probe"]["type"] == "registration" && row["rootOverride"] == false)
        .count();
    assert_eq!(
        registrations, 20,
        "five registrations by two Bools by two Bools"
    );
    let overrides = rows
        .iter()
        .filter(|row| row["rootOverride"] == true)
        .count();
    assert_eq!(overrides, 3);
    assert_eq!(rows.len(), 29);
}
