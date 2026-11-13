//! `environment::lock` (spec 0045): AC-1, AC-2, AC-3, AC-4.

use std::fs;

use serde_json::Value;
use stackly_core::catalog::Catalog;
use stackly_core::environment::lock::{
    EnvironmentLock, LockChange, LockError, LockFile, LockRefresh, LockResolution, LockStaleReason,
    LockState, emit, load, resolve_entries, state,
};
use stackly_core::environment::{BlockerKind, EnvironmentManifest, validate};
use stackly_domain::{Arch, Os, Target};
use stackly_platform::HostFacts;

use crate::common::{catalog, catalog_with, fixture, fixtures, package_with};
use crate::site::Site;

const CACHE: &str = "\
apiVersion: stackly/v1alpha1
kind: Environment
metadata:
  name: shop
services:
  cache:
    provider: official/valkey
    version: stable
    endpoints:
      client: {port: 6379}
";

/// `versions.json` of valkey with a copy of its artifact for another
/// target and digest.
pub fn add_target(value: &mut Value, arch: &str, hex: &str) {
    let artifacts = value["artifacts"].as_array_mut().expect("artifacts");
    let mut copy = artifacts[0].clone();
    let id = copy["id"].as_str().expect("an id").replace("aarch64", arch);
    copy["id"] = Value::from(id.clone());
    copy["arch"] = Value::from(arch);
    copy["sha256"] = Value::from(hex.repeat(32));
    copy["archiveRoot"] = Value::from(id.clone());
    copy["urls"] = Value::from(vec![format!("https://example.com/{id}.tar.gz")]);
    copy["compatibility"] = Value::from(Vec::<Value>::new());
    artifacts.push(copy);
}

/// `versions.json` of valkey with a newer `stable` version 9.1.3.
pub fn bump(value: &mut Value) {
    let artifacts = value["artifacts"].as_array_mut().expect("artifacts");
    let mut newer = artifacts[0].clone();
    artifacts[0]
        .as_object_mut()
        .expect("an artifact")
        .remove("stable");
    let id = "valkey-9.1.3-r1-macos-aarch64";
    newer["id"] = Value::from(id);
    newer["version"] = Value::from("9.1.3");
    newer["sha256"] = Value::from("cd".repeat(32));
    newer["archiveRoot"] = Value::from(id);
    newer["urls"] = Value::from(vec![format!("https://example.com/{id}.tar.gz")]);
    newer["compatibility"] = Value::from(Vec::<Value>::new());
    artifacts.insert(0, newer);
}

pub fn valkey_catalog(edit: impl FnOnce(&mut Value)) -> Catalog {
    catalog_with(vec![("valkey", package_with("valkey", edit))])
}

fn manifest(bytes: &[u8], catalog: &Catalog) -> EnvironmentManifest {
    let validation = validate(bytes, catalog);
    validation
        .manifest
        .unwrap_or_else(|| panic!("the manifest is valid: {:#?}", validation.findings))
}

fn host(arch: Arch) -> HostFacts {
    HostFacts::new(
        Target::new(Os::Macos, arch),
        "27.0".parse().expect("an OS version"),
        None,
    )
}

fn lock_of(
    site: &Site,
    catalog: &Catalog,
    bytes: &[u8],
    existing: &LockFile,
    refresh: &LockRefresh,
    arch: Arch,
) -> LockResolution {
    resolve_entries(
        &manifest(bytes, catalog),
        existing,
        refresh,
        catalog,
        &site.registry,
        &host(arch),
    )
    .expect("the lock resolves")
}

fn valid(lock: &EnvironmentLock) -> LockFile {
    LockFile::from_read(Ok(emit(lock)))
}

fn changes(resolution: &LockResolution) -> Vec<(String, LockChange)> {
    resolution
        .services
        .iter()
        .map(|s| (s.alias.to_string(), s.change))
        .collect()
}

