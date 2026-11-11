//! The trust gate of local providers through the real daemon (spec 0037
//! milestone 2, build steps 7 and 8, *Gate trust*, *Revocation di
//! catalog*): a local entry that is not
//! approved, or whose approval was revoked, is refused at
//! `runtime.install`, `runtime.resolve`, the create form, `instance.create`,
//! and the duplicate form, with `provider.approval_required` or
//! `provider.revoked`; an approved entry creates with the `local_import`
//! source and pins a revision record that names its namespace as publisher,
//! the approval, and the local source; a revision the installed catalog
//! revokes is listed, shown in the preview, and refused by the catalog.
#![cfg(target_os = "macos")]

mod common;
#[path = "supervisor/support.rs"]
mod support;

use std::path::{Path, PathBuf};

use stackly_domain::{
    InstanceFilter, ProviderName, ProviderRevisionKey, Registry as _, TrustDecision,
};
use stackly_ipc::{CallOptions, Client};
use stackly_protocol::instance::CreateProviderSource;
use stackly_protocol::operation::OperationState;
use stackly_protocol::provider::{
    DecideProviderParams, ImportProviderParams, PreviewImportParams, ProviderTargetParams,
};
use stackly_protocol::runtime::InstallRuntimeParams;
use stackly_protocol::{Call, ErrorBody, Reply};

use crate::support::{Api, Site, fast_with_catalog, key};

const LOCAL: &str = "acme/fake";

/// A copy of the fake package under the `acme` namespace, so it imports as
/// a local entry of the software the seeded fake runtime runs.
fn local_fake(root: &Path) -> PathBuf {
    let from = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../providers/fake");
    let to = root.join("packages").join("acme").join("fake");
    copy_dir(&from, &to);
    let _ = std::fs::remove_file(to.join("README.md"));
    let manifest = to.join("provider.yaml");
    let text = std::fs::read_to_string(&manifest).expect("the manifest reads");
    let text = text
        .replace("id: testing/fake", "id: acme/fake")
        .replace("category: testing", "category: database")
        .replace("displayName: Fake Provider", "displayName: Acme Fake");
    std::fs::write(&manifest, text).expect("the manifest writes");
    // A fixture artifact belongs only to the testing namespace: the copy
    // lists an available one, which no test downloads.
    std::fs::write(to.join("versions.json"), VERSIONS).expect("the versions write");
    to
}

const VERSIONS: &str = r#"{
  "schema": "stackly.provider-versions/v1",
  "software": "fake",
  "artifacts": [
    {
      "id": "fake-0.1.0-macos-aarch64",
      "line": "0.1",
      "version": "0.1.0",
      "os": "macos",
      "arch": "aarch64",
      "status": "available",
      "minOs": "13.0",
      "format": "tar_gz",
      "urls": ["https://downloads.example.test/fake/fake-0.1.0-macos-aarch64.tar.gz"],
      "sha256": "1111111111111111111111111111111111111111111111111111111111111111",
      "size": 1048576,
      "signingPolicy": "keep_or_adhoc",
      "license": "MIT",
      "licenseUrl": "https://downloads.example.test/fake/LICENSE",
      "executables": ["stackly-fake-server"],
      "stable": true
    }
  ]
}
"#;

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

async fn call(client: &Client, call: Call) -> Result<Reply, Box<ErrorBody>> {
    client
        .call_detailed(call, CallOptions::default().with_idempotency_key(key()))
        .await
}

/// Previews and imports `path`; returns the stored package digest.
async fn import(client: &Client, path: &Path) -> String {
    let preview = match client
        .call(Call::ProviderPreviewImport(PreviewImportParams {
            path: path.display().to_string(),
        }))
        .await
        .expect("the preview answers")
    {
        Reply::ProviderPreviewImport(preview) => *preview,
        other => panic!("not a preview reply: {other:?}"),
    };
    let digest = preview
        .package_sha256
        .clone()
        .unwrap_or_else(|| panic!("the package has a digest: {preview:?}"));
    call(
        client,
        Call::ProviderImport(ImportProviderParams {
            path: path.display().to_string(),
            expected_package_sha256: digest.clone(),
        }),
    )
    .await
    .expect("it imports");
    digest
}

async fn approve(client: &Client, digest: &str) {
    call(
        client,
        Call::ProviderApprove(DecideProviderParams {
            provider: LOCAL.to_owned(),
            expected_package_sha256: digest.to_owned(),
        }),
    )
    .await
    .expect("it approves");
}

async fn revoke(client: &Client) {
    call(
        client,
        Call::ProviderRevoke(ProviderTargetParams {
            provider: LOCAL.to_owned(),
        }),
    )
    .await
    .expect("it revokes");
}

async fn install(client: &Client) -> Result<Reply, Box<ErrorBody>> {
    call(
        client,
        Call::RuntimeInstall(InstallRuntimeParams {
            provider: LOCAL.to_owned(),
            selector: "stable".to_owned(),
            expected_sha256: None,
        }),
    )
    .await
}

