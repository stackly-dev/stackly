//! Checks the runtime DTO fixtures in `protocols/fixtures/runtime/`, which
//! the CLI goldens and the Swift runtime tests read (spec 0018 AC-18), and
//! the runtime frames in `protocols/fixtures/ipc/` that carry them.

use std::collections::BTreeSet;
use std::path::PathBuf;

use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use stackly_protocol::catalog::{
    DependencyKind, SupportReason, SupportStatus, TargetArch, TargetOs,
};
use stackly_protocol::operation::OperationKind;
use stackly_protocol::runtime::{
    ArtifactFormat, CompatibilityResult, CompatibilityScope, InstallModeKind, InstallOutcome,
    ResolvedArtifact, RuntimeDetail, RuntimeListPage, RuntimeState, SignatureKindDto,
    SigningPolicy,
};
use stackly_protocol::{ErrorFrame, Reply, ServerFrame, codes};

#[derive(Debug, Deserialize)]
struct Index {
    fixtures: Vec<Entry>,
}

#[derive(Debug, Deserialize)]
struct Entry {
    file: String,
    #[serde(rename = "type")]
    type_name: String,
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../protocols/fixtures")
}

fn read(path: &str) -> Vec<u8> {
    let path = fixtures_dir().join(path);
    std::fs::read(&path).unwrap_or_else(|err| panic!("cannot read {}: {err}", path.display()))
}

fn decode<T: DeserializeOwned>(path: &str) -> T {
    serde_json::from_slice(&read(path)).unwrap_or_else(|err| panic!("{path}: {err}"))
}

fn index() -> Vec<Entry> {
    decode::<Index>("runtime/index.json").fixtures
}

fn reply(file: &str) -> Reply {
    let ServerFrame::Response(response) = decode(&format!("ipc/{file}")) else {
        panic!("{file}: expected a response");
    };
    response.reply
}

fn assert_roundtrip<T>(file: &str)
where
    T: DeserializeOwned + serde::Serialize,
{
    let bytes = read(&format!("runtime/{file}"));
    let value: T = serde_json::from_slice(&bytes).unwrap_or_else(|err| panic!("{file}: {err}"));
    let written = serde_json::to_value(&value).expect("a decoded value always encodes");
    let fixture: Value = serde_json::from_slice(&bytes).expect("a fixture is JSON");
    assert_eq!(written, fixture, "{file}: Rust writes another shape");
}

// covers: AC-18
#[test]
fn every_runtime_fixture_roundtrips_as_its_type() {
    for entry in index() {
        match entry.type_name.as_str() {
            "RuntimeListPage" => assert_roundtrip::<RuntimeListPage>(&entry.file),
            "RuntimeDetail" => assert_roundtrip::<RuntimeDetail>(&entry.file),
            "ResolvedArtifact" => assert_roundtrip::<ResolvedArtifact>(&entry.file),
            other => panic!("{}: no decoder for type {other}", entry.file),
        }
    }
}

