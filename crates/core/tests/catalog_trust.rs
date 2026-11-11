//! The read model and the trust gate of local providers (spec 0037, build
//! step 3): `catalog::local::load` over store copies, local entries in the
//! catalog, the trust verdicts for display, the gate, guarded commits, and
//! pinned revisions, the `approval_required` and `trust_revoked` refusals,
//! and compatibility records a local package cannot bring.
#![cfg(unix)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use stackly_core::catalog::local::{ImportCheck, load};
use stackly_core::catalog::{
    Catalog, CatalogEntry, CatalogOptions, EntryOrigin, EntryState, Installability,
    NotInstallableReason, ProviderEntry, RevokedBy, StoreMode, SupportReason, TileColor, TileHint,
    TrustVerdict, entry_status, entry_trust, guard_pinned_trust, guard_trust, installability,
    pinned_trust, require_entry_trust, version_support, write_package,
};
use stackly_core::provider::{IssueCode, NameCheck, ValidatedPackage, check_package};
use stackly_core::{PreparedLayout, prepare_layout};
use stackly_domain::{
    GuardDecision, Guarded, ImportDecision, Os, OsVersion, ProviderImportRecord, ProviderName,
    ProviderRevisionKey, ProviderSourceId, ProviderSourceKind, ProviderSourceRecord, Registry,
    Sha256Digest, SupportStatus, TrustDecision, WriteBatch, WriteOp,
};
use stackly_platform::{Clock, HostFacts, Platform};
use stackly_testkit::{FakePlatform, FakeRegistry, ProviderRevisionBuilder, TestRoot};

const RUNTIME_RECORD: &str = r#"{ "scope": "runtime", "result": "pass", "providerRevision": null, "os": "macos", "osVersion": "27.0", "osBuild": "26A428", "arch": "aarch64", "checks": ["closure", "launch_agent"], "knownLimitations": ["provider_integration_not_covered"], "summary": "ready in 493 ms", "testedAt": "2026-10-18T11:07:19.973Z", "evidence": "runtime/01a14eb1-694f-7400-b05e-71b9a321b96b/01a14eb1-6c78-7311-bd09-2b75053ea60d" }"#;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/packages")
        .join(name)
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).expect("the copy target can be made");
    for entry in fs::read_dir(from).expect("the package folder lists") {
        let entry = entry.expect("an entry reads");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("the entry has a type").is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target).expect("a file copies");
        }
    }
}

fn host() -> HostFacts {
    HostFacts::new(
        "macos-aarch64".parse().expect("a target"),
        OsVersion::major_minor(27, 0),
        Some("26A428".to_owned()),
    )
}

/// A test root with the fake platform, a fake registry holding the local
/// source, and imported packages with their store copies.
struct Site {
    root: TestRoot,
    platform: Platform,
    prepared: PreparedLayout,
    registry: FakeRegistry,
    source: ProviderSourceId,
    fake: FakePlatform,
}

impl Site {
    fn new() -> Self {
        let root = TestRoot::new().expect("a test root can be made");
        let fake = FakePlatform::new(&root);
        let platform = fake.platform();
        let prepared = prepare_layout(&platform).expect("the fake roots prepare");
        let registry = FakeRegistry::new(Arc::new(fake.clock().clone()), fake.clock().now());
        let source = ProviderSourceRecord::new(
            ProviderSourceId::generate(),
            ProviderSourceKind::Local,
            "imports".to_owned(),
            fake.clock().now(),
        );
        let id = source.id;
        registry
            .commit(WriteBatch::new(vec![WriteOp::InsertProviderSource(source)]))
            .expect("the local source commits");
        Self {
            root,
            platform,
            prepared,
            registry,
            source: id,
            fake,
        }
    }

    /// Copies a fixture to `input/<name>`, edits a file, and checks it.
    fn package(&self, fixture_name: &str, name: &str, edit: &dyn Fn(&Path)) -> ImportCheck {
        let dir = self.root.path().join("input").join(name);
        copy_dir(&fixture(fixture_name), &dir);
        edit(&dir);
        let check = ImportCheck::new(
            check_package(self.platform.fs(), &dir, NameCheck::Skip),
            CatalogOptions::default(),
            None,
        );
        assert!(
            check.valid(),
            "{:?} {:?}",
            check.findings(),
            check.import_issues
        );
        check
    }

