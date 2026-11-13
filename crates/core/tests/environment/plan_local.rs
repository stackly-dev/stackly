//! `environment::plan` with `.stackly.local.yaml` (spec 0045): AC-9,
//! AC-10, AC-11.

use std::fs;

use stackly_core::environment::local::{LocalFile, LocalInput, LocalState, OverrideState};
use stackly_core::environment::lock::{LockFile, LockRefresh, emit, resolve_entries};
use stackly_core::environment::{BlockerKind, Plan, validate};
use stackly_core::storage::prepare_location;
use stackly_domain::{
    InstanceLifecycle, LocationKind, Ownership, PortRequest, Registry, StorageLocation,
    StorageLocationId, WriteBatch, WriteOp,
};
use stackly_platform::Clock;
use stackly_testkit::EnvironmentBuilder;

use crate::site::{Inputs, Site};

const SHOP: &str = "\
apiVersion: stackly/v1alpha1
kind: Environment
metadata:
  name: shop
services:
  api:
    provider: testing/fake
    version: \"0.1\"
    endpoints:
      http: {port: 18080}
  db:
    provider: official/postgresql
    version: \"18\"
    endpoints:
      client: {port: 15432}
  borrowed:
    provider: testing/fake
    ownership: shared
    instanceRef: main-api
";

fn local(text: &str) -> LocalInput {
    LocalInput {
        file: ".stackly.local.yaml".into(),
        content: LocalFile::from_read(Ok(text.as_bytes().to_vec())),
    }
}

fn overrides(body: &str) -> String {
    format!("apiVersion: stackly/v1alpha1\nkind: EnvironmentLocal\nservices:\n{body}")
}

fn plan(site: &Site, manifest: &str, local: &LocalInput) -> Plan {
    site.plan_with(
        manifest.as_bytes(),
        &site.location("stackly.yaml"),
        None,
        &Inputs {
            local: Some(local),
            ..Inputs::default()
        },
    )
}

fn finding_codes(plan: &Plan) -> Vec<&'static str> {
    plan.local
        .as_ref()
        .expect("a local file")
        .findings
        .list()
        .iter()
        .map(|f| f.code().as_code())
        .collect()
}

fn codes(plan: &Plan) -> Vec<&'static str> {
    plan.blockers.iter().map(|b| b.kind.code()).collect()
}

fn site_with_borrowed() -> Site {
    let mut site = Site::new();
    site.instance("main-api", 18090, InstanceLifecycle::Running);
    site
}

// covers: AC-9
#[test]
fn every_override_rule_has_its_finding_and_an_error_blocks_once() {
    let site = site_with_borrowed();
    let cases = [
        ("  api: [\n", "yaml_syntax"),
        (
            "  api:\n    endpoints:\n      http: {port: 1, host: x}\n",
            "unknown_field",
        ),
        ("  api:\n    endpoints:\n      http: {}\n", "missing_field"),
        (
            "  api:\n    endpoints:\n      http: {port: 0}\n",
            "invalid_value",
        ),
        ("  api: {}\n  api: {}\n", "duplicate_key"),
        ("  api: &a {}\n  db: *a\n", "yaml_alias"),
        (
            "  borrowed:\n    endpoints:\n      http: {port: 18100}\n",
            "override_not_allowed",
        ),
        (
            "  api:\n    endpoints:\n      nope: {port: 18100}\n",
            "unknown_endpoint",
        ),
        (
            "  db:\n    endpoints:\n      socket: {port: 18100}\n",
            "endpoint_not_ip",
        ),
        (
            "  api:\n    endpoints:\n      http: {port: 15432}\n",
            "override_endpoint_conflict",
        ),
        (
            "  db:\n    storage:\n      dataLocation: relative/path\n",
            "storage_location_invalid",
        ),
        (
            "  db:\n    storage:\n      dataLocation: /Volumes/../etc\n",
            "storage_location_invalid",
        ),
    ];
    for (body, code) in cases {
        let planned = plan(&site, SHOP, &local(&overrides(body)));
        assert!(
            finding_codes(&planned).contains(&code),
            "{code}: {:?}",
            finding_codes(&planned)
        );
        assert_eq!(codes(&planned), ["env.local_invalid"], "{code}");
        assert_eq!(
            planned.local.as_ref().map(|l| l.state),
            Some(LocalState::Invalid)
        );
    }
    let foreign = plan(
        &site,
        SHOP,
        &local("apiVersion: stackly/v9\nkind: EnvironmentLocal\nservices: {}\n"),
    );
    assert_eq!(finding_codes(&foreign), ["unsupported_api_version"]);
    let kind = plan(
        &site,
        SHOP,
        &local("apiVersion: stackly/v1alpha1\nkind: Environment\nservices: {}\n"),
    );
    assert_eq!(finding_codes(&kind), ["unsupported_kind"]);
    let message = kind.local.as_ref().expect("a file").findings.list()[0].message();
    assert_eq!(message, "kind must be EnvironmentLocal");

    // An alias the manifest does not declare is only a warning.
    let unknown = plan(
        &site,
        SHOP,
        &local(&overrides(
            "  ghost:\n    endpoints:\n      http: {port: 18100}\n",
        )),
    );
    assert_eq!(finding_codes(&unknown), ["override_unknown_service"]);
    assert!(unknown.applicable, "{:#?}", unknown.blockers);
    let file = unknown.local.as_ref().expect("a file");
    assert_eq!(file.state, LocalState::Present);
    assert_eq!(file.overrides[0].state, OverrideState::Ignored);

    // A file that can't be read blocks too.
    let unreadable = LocalInput {
        file: ".stackly.local.yaml".into(),
        content: LocalFile::from_read(Err(
            stackly_core::environment::UnreadableReason::PermissionDenied,
        )),
    };
    let planned = plan(&site, SHOP, &unreadable);
    assert_eq!(codes(&planned), ["env.local_unreadable"]);
}

