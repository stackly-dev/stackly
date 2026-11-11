//! Approving and revoking local providers through the real daemon (spec
//! 0037 milestone 2, build step 6): `provider.approve` binds exactly the
//! stored bytes, `provider.revoke` closes the gate for every approved
//! revision, a decision follows the bytes it covers across imports, the
//! refusals carry their codes and hints, and nothing but these two methods
//! changes a trust decision (VAL-018).
#![cfg(target_os = "macos")]

mod common;
#[path = "supervisor/support.rs"]
mod support;

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use stackly_domain::{AuditQuery, OperationKind, Registry as _};
use stackly_ipc::{CallOptions, Client};
use stackly_protocol::catalog::{CatalogEntryState, ProviderDetail, SupportReason};
use stackly_protocol::events::{EventKind, SubscribeParams};
use stackly_protocol::json::DecodedFrame;
use stackly_protocol::provider::{
    ApprovalState, CatalogChangeKind, DecideProviderParams, ImportPreview, ImportProviderParams,
    PermissionChange, PreviewImportParams, ProviderTargetParams, RevocationSource,
};
use stackly_protocol::{Call, ErrorBody, Reply};
use stackly_testkit::{EMPTY_CATALOG_JSON, TestRoot};
use stacklyd::server::DaemonHandle;

use crate::support::{Site, fast_with_catalog, key};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/core/tests/fixtures/packages")
        .join(name)
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("the copy target can be created");
    for entry in std::fs::read_dir(from).expect("the folder can be listed") {
        let entry = entry.expect("the entry can be read");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("the entry has a type").is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).expect("the file can be copied");
        }
    }
}

fn package_in(root: &Path, fixture_name: &str, name: &str) -> PathBuf {
    let dir = root.join("packages").join(name);
    copy_dir(&fixture(fixture_name), &dir);
    dir
}

async fn call(client: &Client, call: Call) -> Result<Reply, Box<ErrorBody>> {
    client
        .call_detailed(call, CallOptions::default().with_idempotency_key(key()))
        .await
}

async fn preview(client: &Client, path: &Path) -> ImportPreview {
    let params = PreviewImportParams {
        path: path.display().to_string(),
    };
    match client
        .call(Call::ProviderPreviewImport(params))
        .await
        .expect("the preview answers")
    {
        Reply::ProviderPreviewImport(preview) => *preview,
        other => panic!("not a preview reply: {other:?}"),
    }
}

/// Previews and imports `path`, returning the detail and its digest.
async fn import(client: &Client, path: &Path) -> (ProviderDetail, String) {
    let digest = preview(client, path)
        .await
        .package_sha256
        .expect("the package has a digest");
    let params = ImportProviderParams {
        path: path.display().to_string(),
        expected_package_sha256: digest.clone(),
    };
    match call(client, Call::ProviderImport(params))
        .await
        .expect("it imports")
    {
        Reply::ProviderImport(detail) => (*detail, digest),
        other => panic!("not an import reply: {other:?}"),
    }
}

async fn approve(
    client: &Client,
    provider: &str,
    digest: &str,
) -> Result<ProviderDetail, Box<ErrorBody>> {
    let params = DecideProviderParams {
        provider: provider.to_owned(),
        expected_package_sha256: digest.to_owned(),
    };
    match call(client, Call::ProviderApprove(params)).await? {
        Reply::ProviderApprove(detail) => Ok(*detail),
        other => panic!("not an approve reply: {other:?}"),
    }
}

async fn revoke(client: &Client, provider: &str) -> Result<ProviderDetail, Box<ErrorBody>> {
    let params = ProviderTargetParams {
        provider: provider.to_owned(),
    };
    match call(client, Call::ProviderRevoke(params)).await? {
        Reply::ProviderRevoke(detail) => Ok(*detail),
        other => panic!("not a revoke reply: {other:?}"),
    }
}

fn detail_of<'a>(error: &'a ErrorBody, key: &str) -> Option<&'a str> {
    error
        .details()
        .iter()
        .find(|detail| detail.key() == key)
        .map(stackly_protocol::error::ErrorDetail::value)
}

async fn stop(daemon: DaemonHandle) {
    daemon
        .shutdown(
            stackly_protocol::events::ShutdownReason::Requested,
            std::future::pending(),
        )
        .await;
}