    /// Writes the store copy and the import row of a valid package.
    fn import(&self, check: &ImportCheck) -> ProviderImportRecord {
        let package = check.package().expect("valid");
        let digest = check.package_sha256.expect("valid");
        let layout = self.prepared.layout();
        write_package(
            &self.platform,
            &self.prepared,
            &layout.provider_imports_dir(),
            &layout.provider_import_dir(&digest),
            package.files(),
            StoreMode::Import(&digest),
        )
        .expect("the store copy writes");
        let key = package.key();
        let record = ProviderImportRecord::new(
            key.provider().clone(),
            self.source,
            key.revision().clone(),
            package.software().clone(),
            *key.manifest_sha256(),
            stackly_core::catalog::versions_sha256(package.files())
                .expect("a package has versions.json")
                .parse()
                .expect("hex"),
            digest,
            check.package_bytes,
            "~/acme-demo".to_owned(),
            self.fake.clock().now(),
        );
        let expected = self
            .registry
            .provider_import(key.provider())
            .expect("the read works")
            .value
            .map(|stored| stored.revision);
        let mut put = record.clone();
        if let Some(expected) = expected {
            put.revision = expected.next();
        }
        self.registry
            .commit(WriteBatch::new(vec![WriteOp::PutProviderImport {
                record: Box::new(put),
                expected,
            }]))
            .expect("the import row commits");
        self.registry
            .provider_import(key.provider())
            .expect("the read works")
            .value
            .expect("the row is stored")
    }

    fn decide(&self, record: &ProviderImportRecord, decision: TrustDecision) {
        self.registry
            .commit(WriteBatch::new(vec![WriteOp::DecideProviderImport {
                provider: record.provider.clone(),
                decision: ImportDecision::new(
                    decision,
                    record.package_sha256,
                    record.provider_revision.clone(),
                    "{}".to_owned(),
                    self.fake.clock().now(),
                ),
            }]))
            .expect("the decision commits");
    }

    /// Loads every import into a catalog built on an empty base.
    fn catalog(&self) -> Catalog {
        let entries = load(self.platform.fs(), &self.registry, self.prepared.layout())
            .expect("the rows read")
            .value;
        Catalog::for_tests(&[]).with_local(entries)
    }
}

fn demo(site: &Site) -> ImportCheck {
    site.package("acme-demo", "acme-demo", &|_| {})
}

fn provider(catalog: &Catalog, id: &str) -> ProviderEntry {
    match catalog.entry(&id.parse().expect("an id")) {
        Some(CatalogEntry::Provider(entry)) => entry.clone(),
        other => panic!("{id} is not a provider entry: {other:?}"),
    }
}

fn refusal(entry: &CatalogEntry) -> Option<NotInstallableReason> {
    match installability(entry, &"1.0.0".parse().expect("a version"), &host())
        .expect("the version exists")
    {
        Installability::Installable { .. } => None,
        Installability::NotInstallable { reason, .. } => Some(reason),
    }
}

// covers: AC-10, AC-4 (spec 0037)
#[test]
fn an_imported_package_loads_from_its_store_copy_as_a_pending_local_entry() {
    let site = Site::new();
    let record = site.import(&demo(&site));

    let catalog = site.catalog();

    assert_eq!(catalog.local_count(), 1);
    let entry = catalog
        .entry(&record.provider)
        .expect("the local entry is listed");
    assert!(entry.is_local());
    let local = provider(&catalog, "acme/demo");
    let info = local.local().expect("it is local");
    assert_eq!(info.record, record);
    assert_eq!(info.permissions.secrets, ["password"]);
    assert_eq!(
        local.tile,
        TileHint {
            mark: "Ac".to_owned(),
            tile: TileColor::Gray
        }
    );
    assert_eq!(
        entry_trust(&catalog, &local),
        TrustVerdict::ApprovalRequired
    );
    let status = entry_status(entry, &host());
    assert_eq!(
        (status.state, status.reason),
        (EntryState::Blocked, Some(SupportReason::ApprovalRequired))
    );
    assert_eq!(refusal(entry), Some(NotInstallableReason::ApprovalRequired));
    assert_eq!(
        NotInstallableReason::ApprovalRequired.as_code(),
        "approval_required"
    );
}