async fn resolve(client: &Client) -> Result<Reply, Box<ErrorBody>> {
    client
        .call_detailed(
            Call::RuntimeResolve(InstallRuntimeParams {
                provider: LOCAL.to_owned(),
                selector: "stable".to_owned(),
                expected_sha256: None,
            }),
            CallOptions::default(),
        )
        .await
}

fn local_params(api: &Api, name: &str) -> stackly_protocol::instance::CreateInstanceParams {
    let mut params = api.params(name, serde_json::json!({}));
    params.provider.name = LOCAL.to_owned();
    params
}

fn detail<'a>(error: &'a ErrorBody, key: &str) -> Option<&'a str> {
    error
        .details()
        .iter()
        .find(|detail| detail.key() == key)
        .map(stackly_protocol::error::ErrorDetail::value)
}

/// Every refusal of the gate for `code`, from every entry point a client
/// reaches before an operation runs.
async fn assert_gate_refuses(api: &Api, code: &str) {
    let runtime = api.runtime_id;
    let refused = [
        ("runtime.install", install(&api.client).await.err()),
        ("runtime.resolve", resolve(&api.client).await.err()),
        ("form", api.form(runtime, Some(LOCAL)).await.err()),
        (
            "form at revision",
            api.form_at(runtime, Some(LOCAL), Some("0.4.0")).await.err(),
        ),
        ("create", api.create(local_params(api, "gated")).await.err()),
    ];
    for (point, error) in refused {
        let error = error.unwrap_or_else(|| panic!("{point} is not refused"));
        assert_eq!(error.code(), code, "{point}: {error:?}");
        assert_eq!(detail(&error, "provider"), Some(LOCAL), "{point}");
        assert!(error.hint().is_some(), "{point} has a hint");
    }
}

fn instance_count(site: &Site) -> usize {
    site.open_registry()
        .instances(InstanceFilter {
            include_retained: true,
        })
        .expect("the registry reads")
        .value
        .len()
}

// covers: AC-4, AC-11, AC-19 (spec 0037)
#[tokio::test(flavor = "multi_thread")]
async fn a_pending_local_entry_is_refused_at_every_gate_and_creates_nothing() {
    let site = Site::new();
    let daemon = site.start_with(fast_with_catalog()).await;
    let api = site.api().await;
    let before = instance_count(&site);
    let dir = local_fake(site.root.path());
    let _digest = import(&api.client, &dir).await;

    assert_gate_refuses(&api, "provider.approval_required").await;

    // Without a provider the form picks the catalog entry of the software,
    // never the local one.
    let form = api
        .form(api.runtime_id, None)
        .await
        .expect("the catalog entry builds a form");
    assert_eq!(form.provider.name, "testing/fake");
    assert_ne!(form.provider.source, CreateProviderSource::LocalImport);

    drop(daemon);
    assert_eq!(instance_count(&site), before, "no instance was created");
    let registry = site.open_registry();
    let record = registry
        .provider_import(&LOCAL.parse::<ProviderName>().expect("an id"))
        .expect("reads")
        .value
        .expect("the import row");
    let key = ProviderRevisionKey::new(
        record.provider.clone(),
        record.provider_revision.clone(),
        record.manifest_sha256,
    );
    assert!(
        registry
            .provider_revision(&key)
            .expect("reads")
            .value
            .is_none(),
        "nothing was pinned"
    );
}

// covers: AC-4, AC-7, AC-19 (spec 0037)
#[tokio::test(flavor = "multi_thread")]
async fn an_approved_local_entry_pins_as_local_and_a_revoke_closes_create_and_duplicate() {
    let site = Site::new();
    let _guard = support::reap_guard(&site.root);
    let daemon = site.start_with(fast_with_catalog()).await;
    let api = site.api().await;
    let dir = local_fake(site.root.path());
    let digest = import(&api.client, &dir).await;
    approve(&api.client, &digest).await;

    let form = api
        .form(api.runtime_id, Some(LOCAL))
        .await
        .expect("an approved entry builds a form");
    assert_eq!(form.provider.name, LOCAL);
    assert_eq!(form.provider.source, CreateProviderSource::LocalImport);

    let accepted = api
        .create(local_params(&api, "local-one"))
        .await
        .expect("the create is accepted")
        .accepted
        .expect("a create starts an operation");
    let done = api.wait_final(accepted.operation.id).await;
    assert_eq!(done.operation.state, OperationState::Succeeded, "{done:?}");
    let created = accepted
        .operation
        .resource
        .as_ref()
        .map(|resource| resource.id.parse::<uuid::Uuid>().expect("an instance id"))
        .expect("the operation names the instance");

    let registry = daemon.core().registry().expect("the registry is open");
    let record = registry
        .provider_import(&LOCAL.parse::<ProviderName>().expect("an id"))
        .expect("reads")
        .value
        .expect("the import row");
    let key = ProviderRevisionKey::new(
        record.provider.clone(),
        record.provider_revision.clone(),
        record.manifest_sha256,
    );
    let pinned = registry
        .provider_revision(&key)
        .expect("reads")
        .value
        .expect("the create pinned the revision");
    assert_eq!(pinned.publisher.as_deref(), Some("acme"));
    assert_eq!(pinned.approval, Some(TrustDecision::Approved));
    assert_eq!(
        pinned.approved_at,
        record.decision.as_ref().map(|decision| decision.at)
    );
    assert_eq!(pinned.source_id, Some(record.source_id));

    revoke(&api.client).await;
    assert_gate_refuses(&api, "provider.revoked").await;
    let duplicate = api
        .duplicate_form(created)
        .await
        .expect_err("a duplicate of a revoked revision is refused");
    assert_eq!(duplicate.code(), "provider.revoked");
    assert_eq!(detail(&duplicate, "by"), Some("user"));

    // The instance that exists keeps running (AC-7).
    let started = api.start_ready(created).await;
    assert_eq!(started.id, created);
    api.stop_when_free(created).await;
}

