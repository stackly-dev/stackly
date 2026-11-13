//! Checks the environment fixtures of spec 0044 in
//! `protocols/fixtures/env/`, which the Swift tests read too: each DTO
//! keeps its shape, open enums read a newer value as `unrecognized`, and
//! every `env` code has a static hint.

use std::path::PathBuf;

use serde::Deserialize;
use serde_json::Value;
use stackly_protocol::codes;
use stackly_protocol::env::{
    EnvOwnership, EnvPlan, EnvValidation, EnvironmentDetail, EnvironmentStatus, ManifestState,
    NotReadyReason, PlanAction, PlanRegistration, PlanSecretStatus, PlanStep, PlanTrust,
};
use stackly_protocol::instance::InstanceOwnership;

#[derive(Debug, Deserialize)]
struct Index {
    fixtures: Vec<Entry>,
}

#[derive(Debug, Deserialize)]
struct Entry {
    file: String,
    #[serde(rename = "type")]
    type_name: String,
    expect: String,
}

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../protocols/fixtures/env")
}

fn read(file: &str) -> Value {
    let path = dir().join(file);
    let bytes = std::fs::read(&path).unwrap_or_else(|err| panic!("{}: {err}", path.display()));
    serde_json::from_slice(&bytes).unwrap_or_else(|err| panic!("{file}: {err}"))
}

fn roundtrip<T: serde::de::DeserializeOwned + serde::Serialize>(file: &str, value: &Value) {
    let decoded: T =
        serde_json::from_value(value.clone()).unwrap_or_else(|err| panic!("{file}: {err}"));
    assert_eq!(
        &serde_json::to_value(&decoded).expect("a decoded DTO encodes again"),
        value,
        "{file}: Rust writes another shape"
    );
}

// covers: AC-24 (spec 0044)
#[test]
fn every_env_fixture_is_listed_and_keeps_its_shape() {
    let index: Index = serde_json::from_value(read("index.json")).unwrap();
    let mut files: Vec<String> = std::fs::read_dir(dir())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name != "index.json")
        .collect();
    files.sort();
    let mut listed: Vec<String> = index.fixtures.iter().map(|e| e.file.clone()).collect();
    listed.sort();
    assert_eq!(files, listed);
    for entry in &index.fixtures {
        assert_eq!(entry.expect, "roundtrip", "{}", entry.file);
        let value = read(&entry.file);
        match entry.type_name.as_str() {
            "EnvPlan" => roundtrip::<EnvPlan>(&entry.file, &value),
            "EnvValidation" => roundtrip::<EnvValidation>(&entry.file, &value),
            "EnvironmentDetail" => roundtrip::<EnvironmentDetail>(&entry.file, &value),
            other => panic!("{}: no decoder for `{other}`", entry.file),
        }
    }
}

// covers: AC-5, AC-7 (spec 0044)
#[test]
fn a_blocked_plan_has_no_digest_and_carries_catalog_errors() {
    let plan: EnvPlan = serde_json::from_value(read("plan-blocked.json")).unwrap();
    assert!(!plan.applicable && plan.plan_digest.is_none());
    for blocker in &plan.blockers {
        let spec = blocker.error.spec().expect("a catalog code");
        for detail in blocker.error.details() {
            assert!(
                spec.details.contains(&detail.key()),
                "{}",
                blocker.error.code()
            );
        }
    }
    let applicable: EnvPlan = serde_json::from_value(read("plan-applicable.json")).unwrap();
    assert!(applicable.applicable && applicable.plan_digest.is_some());
    assert_eq!(applicable.items[0].steps.len(), 3);
}

// covers: AC-24 (spec 0044)
#[test]
fn open_enums_read_a_newer_value_as_unrecognized() {
    fn read_as<T: serde::de::DeserializeOwned>(text: &str) -> T {
        serde_json::from_str(&format!("\"{text}\"")).unwrap()
    }
    assert_eq!(
        read_as::<EnvOwnership>("borrowed"),
        EnvOwnership::Unrecognized
    );
    assert_eq!(read_as::<PlanAction>("upgrade"), PlanAction::Unrecognized);
    assert_eq!(read_as::<PlanStep>("migrate_data"), PlanStep::Unrecognized);
    assert_eq!(read_as::<PlanTrust>("pinned"), PlanTrust::Unrecognized);
    assert_eq!(
        read_as::<PlanSecretStatus>("locked"),
        PlanSecretStatus::Unrecognized
    );
    assert_eq!(
        read_as::<PlanRegistration>("moved"),
        PlanRegistration::Unrecognized
    );
    assert_eq!(
        read_as::<EnvironmentStatus>("coming_up"),
        EnvironmentStatus::Unrecognized
    );
    assert_eq!(
        read_as::<NotReadyReason>("waiting"),
        NotReadyReason::Unrecognized
    );
    assert_eq!(
        read_as::<ManifestState>("locked"),
        ManifestState::Unrecognized
    );
    assert_eq!(
        read_as::<InstanceOwnership>("dedicated"),
        InstanceOwnership::Dedicated
    );
    assert_eq!(
        read_as::<InstanceOwnership>("leased"),
        InstanceOwnership::Unrecognized
    );
}

// covers: AC-24 (spec 0044)
#[test]
fn every_env_code_has_a_static_hint_and_no_other_does() {
    for spec in codes::env::ALL {
        assert!(codes::env::hint(spec.code, &[]).is_some(), "{}", spec.code);
        assert!(!spec.retryable, "{}", spec.code);
    }
    assert_eq!(codes::env::hint("provider.revoked", &[]), None);
}