// covers: AC-1, AC-2, AC-3
#[test]
fn the_erp_example_locks_six_services_into_the_same_bytes_every_time() {
    let site = Site::new();
    let catalog = catalog();
    let bytes = fixture("erp-development.yaml");
    let first = lock_of(
        &site,
        &catalog,
        &bytes,
        &LockFile::Absent,
        &LockRefresh::None,
        Arch::Aarch64,
    );
    assert!(first.blockers.is_empty(), "{:#?}", first.blockers);
    let lock = first.lock.expect("a lock");
    assert_eq!(lock.services().len(), 6);
    assert!(
        first
            .services
            .iter()
            .all(|s| s.change == LockChange::Added && s.version.is_some())
    );
    let written = emit(&lock);

    let golden = fixtures().join("erp-development.lock");
    if std::env::var_os("STACKLY_UPDATE_GOLDEN").is_some() {
        fs::write(&golden, &written).expect("the golden writes");
    }
    let expected = fs::read(&golden).expect("the golden lock exists");
    assert_eq!(
        String::from_utf8_lossy(&written),
        String::from_utf8_lossy(&expected)
    );
    assert_eq!(load(&written).expect("the lock loads"), lock);

    // A second run keeps everything and gives the same bytes.
    let second = lock_of(
        &site,
        &catalog,
        &bytes,
        &valid(&lock),
        &LockRefresh::None,
        Arch::Aarch64,
    );
    assert!(second.services.iter().all(|s| s.change == LockChange::Kept));
    assert_eq!(emit(&second.lock.expect("a lock")), written);

    // No secret reference, setting, port, path, or host.
    let text = String::from_utf8(written).expect("UTF-8");
    for absent in [
        "local:",
        "erp/postgres/admin",
        "5432",
        "maxConnections",
        "/Users",
        "127.0.0.1",
        "https://",
        site.root.path().to_str().expect("a UTF-8 root"),
    ] {
        assert!(!text.contains(absent), "{absent} is in the lock:\n{text}");
    }
}

// covers: AC-2
#[test]
fn a_selector_without_an_artifact_blocks_and_gives_no_lock() {
    let site = Site::new();
    let catalog = catalog();
    let bytes = CACHE.replace("version: stable", "version: \"99\"");
    let resolution = lock_of(
        &site,
        &catalog,
        bytes.as_bytes(),
        &LockFile::Absent,
        &LockRefresh::None,
        Arch::Aarch64,
    );
    assert!(resolution.lock.is_none());
    let codes: Vec<&str> = resolution.blockers.iter().map(|b| b.kind.code()).collect();
    assert_eq!(codes, ["runtime.selector_unresolved"]);
}

// covers: AC-2
#[test]
fn a_shared_service_never_enters_the_lock() {
    let site = Site::new();
    let catalog = catalog();
    let bytes = format!(
        "{CACHE}  db:\n    provider: official/postgresql\n    ownership: shared\n    instanceRef: main-db\n"
    );
    let resolution = lock_of(
        &site,
        &catalog,
        bytes.as_bytes(),
        &LockFile::Absent,
        &LockRefresh::None,
        Arch::Aarch64,
    );
    let lock = resolution.lock.expect("a lock");
    let aliases: Vec<&str> = lock.services().iter().map(|s| s.alias.as_str()).collect();
    assert_eq!(aliases, ["cache"]);
}

// covers: AC-3
#[test]
fn refresh_none_keeps_versions_all_updates_them_and_services_updates_one() {
    let site = Site::new();
    let old = catalog();
    let bytes = format!(
        "{CACHE}  other:\n    provider: official/valkey\n    endpoints:\n      client: {{port: 6380}}\n"
    );
    let locked = lock_of(
        &site,
        &old,
        bytes.as_bytes(),
        &LockFile::Absent,
        &LockRefresh::None,
        Arch::Aarch64,
    )
    .lock
    .expect("a lock");
    let newer = valkey_catalog(bump);

    let none = lock_of(
        &site,
        &newer,
        bytes.as_bytes(),
        &valid(&locked),
        &LockRefresh::None,
        Arch::Aarch64,
    );
    assert_eq!(
        changes(&none),
        [
            ("cache".to_owned(), LockChange::Kept),
            ("other".to_owned(), LockChange::Kept)
        ]
    );
    // The versions and artifacts stay; the provider key is the catalog's
    // now, whose package changed.
    let kept = none.lock.expect("a lock");
    for (kept, before) in kept.services().iter().zip(locked.services()) {
        assert_eq!(kept.version, before.version);
        assert_eq!(kept.artifacts, before.artifacts);
        assert_ne!(
            kept.provider_manifest_sha256,
            before.provider_manifest_sha256
        );
    }

    let all = lock_of(
        &site,
        &newer,
        bytes.as_bytes(),
        &valid(&locked),
        &LockRefresh::All,
        Arch::Aarch64,
    );
    assert_eq!(
        changes(&all),
        [
            ("cache".to_owned(), LockChange::Updated),
            ("other".to_owned(), LockChange::Updated)
        ]
    );
    assert_eq!(
        all.services[0]
            .previous_version
            .as_ref()
            .map(ToString::to_string)
            .as_deref(),
        Some("9.1.2")
    );

    let one = lock_of(
        &site,
        &newer,
        bytes.as_bytes(),
        &valid(&locked),
        &LockRefresh::Services(vec!["other".parse().expect("an alias")]),
        Arch::Aarch64,
    );
    assert_eq!(
        changes(&one),
        [
            ("cache".to_owned(), LockChange::Kept),
            ("other".to_owned(), LockChange::Updated)
        ]
    );

    let unknown = resolve_entries(
        &manifest(bytes.as_bytes(), &newer),
        &valid(&locked),
        &LockRefresh::Services(vec!["nope".parse().expect("an alias")]),
        &newer,
        &site.registry,
        &host(Arch::Aarch64),
    );
    assert!(matches!(unknown, Err(LockError::ServiceNotFound(_))));
}

