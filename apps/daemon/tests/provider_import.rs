//! Importing and removing local providers through the real daemon (spec
//! 0037 milestone 1, build step 5): `provider.preview_import` reads a
//! folder and writes nothing; `provider.import` stores a private copy, a
//! row, and the local source, shows the entry `pending` in the catalog,
//! announces `catalog.changed`, and audits; the same bytes again run no
//! operation; `provider.remove` deletes the row and the unreferenced copy;
//! a restart loads the local entries again without an event.
#![cfg(target_os = "macos")]

mod common;
#[path = "supervisor/support.rs"]
mod support;

use std::os::unix::fs::{MetadataExt, symlink};
use std::path::{Path, PathBuf};
use std::time::Duration;

use stackly_domain::{AuditQuery, OperationKind, ProviderSourceKind, Registry as _, Sha256Digest};
use stackly_ipc::CallOptions;
use stackly_protocol::catalog::{
    CatalogEntryState, GetProviderParams, ListProvidersReply, ProviderDetail, SupportReason,
    TrustLevel,
};
use stackly_protocol::events::{EventKind, SubscribeParams};
use stackly_protocol::json::DecodedFrame;
use stackly_protocol::provider::{
    ApprovalState, CatalogChangeKind, ImportBlockReason, ImportPreview, ImportProviderParams,
    PreviewImportParams, ProviderTargetParams,
};
use stackly_protocol::{Call, ErrorBody, Reply};
use stackly_testkit::evidence::{Kind, evidence};
use stacklyd::server::DaemonHandle;

use crate::support::{Api, Site, fast_with_catalog, key};

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

/// A copy of a fixture package at `<test root>/packages/<name>`.
fn package(site: &Site, fixture_name: &str, name: &str) -> PathBuf {
    let dir = site.root.path().join("packages").join(name);
    copy_dir(&fixture(fixture_name), &dir);
    dir
}

async fn preview(api: &Api, path: &Path) -> Result<ImportPreview, Box<ErrorBody>> {
    let params = PreviewImportParams {
        path: path.display().to_string(),
    };
    match api.client.call(Call::ProviderPreviewImport(params)).await? {
        Reply::ProviderPreviewImport(preview) => Ok(*preview),
        other => panic!("not a preview reply: {other:?}"),
    }
}

async fn import(
    api: &Api,
    path: &Path,
    digest: &str,
    idempotency: &str,
) -> Result<ProviderDetail, Box<ErrorBody>> {
    let params = ImportProviderParams {
        path: path.display().to_string(),
        expected_package_sha256: digest.to_owned(),
    };
    match api
        .client
        .call_detailed(
            Call::ProviderImport(params),
            CallOptions::default().with_idempotency_key(idempotency),
        )
        .await?
    {
        Reply::ProviderImport(detail) => Ok(*detail),
        other => panic!("not an import reply: {other:?}"),
    }
}

async fn remove(api: &Api, provider: &str) -> Result<String, Box<ErrorBody>> {
    let params = ProviderTargetParams {
        provider: provider.to_owned(),
    };
    match api
        .client
        .call_detailed(
            Call::ProviderRemove(params),
            CallOptions::default().with_idempotency_key(key()),
        )
        .await?
    {
        Reply::ProviderRemove(removed) => Ok(removed.provider),
        other => panic!("not a remove reply: {other:?}"),
    }
}

async fn list(api: &Api) -> ListProvidersReply {
    match api
        .client
        .call(Call::CatalogListProviders)
        .await
        .expect("the list answers")
    {
        Reply::CatalogListProviders(reply) => reply,
        other => panic!("not a list reply: {other:?}"),
    }
}

async fn get(api: &Api, provider: &str) -> Result<ProviderDetail, Box<ErrorBody>> {
    let params = GetProviderParams {
        provider: provider.to_owned(),
    };
    match api.client.call(Call::CatalogGetProvider(params)).await? {
        Reply::CatalogGetProvider(detail) => Ok(*detail),
        other => panic!("not a detail reply: {other:?}"),
    }
}

