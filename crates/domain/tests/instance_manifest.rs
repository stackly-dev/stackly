//! The instance manifest against the shared JSON fixtures in
//! `tests/fixtures/`: a complete v1 manifest, a v1 manifest with fields
//! this version does not know, and a v2 manifest.

use jiff::Timestamp;
use serde_json::Value;
use stackly_domain::{Arch, INSTANCE_MANIFEST_SCHEMA, InstallKind, InstanceManifest, Os, Target};

const COMPLETE_V1: &str = include_str!("fixtures/instance-manifest-v1.json");
const UNKNOWN_FIELDS_V1: &str = include_str!("fixtures/instance-manifest-v1-unknown-fields.json");
const V2: &str = include_str!("fixtures/instance-manifest-v2.json");

fn at(text: &str) -> Timestamp {
    text.parse().expect("fixture timestamps are valid RFC 3339")
}

fn json(text: &str) -> Value {
    serde_json::from_str(text).expect("fixtures are valid JSON")
}

#[test]
fn complete_v1_fixture_decodes_every_field() {
    let manifest: InstanceManifest = serde_json::from_str(COMPLETE_V1).unwrap();

    assert_eq!(
        manifest.instance_id().to_string(),
        "0199f2a4-6b1e-7c3d-9e4f-5a6b7c8d9e0f"
    );
    assert_eq!(manifest.name().as_str(), "orders-db");
    assert_eq!(
        manifest.display_name().map(|name| name.as_str()),
        Some("Orders database")
    );
    assert_eq!(manifest.software().as_str(), "postgresql");
    assert_eq!(manifest.provider().provider().namespace(), "official");
    assert_eq!(manifest.provider().revision().as_str(), "2026.10.1");
    assert_eq!(
        manifest.provider().manifest_sha256().to_string(),
        "cb95f16d56a9ab7b938a03732ddf4ab691346124747fbb811ac6c09928f1fa6b"
    );

    let runtime = manifest.runtime();
    assert_eq!(
        runtime.id().to_string(),
        "0199f2a3-1d2e-7f30-8a41-b2c3d4e5f607"
    );
    assert_eq!(runtime.version().as_str(), "17.6");
    assert_eq!(runtime.line().as_str(), "17");
    assert_eq!(runtime.target(), Target::new(Os::Macos, Arch::Aarch64));
    assert_eq!(runtime.install_kind(), InstallKind::Managed);
    assert!(runtime.artifact_sha256().is_some());

    assert_eq!(manifest.data().initialized_line().as_str(), "17");
    assert_eq!(manifest.data().initialized_version().as_str(), "17.6");
    assert_eq!(
        manifest.data().initialized_at(),
        Some(at("2026-10-18T09:30:05Z"))
    );
    assert_eq!(manifest.created_at(), at("2026-10-18T09:30:00Z"));
    assert_eq!(
        manifest.created_by_operation().to_string(),
        "0199f2a4-6a00-7b11-8c22-d33e44f55a66"
    );
    assert_eq!(manifest.updated_at(), at("2026-10-18T11:00:00Z"));
    assert_eq!(manifest.stackly_version(), "0.1.0");

    let retained = manifest.retained().unwrap();
    assert_eq!(retained.retained_at(), at("2026-10-18T11:00:00Z"));
    assert_eq!(
        retained.by_operation().to_string(),
        "0199f2b0-0000-7123-8456-789abcdef012"
    );
}

#[test]
fn complete_v1_fixture_encodes_back_to_the_same_json() {
    let manifest: InstanceManifest = serde_json::from_str(COMPLETE_V1).unwrap();

    assert_eq!(serde_json::to_value(&manifest).unwrap(), json(COMPLETE_V1));
}

#[test]
fn unknown_fields_are_ignored_and_dropped_on_rewrite() {
    let manifest: InstanceManifest = serde_json::from_str(UNKNOWN_FIELDS_V1).unwrap();

    assert_eq!(manifest.display_name(), None);
    assert_eq!(manifest.runtime().install_kind(), InstallKind::External);
    assert_eq!(manifest.runtime().artifact_sha256(), None);
    assert_eq!(manifest.data().initialized_at(), None);
    assert_eq!(manifest.retained(), None);

    let mut expected = json(UNKNOWN_FIELDS_V1);
    let object = expected.as_object_mut().unwrap();
    object.remove("labels");
    object["provider"]
        .as_object_mut()
        .unwrap()
        .remove("signedBy");
    object["runtime"]
        .as_object_mut()
        .unwrap()
        .remove("observedAt");
    assert_eq!(serde_json::to_value(&manifest).unwrap(), expected);
}

#[test]
fn optional_fields_may_be_left_out_of_a_v1_manifest() {
    let mut value = json(UNKNOWN_FIELDS_V1);
    let object = value.as_object_mut().unwrap();
    object.remove("displayName");
    object.remove("retained");
    object["data"]
        .as_object_mut()
        .unwrap()
        .remove("initializedAt");
    object["runtime"]
        .as_object_mut()
        .unwrap()
        .remove("artifactSha256");

    let manifest: InstanceManifest = serde_json::from_value(value).unwrap();

    assert_eq!(manifest.display_name(), None);
    assert_eq!(manifest.data().initialized_at(), None);
    assert_eq!(manifest.retained(), None);
    assert_eq!(manifest.runtime().install_kind(), InstallKind::External);
}

#[test]
fn v2_manifest_does_not_decode_as_v1_but_its_schema_is_readable() {
    let err = serde_json::from_str::<InstanceManifest>(V2).unwrap_err();

    assert!(err.to_string().contains(INSTANCE_MANIFEST_SCHEMA), "{err}");
    assert_eq!(json(V2)["schema"], "stackly.instance-manifest/v2");
}

#[test]
fn invalid_values_in_a_v1_manifest_are_rejected() {
    let cases: [(&str, &[&str], Value); 6] = [
        (
            "instance ID of version 4",
            &["instanceId"],
            "9f1c2d3e-4b5a-4c6d-8e7f-0a1b2c3d4e5f".into(),
        ),
        ("uppercase name", &["name"], "Orders".into()),
        (
            "control character in display name",
            &["displayName"],
            "a\tb".into(),
        ),
        (
            "provider without namespace",
            &["provider", "name"],
            "postgresql".into(),
        ),
        ("short digest", &["runtime", "artifactSha256"], "abc".into()),
        (
            "timestamp without offset",
            &["createdAt"],
            "2026-10-18T09:30:00".into(),
        ),
    ];
    for (case, path, replacement) in cases {
        let mut value = json(COMPLETE_V1);
        let (last, parents) = path.split_last().unwrap();
        let mut target = &mut value;
        for key in parents {
            target = &mut target[*key];
        }
        target[*last] = replacement;

        assert!(
            serde_json::from_value::<InstanceManifest>(value).is_err(),
            "{case} was accepted"
        );
    }
}

#[test]
fn missing_required_fields_are_rejected() {
    for field in [
        "instanceId",
        "name",
        "software",
        "provider",
        "runtime",
        "data",
        "createdAt",
        "createdByOperation",
        "updatedAt",
        "stacklyVersion",
        "schema",
    ] {
        let mut value = json(COMPLETE_V1);
        value.as_object_mut().unwrap().remove(field);

        assert!(
            serde_json::from_value::<InstanceManifest>(value).is_err(),
            "manifest without {field} was accepted"
        );
    }
}
