//! `environment::lock` at its edges (spec 0045): the size limit and the
//! file kind of the read, anchors and aliases that load while tags and
//! merge keys never do, an emitter whose bytes never depend on the input
//! order, a lock that is never a source of trust, and an entry whose digest
//! left the catalog that `refresh` `none` never keeps.

use std::fs;
use std::sync::Arc;

use stackly_core::catalog::{
    Catalog, CatalogEntry, EntryOrigin, LocalInfo, ProviderEntry, Revocation, TileColor, TileHint,
};
use stackly_core::environment::lock::{
    EnvironmentLock, LOCK_MAX_LEN, LockChange, LockFile, LockInput, LockRefresh, LockState, emit,
    load, resolve_entries, state,
};
use stackly_core::environment::{BlockerKind, Plan, PlanAction, UnreadableReason, validate};
use stackly_core::provider::{NameCheck, PackageFiles, ValidatedPackage};
use stackly_domain::{Arch, Os, Target};
use stackly_platform::HostFacts;

use crate::common::{catalog, fixture};
use crate::site::{Inputs, Site};

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
      client: {port: 16379}
";

const DEMO: &str = "\
apiVersion: stackly/v1alpha1
kind: Environment
metadata:
  name: shop
services:
  demo:
    provider: acme/demo
    version: \"1.0\"
";

fn mac() -> HostFacts {
    HostFacts::new(
        Target::new(Os::Macos, Arch::Aarch64),
        "27.0".parse().expect("an OS version"),
        None,
    )
}

fn lock_on(site: &Site, catalog: &Catalog, bytes: &str) -> EnvironmentLock {
    let manifest = validate(bytes.as_bytes(), catalog)
        .manifest
        .expect("a valid manifest");
    let resolution = resolve_entries(
        &manifest,
        &LockFile::Absent,
        &LockRefresh::None,
        catalog,
        &site.registry,
        &mac(),
    )
    .expect("the lock resolves");
    assert!(resolution.blockers.is_empty(), "{:#?}", resolution.blockers);
    resolution.lock.expect("a lock")
}

fn input(lock: &EnvironmentLock) -> LockInput {
    LockInput {
        file: "stackly.lock".into(),
        content: LockFile::from_read(Ok(emit(lock))),
    }
}

fn plan(site: &Site, bytes: &str, catalog: &Catalog, lock: &LockInput, locked: bool) -> Plan {
    site.plan_with(
        bytes.as_bytes(),
        &site.location("stackly.yaml"),
        None,
        &Inputs {
            catalog: Some(catalog),
            host: Some(mac()),
            lock: Some(lock),
            local: None,
            locked,
        },
    )
}

fn codes(plan: &Plan) -> Vec<&'static str> {
    plan.blockers.iter().map(|b| b.kind.code()).collect()
}

/// The golden lock of the ERP example: six services, one artifact each.
fn erp_lock_text() -> String {
    String::from_utf8(fixture("erp-development.lock")).expect("UTF-8")
}

// covers: AC-1 (Nama file): a lock is read up to 256 KiB, a byte more is
// `unreadable` `too_large`, and a symlink or a folder at the lock path is
// `unreadable` `not_a_file`, never followed
#[test]
fn a_lock_reads_up_to_256_kib_and_a_larger_file_a_symlink_or_a_folder_is_unreadable() {
    let site = Site::new();
    let catalog = catalog();
    let fs_port = site.platform.fs();
    let dir = site.root.path().join("project");
    fs::create_dir_all(&dir).expect("the folder");
    let text = erp_lock_text();
    let manifest = validate(&fixture("erp-development.yaml"), &catalog)
        .manifest
        .expect("the ERP manifest is valid");

    // A comment line pads the golden lock to exactly the limit.
    let limit = usize::try_from(LOCK_MAX_LEN).expect("fits");
    let pad = limit - text.len() - 3;
    let at_limit = format!("{text}# {}\n", "x".repeat(pad));
    assert_eq!(at_limit.len(), limit);
    let path = dir.join("stackly.lock");
    fs::write(&path, &at_limit).expect("writes");
    let read = LockFile::read(fs_port, &path);
    assert!(matches!(read, LockFile::Valid { .. }), "{read:?}");
    assert_eq!(state(&manifest, &read).0, LockState::InSync);

    fs::write(&path, format!("{at_limit}#")).expect("writes");
    let read = LockFile::read(fs_port, &path);
    assert_eq!(read, LockFile::Unreadable(UnreadableReason::TooLarge));
    assert_eq!(state(&manifest, &read).0, LockState::Unreadable);

    // A symlink to a valid lock is not followed.
    let target = dir.join("elsewhere.lock");
    fs::write(&target, &text).expect("writes");
    fs::remove_file(&path).expect("removes");
    std::os::unix::fs::symlink(&target, &path).expect("links");
    assert_eq!(
        LockFile::read(fs_port, &path),
        LockFile::Unreadable(UnreadableReason::NotAFile)
    );

    fs::remove_file(&path).expect("removes the link");
    fs::create_dir_all(&path).expect("a folder");
    assert_eq!(
        LockFile::read(fs_port, &path),
        LockFile::Unreadable(UnreadableReason::NotAFile)
    );
    fs::remove_dir(&path).expect("removes the folder");
    assert_eq!(LockFile::read(fs_port, &path), LockFile::Absent);
}

