//! `environment::plan` at its edges: an installed runtime, a local provider
//! that waits for approval, a provider that runs one instance, every
//! `differences` field, and the secret store states (spec 0044 AC-5, AC-7,
//! AC-9, AC-10).

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;

use stackly_core::catalog::{
    Catalog, CatalogEntry, CatalogState, EntryOrigin, LocalInfo, ProviderEntry, TileColor, TileHint,
};
use stackly_core::environment::{Plan, PlanAction, PlanSecretStatus, PlanStep, PlanTrust};
use stackly_core::provider::{NameCheck, PackageFiles, ValidatedPackage};
use stackly_domain::{
    InstanceLifecycle, Ownership, Registry, RuntimeState, SecretRef, WriteBatch, WriteOp,
};
use stackly_platform::{Clock, SecretStoreError};
use stackly_testkit::{EnvironmentBuilder, RuntimeBuilder};

use crate::common::{PACKAGES, entry, package};
use crate::site::Site;

fn codes(plan: &Plan) -> Vec<&'static str> {
    plan.blockers.iter().map(|b| b.kind.code()).collect()
}

fn manifest(services: &str) -> String {
    format!(
        "apiVersion: stackly/v1alpha1\nkind: Environment\nmetadata:\n  name: shop\nservices:\n{services}"
    )
}

const VALKEY: &str = "  cache:\n    provider: official/valkey\n";

/// Commits a managed runtime of the artifact the plan resolved, in
/// `state`, with the software and version of that artifact.
fn install_resolved(site: &Site, plan: &Plan, state: RuntimeState) -> stackly_domain::RuntimeId {
    let runtime = plan.items[0].runtime.as_ref().expect("a resolved runtime");
    let version = runtime.version.to_string();
    let line = version
        .rsplit_once('.')
        .map(|(line, _)| line.to_owned())
        .expect("a version with a patch");
    let builder = RuntimeBuilder::new()
        .software("valkey")
        .version(&version, &line)
        .managed(runtime.artifact_sha256)
        .state(state);
    let artifact = builder.artifact();
    let row = builder.build();
    let id = row.id();
    // The programs of the package, so create's own runtime row is ok.
    let folder = site.prepared.layout().runtime_dir(
        row.software(),
        row.version(),
        row.target(),
        &runtime.artifact_sha256,
    );
    for program in ["bin/valkey-server", "bin/valkey-cli"] {
        let path = folder.join(program);
        fs::create_dir_all(path.parent().expect("a folder")).expect("the folder can be made");
        fs::write(&path, b"#!/bin/sh\nexit 0\n").expect("the program writes");
        let mut mode = fs::metadata(&path).expect("metadata").permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut mode, 0o755);
        fs::set_permissions(&path, mode).expect("the program is executable");
    }
    site.registry
        .commit(WriteBatch::new(vec![
            WriteOp::InsertArtifact(artifact),
            WriteOp::InsertRuntime(row),
        ]))
        .expect("the runtime commits");
    id
}

// covers: AC-5 (spec 0044, Runtime): an installed managed runtime of the
// resolved artifact means no install, and the digest follows `install`
#[test]
fn an_installed_runtime_of_the_resolved_artifact_needs_no_install_and_moves_the_digest() {
    let site = Site::new();
    let text = manifest(VALKEY);
    let before = site.plan(text.as_bytes());
    assert!(before.applicable, "{:#?}", before.blockers);
    let first = before.items[0].runtime.clone().expect("a runtime");
    assert!(first.install && first.download_size.is_some() && first.runtime_id.is_none());

    let id = install_resolved(&site, &before, RuntimeState::Installed);
    let after = site.plan(text.as_bytes());

    let item = &after.items[0];
    let runtime = item.runtime.as_ref().expect("a runtime");
    assert!(!runtime.install, "an installed row of the same artifact");
    assert_eq!(runtime.runtime_id, Some(id));
    assert_eq!(runtime.download_size, None);
    assert_eq!(runtime.artifact_sha256, first.artifact_sha256);
    assert_eq!(
        item.steps,
        [PlanStep::CreateInstance, PlanStep::StartInstance]
    );
    let row = item
        .checks
        .iter()
        .find(|c| c.id == "runtime")
        .expect("a runtime row");
    assert_ne!(row.detail, "will be installed");
    assert!(after.applicable, "{:#?}", after.blockers);
    assert_ne!(after.digest, before.digest, "install is part of the digest");
}