async fn changes(events: &mut stackly_ipc::EventStream) -> Vec<CatalogChangeKind> {
    let mut seen = Vec::new();
    while let Ok(Ok(Some(frame))) =
        tokio::time::timeout(Duration::from_millis(300), events.next()).await
    {
        if let DecodedFrame::Event(event) = frame
            && let EventKind::CatalogChanged(changed) = event.kind
        {
            seen.push(changed.change);
        }
    }
    seen
}

fn audited(daemon: &DaemonHandle, kind: OperationKind) -> usize {
    daemon
        .core()
        .registry()
        .expect("the registry is open")
        .audit_events(AuditQuery::page(500, None))
        .expect("reads")
        .value
        .items
        .iter()
        .filter(|row| row.operation_kind == Some(kind))
        .count()
}

// covers: AC-5, AC-17 (spec 0037)
#[tokio::test(flavor = "multi_thread")]
async fn an_approval_binds_the_shown_bytes_opens_the_gate_and_runs_once() {
    let site = Site::new();
    let daemon = site.start_with(fast_with_catalog()).await;
    let api = site.api().await;
    let (_, mut events) = api
        .client
        .subscribe(SubscribeParams::default())
        .await
        .expect("subscribes");
    let dir = package_in(site.root.path(), "acme-demo", "acme-demo");
    let (_, digest) = import(&api.client, &dir).await;
    let _ = changes(&mut events).await;

    let wrong = approve(&api.client, "acme/demo", &"0".repeat(64))
        .await
        .expect_err("another digest is refused");
    assert_eq!(wrong.code(), "provider.import_changed");
    assert!(wrong.hint().is_some());

    let approved = approve(&api.client, "acme/demo", &digest)
        .await
        .expect("it approves");
    assert_eq!(approved.summary.approval, Some(ApprovalState::Approved));
    assert_eq!(approved.summary.state, CatalogEntryState::Available);
    assert!(approved.versions[0].install.installable);
    let local = approved.local.expect("local facts");
    let decision = local.decision.expect("a decision");
    assert_eq!(decision.decision, ApprovalState::Approved);
    assert_eq!(decision.package_sha256, digest);
    assert_eq!(changes(&mut events).await, [CatalogChangeKind::Approved]);
    assert_eq!(audited(&daemon, OperationKind::ApproveProvider), 1);

    let again = approve(&api.client, "acme/demo", &digest)
        .await
        .expect("approving again answers");
    assert_eq!(again.summary.approval, Some(ApprovalState::Approved));
    assert_eq!(
        audited(&daemon, OperationKind::ApproveProvider),
        1,
        "the same bytes approve without an operation"
    );
    assert!(changes(&mut events).await.is_empty());

    let catalog_entry = approve(&api.client, "testing/fake", &digest)
        .await
        .expect_err("a catalog entry has no approval");
    assert_eq!(catalog_entry.code(), "provider.import_not_found");
    assert_eq!(detail_of(&catalog_entry, "reason"), Some("not_local"));
    drop((api, events));
    stop(daemon).await;
}