// covers: AC-1 (Schema `stackly.lock`, loader): an anchor and its alias
// load into the same lock as the plain text, as does a tag the parser
// resolves, while a merge key or a local tag is refused, and a lock above 32 services is refused
#[test]
fn anchors_and_aliases_load_while_a_merge_key_a_tag_or_33_services_are_refused() {
    let text = erp_lock_text();
    let plain = load(text.as_bytes()).expect("the golden loads");

    // clickhouse and mysql share the revision text.
    let anchored = text
        .replacen(
            "providerRevision: \"1.0.0\"",
            "providerRevision: &rev \"1.0.0\"",
            1,
        )
        .replacen(
            "    providerRevision: \"1.0.0\"\n    providerManifestSha256: \"77a4",
            "    providerRevision: *rev\n    providerManifestSha256: \"77a4",
            1,
        );
    assert!(anchored.contains("*rev"), "the alias is in place");
    assert_eq!(
        load(anchored.as_bytes()).expect("an anchor and its alias load"),
        plain
    );

    let merged = text.replacen(
        "      - target: \"macos-aarch64\"\n        id: \"mysql",
        "      - <<: {target: \"macos-aarch64\"}\n        id: \"mysql",
        1,
    );
    assert!(merged.contains("<<:"));
    assert!(load(merged.as_bytes()).is_err(), "a merge key is refused");

    // A tag the parser resolves (`!!str`) is no tag in the value, as in
    // the manifest (spec 0044, *YAML yang diterima*); a local tag is.
    let tag = |with: &str| {
        text.replacen(
            "providerRevision: \"1.0.0\"",
            &format!("providerRevision: {with} \"1.0.0\""),
            1,
        )
    };
    assert_eq!(
        load(tag("!!str").as_bytes()).expect("a resolved tag loads"),
        plain
    );
    for local in ["!", "!<!name>", "!local"] {
        assert!(
            load(tag(local).as_bytes()).is_err(),
            "the local tag {local} is refused"
        );
    }

    let alias_to_nothing = text.replacen("\"1.0.0\"", "*missing", 1);
    assert!(load(alias_to_nothing.as_bytes()).is_err());

    // 33 entries: the clickhouse entry under 33 aliases.
    let (head, rest) = text.split_once("  clickhouse:\n").expect("an entry");
    let entry = rest.split_once("  mysql:\n").expect("the next entry").0;
    let mut many = head.to_owned();
    for at in 0..33 {
        many.push_str(&format!("  s{at:02}:\n{entry}"));
    }
    assert!(load(many.as_bytes()).is_err(), "33 services are refused");
    let thirty_two = many
        .split_once("  s32:\n")
        .map(|(kept, _)| kept.to_owned())
        .expect("the last entry");
    assert_eq!(
        load(thirty_two.as_bytes())
            .expect("32 services load")
            .services()
            .len(),
        32
    );
}

// covers: AC-1: the bytes come from the content only: services by alias
// and artifacts by target whatever order they were given in, and loading
// the emitted bytes gives back the same lock
#[test]
fn the_emitted_bytes_never_depend_on_the_order_the_entries_came_in() {
    let text = erp_lock_text();
    let lock = load(text.as_bytes()).expect("the golden loads");
    assert_eq!(
        emit(&lock),
        text.as_bytes(),
        "the golden is what emit writes"
    );

    let mut services = lock.services().to_vec();
    services.reverse();
    // A second target, given before the first.
    let mut extra = services[0].artifacts[0].clone();
    extra.target = Target::new(Os::Macos, Arch::X86_64);
    extra.id = format!("{}-intel", extra.id);
    services[0].artifacts.insert(0, extra);
    let shuffled = EnvironmentLock::new(services.clone());
    services.reverse();
    let ordered = EnvironmentLock::new(services);
    let bytes = emit(&shuffled);
    assert_eq!(bytes, emit(&ordered));
    assert_eq!(load(&bytes).expect("loads"), shuffled);
    let written = String::from_utf8(bytes).expect("UTF-8");
    let aarch = written.find("\"macos-aarch64\"").expect("aarch64");
    let intel = written.find("\"macos-x86_64\"").expect("x86_64");
    assert!(aarch < intel, "artifacts by target text");
    assert!(written.ends_with("\"\n") && !written.ends_with("\n\n"));
}

/// The test catalog with valkey revoked by the catalog's `[[revoked]]`.
fn revoked_valkey(catalog: &Catalog) -> Catalog {
    let name = "official/valkey".parse().expect("a name");
    let Some(CatalogEntry::Provider(entry)) = catalog.entry(&name) else {
        panic!("valkey is in the catalog");
    };
    let key = entry.package.key().clone();
    catalog.clone().with_revocations(vec![Revocation::new(
        key.provider().clone(),
        key.revision().clone(),
        None,
        "a test revocation".into(),
        jiff::civil::date(2026, 11, 1),
    )])
}