#[test]
fn every_file_in_the_runtime_fixture_folder_is_listed_in_the_index() {
    let listed: BTreeSet<String> = index().into_iter().map(|entry| entry.file).collect();
    let present: BTreeSet<String> = std::fs::read_dir(fixtures_dir().join("runtime"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .filter(|name| name != "index.json")
        .collect();
    assert_eq!(listed, present);
}

#[test]
fn the_runtime_frames_carry_the_runtime_fixtures() {
    let Reply::RuntimeList(page) = reply("frame-response-runtime-list.json") else {
        panic!("expected a runtime.list reply");
    };
    assert_eq!(*page, decode::<RuntimeListPage>("runtime/list.json"));
    let Reply::RuntimeList(last) = reply("frame-response-runtime-list-last-page.json") else {
        panic!("expected a runtime.list reply");
    };
    assert_eq!(
        *last,
        decode::<RuntimeListPage>("runtime/list-last-page.json")
    );
    let Reply::RuntimeGet(detail) = reply("frame-response-runtime-get.json") else {
        panic!("expected a runtime.get reply");
    };
    assert_eq!(
        *detail,
        decode::<RuntimeDetail>("runtime/detail-postgresql.json")
    );
    let Reply::RuntimeResolve(resolved) = reply("frame-response-runtime-resolve.json") else {
        panic!("expected a runtime.resolve reply");
    };
    assert_eq!(
        *resolved,
        decode::<ResolvedArtifact>("runtime/resolved-postgresql.json")
    );
}

#[test]
fn an_install_reply_is_started_or_already_installed_and_names_the_resolved_artifact() {
    let resolved = decode::<ResolvedArtifact>("runtime/resolved-postgresql.json");
    let Reply::RuntimeInstall(started) = reply("frame-response-runtime-install-started.json")
    else {
        panic!("expected a runtime.install reply");
    };
    assert_eq!(started.resolved, resolved);
    let InstallOutcome::Started(accepted) = started.outcome else {
        panic!("expected a started outcome");
    };
    assert!(!accepted.replayed);
    assert_eq!(accepted.operation.kind, OperationKind::InstallRuntime);

    let Reply::RuntimeInstall(done) =
        reply("frame-response-runtime-install-already-installed.json")
    else {
        panic!("expected a runtime.install reply");
    };
    let InstallOutcome::AlreadyInstalled(runtime) = done.outcome else {
        panic!("expected an alreadyInstalled outcome");
    };
    assert_eq!(runtime.state, RuntimeState::Installed);
    assert_eq!(
        runtime.artifact_sha256.as_deref(),
        Some(resolved.sha256.as_str())
    );
}

#[test]
fn every_detail_summary_is_its_list_item() {
    let list = decode::<RuntimeListPage>("runtime/list.json");
    let detail = decode::<RuntimeDetail>("runtime/detail-postgresql.json");
    assert!(list.items.contains(&detail.summary));
    assert_eq!(
        detail
            .artifact
            .as_ref()
            .map(|artifact| artifact.sha256.as_str()),
        detail.summary.artifact_sha256.as_deref()
    );
    for record in &detail.compatibility {
        assert_eq!(
            Some(record.sha256.as_str()),
            detail.summary.artifact_sha256.as_deref()
        );
    }
}

#[test]
fn foreign_runtime_values_decode_as_unrecognized() {
    let Reply::RuntimeGet(detail) = reply("frame-response-runtime-get-unknown-values.json") else {
        panic!("expected a runtime.get reply");
    };
    assert_eq!(detail.summary.install_mode, InstallModeKind::Unrecognized);
    assert_eq!(detail.summary.state, RuntimeState::Unrecognized);
    assert_eq!(detail.summary.support, SupportStatus::Unrecognized);
    assert_eq!(detail.summary.support_reason, SupportReason::Unrecognized);
    assert_eq!(detail.summary.target.os, TargetOs::Unrecognized);
    assert_eq!(detail.summary.target.arch, TargetArch::Unrecognized);
    let artifact = detail.artifact.expect("the fixture has an artifact");
    assert_eq!(artifact.format, Some(ArtifactFormat::Unrecognized));
    let signature = artifact.signature.expect("the fixture has a signature");
    assert_eq!(signature.policy, SigningPolicy::Unrecognized);
    assert_eq!(
        signature.executables[0].kind,
        SignatureKindDto::Unrecognized
    );
    assert_eq!(artifact.dependencies[0].kind, DependencyKind::Unrecognized);
    let record = &detail.compatibility[0];
    assert_eq!(record.scope, CompatibilityScope::Unrecognized);
    assert_eq!(record.result, CompatibilityResult::Unrecognized);
    assert_eq!(record.os, TargetOs::Unrecognized);
    assert_eq!(record.arch, TargetArch::Unrecognized);

    let Reply::RuntimeResolve(resolved) =
        reply("frame-response-runtime-resolve-unknown-values.json")
    else {
        panic!("expected a runtime.resolve reply");
    };
    assert_eq!(resolved.format, ArtifactFormat::Unrecognized);
    assert_eq!(resolved.support, SupportStatus::Unrecognized);
    assert_eq!(resolved.support_reason, SupportReason::Unrecognized);
}

#[test]
fn the_runtime_errors_carry_their_catalog_details_and_static_hint() {
    for (file, code) in [
        (
            "frame-error-runtime-selector-unresolved.json",
            &codes::runtime::SELECTOR_UNRESOLVED,
        ),
        (
            "frame-error-runtime-download-offline.json",
            &codes::runtime::DOWNLOAD_OFFLINE,
        ),
    ] {
        let ServerFrame::Error(ErrorFrame { error, .. }) = decode(&format!("ipc/{file}")) else {
            panic!("{file}: expected an error frame");
        };
        assert_eq!(error.code(), code.code, "{file}");
        let keys: Vec<&str> = error.details().iter().map(|detail| detail.key()).collect();
        assert_eq!(keys, code.details, "{file}");
        assert_eq!(
            error.hint(),
            codes::runtime::hint(code.code, error.details()),
            "{file}"
        );
    }
}