/// Every file and folder under `root` with its size, sorted, skipping the
/// daemon's own logs, socket, and registry files, which change on their
/// own.
fn tree(root: &Path) -> Vec<(String, u64)> {
    let mut found = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let relative = path
                .strip_prefix(root)
                .expect("under the root")
                .display()
                .to_string();
            if relative.contains("logs")
                || relative.ends_with(".sock")
                || relative.contains("-wal")
                || relative.contains("-shm")
            {
                continue;
            }
            let meta = entry.metadata().expect("the entry has metadata");
            if meta.is_dir() {
                pending.push(path);
                found.push((relative, 0));
            } else {
                found.push((relative, meta.len()));
            }
        }
    }
    found.sort();
    found
}

async fn stop(daemon: DaemonHandle) {
    daemon
        .shutdown(
            stackly_protocol::events::ShutdownReason::Requested,
            std::future::pending(),
        )
        .await;
}

fn store_dir(site: &Site, digest: &str) -> PathBuf {
    site.root.path().join("data/provider-imports").join(digest)
}

/// The `catalog.changed` events that arrive within a short wait.
async fn changes(
    events: &mut stackly_ipc::EventStream,
) -> Vec<(Option<String>, CatalogChangeKind)> {
    let mut seen = Vec::new();
    while let Ok(Ok(Some(frame))) =
        tokio::time::timeout(Duration::from_millis(300), events.next()).await
    {
        if let DecodedFrame::Event(event) = frame
            && let EventKind::CatalogChanged(changed) = event.kind
        {
            seen.push((changed.provider, changed.change));
        }
    }
    seen
}

// covers: AC-1 (spec 0037)
#[tokio::test(flavor = "multi_thread")]
async fn a_preview_reads_the_folder_and_writes_nothing() {
    let ev = evidence!(
        covers = ["AC-033", "REQ-033"],
        kind = Kind::Fixture,
        expected = "provider.preview_import reads a local package folder and answers its identity, digests, and permissions without writing a file, a registry row, or an audit row",
    );
    let site = Site::new();
    let daemon = site.start_with(fast_with_catalog()).await;
    let api = site.api().await;
    let dir = package(&site, "acme-demo", "acme-demo");
    let registry = daemon.core().registry().expect("the registry is open");
    let generation = registry.known_instance_ids().expect("reads").generation;
    let audit = registry
        .audit_events(AuditQuery::page(500, None))
        .expect("reads")
        .value
        .items
        .len();
    let before = tree(site.root.path());

    let reply = preview(&api, &dir).await.expect("the preview answers");

    assert!(reply.verdict.importable, "{:?}", reply.import_issues);
    assert_eq!(reply.provider.as_deref(), Some("acme/demo"));
    assert_eq!(reply.publisher.as_deref(), Some("acme"));
    assert_eq!(reply.quarantined, Some(false));
    assert!(reply.existing.is_none());
    assert_eq!(
        reply
            .permissions
            .as_ref()
            .map(|permissions| permissions.secrets.clone()),
        Some(vec!["password".to_owned()])
    );
    assert_eq!(reply.package_sha256.as_ref().map(String::len), Some(64));
    assert_eq!(u64::from(reply.file_count), 4);
    assert_eq!(
        registry.known_instance_ids().expect("reads").generation,
        generation,
        "the registry generation stays"
    );
    assert_eq!(
        registry
            .audit_events(AuditQuery::page(500, None))
            .expect("reads")
            .value
            .items
            .len(),
        audit,
        "a preview is not audited"
    );
    assert_eq!(tree(site.root.path()), before, "nothing is written");
    drop(api);
    stop(daemon).await;
    ev.pass();
}

