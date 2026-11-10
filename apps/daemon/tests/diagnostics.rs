//! `diagnostics.preview_bundle` and `diagnostics.export_bundle` through the
//! real daemon (spec 0034 step 10, AC-2, AC-12, AC-13, AC-14, AC-22): the
//! fake provider in a test root on the macOS platform, a running instance
//! with logs, and an instance that crashed while printing its fixture
//! credential. A preview stages one zip and changes nothing else; an export
//! writes its exact bytes, refuses what it must, and leaves no partial; and
//! no form of a secret the daemon learned is in the report, the preview,
//! any entry of the bundle, or the daemon log.
#![cfg(target_os = "macos")]

#[path = "supervisor/support.rs"]
mod support;

use std::collections::BTreeMap;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::json;
use sha2::{Digest, Sha256};
use stackly_domain::Registry as _;
use stackly_protocol::diagnostics::{
    BundleEntryKind, BundlePreview, ExportBundleParams, ExportedBundle,
};
use stackly_protocol::doctor::DoctorParams;
use stackly_protocol::instance::{InstanceLifecycle, SecretFieldInput};
use stackly_protocol::operation::{ListOperationsParams, OperationState};
use stackly_protocol::secret::{SecretInput, SecretText};
use stackly_protocol::{Call, ErrorBody, Reply};
use stackly_testkit::evidence::{FixtureCredential, Kind, evidence};
use uuid::Uuid;

use crate::support::{Api, Site};

async fn preview(api: &Api) -> Result<BundlePreview, Box<ErrorBody>> {
    match api.client.call(Call::DiagnosticsPreviewBundle).await? {
        Reply::DiagnosticsPreviewBundle(preview) => Ok(*preview),
        other => panic!("not a preview reply: {other:?}"),
    }
}

async fn export(
    api: &Api,
    preview_id: Uuid,
    destination: &Path,
    replace: bool,
) -> Result<ExportedBundle, Box<ErrorBody>> {
    let call = Call::DiagnosticsExportBundle(ExportBundleParams {
        preview_id,
        destination: destination.display().to_string(),
        replace,
    });
    match api.client.call(call).await? {
        Reply::DiagnosticsExportBundle(exported) => Ok(exported),
        other => panic!("not an export reply: {other:?}"),
    }
}

/// Every entry of a zip, decompressed.
fn unzip(bytes: &[u8]) -> BTreeMap<String, Vec<u8>> {
    stackly_core::diagnostics::testing::unzip(bytes)
}

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// The sha256 of every file under `folder`, by relative path.
fn digests(folder: &Path) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut stack = vec![folder.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("the folder lists") {
            let entry = entry.expect("an entry reads");
            let path = entry.path();
            let kind = entry.file_type().expect("the entry has a type");
            if kind.is_dir() {
                stack.push(path);
            } else if kind.is_file() {
                let rel = path
                    .strip_prefix(folder)
                    .expect("inside the folder")
                    .display()
                    .to_string();
                out.push((rel, sha256(&std::fs::read(&path).expect("a file reads"))));
            }
        }
    }
    out.sort();
    out
}

async fn operation_count(api: &Api) -> usize {
    match api
        .client
        .call(Call::OperationList(ListOperationsParams::default()))
        .await
        .expect("operation.list answers")
    {
        Reply::OperationList(page) => page.items.len(),
        other => panic!("not an operation page: {other:?}"),
    }
}

async fn stop(daemon: stacklyd::server::DaemonHandle) {
    daemon
        .shutdown(
            stackly_protocol::events::ShutdownReason::Requested,
            std::future::pending(),
        )
        .await;
}

fn out_dir(site: &Site) -> PathBuf {
    let out = site.root.path().join("out");
    if !out.exists() {
        std::fs::create_dir(&out).expect("the export folder can be made");
    }
    out
}