// covers: AC-6, AC-7, AC-17 (spec 0037)
#[tokio::test(flavor = "multi_thread")]
async fn a_revoke_closes_the_gate_and_a_decision_follows_its_bytes() {
    let site = Site::new();
    let daemon = site.start_with(fast_with_catalog()).await;
    let api = site.api().await;
    let demo = package_in(site.root.path(), "acme-demo", "acme-demo");
    let (_, digest) = import(&api.client, &demo).await;

    let nothing = revoke(&api.client, "acme/demo")
        .await
        .expect_err("nothing approved yet");
    assert_eq!(nothing.code(), "provider.not_approved");
    assert_eq!(detail_of(&nothing, "approval"), Some("pending"));
    assert!(nothing.hint().is_some());

    approve(&api.client, "acme/demo", &digest)
        .await
        .expect("approves");
    let revoked = revoke(&api.client, "acme/demo").await.expect("revokes");
    assert_eq!(revoked.summary.approval, Some(ApprovalState::Revoked));
    assert_eq!(revoked.summary.state, CatalogEntryState::Blocked);
    assert_eq!(
        revoked.summary.state_reason,
        Some(SupportReason::TrustRevoked)
    );
    assert_eq!(
        revoked.revocation.as_ref().map(|revocation| revocation.by),
        Some(RevocationSource::User)
    );
    assert!(!revoked.versions[0].install.installable);
    assert_eq!(audited(&daemon, OperationKind::RevokeProvider), 1);

    // Approving again covers only the stored bytes.
    approve(&api.client, "acme/demo", &digest)
        .await
        .expect("approves again");

    // Other bytes of the same provider are pending, with what changed.
    let next = package_in(site.root.path(), "acme-demo-next", "acme-demo-next");
    let (replaced, _) = import(&api.client, &next).await;
    assert_eq!(replaced.summary.approval, Some(ApprovalState::Pending));
    let local = replaced.local.expect("local facts");
    assert_eq!(
        local.changes,
        [
            PermissionChange::Revision,
            PermissionChange::Downloads,
            PermissionChange::DownloadHosts
        ]
    );
    assert_eq!(
        local.decision.map(|decision| decision.decision),
        Some(ApprovalState::Approved),
        "the decision on the old bytes is kept"
    );

    // A pending entry whose decision was an approval can still be revoked.
    let pending = revoke(&api.client, "acme/demo")
        .await
        .expect("a pending entry with an approval revokes");
    assert_eq!(pending.summary.approval, Some(ApprovalState::Pending));

    // Importing the decided bytes again brings their decision back.
    let (back, _) = import(&api.client, &demo).await;
    assert_eq!(back.summary.approval, Some(ApprovalState::Revoked));
    drop(api);
    stop(daemon).await;
}

// covers: AC-12 (spec 0037), VAL-018
#[test]
fn a_settings_patch_with_a_trust_key_is_refused_before_it_reaches_the_daemon() {
    let request = br#"{"requestId":"0192f3a1-0000-7000-8000-000000000001","idempotencyKey":"0192f3a1-0000-7000-8000-000000000002","call":{"type":"settings.update","content":{"expectedRevision":1,"patch":{"trust":"all"}}}}"#;
    let refused = stackly_protocol::json::decode_request(request)
        .expect_err("the daemon refuses a trust key");
    assert_eq!(refused.error.code(), "request.invalid_params");
}

/// Kills and reaps the daemon a test started itself.
struct Child(std::process::Child);

impl Drop for Child {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

// covers: AC-12 (spec 0037), VAL-018
#[test]
fn a_daemon_started_with_trust_variables_still_imports_pending() {
    let root = TestRoot::new().expect("a test root");
    let paths = common::daemon_paths(&root);
    let catalog = root.path().join("catalog");
    std::fs::create_dir_all(&catalog).expect("the catalog folder is made");
    std::fs::write(catalog.join("catalog.json"), EMPTY_CATALOG_JSON).expect("writes");
    let _daemon = Child(
        Command::new(common::STACKLYD)
            .env("STACKLY_ROOT", root.path())
            .env("STACKLY_CATALOG_DIR", &catalog)
            .env("STACKLY_TRUST", "1")
            .env("STACKLY_APPROVE_ALL", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("stacklyd starts"),
    );
    let deadline = Instant::now() + Duration::from_secs(20);
    while !common::handshakes(&paths.socket) {
        assert!(Instant::now() < deadline, "stacklyd did not get ready");
        std::thread::sleep(Duration::from_millis(50));
    }
    let dir = package_in(root.path(), "acme-demo", "acme-demo");
    let runtime = common::runtime();
    let detail = runtime.block_on(async {
        let (client, _) = common::client(&paths.socket).await;
        // The registry and catalog may still be settling after the
        // handshake; the import retries until the catalog answers.
        loop {
            let digest = match client
                .call(Call::ProviderPreviewImport(PreviewImportParams {
                    path: dir.display().to_string(),
                }))
                .await
            {
                Ok(Reply::ProviderPreviewImport(preview)) => preview.package_sha256,
                _ => None,
            };
            if let Some(digest) = digest {
                let params = ImportProviderParams {
                    path: dir.display().to_string(),
                    expected_package_sha256: digest,
                };
                if let Ok(Reply::ProviderImport(detail)) =
                    call(&client, Call::ProviderImport(params)).await
                {
                    return *detail;
                }
            }
            assert!(Instant::now() < deadline, "the import never answered");
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    });
    assert_eq!(detail.summary.approval, Some(ApprovalState::Pending));
}