// covers: AC-1, AC-2 (spec 0037)
#[tokio::test(flavor = "multi_thread")]
async fn a_preview_answers_an_invalid_package_and_refuses_an_unreadable_folder() {
    let site = Site::new();
    let daemon = site.start_with(fast_with_catalog()).await;
    let api = site.api().await;
    let official = package(&site, "acme-demo", "official");
    let yaml = official.join("provider.yaml");
    let text = std::fs::read_to_string(&yaml).expect("reads");
    std::fs::write(&yaml, text.replace("id: acme/demo", "id: official/demo")).expect("writes");

    let invalid = preview(&api, &official).await.expect("the preview answers");
    assert!(!invalid.verdict.importable);
    assert_eq!(
        invalid.verdict.reason,
        Some(ImportBlockReason::PackageInvalid)
    );
    assert_eq!(invalid.import_issues[0].code, "namespace_not_allowed");

    let reason = |error: &ErrorBody| {
        error
            .details()
            .iter()
            .find(|detail| detail.key() == "reason")
            .map(|detail| detail.value().to_owned())
    };
    let missing = preview(&api, &site.root.path().join("packages/none"))
        .await
        .expect_err("a missing folder is refused");
    assert_eq!(missing.code(), "provider.import_unreadable");
    assert_eq!(reason(&missing).as_deref(), Some("not_found"));
    let link = site.root.path().join("packages/link");
    symlink(&official, &link).expect("the symlink is made");
    let symlinked = preview(&api, &link)
        .await
        .expect_err("a symlink is refused");
    assert_eq!(reason(&symlinked).as_deref(), Some("symlink"));
    let file = preview(&api, &yaml).await.expect_err("a file is refused");
    assert_eq!(reason(&file).as_deref(), Some("not_a_directory"));
    let relative = preview(&api, Path::new("packages/official"))
        .await
        .expect_err("a relative path is refused");
    assert_eq!(relative.code(), "request.invalid_params");
    drop(api);
    stop(daemon).await;
}