// covers: AC-5 (spec 0044, Runtime): a row of the artifact in any other
// state still installs
#[test]
fn a_runtime_row_that_is_not_installed_still_installs() {
    for state in [
        RuntimeState::Failed,
        RuntimeState::Missing,
        RuntimeState::Drifted,
        RuntimeState::Downloading,
    ] {
        let site = Site::new();
        let text = manifest(VALKEY);
        let before = site.plan(text.as_bytes());
        install_resolved(&site, &before, state);

        let after = site.plan(text.as_bytes());

        let runtime = after.items[0].runtime.as_ref().expect("a runtime");
        assert!(runtime.install, "{state:?}");
        assert_eq!(runtime.runtime_id, None, "{state:?}");
        assert!(runtime.download_size.is_some(), "{state:?}");
        assert_eq!(
            after.items[0].steps[0],
            PlanStep::InstallRuntime,
            "{state:?}"
        );
        assert_eq!(after.digest, before.digest, "{state:?}");
    }
}

// covers: AC-5 (spec 0044, Runtime): no "use what is installed if the line
// matches": a row of another artifact never counts
#[test]
fn an_installed_runtime_of_another_artifact_never_counts() {
    let site = Site::new();
    let text = manifest(VALKEY);
    let before = site.plan(text.as_bytes());
    let runtime = before.items[0].runtime.as_ref().expect("a runtime");
    let version = runtime.version.to_string();
    let line = version
        .rsplit_once('.')
        .map(|(line, _)| line.to_owned())
        .expect("a patch");
    let builder = RuntimeBuilder::new()
        .software("valkey")
        .version(&version, &line)
        .state(RuntimeState::Installed);
    site.registry
        .commit(WriteBatch::new(vec![
            WriteOp::InsertArtifact(builder.artifact()),
            WriteOp::InsertRuntime(builder.build()),
        ]))
        .expect("commits");

    let after = site.plan(text.as_bytes());

    assert!(after.items[0].runtime.as_ref().expect("a runtime").install);
}

/// The files of the package folder `dir`.
fn package_at(dir: &Path, name: &str) -> ValidatedPackage {
    let mut files = BTreeMap::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(folder) = stack.pop() {
        for item in fs::read_dir(&folder).expect("reads") {
            let item = item.expect("an entry");
            let path = item.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let relative = path
                .strip_prefix(dir)
                .expect("inside")
                .to_str()
                .expect("UTF-8")
                .to_owned();
            if relative.starts_with("templates/")
                || relative.starts_with("schemas/")
                || relative == "provider.yaml"
                || relative == "versions.json"
            {
                files.insert(relative, fs::read(&path).expect("reads"));
            }
        }
    }
    ValidatedPackage::from_files(PackageFiles::new(name, files), NameCheck::Skip)
        .unwrap_or_else(|err| panic!("{name} validates: {:#?}", err.issues))
}

fn with_catalog(site: &mut Site, catalog: Catalog) {
    site.catalog = Arc::new(catalog);
    site.state = CatalogState::Loaded(Arc::clone(&site.catalog));
}