// covers: AC-3
#[test]
fn a_dropped_service_is_removed_and_a_new_target_is_added_without_a_new_version() {
    let site = Site::new();
    let old = catalog();
    let bytes = format!(
        "{CACHE}  other:\n    provider: official/valkey\n    endpoints:\n      client: {{port: 6380}}\n"
    );
    let locked = lock_of(
        &site,
        &old,
        bytes.as_bytes(),
        &LockFile::Absent,
        &LockRefresh::None,
        Arch::Aarch64,
    )
    .lock
    .expect("a lock");
    let two_targets = valkey_catalog(|v| add_target(v, "x86_64", "ab"));

    // An Intel Mac with the lock of an Apple silicon Mac keeps the
    // version and adds its own artifact.
    let intel = lock_of(
        &site,
        &two_targets,
        CACHE.as_bytes(),
        &valid(&locked),
        &LockRefresh::None,
        Arch::X86_64,
    );
    assert_eq!(
        changes(&intel),
        [
            ("cache".to_owned(), LockChange::Kept),
            ("other".to_owned(), LockChange::Removed)
        ]
    );
    let lock = intel.lock.expect("a lock");
    let cache = &lock.services()[0];
    assert_eq!(cache.version, locked.services()[0].version);
    assert_eq!(cache.targets(), ["macos-aarch64", "macos-x86_64"]);
    assert_eq!(lock.services().len(), 1);
}

// covers: AC-3
#[test]
fn a_foreign_lock_is_never_replaced_and_a_damaged_one_only_with_refresh_all() {
    let site = Site::new();
    let catalog = catalog();
    let foreign = LockFile::from_read(Ok(
        b"apiVersion: stackly/v2\nkind: EnvironmentLock\nservices: {}\n".to_vec(),
    ));
    assert!(matches!(&foreign, LockFile::Invalid(problem) if problem.is_foreign()));
    for refresh in [LockRefresh::None, LockRefresh::All] {
        let resolution = lock_of(
            &site,
            &catalog,
            CACHE.as_bytes(),
            &foreign,
            &refresh,
            Arch::Aarch64,
        );
        assert!(resolution.lock.is_none());
        assert!(matches!(
            resolution.blockers[0].kind,
            BlockerKind::LockInvalid { .. }
        ));
    }

    let damaged = LockFile::from_read(Ok(
        b"apiVersion: stackly/v1alpha1\nkind: EnvironmentLock\nservices:\n  cache: {provider: x}\n"
            .to_vec(),
    ));
    assert!(matches!(&damaged, LockFile::Invalid(problem) if !problem.is_foreign()));
    let none = lock_of(
        &site,
        &catalog,
        CACHE.as_bytes(),
        &damaged,
        &LockRefresh::None,
        Arch::Aarch64,
    );
    assert_eq!(
        none.blockers
            .iter()
            .map(|b| b.kind.code())
            .collect::<Vec<_>>(),
        ["env.lock_invalid"]
    );
    let all = lock_of(
        &site,
        &catalog,
        CACHE.as_bytes(),
        &damaged,
        &LockRefresh::All,
        Arch::Aarch64,
    );
    assert!(all.lock.is_some());
    assert_eq!(changes(&all), [("cache".to_owned(), LockChange::Added)]);
}