// covers: AC-4, AC-10 (spec 0037)
#[test]
fn approving_the_stored_bytes_opens_the_gate_and_revoking_closes_it_as_user() {
    let site = Site::new();
    let record = site.import(&demo(&site));
    site.decide(&record, TrustDecision::Approved);

    let catalog = site.catalog();
    let entry = catalog.entry(&record.provider).expect("listed");
    let local = provider(&catalog, "acme/demo");
    assert_eq!(entry_trust(&catalog, &local), TrustVerdict::Trusted);
    assert_eq!(
        require_entry_trust(&catalog, &site.registry, &local).expect("the registry reads"),
        TrustVerdict::Trusted
    );
    assert_eq!(entry_status(entry, &host()).state, EntryState::Available);
    assert_eq!(refusal(entry), None);

    site.decide(&record, TrustDecision::Revoked);
    let catalog = site.catalog();
    let entry = catalog.entry(&record.provider).expect("listed");
    let status = entry_status(entry, &host());
    assert_eq!(
        (status.state, status.reason),
        (EntryState::Blocked, Some(SupportReason::TrustRevoked))
    );
    assert_eq!(refusal(entry), Some(NotInstallableReason::TrustRevoked));
    match entry_trust(&catalog, &provider(&catalog, "acme/demo")) {
        TrustVerdict::Revoked(info) => {
            assert_eq!(info.by, RevokedBy::User);
            assert_eq!(info.since, site.fake.clock().now());
            assert_eq!(info.catalog_revision, None);
        }
        other => panic!("not revoked: {other:?}"),
    }
}

// covers: AC-4 (spec 0037)
#[test]
fn the_gate_reads_the_registry_again_and_never_trusts_a_stale_snapshot() {
    let site = Site::new();
    let record = site.import(&demo(&site));
    site.decide(&record, TrustDecision::Approved);
    let snapshot = site.catalog();
    let local = provider(&snapshot, "acme/demo");

    site.decide(&record, TrustDecision::Revoked);
    assert_eq!(
        entry_trust(&snapshot, &local),
        TrustVerdict::Trusted,
        "the display reads the old snapshot"
    );
    assert!(matches!(
        require_entry_trust(&snapshot, &site.registry, &local).expect("the registry reads"),
        TrustVerdict::Revoked(_)
    ));

    let next = site.import(&site.package("acme-demo-next", "acme-demo-next", &|_| {}));
    assert_ne!(next.package_sha256, record.package_sha256);
    assert_eq!(
        require_entry_trust(&snapshot, &site.registry, &local).expect("the registry reads"),
        TrustVerdict::ApprovalRequired,
        "a row that holds other bytes needs an approval"
    );

    site.registry
        .commit(WriteBatch::new(vec![WriteOp::DeleteProviderImport(
            record.provider.clone(),
        )]))
        .expect("the row deletes");
    assert_eq!(
        require_entry_trust(&snapshot, &site.registry, &local).expect("the registry reads"),
        TrustVerdict::ApprovalRequired,
        "a row that is gone needs an approval"
    );
}

/// Runs `guard_trust` inside a guarded commit and hands its verdict back.
fn guarded(site: &Site, provider: &ProviderName, digest: Sha256Digest) -> TrustVerdict {
    let provider = provider.clone();
    let outcome = site
        .registry
        .commit_guarded(Box::new(move |view| {
            let verdict = guard_trust(view, &provider, &digest)?;
            Ok(GuardDecision::Refuse(Box::new(verdict)))
        }))
        .expect("the guarded commit runs");
    match outcome {
        Guarded::Refused(reason) => *reason
            .downcast::<TrustVerdict>()
            .expect("the guard hands back a verdict"),
        Guarded::Committed(_) => panic!("the guard wrote"),
    }
}