// covers: AC-2, AC-12, AC-13, AC-22 (spec 0034)
#[tokio::test(flavor = "multi_thread")]
async fn a_preview_stages_the_bundle_alone_and_an_export_writes_its_exact_bytes() {
    let ev = evidence!(
        covers = ["REQ-019", "NFR-016"],
        kind = Kind::Fixture,
        expected = "diagnostics.preview_bundle answers README, doctor, system, instances, runtimes, operations, and the instance log of a running instance, stages one 0600 zip in a 0700 cache folder, and changes no registry row, operation, or data file; diagnostics.export_bundle writes the same sha256 to the destination and leaves no partial",
    );
    let site = Site::new();
    let daemon = site.start().await;
    let api = site.api().await;
    let id = api.create_ready("alpha", json!({})).await;
    api.start_ready(id).await;
    let registry = daemon.core().registry().expect("the registry is open");
    let generation = registry
        .known_instance_ids()
        .expect("the ids read")
        .generation;
    let operations = operation_count(&api).await;
    let data = site.instance_paths(id).data;
    let data_before = digests(&data);

    let preview = preview(&api).await.expect("the preview is built");

    let paths: Vec<&str> = preview
        .entries
        .iter()
        .map(|entry| entry.path.as_str())
        .collect();
    for path in [
        "README.txt",
        "doctor.json",
        "system.json",
        "instances.json",
        "runtimes.json",
        "operations.json",
        "logs/instances/alpha.log",
    ] {
        assert!(paths.contains(&path), "{path} in {paths:?}");
    }
    assert_eq!(paths[0], "README.txt");
    assert!(
        paths.iter().all(|path| !path.contains("config")),
        "no rendered config: {paths:?}"
    );
    assert!(!preview.recovery);
    assert!(
        preview
            .suggested_file_name
            .starts_with("stackly-diagnostics-")
            && preview.suggested_file_name.ends_with("Z.zip"),
        "{}",
        preview.suggested_file_name
    );
    assert_eq!(
        preview
            .expires_at
            .as_jiff()
            .duration_since(preview.created_at.as_jiff()),
        jiff::SignedDuration::from_mins(15),
    );
    let staged = site
        .prepared
        .layout()
        .diagnostics_cache_dir()
        .join(format!("{}.zip", preview.preview_id));
    let zip = std::fs::read(&staged).expect("the preview is staged");
    assert_eq!(std::fs::metadata(&staged).unwrap().mode() & 0o7777, 0o600);
    assert_eq!(
        std::fs::metadata(staged.parent().unwrap()).unwrap().mode() & 0o7777,
        0o700
    );
    assert_eq!(u64::from(preview.zip_bytes), zip.len() as u64);
    let entries = unzip(&zip);
    let listed: Vec<&str> = entries.keys().map(String::as_str).collect();
    let mut sorted = paths.clone();
    sorted.sort_unstable();
    assert_eq!(listed, sorted);
    let total: u64 = entries.values().map(|bytes| bytes.len() as u64).sum();
    assert_eq!(u64::from(preview.total_bytes), total);
    let instances = String::from_utf8(entries["instances.json"].clone()).unwrap();
    assert!(instances.contains("\"alpha\""), "{instances}");
    let log = String::from_utf8(entries["logs/instances/alpha.log"].clone()).unwrap();
    assert!(
        log.contains(" stdout ") || log.contains(" stderr "),
        "{log}"
    );
    assert!(
        entries["README.txt"]
            .windows(b"Never included".len())
            .any(|window| window == b"Never included")
    );

    assert_eq!(
        registry
            .known_instance_ids()
            .expect("the ids read")
            .generation,
        generation,
        "a preview writes no registry row"
    );
    assert_eq!(operation_count(&api).await, operations, "and no operation");
    assert_eq!(digests(&data), data_before, "and no data file");

    let target = out_dir(&site).join(&preview.suggested_file_name);
    let exported = export(&api, preview.preview_id, &target, false)
        .await
        .expect("the export succeeds");
    let written = std::fs::read(&target).expect("the bundle is written");
    assert_eq!(written, zip);
    assert_eq!(exported.sha256, sha256(&zip));
    assert_eq!(u64::from(exported.bytes), zip.len() as u64);
    assert_eq!(
        PathBuf::from(&exported.path),
        target.canonicalize().unwrap()
    );
    assert_eq!(
        std::fs::read_dir(out_dir(&site)).unwrap().count(),
        1,
        "no partial file is left"
    );
    ev.pass();
}