// covers: AC-3, AC-10, AC-17 (spec 0037)
#[tokio::test(flavor = "multi_thread")]
async fn an_import_stores_a_private_copy_shows_a_pending_local_entry_and_audits() {
    let ev = evidence!(
        covers = ["AC-033", "REQ-033"],
        kind = Kind::Fixture,
        expected = "provider.import stores a private copy, lists the entry as trust local, approval pending, and blocked with approval_required, audits once, and publishes catalog.changed imported",
    );
    let site = Site::new();
    let daemon = site.start_with(fast_with_catalog()).await;
    let api = site.api().await;
    let (_, mut events) = api
        .client
        .subscribe(SubscribeParams::default())
        .await
        .expect("the subscription is accepted");
    let dir = package(&site, "acme-demo", "acme-demo");
    let origin = std::fs::read(dir.join("provider.yaml")).expect("reads");
    let shown = preview(&api, &dir).await.expect("previews");
    let digest = shown.package_sha256.clone().expect("a digest");

    let detail = import(&api, &dir, &digest, &key())
        .await
        .expect("it imports");

    assert_eq!(detail.summary.trust, TrustLevel::Local);
    assert_eq!(detail.summary.approval, Some(ApprovalState::Pending));
    assert_eq!(detail.summary.state, CatalogEntryState::Blocked);
    assert_eq!(
        detail.summary.state_reason,
        Some(SupportReason::ApprovalRequired)
    );
    assert_eq!(detail.summary.publisher.as_deref(), Some("acme"));
    let local = detail.local.as_ref().expect("local facts");
    assert_eq!(local.package_sha256, digest);
    assert!(local.decision.is_none());
    assert!(local.permissions.is_some());
    assert!(!detail.versions[0].install.installable);
    let copy = store_dir(&site, &digest);
    assert_eq!(
        std::fs::metadata(&copy).expect("the copy exists").mode() & 0o777,
        0o700
    );
    assert_eq!(
        std::fs::metadata(copy.join("provider.yaml"))
            .expect("a file")
            .mode()
            & 0o777,
        0o600
    );
    assert_eq!(
        std::fs::read(dir.join("provider.yaml")).expect("reads"),
        origin
    );

    let registry = daemon.core().registry().expect("the registry is open");
    let row = registry
        .provider_import(&"acme/demo".parse().expect("an id"))
        .expect("reads")
        .value
        .expect("the row is stored");
    assert_eq!(
        row.package_sha256,
        digest.parse::<Sha256Digest>().expect("hex")
    );
    let source = registry
        .provider_source_by_location(ProviderSourceKind::Local, "imports")
        .expect("reads")
        .value
        .expect("the local source is recorded");
    assert_eq!(row.source_id, source.id);
    let reply = list(&api).await;
    assert_eq!(u64::from(reply.catalog.local_count), 1);
    assert!(reply.items.iter().any(|item| item.provider == "acme/demo"));
    let audit = registry
        .audit_events(AuditQuery::page(500, None))
        .expect("reads")
        .value
        .items;
    let row = audit
        .iter()
        .find(|row| row.operation_kind == Some(OperationKind::ImportProvider))
        .expect("the import is audited");
    let detail_of = |key: &str| row.safe_details.get(key).map(str::to_owned);
    assert_eq!(detail_of("provider").as_deref(), Some("acme/demo"));
    assert_eq!(detail_of("packageSha256").as_deref(), Some(digest.as_str()));
    assert_eq!(
        changes(&mut events).await,
        [(Some("acme/demo".to_owned()), CatalogChangeKind::Imported)]
    );

    // The same bytes again: the same answer, no operation, no event.
    let operations = registry.unfinished_operations().expect("reads").value.len();
    let again = import(&api, &dir, &digest, &key())
        .await
        .expect("it answers");
    assert_eq!(
        again.local.map(|local| local.package_sha256),
        Some(digest.clone())
    );
    assert_eq!(
        registry.unfinished_operations().expect("reads").value.len(),
        operations
    );
    assert!(changes(&mut events).await.is_empty(), "no second event");
    let imports = registry
        .audit_events(AuditQuery::page(500, None))
        .expect("reads")
        .value
        .items
        .iter()
        .filter(|row| row.operation_kind == Some(OperationKind::ImportProvider))
        .count();
    assert_eq!(imports, 1, "the second import is not an operation");
    drop((api, events));
    stop(daemon).await;
    ev.pass();
}

// covers: AC-3 (spec 0037)
#[tokio::test(flavor = "multi_thread")]
async fn an_import_of_changed_bytes_is_refused_and_writes_nothing() {
    let site = Site::new();
    let daemon = site.start_with(fast_with_catalog()).await;
    let api = site.api().await;
    let dir = package(&site, "acme-demo", "acme-demo");
    let shown = preview(&api, &dir).await.expect("previews");
    let digest = shown.package_sha256.expect("a digest");
    let template = dir.join("templates/demo.conf.tmpl");
    let text = std::fs::read_to_string(&template).expect("reads");
    std::fs::write(&template, format!("{text}# changed\n")).expect("writes");

    let refused = import(&api, &dir, &digest, &key())
        .await
        .expect_err("changed bytes are refused");

    assert_eq!(refused.code(), "provider.import_changed");
    let registry = daemon.core().registry().expect("the registry is open");
    assert!(registry.provider_imports().expect("reads").value.is_empty());
    assert!(
        !site
            .root
            .path()
            .join("data/provider-imports")
            .join(&digest)
            .exists()
    );
    drop(api);
    stop(daemon).await;
}

