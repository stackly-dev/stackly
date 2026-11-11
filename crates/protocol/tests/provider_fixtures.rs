//! Checks the provider fixtures: the `provider.validate` reply DTOs (spec
//! 0036, *Kontrak wire*) and the import preview and local detail DTOs (spec
//! 0037) in `protocols/fixtures/provider/`, which the Swift tests read too,
//! the approval statement, and the request and response frames in
//! `protocols/fixtures/ipc/`.

use std::path::PathBuf;

use serde::Deserialize;
use serde_json::Value;
use stackly_protocol::catalog::{ProviderDetail, SupportReason, TrustLevel};
use stackly_protocol::json::decode_request;
use stackly_protocol::provider::{
    ApiVersionStatus, ApprovalState, CatalogChangeKind, ExecutableUse, FindingOrigin,
    FindingSeverity, ImportBlockReason, ImportPreview, PermissionChange, ProviderValidation,
    RevocationSource,
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
        .filter(|name| name != "index.json" && name.ends_with(".json"))
        .collect();
    files.sort();
    let mut listed: Vec<String> = index.fixtures.iter().map(|e| e.file.clone()).collect();
    listed.sort();
    assert_eq!(files, listed, "every fixture file is listed in index.json");
    for entry in &index.fixtures {
        if entry.type_name != "ProviderValidation" {
            check_import_fixture(entry);
            continue;
        }
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

/// Decodes an import fixture as its type and, for a roundtrip one, checks
/// that Rust writes the same shape.
fn check_import_fixture(entry: &Entry) {
    let bytes = read(&format!("provider/{}", entry.file));
    let written = match entry.type_name.as_str() {
        "ImportPreview" => serde_json::to_value(
            serde_json::from_slice::<ImportPreview>(&bytes)
                .unwrap_or_else(|err| panic!("{}: {err}", entry.file)),
        ),
        "ProviderDetail" => serde_json::to_value(
            serde_json::from_slice::<ProviderDetail>(&bytes)
                .unwrap_or_else(|err| panic!("{}: {err}", entry.file)),
        ),
        other => panic!("{}: unknown fixture type {other}", entry.file),
    }
    .expect("a decoded fixture encodes");
    if entry.expect == "roundtrip" {
        let fixture: Value = serde_json::from_slice(&bytes).expect("the fixture is JSON");
        assert_eq!(
            written, fixture,
            "{}: Rust writes another shape",
            entry.file
        );
    }
}

fn import_fixture<T: serde::de::DeserializeOwned>(file: &str) -> T {
    serde_json::from_slice(&read(&format!("provider/{file}")))
        .unwrap_or_else(|err| panic!("{file}: {err}"))
}

#[test]
fn the_approval_statement_is_one_line_without_sandbox_or_safety_claims() {
    let text = String::from_utf8(read("provider/approval-statement.txt")).unwrap();
    let line = text
        .strip_suffix('\n')
        .expect("the file ends with one newline");
    assert!(!line.contains('\n'), "one line");
    assert!(line.starts_with("This provider runs native programs on your Mac"));
    assert!(line.contains("Stackly does not sandbox them"));
    let lower = line.to_lowercase();
    for phrase in [
        "sandboxed",
        "safe to run",
        "verified publisher",
        "trusted publisher",
        "malware",
        "up to date",
        "latest",
    ] {
        assert!(!lower.contains(phrase), "{phrase}");
    }
}

#[test]
fn the_import_previews_carry_their_verdict_existing_import_and_changes() {
    let pending: ImportPreview = import_fixture("import-preview-pending.json");
    assert!(pending.verdict.importable && pending.verdict.reason.is_none());
    assert_eq!(pending.publisher.as_deref(), Some("acme"));
    assert!(pending.permissions.is_some() && pending.existing.is_none());
    let invalid: ImportPreview = import_fixture("import-preview-invalid.json");
    assert_eq!(
        invalid.verdict.reason,
        Some(ImportBlockReason::PackageInvalid)
    );
    assert_eq!(invalid.import_issues[0].code, "namespace_not_allowed");
    let replaces: ImportPreview = import_fixture("import-preview-replaces.json");
    let existing = replaces.existing.expect("it replaces an import");
    assert_eq!(existing.approval, ApprovalState::Approved);
    assert!(!existing.same_package);
    assert_eq!(
        replaces.changes,
        [
            PermissionChange::Revision,
            PermissionChange::Downloads,
            PermissionChange::DownloadHosts
        ]
    );
}

#[test]
fn import_values_this_build_does_not_know_read_unrecognized() {
    let preview: ImportPreview = import_fixture("import-preview-unknown-values.json");
    assert_eq!(
        preview.verdict.reason,
        Some(ImportBlockReason::Unrecognized)
    );
    assert_eq!(preview.changes, [PermissionChange::Unrecognized]);
    assert_eq!(
        preview.existing.map(|existing| existing.approval),
        Some(ApprovalState::Unrecognized)
    );
    assert_eq!(
        preview.revocation.map(|revocation| revocation.by),
        Some(RevocationSource::Unrecognized)
    );
    let permissions = preview.permissions.expect("permissions");
    assert_eq!(
        permissions.executables[0].uses,
        [ExecutableUse::Start, ExecutableUse::Unrecognized]
    );
    let change: CatalogChangeKind = serde_json::from_str("\"renamed\"").unwrap();
    assert_eq!(change, CatalogChangeKind::Unrecognized);
}

#[test]
fn a_local_detail_is_local_trust_with_its_approval_and_decision() {
    let pending: ProviderDetail = import_fixture("detail-local-pending.json");
    assert_eq!(pending.summary.trust, TrustLevel::Local);
    assert_eq!(pending.summary.approval, Some(ApprovalState::Pending));
    assert_eq!(
        pending.summary.state_reason,
        Some(SupportReason::ApprovalRequired)
    );
    assert!(
        pending
            .local
            .as_ref()
            .is_some_and(|local| local.decision.is_none())
    );
    let revoked: ProviderDetail = import_fixture("detail-local-revoked.json");
    assert_eq!(
        revoked.summary.state_reason,
        Some(SupportReason::TrustRevoked)
    );
    assert_eq!(
        revoked.revocation.map(|revocation| revocation.by),
        Some(RevocationSource::User)
    );
    let approved: ProviderDetail = import_fixture("detail-local-approved.json");
    assert!(approved.versions[0].install.installable);
}