// covers: AC-9
#[test]
fn validate_reads_no_override_file() {
    let site = site_with_borrowed();
    let dir = site.root.path().join("project");
    fs::create_dir_all(&dir).expect("the folder");
    let before = validate(SHOP.as_bytes(), &site.catalog);
    fs::write(dir.join(".stackly.local.yaml"), overrides("  api: [\n")).expect("writes");
    assert_eq!(validate(SHOP.as_bytes(), &site.catalog), before);
}

// covers: AC-10
#[test]
fn a_port_override_clears_a_conflict_creates_one_and_never_changes_the_files() {
    let mut site = site_with_borrowed();
    // Another instance holds 18080, the manifest's port.
    site.instance("other", 18080, InstanceLifecycle::Stopped);
    let free = site.plan(SHOP.as_bytes());
    assert!(
        codes(&free).contains(&"supervisor.endpoint_conflict"),
        "{:?}",
        codes(&free)
    );

    let dir = site.root.path().join("project");
    fs::create_dir_all(&dir).expect("the folder");
    let manifest = dir.join("stackly.yaml");
    fs::write(&manifest, SHOP).expect("writes");
    let manifest_sha = validate(SHOP.as_bytes(), &site.catalog).sha256;
    let lock = resolve_entries(
        &validate(SHOP.as_bytes(), &site.catalog)
            .manifest
            .expect("valid"),
        &LockFile::Absent,
        &LockRefresh::None,
        &site.catalog,
        &site.registry,
        &site.host(),
    )
    .expect("resolves");
    // The fake provider is a fixture, so only postgres is locked.
    let lock_bytes = lock.lock.as_ref().map(emit).unwrap_or_default();
    fs::write(dir.join("stackly.lock"), &lock_bytes).expect("writes");
    let file = overrides("  api:\n    endpoints:\n      http: {port: 18081}\n");
    fs::write(dir.join(".stackly.local.yaml"), &file).expect("writes");

    let moved = plan(&site, SHOP, &local(&file));
    assert!(moved.applicable, "{:#?}", moved.blockers);
    let api = &moved.items[0];
    assert_eq!(
        api.endpoints[0].port,
        PortRequest::Fixed(stackly_domain::Port::new(18081).expect("a port"))
    );
    let applied = &moved.local.as_ref().expect("a file").overrides[0];
    assert_eq!(applied.state, OverrideState::Applied);
    assert_eq!(applied.field, "endpoints.http.port");
    assert_eq!(applied.value, "18081");
    // The override changes the digest.
    site.views.retain(|view| view.name.as_str() != "other");
    let without = site.plan(SHOP.as_bytes());
    assert_ne!(without.digest, moved.digest);

    // The files are as they were.
    assert_eq!(fs::read(&manifest).expect("reads"), SHOP.as_bytes());
    assert_eq!(
        fs::read(dir.join("stackly.lock")).expect("reads"),
        lock_bytes
    );
    assert_eq!(
        validate(SHOP.as_bytes(), &site.catalog).sha256,
        manifest_sha
    );
}