// covers: AC-9, AC-10 (spec 0037)
#[tokio::test(flavor = "multi_thread")]
async fn a_remove_deletes_the_row_and_its_copy_and_a_restart_loads_the_rest() {
    let ev = evidence!(
        covers = ["REQ-033"],
        kind = Kind::Fixture,
        expected = "provider.remove deletes the import row and its stored copy, refuses a catalog ID with provider.import_not_found, and a restart loads the remaining local entries",
    );
    let site = Site::new();
    let daemon = site.start_with(fast_with_catalog()).await;
    let api = site.api().await;
    let demo = package(&site, "acme-demo", "acme-demo");
    let first = preview(&api, &demo)
        .await
        .expect("previews")
        .package_sha256
        .expect("digest");
    import(&api, &demo, &first, &key()).await.expect("imports");
    let next = package(&site, "acme-demo-next", "acme-demo-next");
    let second = preview(&api, &next).await.expect("previews");
    let existing = second.existing.as_ref().expect("it replaces the import");
    assert!(!existing.same_package);
    let second = second.package_sha256.expect("digest");
    let replaced = import(&api, &next, &second, &key()).await.expect("imports");
    assert_eq!(replaced.summary.provider_revision.as_deref(), Some("1.1.0"));
    assert!(
        !store_dir(&site, &first).exists(),
        "the replaced copy is removed"
    );
    assert!(store_dir(&site, &second).exists());

    let not_local = remove(&api, "testing/fake")
        .await
        .expect_err("a catalog entry");
    assert_eq!(not_local.code(), "provider.import_not_found");

    // A restart loads the import again.
    drop(api);
    stop(daemon).await;
    let daemon = site.start_with(fast_with_catalog()).await;
    let api = site.api().await;
    let reloaded = get(&api, "acme/demo").await.expect("the entry is loaded");
    assert_eq!(reloaded.summary.trust, TrustLevel::Local);
    assert_eq!(reloaded.summary.provider_revision.as_deref(), Some("1.1.0"));

    assert_eq!(
        remove(&api, "acme/demo").await.expect("removes"),
        "acme/demo"
    );
    assert!(get(&api, "acme/demo").await.is_err(), "the entry is gone");
    assert!(!store_dir(&site, &second).exists(), "the copy is removed");
    let registry = daemon.core().registry().expect("the registry is open");
    assert!(registry.provider_imports().expect("reads").value.is_empty());
    let again = remove(&api, "acme/demo")
        .await
        .expect_err("nothing to remove");
    assert_eq!(again.code(), "provider.import_not_found");
    assert_eq!(u64::from(list(&api).await.catalog.local_count), 0);
    drop(api);
    stop(daemon).await;
    ev.pass();
}

// covers: AC-13 (spec 0037)
#[tokio::test(flavor = "multi_thread")]
async fn a_quarantined_folder_is_reported_kept_and_never_copied_into_the_store() {
    let site = Site::new();
    let daemon = site.start_with(fast_with_catalog()).await;
    let api = site.api().await;
    let dir = package(&site, "acme-demo", "downloaded");
    let yaml = dir.join("provider.yaml");
    let attribute = "com.apple.quarantine";
    let written = std::process::Command::new("/usr/bin/xattr")
        .args(["-w", attribute, "0081;672b0a00;Safari;"])
        .arg(&yaml)
        .status()
        .expect("xattr runs");
    assert!(written.success(), "the test root takes the attribute");
    let read = |path: &Path| {
        std::process::Command::new("/usr/bin/xattr")
            .arg("-p")
            .arg(attribute)
            .arg(path)
            .output()
            .expect("xattr runs")
    };
    let before = read(&yaml).stdout;

    let shown = preview(&api, &dir).await.expect("previews");
    assert_eq!(shown.quarantined, Some(true));
    assert_eq!(shown.quarantine_agent.as_deref(), Some("Safari"));
    let digest = shown.package_sha256.expect("a digest");
    let detail = import(&api, &dir, &digest, &key()).await.expect("imports");

    assert_eq!(detail.local.and_then(|local| local.quarantined), Some(true));
    assert_eq!(read(&yaml).stdout, before, "the origin keeps its attribute");
    let copy = store_dir(&site, &digest).join("provider.yaml");
    assert!(
        !read(&copy).status.success(),
        "the copy has no quarantine attribute"
    );
    drop(api);
    stop(daemon).await;
}