// covers: AC-7 (spec 0044, spec 0037): a local provider that waits for its
// approval has one blocker, `provider.approval_required`, and the plan
// leaves the approval as it is
#[test]
fn a_pending_local_provider_has_one_blocker_and_the_plan_never_approves_it() {
    use stackly_core::catalog::local::{package_bytes, package_sha256, permissions};
    use stackly_domain::ProviderImportRecord;

    let mut site = Site::new();
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/packages/acme-demo");
    let acme = package_at(&dir, "acme-demo");
    let key = acme.key().clone();
    let record = ProviderImportRecord::new(
        key.provider().clone(),
        "0199f000-0000-7000-8000-000000000010"
            .parse()
            .expect("an id"),
        key.revision().clone(),
        acme.software().clone(),
        *key.manifest_sha256(),
        *key.manifest_sha256(),
        package_sha256(acme.files()),
        package_bytes(acme.files()),
        "~/acme".to_owned(),
        jiff::Timestamp::UNIX_EPOCH,
    );
    let local = CatalogEntry::Provider(ProviderEntry {
        package: Arc::new(acme.clone()),
        tile: TileHint {
            mark: "Ac".to_owned(),
            tile: TileColor::Gray,
        },
        notes: Vec::new(),
        origin: EntryOrigin::Local(Box::new(LocalInfo::new(record, permissions(&acme)))),
    });
    let catalog = (*site.catalog).clone().with_local(vec![local]);
    with_catalog(&mut site, catalog);
    let before = site
        .registry
        .known_instance_ids()
        .expect("reads")
        .generation;

    let plan =
        site.plan(manifest("  demo:\n    provider: acme/demo\n    version: \"1.0\"\n").as_bytes());

    assert_eq!(
        codes(&plan),
        ["provider.approval_required"],
        "{:#?}",
        plan.blockers
    );
    assert_eq!(plan.items[0].trust, Some(PlanTrust::ApprovalRequired));
    assert!(!plan.applicable && plan.digest.is_none());
    assert!(
        site.registry
            .provider_import(key.provider())
            .expect("reads")
            .value
            .is_none(),
        "no approval is written"
    );
    assert_eq!(
        site.registry
            .known_instance_ids()
            .expect("reads")
            .generation,
        before
    );
}

/// The test catalog with `testing/fake` replaced by a package without
/// `multiInstance`.
fn single_instance_catalog() -> Catalog {
    let original = package("fake");
    let mut files = BTreeMap::new();
    for (path, bytes) in original.files().iter() {
        files.insert(path.to_owned(), bytes.to_vec());
    }
    let manifest = files.get_mut("provider.yaml").expect("a manifest");
    let text = String::from_utf8(manifest.clone()).expect("UTF-8");
    assert!(text.contains("  multiInstance: true\n"));
    *manifest = text.replace("  multiInstance: true\n", "").into_bytes();
    let single = ValidatedPackage::from_files(PackageFiles::new("fake", files), NameCheck::Skip)
        .expect("the package without multiInstance validates");
    let entries: Vec<CatalogEntry> = PACKAGES
        .iter()
        .filter(|name| **name != "fake")
        .map(|name| entry(package(name)))
        .chain(std::iter::once(entry(single)))
        .collect();
    Catalog::for_tests(&entries)
}

const FAKE: &str = "  api:\n    provider: testing/fake\n    version: \"0.1\"\n";

// covers: AC-7 (spec 0044): a provider without `multi_instance` that
// already has an instance blocks the create with create's own code
#[test]
fn a_single_instance_provider_with_an_instance_blocks_with_capability_unsupported() {
    let mut site = Site::new();
    with_catalog(&mut site, single_instance_catalog());
    let text = manifest(FAKE);
    assert!(
        !codes(&site.plan(text.as_bytes())).contains(&"instance.capability_unsupported"),
        "no instance yet"
    );

    site.instance("other-fake", 18070, InstanceLifecycle::Stopped);
    let plan = site.plan(text.as_bytes());

    assert!(
        codes(&plan).contains(&"instance.capability_unsupported"),
        "{:?}",
        codes(&plan)
    );
    assert_eq!(
        plan.blockers[0].service.as_ref().map(|a| a.as_str()),
        Some("api")
    );
    assert!(!plan.applicable);
}

/// Registers `shop` at the site's location with `api` as a dedicated
/// member, an instance of `testing/fake` named `shop-api` on 18080.
fn registered(site: &mut Site) {
    let api = site.instance("shop-api", 18080, InstanceLifecycle::Running);
    let env = EnvironmentBuilder::new()
        .name("shop")
        .manifest_location(site.location("stackly.yaml").as_str())
        .member("api", api.id(), Ownership::Dedicated)
        .build();
    site.registry
        .commit(WriteBatch::new(vec![WriteOp::InsertEnvironment(env)]))
        .expect("commits");
}