// covers: AC-13 (spec 0034)
#[tokio::test(flavor = "multi_thread")]
async fn an_export_refuses_what_it_must_and_replaces_only_when_asked() {
    let ev = evidence!(
        covers = ["NFR-016"],
        kind = Kind::Fixture,
        expected = "an existing destination without replace answers diagnostics.destination_exists with its hint and keeps the file, replace overwrites it, a read only folder answers write_failed permission without a partial, a relative path and a missing parent answer destination_invalid, and an unknown or pushed out preview answers preview_not_found with its hint",
    );
    let site = Site::new();
    let _daemon = site.start().await;
    let api = site.api().await;
    let first = preview(&api).await.expect("the preview is built");
    let out = out_dir(&site);

    let target = out.join("bundle.zip");
    std::fs::write(&target, "old").unwrap();
    let refused = export(&api, first.preview_id, &target, false)
        .await
        .unwrap_err();
    assert_eq!(refused.code(), "diagnostics.destination_exists");
    assert_eq!(
        refused.hint(),
        Some("Choose another path, or pass --force to replace it.")
    );
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "old");
    export(&api, first.preview_id, &target, true)
        .await
        .expect("replace overwrites the file");
    assert_ne!(std::fs::read(&target).unwrap(), b"old");

    let locked = out.join("locked");
    std::fs::create_dir(&locked).unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o500)).unwrap();
    let failed = export(&api, first.preview_id, &locked.join("b.zip"), false)
        .await
        .unwrap_err();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(failed.code(), "diagnostics.write_failed");
    assert_eq!(failed.detail("reason"), Some("permission"));
    assert_eq!(std::fs::read_dir(&locked).unwrap().count(), 0, "no partial");

    for (destination, reason) in [
        (PathBuf::from("relative.zip"), "not_absolute"),
        (out.join("missing/b.zip"), "parent_missing"),
        (out.clone(), "is_directory"),
    ] {
        let invalid = export(&api, first.preview_id, &destination, true)
            .await
            .unwrap_err();
        assert_eq!(invalid.code(), "diagnostics.destination_invalid");
        assert_eq!(invalid.detail("reason"), Some(reason), "{destination:?}");
    }

    let unknown = export(&api, Uuid::now_v7(), &out.join("x.zip"), false)
        .await
        .unwrap_err();
    assert_eq!(unknown.code(), "diagnostics.preview_not_found");
    assert_eq!(unknown.hint(), Some("Preview the bundle again."));
    preview(&api).await.expect("a second preview");
    preview(&api)
        .await
        .expect("a third preview pushes out the first");
    let gone = export(&api, first.preview_id, &out.join("y.zip"), false)
        .await
        .unwrap_err();
    assert_eq!(gone.code(), "diagnostics.preview_not_found");
    ev.pass();
}

// covers: AC-12 (spec 0034)
#[tokio::test(flavor = "multi_thread")]
async fn in_recovery_mode_the_preview_leaves_out_the_registry_entries_and_says_so() {
    let ev = evidence!(
        covers = ["REQ-019"],
        kind = Kind::Fixture,
        expected = "with the registry in recovery mode diagnostics.preview_bundle answers recovery true without instances, runtimes, or operations, README.txt names each one left out, and the registry file stays byte for byte",
    );
    let site = Site::new();
    let db = site.prepared.layout().registry_db();
    let mut bytes = std::fs::read(&db).expect("the registry file reads");
    bytes[60..64].copy_from_slice(&999_u32.to_be_bytes());
    std::fs::write(&db, &bytes).expect("the registry file is written");
    let before = std::fs::read(&db).unwrap();
    let daemon = site.start().await;
    let api = site.api().await;
    let preview = preview(&api).await.expect("the preview works in recovery");
    assert!(preview.recovery);
    let kinds: Vec<BundleEntryKind> = preview.entries.iter().map(|entry| entry.kind).collect();
    for left_out in [
        BundleEntryKind::Instances,
        BundleEntryKind::Runtimes,
        BundleEntryKind::Operations,
    ] {
        assert!(!kinds.contains(&left_out), "{left_out:?}");
    }
    let staged = site
        .prepared
        .layout()
        .diagnostics_cache_dir()
        .join(format!("{}.zip", preview.preview_id));
    let entries = unzip(&std::fs::read(staged).unwrap());
    let readme = String::from_utf8(entries["README.txt"].clone()).unwrap();
    assert!(
        readme.contains("instances.json is left out: the registry is in recovery mode"),
        "{readme}"
    );
    assert!(readme.contains("logs/instances/ is left out"), "{readme}");
    let doctor = String::from_utf8(entries["doctor.json"].clone()).unwrap();
    assert!(doctor.contains("\"recovery\": true"), "{doctor}");
    drop(api);
    stop(daemon).await;
    assert_eq!(std::fs::read(&db).unwrap(), before);
    ev.pass();
}