// covers: AC-1
#[test]
fn the_loader_refuses_unknown_fields_repeated_targets_and_tags() {
    let site = Site::new();
    let catalog = catalog();
    let lock = lock_of(
        &site,
        &catalog,
        CACHE.as_bytes(),
        &LockFile::Absent,
        &LockRefresh::None,
        Arch::Aarch64,
    )
    .lock
    .expect("a lock");
    let text = String::from_utf8(emit(&lock)).expect("UTF-8");
    let with = |extra: &str, at: &str| text.replace(at, &format!("{at}{extra}"));
    let cases = [
        with("    port: 1\n", "    line: \"9.1\"\n"),
        with("    selector: \"stable\"\n", "    line: \"9.1\"\n"),
        text.replace("\"stable\"", "*x"),
        text.replace("selector: \"stable\"", "selector: !custom stable"),
        {
            let artifact = text
                .split_once("      - target")
                .map(|(_, rest)| format!("      - target{rest}"))
                .expect("an artifact");
            format!("{text}{artifact}")
        },
    ];
    for case in cases {
        assert!(load(case.as_bytes()).is_err(), "{case}");
    }
}

// covers: AC-4
#[test]
fn each_stale_reason_has_its_case_and_ports_settings_comments_and_order_stay_in_sync() {
    let site = Site::new();
    let catalog = catalog();
    let two = format!(
        "{CACHE}  db:\n    provider: official/postgresql\n    version: \"18\"\n    endpoints:\n      client: {{port: 5432}}\n"
    );
    let lock = lock_of(
        &site,
        &catalog,
        two.as_bytes(),
        &LockFile::Absent,
        &LockRefresh::None,
        Arch::Aarch64,
    )
    .lock
    .expect("a lock");
    let file = valid(&lock);
    let state_of = |text: &str| state(&manifest(text.as_bytes(), &catalog), &file);

    // Same services, other ports, settings, comments, and order.
    let reordered = "# a comment\napiVersion: stackly/v1alpha1\nkind: Environment\nmetadata:\n  name: shop\nservices:\n  db:\n    provider: official/postgresql\n    version: \"18\"\n    endpoints:\n      client: {port: 5544}\n    settings:\n      maxConnections: 50\n  cache:\n    provider: official/valkey\n    endpoints:\n      client: {port: auto}\n";
    // `stable` written and not written are one selector.
    assert_eq!(state_of(reordered), (LockState::InSync, Vec::new()));

    let reason = |text: &str| {
        let (found, items) = state_of(text);
        assert_eq!(found, LockState::Stale);
        items.iter().map(|i| i.reason).collect::<Vec<_>>()
    };
    assert_eq!(
        reason(&two.replace("version: \"18\"", "version: \"17\"")),
        [LockStaleReason::SelectorChanged]
    );
    assert_eq!(reason(CACHE), [LockStaleReason::NotInManifest]);
    assert_eq!(
        reason(&format!(
            "{two}  extra:\n    provider: official/valkey\n    endpoints:\n      client: {{port: 6390}}\n"
        )),
        [LockStaleReason::NotLocked]
    );
    assert_eq!(
        reason(&two.replace(
            "  db:\n    provider: official/postgresql\n    version: \"18\"\n    endpoints:\n      client: {port: 5432}\n",
            "  db:\n    provider: official/postgresql\n    ownership: shared\n    instanceRef: main-db\n"
        )),
        [LockStaleReason::NotInManifest]
    );

    // Another provider under the alias, and a selector the version does
    // not satisfy.
    let mut swapped = lock.clone();
    let mut services = swapped.services().to_vec();
    services[1].provider = "official/mysql".parse().expect("a provider");
    swapped = EnvironmentLock::new(services);
    let (_, items) = state(&manifest(two.as_bytes(), &catalog), &valid(&swapped));
    assert_eq!(
        items.iter().map(|i| i.reason).collect::<Vec<_>>(),
        [LockStaleReason::ProviderChanged]
    );
    let mut services = lock.services().to_vec();
    services[1].version = "17.9".parse().expect("a version");
    services[1].line = "17".parse().expect("a line");
    let (_, items) = state(
        &manifest(two.as_bytes(), &catalog),
        &valid(&EnvironmentLock::new(services)),
    );
    assert_eq!(
        items.iter().map(|i| i.reason).collect::<Vec<_>>(),
        [LockStaleReason::SelectorUnsatisfied]
    );

    assert_eq!(
        state(&manifest(two.as_bytes(), &catalog), &LockFile::Absent).0,
        LockState::Absent
    );
}