fn fields(plan: &Plan) -> Vec<(String, String, String)> {
    plan.items[0]
        .differences
        .iter()
        .map(|d| (d.field.clone(), d.manifest.clone(), d.instance.clone()))
        .collect()
}

// covers: AC-9 (spec 0044, Aturan differences): `provider`
#[test]
fn a_member_of_another_provider_differs_in_provider_and_never_blocks() {
    let mut site = Site::new();
    registered(&mut site);

    let plan = site.plan(manifest("  api:\n    provider: official/valkey\n").as_bytes());

    assert_eq!(
        fields(&plan),
        [(
            "provider".to_owned(),
            "official/valkey".to_owned(),
            "testing/fake".to_owned()
        )]
    );
    assert_eq!(plan.items[0].action, PlanAction::Keep);
    assert!(plan.applicable, "{:#?}", plan.blockers);
}

// covers: AC-9 (spec 0044, Aturan differences): `ownership` and
// `instanceRef`
#[test]
fn a_dedicated_member_now_borrowed_differs_in_ownership_and_instance_ref() {
    let mut site = Site::new();
    registered(&mut site);

    let plan = site.plan(
        manifest("  api:\n    provider: testing/fake\n    instanceRef: elsewhere\n").as_bytes(),
    );

    assert_eq!(
        fields(&plan),
        [
            (
                "ownership".to_owned(),
                "shared".to_owned(),
                "dedicated".to_owned()
            ),
            (
                "instanceRef".to_owned(),
                "elsewhere".to_owned(),
                "shop-api".to_owned()
            ),
        ]
    );
    assert_eq!(
        plan.items[0].instance_name.as_str(),
        "shop-api",
        "the member stays"
    );
    assert!(plan.applicable, "{:#?}", plan.blockers);
}

// covers: AC-9 (spec 0044, Aturan differences): `stable` and `sha256:`
// selectors and an `auto` port are never compared
#[test]
fn stable_and_auto_never_differ() {
    let mut site = Site::new();
    registered(&mut site);

    let plan = site.plan(
        manifest(
            "  api:\n    provider: testing/fake\n    version: stable\n    endpoints:\n      http: {port: auto}\n",
        )
        .as_bytes(),
    );

    assert_eq!(fields(&plan), []);
    assert!(plan.applicable, "{:#?}", plan.blockers);
}

// covers: AC-9 (spec 0044): a member whose runtime row is gone has no
// line, so a line selector reports no `runtime` difference and nothing
// blocks; with the row, the same selector differs
#[test]
fn a_member_without_a_runtime_line_keeps_its_item_with_no_runtime_difference() {
    let mut site = Site::new();
    registered(&mut site);
    // `0` is a line only selector; the fake runtime's line is `0.1`.
    let text = manifest("  api:\n    provider: testing/fake\n    version: \"0\"\n");
    let with_row = site.plan(text.as_bytes());
    assert_eq!(
        fields(&with_row).first().map(|f| f.0.as_str()),
        Some("runtime"),
        "the row's line differs"
    );

    site.views[0].runtime_line = None;
    let plan = site.plan(text.as_bytes());

    assert_eq!(plan.items.len(), 1);
    assert_eq!(plan.items[0].action, PlanAction::Keep);
    assert_eq!(fields(&plan), []);
    assert!(plan.applicable, "{:#?}", plan.blockers);
}

/// Registers `local:shop/token` as a user reference and returns it.
fn register_reference(site: &Site) -> SecretRef {
    let reference = SecretRef::parse("local:shop/token").expect("a reference");
    let record = stackly_domain::SecretReferenceRecord::new(
        stackly_domain::SecretReferenceId::generate(),
        reference.clone(),
        stackly_domain::PlatformStoreKey::new("fake:token".to_owned()).expect("a key"),
        stackly_domain::SecretOwner::User,
        stackly_domain::SecretPurpose::user(),
        site.fake.clock().now(),
    );
    site.registry
        .commit(WriteBatch::new(vec![WriteOp::InsertSecretReference(
            record,
        )]))
        .expect("commits");
    reference
}