// covers: AC-10
#[test]
fn a_member_compares_its_port_against_the_override() {
    let mut site = site_with_borrowed();
    let api = site.instance("shop-api", 18080, InstanceLifecycle::Running);
    let env = EnvironmentBuilder::new()
        .name("shop")
        .manifest_location(site.location("stackly.yaml").as_str())
        .member("api", api.id(), Ownership::Dedicated)
        .build();
    site.registry
        .commit(WriteBatch::new(vec![WriteOp::InsertEnvironment(env)]))
        .expect("commits");
    let planned = plan(
        &site,
        SHOP,
        &local(&overrides(
            "  api:\n    endpoints:\n      http: {port: 18095}\n    storage:\n      dataLocation: /Volumes/Fast\n",
        )),
    );
    let item = &planned.items[0];
    let port: Vec<_> = item
        .differences
        .iter()
        .filter(|d| d.field == "endpoint.http.port")
        .map(|d| (d.manifest.as_str(), d.instance.as_str()))
        .collect();
    assert_eq!(port, [("18095", "18080")]);
    let states: Vec<_> = planned
        .local
        .as_ref()
        .expect("a file")
        .overrides
        .iter()
        .map(|o| o.state)
        .collect();
    assert_eq!(
        states,
        [OverrideState::Compared, OverrideState::NotAppliedExisting]
    );
    // No storage lookup for a member: nothing blocks.
    assert!(
        !codes(&planned).contains(&"env.override_location_unknown"),
        "{:?}",
        codes(&planned)
    );
}

// covers: AC-11
#[test]
fn a_storage_override_names_a_registered_data_location_or_blocks() {
    let site = site_with_borrowed();
    let folder = site.root.path().join("fast");
    fs::create_dir_all(&folder).expect("the folder");
    let id = StorageLocationId::generate();
    let made = prepare_location(&site.platform, &folder, LocationKind::Data, id)
        .expect("the location folders");
    site.registry
        .commit(WriteBatch::new(vec![WriteOp::InsertStorageLocation(
            StorageLocation::custom(
                id,
                LocationKind::Data,
                made.to_str().expect("text"),
                site.fake.clock().now(),
            )
            .expect("a location"),
        )]))
        .expect("commits");
    let canonical = fs::canonicalize(&folder).expect("canonical");
    for written in [canonical.clone(), canonical.join("Stackly")] {
        let body = format!(
            "  db:\n    storage:\n      dataLocation: {}\n",
            written.display()
        );
        let planned = plan(&site, SHOP, &local(&overrides(&body)));
        assert!(planned.applicable, "{:#?}", planned.blockers);
        let db = &planned.items[1];
        let storage = db.storage.as_ref().expect("a storage");
        assert_eq!(storage.location_id, id);
        let row = db
            .checks
            .iter()
            .find(|c| c.id == "storage_location")
            .map(|c| c.status);
        assert!(row.is_some(), "a storage row: {:#?}", db.checks);
    }
    let unknown = plan(
        &site,
        SHOP,
        &local(&overrides(
            "  db:\n    storage:\n      dataLocation: /nowhere/at/all\n",
        )),
    );
    assert_eq!(codes(&unknown), ["env.override_location_unknown"]);
    assert!(matches!(
        &unknown.blockers[0].kind,
        BlockerKind::OverrideLocationUnknown { path } if path == "/nowhere/at/all"
    ));
    assert!(unknown.items[1].storage.is_none());

    // A registered location whose folder is gone blocks through create's
    // storage row.
    fs::remove_dir_all(&made).expect("the folder goes");
    let body = format!(
        "  db:\n    storage:\n      dataLocation: {}\n",
        canonical.display()
    );
    let gone = plan(&site, SHOP, &local(&overrides(&body)));
    assert!(!gone.applicable);
    let row = gone.items[1]
        .checks
        .iter()
        .find(|c| c.id == "storage_location")
        .expect("a storage row");
    assert!(row.error.is_some(), "{row:#?}");
}