/// The fake catalog with a `revoked` entry for every digest of the local
/// package's revision.
fn revoking_catalog() -> stacklyd::server::DaemonConfig {
    let mut config = support::fast();
    let catalog =
        support::fake_catalog().with_revocations(vec![stackly_core::catalog::Revocation::new(
            LOCAL.parse().expect("an id"),
            "0.4.0".parse().expect("a label"),
            None,
            "Starts with an insecure default.".to_owned(),
            "2026-09-01".parse().expect("a date"),
        )]);
    config.catalog = Some(stacklyd::catalog::CatalogOverride::Preloaded(catalog));
    config
}

// covers: AC-8 (spec 0037)
#[tokio::test(flavor = "multi_thread")]
async fn a_revision_the_installed_catalog_revokes_is_listed_shown_and_refused_by_catalog() {
    let site = Site::new();
    let _daemon = site.start_with(revoking_catalog()).await;
    let api = site.api().await;

    let list = match api
        .client
        .call(Call::CatalogListProviders)
        .await
        .expect("the list answers")
    {
        Reply::CatalogListProviders(list) => list,
        other => panic!("not a list reply: {other:?}"),
    };
    assert_eq!(list.revocations.len(), 1);
    assert_eq!(list.revocations[0].provider, LOCAL);
    assert_eq!(list.revocations[0].provider_revision, "0.4.0");
    assert_eq!(list.revocations[0].manifest_sha256, None);
    assert_eq!(list.revocations[0].since, "2026-09-01");

    let dir = local_fake(site.root.path());
    let preview = match api
        .client
        .call(Call::ProviderPreviewImport(PreviewImportParams {
            path: dir.display().to_string(),
        }))
        .await
        .expect("the preview answers")
    {
        Reply::ProviderPreviewImport(preview) => preview,
        other => panic!("not a preview reply: {other:?}"),
    };
    let shown = preview.revocation.clone().expect("the preview shows it");
    assert_eq!(
        shown.reason.as_deref(),
        Some("Starts with an insecure default.")
    );
    assert_eq!(
        shown.catalog_revision.map(|revision| revision.get()),
        Some(1)
    );

    let digest = import(&api.client, &dir).await;
    let refused = call(
        &api.client,
        Call::ProviderApprove(DecideProviderParams {
            provider: LOCAL.to_owned(),
            expected_package_sha256: digest,
        }),
    )
    .await
    .expect_err("an approval of a revoked revision is refused");
    assert_eq!(refused.code(), "provider.revoked");
    assert_eq!(detail(&refused, "by"), Some("catalog"));
    assert_eq!(detail(&refused, "catalogRevision"), Some("1"));
    assert_eq!(detail(&refused, "since"), Some("2026-09-01T00:00:00.000Z"));

    assert_gate_refuses(&api, "provider.revoked").await;
}

// covers: AC-11 (spec 0037, *Software yang sama*)
#[tokio::test(flavor = "multi_thread")]
async fn an_installed_runtime_keeps_the_name_of_the_catalog_entry_of_its_software() {
    let site = Site::new();
    let _daemon = site.start_with(fast_with_catalog()).await;
    let api = site.api().await;
    let dir = local_fake(site.root.path());
    let digest = import(&api.client, &dir).await;
    approve(&api.client, &digest).await;

    let detail = match api
        .client
        .call(Call::RuntimeGet(
            stackly_protocol::runtime::GetRuntimeParams {
                runtime_id: api.runtime_id,
            },
        ))
        .await
        .expect("the runtime reads")
    {
        Reply::RuntimeGet(detail) => detail,
        other => panic!("not a runtime reply: {other:?}"),
    };
    assert_eq!(detail.summary.display_name, "Fake Provider");
}