const BOUND: &str = "  api:\n    provider: testing/fake\n    version: \"0.1\"\n    secretRefs: {authToken: \"local:shop/token\"}\n";

// covers: AC-10 (spec 0044): a store that can't answer gives `unknown`,
// which needs a secret but never blocks
#[test]
fn a_locked_busy_or_failing_store_reads_unknown_and_never_blocks() {
    for error in [
        SecretStoreError::Locked,
        SecretStoreError::Busy,
        SecretStoreError::AccessDenied { by_user: false },
    ] {
        let site = Site::new();
        let reference = register_reference(&site);
        site.fake
            .secrets()
            .insert(&reference, "value-from-the-store");
        site.fake.secrets().fail_next(error);

        let plan = site.plan(manifest(BOUND).as_bytes());

        let secret = &plan.items[0].secrets[0];
        assert_eq!(secret.status, PlanSecretStatus::Unknown, "{error:?}");
        assert_eq!(secret.reference.as_ref(), Some(&reference));
        assert_eq!(plan.needs_secrets, 1, "{error:?}");
        assert!(plan.applicable, "{error:?}: {:#?}", plan.blockers);
        assert!(!format!("{plan:?}").contains("value-from-the-store"));
    }
}

// covers: AC-10 (spec 0044): a store whose state is `unsupported` reads
// `unknown` and needs a secret. The `secret.store_unsupported` blocker
// follows the store's backend, which the fake store never reports as
// unsupported; on an OS without an adapter both say so together.
#[test]
fn an_unsupported_store_state_reads_unknown() {
    let site = Site::new();
    register_reference(&site);
    site.fake.secrets().unsupported();

    let plan = site.plan(manifest(BOUND).as_bytes());

    assert_eq!(plan.items[0].secrets[0].status, PlanSecretStatus::Unknown);
    assert_eq!(plan.needs_secrets, 1);
}

/// Plans the registered `api` member with the selector `version`, with the
/// instance on `runtime` in line `line`.
fn runtime_fields(version: &str, runtime: &str, line: &str) -> Vec<(String, String, String)> {
    let mut site = Site::new();
    registered(&mut site);
    site.views[0].runtime_version = runtime.parse().expect("an exact version");
    site.views[0].runtime_line = Some(line.parse().expect("a line"));
    let text = manifest(&format!(
        "  api:\n    provider: testing/fake\n    version: \"{version}\"\n"
    ));
    let plan = site.plan(text.as_bytes());
    assert!(plan.applicable, "{:#?}", plan.blockers);
    fields(&plan)
}

// covers: AC-9 (spec 0044, Aturan differences): a selector that reads as a
// line matches every version of that line, as the resolver reads it
#[test]
fn a_line_selector_matches_a_patch_version_of_its_line() {
    assert_eq!(runtime_fields("2.15", "2.15.1", "2.15"), []);
    assert_eq!(runtime_fields("2.15", "2.15.0", "2.15"), []);
}

// covers: AC-9 (spec 0044, Aturan differences): an exact selector equal to
// the instance's version does not differ, another exact version does
#[test]
fn an_exact_selector_differs_only_from_another_version() {
    assert_eq!(runtime_fields("2.15.1", "2.15.1", "2.15"), []);
    assert_eq!(
        runtime_fields("2.15.0", "2.15.1", "2.15"),
        [(
            "runtime".to_owned(),
            "2.15.0".to_owned(),
            "2.15.1".to_owned()
        )]
    );
}

// covers: AC-9 (spec 0044, Aturan differences): a line the instance is not
// in differs
#[test]
fn a_line_selector_of_another_line_differs() {
    assert_eq!(
        runtime_fields("2.14", "2.15.1", "2.15"),
        [("runtime".to_owned(), "2.14".to_owned(), "2.15.1".to_owned())]
    );
}

// covers: AC-9 (spec 0044, Aturan differences): `stable` is never compared,
// whatever the instance runs
#[test]
fn a_stable_selector_never_differs_from_any_version() {
    assert_eq!(runtime_fields("stable", "9.9.9", "9.9"), []);
}