// covers: AC-10 (spec 0037)
#[test]
fn a_catalog_with_64_providers_64_planned_and_32_local_entries_lists_in_one_control_frame() {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use stackly_core::catalog::local::{package_bytes, package_sha256, permissions};
    use stackly_core::catalog::{
        Catalog, CatalogEntry, EntryOrigin, LocalInfo, PlannedEntry, ProviderEntry, TileColor,
        TileHint,
    };
    use stackly_core::provider::{Category, NameCheck, PackageFiles, ValidatedPackage};
    use stackly_domain::ProviderImportRecord;
    use stackly_protocol::limits::CONTROL_FRAME_MAX;

    let source = fixture("acme-demo");
    let files: BTreeMap<String, Vec<u8>> = [
        "provider.yaml",
        "versions.json",
        "schemas/settings.schema.json",
        "templates/demo.conf.tmpl",
    ]
    .into_iter()
    .map(|path| {
        (
            path.to_owned(),
            std::fs::read(source.join(path)).expect("reads"),
        )
    })
    .collect();
    let package = |id: &str| {
        let mut files = files.clone();
        let yaml = String::from_utf8(files["provider.yaml"].clone()).expect("UTF-8");
        files.insert(
            "provider.yaml".to_owned(),
            yaml.replace("id: acme/demo", &format!("id: {id}"))
                .into_bytes(),
        );
        ValidatedPackage::from_files(PackageFiles::new("p", files), NameCheck::Skip)
            .expect("the package validates")
    };
    let tile = || TileHint {
        mark: "Ac".to_owned(),
        tile: TileColor::Gray,
    };
    let mut entries: Vec<CatalogEntry> = (0..64)
        .map(|n| {
            CatalogEntry::Provider(ProviderEntry {
                package: Arc::new(package(&format!("official/p{n}"))),
                tile: tile(),
                notes: Vec::new(),
                origin: EntryOrigin::Catalog,
            })
        })
        .collect();
    entries.extend((0..64).map(|n| {
        CatalogEntry::Planned(PlannedEntry {
            id: format!("official/q{n}").parse().expect("an id"),
            display_name: "\u{1F600}".repeat(64),
            category: Category::Database,
            description: "\u{1F600}".repeat(512),
            note: None,
            tile: tile(),
        })
    }));
    let base = Catalog::for_tests(&entries);
    let local: Vec<CatalogEntry> = (0..32)
        .map(|n| {
            let package = package(&format!("acme/l{n}"));
            let key = package.key();
            let record = ProviderImportRecord::new(
                key.provider().clone(),
                "0199f000-0000-7000-8000-000000000010"
                    .parse()
                    .expect("an id"),
                key.revision().clone(),
                package.software().clone(),
                *key.manifest_sha256(),
                *key.manifest_sha256(),
                package_sha256(package.files()),
                package_bytes(package.files()),
                format!("~/{}", "a".repeat(1000)),
                jiff::Timestamp::UNIX_EPOCH,
            );
            let permissions = permissions(&package);
            CatalogEntry::Provider(ProviderEntry {
                package: Arc::new(package),
                tile: tile(),
                notes: Vec::new(),
                origin: EntryOrigin::Local(Box::new(LocalInfo::new(record, permissions))),
            })
        })
        .collect();
    let catalog = base.with_local(local);
    let host = stackly_platform::HostFacts::new(
        "macos-aarch64".parse().expect("a target"),
        stackly_domain::OsVersion::major_minor(27, 0),
        None,
    );

    let reply = stacklyd::map::catalog::list_providers(&catalog, &host, false);

    assert_eq!(reply.items.len(), 160);
    assert_eq!(u64::from(reply.catalog.local_count), 32);
    let frame = stackly_protocol::ServerFrame::Response(stackly_protocol::Response {
        request_id: "0192f3a1-0000-7000-8000-000000000001".to_owned(),
        reply: Reply::CatalogListProviders(reply),
    });
    let len = serde_json::to_vec(&frame).expect("encodes").len();
    assert!(len < CONTROL_FRAME_MAX as usize, "{len} bytes");
}