// covers: AC-14, AC-22 (spec 0034)
#[tokio::test(flavor = "multi_thread")]
async fn no_form_of_a_learned_secret_reaches_the_report_the_preview_the_bundle_or_the_daemon_log() {
    let ev = evidence!(
        covers = ["REQ-019", "NFR-016"],
        kind = Kind::Fixture,
        expected = "a fake that prints its fixture credential to stdout and stderr and crashes, and a daemon log line holding the credential after the start learned it, leave no trace of the credential in doctor.run, diagnostics.preview_bundle, any decompressed entry of the exported bundle, or the daemon log, while the bundle still holds the instance log and the failure",
    );
    let site = Site::new();
    let mut config = support::fast();
    config.supervisor.restart_base_delay = Duration::from_millis(20);
    config.supervisor.restart_max_delay = Duration::from_millis(400);
    let daemon = site.start_with(config).await;
    let api = site.api().await;
    let credential = FixtureCredential::new();
    let mut params = api.params(
        "leaky",
        json!({
            "crashAfterMs": 150,
            "crashExitCode": 3,
            "readyDelayMs": 60_000,
            "leakToken": true,
        }),
    );
    params.secrets = vec![SecretFieldInput {
        field: "authToken".to_owned(),
        input: SecretInput::Value(SecretText::new(credential.value().to_owned())),
    }];
    let accepted = api
        .create(params)
        .await
        .expect("the create is accepted")
        .accepted
        .expect("a create starts an operation");
    api.wait_state(accepted.operation.id, OperationState::Succeeded)
        .await;
    let id: Uuid = accepted
        .operation
        .resource
        .as_ref()
        .map(|resource| resource.id.parse().expect("an instance id"))
        .expect("the operation names the instance");
    let started = api
        .start(id)
        .await
        .expect("the start is accepted")
        .accepted
        .expect("a start begins an operation")
        .operation
        .id;
    api.wait_final(started).await;
    let failed = api.get(id).await.expect("the instance reads");
    assert_eq!(failed.lifecycle, InstanceLifecycle::Failed);

    // A daemon log line with the credential, as if written before it was
    // redacted: the bundle redacts it with what the start taught.
    let log_dir = site.prepared.layout().daemon_log_dir();
    std::fs::create_dir_all(&log_dir).unwrap();
    let log_file = log_dir.join("stacklyd.log");
    let mut text = std::fs::read_to_string(&log_file).unwrap_or_default();
    text.push_str(&format!(
        "{{\"message\":\"token {}\"}}\n",
        credential.value()
    ));
    std::fs::write(&log_file, text).unwrap();

    let report = match api
        .client
        .call(Call::DoctorRun(DoctorParams::default()))
        .await
        .expect("doctor.run answers")
    {
        Reply::DoctorRun(report) => *report,
        other => panic!("not a doctor reply: {other:?}"),
    };
    credential.assert_absent(
        &serde_json::to_string(&report).unwrap(),
        "the doctor report",
    );
    let preview = preview(&api).await.expect("the preview is built");
    credential.assert_absent(&serde_json::to_string(&preview).unwrap(), "the preview");
    let target = out_dir(&site).join("leaky.zip");
    export(&api, preview.preview_id, &target, false)
        .await
        .expect("the export succeeds");
    let entries = unzip(&std::fs::read(&target).unwrap());
    for (path, bytes) in &entries {
        credential.assert_absent(&String::from_utf8_lossy(bytes), path);
    }
    let log = String::from_utf8(entries["logs/instances/leaky.log"].clone()).unwrap();
    assert!(log.contains("****"), "the redacted output is kept: {log}");
    let daemon_log = String::from_utf8(entries["logs/stacklyd.log"].clone()).unwrap();
    assert!(daemon_log.contains("token ****"), "{daemon_log}");
    let instances = String::from_utf8(entries["instances.json"].clone()).unwrap();
    assert!(instances.contains("lastFailure"), "{instances}");
    drop(api);
    stop(daemon).await;
    for file in std::fs::read_dir(&log_dir).unwrap() {
        let path = file.unwrap().path();
        let written = std::fs::read_to_string(&path).unwrap_or_default();
        let without_planted = written.replace(
            &format!("{{\"message\":\"token {}\"}}\n", credential.value()),
            "",
        );
        credential.assert_absent(&without_planted, "the daemon log");
    }
    ev.pass();
}