// covers: AC-4 (spec 0037)
#[test]
fn a_commit_guard_sees_the_approval_of_exactly_the_bytes_it_was_built_from() {
    let site = Site::new();
    let record = site.import(&demo(&site));
    let digest = record.package_sha256;
    let other: Sha256Digest = "ab".repeat(32).parse().expect("hex");

    assert_eq!(
        guarded(&site, &record.provider, digest),
        TrustVerdict::ApprovalRequired
    );
    site.decide(&record, TrustDecision::Approved);
    assert_eq!(
        guarded(&site, &record.provider, digest),
        TrustVerdict::Trusted
    );
    assert_eq!(
        guarded(&site, &record.provider, other),
        TrustVerdict::ApprovalRequired
    );
    assert_eq!(
        guarded(&site, &"acme/unknown".parse().expect("an id"), digest),
        TrustVerdict::ApprovalRequired
    );
}

// covers: AC-7 (spec 0037)
#[test]
fn a_pinned_revision_is_revoked_only_by_its_own_approval_column() {
    let site = Site::new();
    let at = site.fake.clock().now();
    let builder = ProviderRevisionBuilder::new()
        .provider("acme/demo")
        .software("demo");
    let key: ProviderRevisionKey = builder.key();
    let record = builder.record();
    for approval in [None, Some(TrustDecision::Approved)] {
        let mut record = record.clone();
        record.approval = approval;
        assert_eq!(
            pinned_trust(None, &key, &record),
            TrustVerdict::Trusted,
            "{approval:?}"
        );
    }
    let mut revoked = record.clone();
    revoked.approval = Some(TrustDecision::Revoked);
    revoked.approved_at = Some(at);
    match pinned_trust(None, &key, &revoked) {
        TrustVerdict::Revoked(info) => assert_eq!((info.by, info.since), (RevokedBy::User, at)),
        other => panic!("not revoked: {other:?}"),
    }

    site.registry
        .commit(WriteBatch::new(vec![
            WriteOp::InsertProviderRevision(record.with_source_id(site.source)),
            WriteOp::SetProviderRevisionApproval {
                key: key.clone(),
                approval: TrustDecision::Revoked,
                at,
            },
        ]))
        .expect("the pinned revision commits");
    let guard_key = key.clone();
    let outcome = site
        .registry
        .commit_guarded(Box::new(move |view| {
            let verdict = guard_pinned_trust(view, None, &guard_key)?;
            Ok(GuardDecision::Refuse(Box::new(verdict)))
        }))
        .expect("the guarded commit runs");
    let Guarded::Refused(verdict) = outcome else {
        panic!("the guard wrote");
    };
    assert!(matches!(
        *verdict.downcast::<TrustVerdict>().expect("a verdict"),
        TrustVerdict::Revoked(_)
    ));
}

// covers: AC-10 (spec 0037)
#[test]
fn a_missing_or_changed_store_copy_is_an_invalid_local_entry_and_others_still_load() {
    let site = Site::new();
    let demo = site.import(&demo(&site));
    let tool = site.import(&site.package("acme-demo", "acme-tool", &|dir| {
        let path = dir.join("provider.yaml");
        let text = fs::read_to_string(&path).expect("reads");
        fs::write(&path, text.replace("id: acme/demo", "id: acme/tool")).expect("writes");
    }));
    let layout = site.prepared.layout();
    fs::remove_dir_all(layout.provider_import_dir(&demo.package_sha256)).expect("removes");

    let catalog = site.catalog();

    match catalog.entry(&demo.provider) {
        Some(CatalogEntry::Invalid(invalid)) => {
            assert_eq!(invalid.local.as_deref(), Some(&demo));
            assert!(!invalid.issues.is_empty());
            assert!(
                invalid
                    .issues
                    .iter()
                    .all(|issue| issue.code != IssueCode::ImportDigestMismatch),
                "a copy that does not load reports its package findings"
            );
        }
        other => panic!("not invalid: {other:?}"),
    }
    assert!(matches!(
        catalog.entry(&tool.provider),
        Some(CatalogEntry::Provider(_))
    ));
    assert_eq!(catalog.local_count(), 2);
    assert_eq!(
        entry_status(catalog.entry(&demo.provider).expect("listed"), &host()).state,
        EntryState::Invalid
    );

    let copy = layout.provider_import_dir(&tool.package_sha256);
    fs::write(
        copy.join("templates/demo.conf.tmpl"),
        "greeting: {{ settings.greeting }}\npassword: {{ secret.password }}\n# changed\n",
    )
    .expect("writes");
    let catalog = site.catalog();
    match catalog.entry(&tool.provider) {
        Some(CatalogEntry::Invalid(invalid)) => {
            let codes: Vec<IssueCode> = invalid.issues.iter().map(|issue| issue.code).collect();
            assert_eq!(codes, [IssueCode::ImportDigestMismatch]);
            assert_eq!(
                invalid.issues[0].message,
                "the stored copy does not match the import record"
            );
            assert!(catalog.entry(&tool.provider).expect("listed").is_local());
        }
        other => panic!("not invalid: {other:?}"),
    }
}

