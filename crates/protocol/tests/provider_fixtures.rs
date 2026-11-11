//! Checks the `provider.validate` fixtures (spec 0036, *Kontrak wire*): the
//! reply DTOs in `protocols/fixtures/provider/`, which the Swift tests read
//! too, and the request and response frames in `protocols/fixtures/ipc/`.

use std::path::PathBuf;

use serde::Deserialize;
use serde_json::Value;
use stackly_protocol::json::decode_request;
use stackly_protocol::provider::{
    ApiVersionStatus, FindingOrigin, FindingSeverity, ProviderValidation,
};
use stackly_protocol::{Call, Reply, ServerFrame};

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

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../protocols/fixtures")
}

fn read(path: &str) -> Vec<u8> {
    let path = fixtures_dir().join(path);
    std::fs::read(&path).unwrap_or_else(|err| panic!("cannot read {}: {err}", path.display()))
}

fn validation(file: &str) -> ProviderValidation {
    serde_json::from_slice(&read(&format!("provider/{file}")))
        .unwrap_or_else(|err| panic!("{file}: {err}"))
}

#[test]
fn every_provider_fixture_decodes_and_the_roundtrip_ones_keep_their_shape() {
    let index: Index = serde_json::from_slice(&read("provider/index.json")).unwrap();
    let mut files: Vec<String> = std::fs::read_dir(fixtures_dir().join("provider"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name != "index.json")
        .collect();
    files.sort();
    let mut listed: Vec<String> = index.fixtures.iter().map(|e| e.file.clone()).collect();
    listed.sort();
    assert_eq!(files, listed, "every fixture file is listed in index.json");
    for entry in &index.fixtures {
        assert_eq!(entry.type_name, "ProviderValidation");
        let bytes = read(&format!("provider/{}", entry.file));
        let value: ProviderValidation =
            serde_json::from_slice(&bytes).unwrap_or_else(|err| panic!("{}: {err}", entry.file));
        assert_eq!(
            u64::from(value.finding_count) - u64::from(value.omitted),
            value.findings.len() as u64,
            "{}: omitted is findingCount minus the findings sent",
            entry.file
        );
        if entry.expect == "roundtrip" {
            let fixture: Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(
                serde_json::to_value(&value).unwrap(),
                fixture,
                "{}: Rust writes another shape",
                entry.file
            );
        }
    }
}

#[test]
fn a_publishable_package_has_its_identity_and_no_finding() {
    let reply = validation("validation-publishable.json");
    assert!(reply.loadable && reply.publishable);
    let identity = reply.identity.expect("a validated package has an identity");
    assert_eq!(identity.provider, "official/nats");
    assert_eq!(identity.manifest_sha256.len(), 64);
    assert!(reply.findings.is_empty());
    assert_eq!(reply.supported_api_versions[0].name, "stackly/v1alpha1");
    assert_eq!(
        reply.supported_api_versions[0].status,
        ApiVersionStatus::Current
    );
}

#[test]
fn an_unsupported_api_version_is_one_package_error_and_no_identity() {
    let reply = validation("validation-unsupported-api-version.json");
    assert!(!reply.loadable && !reply.publishable);
    assert!(reply.identity.is_none());
    assert_eq!(reply.api_version.as_deref(), Some("stackly/v2"));
    let [finding] = reply.findings.as_slice() else {
        panic!("exactly one finding");
    };
    assert_eq!(finding.severity, FindingSeverity::Error);
    assert_eq!(finding.origin, FindingOrigin::Package);
    assert_eq!(finding.code, "unsupported_api_version");
    assert_eq!(
        (finding.file.as_str(), finding.location.as_str()),
        ("provider.yaml", "apiVersion")
    );
    assert!(finding.message.contains("stackly/v1alpha1"));
}

#[test]
fn an_authoring_finding_keeps_the_package_loadable_but_not_publishable() {
    let reply = validation("validation-authoring.json");
    assert!(reply.loadable && !reply.publishable);
    assert!(reply.identity.is_some());
    assert_eq!(reply.findings[0].origin, FindingOrigin::Authoring);
    assert_eq!(reply.findings[0].code, "token_not_allowed_here");
}

#[test]
fn values_a_newer_daemon_sends_read_unrecognized() {
    let reply = validation("validation-unknown-values.json");
    assert_eq!(
        reply.supported_api_versions[0].status,
        ApiVersionStatus::Unrecognized
    );
    assert_eq!(reply.findings[0].severity, FindingSeverity::Unrecognized);
    assert_eq!(reply.findings[0].origin, FindingOrigin::Unrecognized);
    assert_eq!(reply.findings[0].reason, None);
    assert!(reply.identity.is_none());
    assert_eq!(u64::from(reply.omitted), 2);
}

#[test]
fn the_ipc_frames_carry_the_method_and_its_reply() {
    let request = decode_request(&read("ipc/request-provider-validate.json")).unwrap();
    assert!(request.call.allowed_in_recovery());
    let Call::ProviderValidate(params) = request.call else {
        panic!("expected provider.validate");
    };
    assert_eq!(params.path, "/Users/dev/src/stackly/providers/mytool");

    let rejection = decode_request(&read("ipc/reject-provider-validate-relative-path.json"))
        .expect_err("a relative path is refused");
    assert_eq!(rejection.error.code(), "request.invalid_params");
    assert_eq!(
        rejection.error.fields().first().map(|field| field.path()),
        Some("path")
    );

    let frame: ServerFrame =
        serde_json::from_slice(&read("ipc/frame-response-provider-validate.json")).unwrap();
    let ServerFrame::Response(response) = frame else {
        panic!("expected a response");
    };
    let Reply::ProviderValidate(reply) = response.reply else {
        panic!("expected a provider.validate reply");
    };
    assert_eq!(
        *reply,
        validation("validation-unsupported-api-version.json")
    );
}