fn package_at(name: &str) -> ValidatedPackage {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/packages")
        .join(name);
    let mut files = std::collections::BTreeMap::new();
    let mut stack = vec![dir.clone()];
    while let Some(folder) = stack.pop() {
        for item in fs::read_dir(&folder).expect("reads") {
            let path = item.expect("an entry").path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let relative = path
                .strip_prefix(&dir)
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

fn acme_entry(origin: EntryOrigin) -> CatalogEntry {
    CatalogEntry::Provider(ProviderEntry {
        package: Arc::new(package_at("acme-demo")),
        tile: TileHint {
            mark: "Ac".to_owned(),
            tile: TileColor::Gray,
        },
        notes: Vec::new(),
        origin,
    })
}

/// acme/demo as a local import that still waits for its approval.
fn pending_acme(base: &Catalog) -> Catalog {
    use stackly_core::catalog::local::{package_bytes, package_sha256, permissions};
    use stackly_domain::ProviderImportRecord;
    let acme = package_at("acme-demo");
    let key = acme.key().clone();
    let record = ProviderImportRecord::new(
        key.provider().clone(),
        "0199f000-0000-7000-8000-000000000011"
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
    base.with_local(vec![acme_entry(EntryOrigin::Local(Box::new(
        LocalInfo::new(record, permissions(&acme)),
    )))])
}

// covers: AC-2, AC-7 (Runtime dari lock, step 1): a lock is a record of
// a choice, never a source of trust: a matching entry of a provider the
// catalog revoked, or of a local provider that waits for its approval,
// blocks with the gate's own code in both modes and gives no runtime,
// and `env.lock` writes no lock for either
#[test]
fn a_lock_never_trusts_a_revoked_or_pending_provider_in_either_mode() {
    let site = Site::new();
    let base = catalog();
    let valkey_lock = input(&lock_on(&site, &base, CACHE));
    let revoked = revoked_valkey(&base);

    // acme/demo locked while it was a catalog entry, then planned as a
    // local import with no approval: the same package key.
    let trusted_acme = base.with_local(vec![acme_entry(EntryOrigin::Catalog)]);
    let acme_lock = input(&lock_on(&site, &trusted_acme, DEMO));
    let pending = pending_acme(&base);

    let cases: [(&str, &Catalog, &LockInput, &str); 2] = [
        (CACHE, &revoked, &valkey_lock, "provider.revoked"),
        (DEMO, &pending, &acme_lock, "provider.approval_required"),
    ];
    for (manifest, catalog, lock, code) in cases {
        for locked in [false, true] {
            let planned = plan(&site, manifest, catalog, lock, locked);
            assert_eq!(planned.lock.state, LockState::InSync, "{code}");
            assert_eq!(codes(&planned), [code], "{code}, locked {locked}");
            assert!(!planned.applicable && planned.digest.is_none());
            let item = &planned.items[0];
            assert_eq!(item.action, PlanAction::Create);
            assert!(item.runtime.is_none(), "{code}: no runtime from the lock");
        }
        let resolution = resolve_entries(
            &validate(manifest.as_bytes(), catalog)
                .manifest
                .expect("valid"),
            &lock.content,
            &LockRefresh::All,
            catalog,
            &site.registry,
            &mac(),
        )
        .expect("answers");
        assert!(resolution.lock.is_none(), "{code}: no lock is written");
        assert_eq!(
            resolution
                .blockers
                .iter()
                .map(|b| b.kind.code())
                .collect::<Vec<_>>(),
            [code]
        );
    }
}

// covers: AC-3 (Entri yang dipertahankan): an entry whose locked digest
// the catalog no longer has is picked again with `refresh` `none`, so the
// new lock holds the catalog's digest and never the foreign one
#[test]
fn refresh_none_picks_again_an_entry_whose_digest_left_the_catalog() {
    let site = Site::new();
    let catalog = catalog();
    let lock = lock_on(&site, &catalog, CACHE);
    let good = lock.services()[0].artifacts[0].sha256;
    let mut services = lock.services().to_vec();
    services[0].artifacts[0].sha256 = "ef".repeat(32).parse().expect("a digest");
    let foreign = EnvironmentLock::new(services);
    let existing = LockFile::from_read(Ok(emit(&foreign)));

    let resolution = resolve_entries(
        &validate(CACHE.as_bytes(), &catalog)
            .manifest
            .expect("valid"),
        &existing,
        &LockRefresh::None,
        &catalog,
        &site.registry,
        &mac(),
    )
    .expect("resolves");
    assert!(resolution.blockers.is_empty(), "{:#?}", resolution.blockers);
    let written = resolution.lock.expect("a lock");
    assert_eq!(written.services()[0].artifacts[0].sha256, good);
    assert_eq!(written, lock, "the same lock as a fresh one");
    assert_eq!(resolution.services[0].change, LockChange::Kept);

    // The plan never takes the foreign digest either.
    let planned = plan(&site, CACHE, &catalog, &input(&foreign), false);
    assert!(matches!(
        planned.blockers[0].kind,
        BlockerKind::LockArtifactUnknown { .. }
    ));
    assert!(planned.items[0].runtime.is_none());
}