// covers: AC-11 (spec 0037)
#[test]
fn a_local_entry_ignores_its_compatibility_records() {
    let site = Site::new();
    let check = site.package("acme-demo", "acme-compat", &|dir| {
        let path = dir.join("versions.json");
        let text = fs::read_to_string(&path).expect("reads");
        fs::write(
            &path,
            text.replace(
                r#""stable": true"#,
                &format!(r#""stable": true, "compatibility": [{RUNTIME_RECORD}]"#),
            ),
        )
        .expect("writes");
    });
    let record = site.import(&check);
    site.decide(&record, TrustDecision::Approved);
    let catalog = site.catalog();
    let local = provider(&catalog, "acme/demo");
    let as_catalog = ProviderEntry {
        package: Arc::new(
            ValidatedPackage::from_files(local.package.files().clone(), NameCheck::Skip)
                .expect("valid"),
        ),
        tile: local.tile.clone(),
        notes: Vec::new(),
        origin: EntryOrigin::Catalog,
    };
    let version = "1.0.0".parse().expect("a version");
    let mac = |entry: &ProviderEntry| {
        version_support(entry, &version, &host())
            .into_iter()
            .find(|cell| cell.os == Os::Macos)
            .expect("a macOS cell")
    };

    let artifact = &local.artifacts()[0];
    assert_eq!(artifact.compatibility().len(), 1);
    assert!(local.trusted_compatibility(artifact).is_empty());
    assert_eq!(as_catalog.trusted_compatibility(artifact).len(), 1);
    assert_eq!(mac(&as_catalog).status, SupportStatus::Experimental);
    let cell = mac(&local);
    assert_eq!(
        (cell.status, cell.reason),
        (SupportStatus::Unverified, Some(SupportReason::Untested))
    );
}

#[test]
fn with_local_keeps_the_catalog_entries_and_replaces_the_local_ones() {
    let site = Site::new();
    let record = site.import(&demo(&site));
    let nats = site.root.path().join("input/nats");
    copy_dir(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../providers/nats"),
        &nats,
    );
    let catalog_entry = CatalogEntry::Provider(ProviderEntry {
        package: Arc::new(
            ValidatedPackage::from_files(
                stackly_core::provider::read_package(site.platform.fs(), &nats)
                    .expect("nats reads"),
                NameCheck::Skip,
            )
            .expect("nats validates"),
        ),
        tile: TileHint {
            mark: "Na".to_owned(),
            tile: TileColor::Green,
        },
        notes: Vec::new(),
        origin: EntryOrigin::Catalog,
    });
    let base = Catalog::for_tests(&[catalog_entry]);
    let local = load(site.platform.fs(), &site.registry, site.prepared.layout())
        .expect("reads")
        .value;

    let merged = base.with_local(local);
    assert_eq!(merged.entries().len(), 2);
    assert_eq!(merged.local_count(), 1);
    assert_eq!(merged.meta(), base.meta());
    let again = merged.with_local(Vec::new());
    assert_eq!(again.entries().len(), 1, "the local entries are replaced");
    assert!(again.entry(&record.provider).is_none());
    assert_eq!(base.local_count(), 0);
}
