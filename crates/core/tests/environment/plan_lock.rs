//! `environment::plan` with a lock (spec 0045): AC-5, AC-6, AC-7, AC-8,
//! AC-12.

use stackly_core::catalog::Catalog;
use stackly_core::environment::lock::{
    EnvironmentLock, LockFile, LockInput, LockRefresh, LockStaleReason, LockState, LockedArtifact,
    LockedService, emit, resolve_entries,
};
use stackly_core::environment::{BlockerKind, Plan, PlanAction, RuntimeSource, validate};
use stackly_domain::{
    Arch, InstallMode, InstanceLifecycle, Os, Ownership, Registry, Target, WriteBatch, WriteOp,
};
use stackly_platform::HostFacts;
use stackly_testkit::EnvironmentBuilder;

use crate::common::catalog;
use crate::lock::{add_target, bump, valkey_catalog};
use crate::site::{Inputs, Site};

const TWO: &str = "\
apiVersion: stackly/v1alpha1
kind: Environment
metadata:
  name: shop
services:
  cache:
    provider: official/valkey
    endpoints:
      client: {port: 16379}
  db:
    provider: official/postgresql
    version: \"18\"
    endpoints:
      client: {port: 15432}
";

fn host(os: Os, arch: Arch) -> HostFacts {
    HostFacts::new(
        Target::new(os, arch),
        "27.0".parse().expect("an OS version"),
        None,
    )
}

fn lock_on(site: &Site, catalog: &Catalog, bytes: &str, host: &HostFacts) -> EnvironmentLock {
    let manifest = validate(bytes.as_bytes(), catalog)
        .manifest
        .expect("a valid manifest");
    let resolution = resolve_entries(
        &manifest,
        &LockFile::Absent,
        &LockRefresh::None,
        catalog,
        &site.registry,
        host,
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

fn plan(site: &Site, bytes: &str, extra: &Inputs<'_>) -> Plan {
    site.plan_with(
        bytes.as_bytes(),
        &site.location("stackly.yaml"),
        None,
        extra,
    )
}

fn codes(plan: &Plan) -> Vec<&'static str> {
    plan.blockers.iter().map(|b| b.kind.code()).collect()
}

fn mac() -> HostFacts {
    host(Os::Macos, Arch::Aarch64)
}

// covers: AC-5
#[test]
fn a_matching_entry_keeps_its_version_when_stable_moves_and_a_new_service_uses_the_catalog() {
    let site = Site::new();
    // The same provider manifest, so only the versions differ.
    let lock = lock_on(&site, &valkey_catalog(|_| {}), TWO, &mac());
    let newer = valkey_catalog(bump);
    let with_new = format!(
        "{TWO}  queue:\n    provider: official/nats\n    endpoints:\n      client: {{port: 14222}}\n"
    );
    let lock_input = input(&lock);
    let planned = plan(
        &site,
        &with_new,
        &Inputs {
            catalog: Some(&newer),
            lock: Some(&lock_input),
            ..Inputs::default()
        },
    );
    assert!(planned.applicable, "{:#?}", planned.blockers);
    let runtime = |alias: &str| {
        planned
            .items
            .iter()
            .find(|i| i.alias.as_str() == alias)
            .and_then(|i| i.runtime.clone())
            .expect("a runtime")
    };
    assert_eq!(runtime("cache").version.to_string(), "9.1.2");
    assert_eq!(runtime("cache").source, RuntimeSource::Lock);
    assert_eq!(runtime("db").source, RuntimeSource::Lock);
    assert_eq!(runtime("queue").source, RuntimeSource::Catalog);
    assert_eq!(planned.lock.state, LockState::Stale);
    assert_eq!(planned.lock.stale.len(), 1);
    assert_eq!(planned.lock.stale[0].reason, LockStaleReason::NotLocked);
    assert_eq!(planned.lock.sha256, lock_input.content.sha256());
    assert_eq!(planned.lock.targets, ["macos-aarch64"]);
    assert!(planned.items[0].provider_revision.is_some());

    // Without the lock the catalog's stable is used.
    let free = plan(
        &site,
        TWO,
        &Inputs {
            catalog: Some(&newer),
            ..Inputs::default()
        },
    );
    assert_eq!(free.lock.state, LockState::Absent);
    let cache = free.items[0].runtime.as_ref().expect("a runtime");
    assert_eq!(cache.version.to_string(), "9.1.3");
    assert_eq!(cache.source, RuntimeSource::Catalog);
}

// covers: AC-6
#[test]
fn locked_blocks_without_a_lock_with_a_damaged_lock_and_for_every_stale_alias() {
    let site = Site::new();
    let locked = |lock: Option<&LockInput>, bytes: &str| {
        plan(
            &site,
            bytes,
            &Inputs {
                lock,
                locked: true,
                ..Inputs::default()
            },
        )
    };
    let none = locked(None, TWO);
    assert_eq!(codes(&none), ["env.lock_missing"]);
    assert!(!none.applicable && none.digest.is_none());
    assert!(none.lock.locked);
    assert!(none.items.iter().all(|i| i.runtime.is_none()));

    let damaged = LockInput {
        file: "stackly.lock".into(),
        content: LockFile::from_read(Ok(b"kind: x".to_vec())),
    };
    assert_eq!(codes(&locked(Some(&damaged), TWO)), ["env.lock_invalid"]);
    // A damaged lock blocks without `locked` too.
    assert_eq!(
        codes(&plan(
            &site,
            TWO,
            &Inputs {
                lock: Some(&damaged),
                ..Inputs::default()
            }
        )),
        ["env.lock_invalid"]
    );

    let lock = lock_on(&site, &catalog(), TWO, &mac());
    let stale = format!(
        "{}  queue:\n    provider: official/nats\n    endpoints:\n      client: {{port: 14222}}\n",
        TWO.replace("version: \"18\"", "version: \"17\"")
    );
    let lock_input = input(&lock);
    let blocked = locked(Some(&lock_input), &stale);
    assert_eq!(codes(&blocked), ["env.lock_stale", "env.lock_stale"]);
    let stale_kinds: Vec<_> = blocked
        .blockers
        .iter()
        .map(|b| (b.service.as_ref().map(ToString::to_string), b.kind.clone()))
        .collect();
    assert_eq!(
        stale_kinds,
        [
            (
                Some("db".to_owned()),
                BlockerKind::LockStale {
                    reason: LockStaleReason::SelectorChanged
                }
            ),
            (
                Some("queue".to_owned()),
                BlockerKind::LockStale {
                    reason: LockStaleReason::NotLocked
                }
            ),
        ]
    );
    // Nothing came from the catalog.
    assert!(
        blocked
            .items
            .iter()
            .filter_map(|i| i.runtime.as_ref())
            .all(|r| r.source == RuntimeSource::Lock)
    );

    // An in sync lock with `locked` is applicable.
    let fine = locked(Some(&lock_input), TWO);
    assert!(fine.applicable, "{:#?}", fine.blockers);
}

// covers: AC-7
#[test]
fn a_locked_entry_is_met_exactly_or_blocks_in_both_modes() {
    let site = Site::new();
    let two_targets = valkey_catalog(|v| add_target(v, "x86_64", "ab"));
    let lock = lock_on(&site, &two_targets, TWO, &mac());
    assert_eq!(
        lock.services()[0].targets(),
        ["macos-aarch64", "macos-x86_64"]
    );
    let linux = host(Os::Linux, Arch::Aarch64);
    let changed = |edit: &dyn Fn(&mut LockedService)| {
        let mut services = lock.services().to_vec();
        edit(&mut services[0]);
        input(&EnvironmentLock::new(services))
    };
    let cases: Vec<(LockInput, Option<HostFacts>, &str)> = vec![
        (input(&lock), Some(linux.clone()), "env.lock_no_artifact"),
        (
            changed(&|s| s.artifacts[0].sha256 = "ef".repeat(32).parse().expect("a digest")),
            None,
            "env.lock_artifact_unknown",
        ),
        (
            changed(&|s| s.version = "9.1.9".parse().expect("a version")),
            None,
            "env.lock_artifact_unknown",
        ),
        (
            changed(&|s| s.artifacts[0].id = "valkey-other".into()),
            None,
            "env.lock_artifact_unknown",
        ),
        (
            changed(&|s| s.provider_manifest_sha256 = "01".repeat(32).parse().expect("a digest")),
            None,
            "env.lock_provider_mismatch",
        ),
        (
            input(&lock),
            Some(HostFacts::new(
                Target::new(Os::Macos, Arch::Aarch64),
                "26.0".parse().expect("an OS version"),
                None,
            )),
            "catalog.not_installable",
        ),
    ];
    for (lock_input, host, code) in cases {
        for locked in [false, true] {
            let planned = plan(
                &site,
                TWO,
                &Inputs {
                    catalog: Some(&two_targets),
                    host: host.clone(),
                    lock: Some(&lock_input),
                    local: None,
                    locked,
                },
            );
            let found = codes(&planned);
            assert!(
                found.contains(&code),
                "{code} with locked {locked}: {found:?}"
            );
            assert!(!planned.applicable);
            let cache = &planned.items[0];
            assert_eq!(cache.action, PlanAction::Create);
            assert!(cache.runtime.is_none(), "no other artifact is used");
        }
    }
    // The reasons of an unknown digest.
    let reason = |lock_input: &LockInput| {
        let planned = plan(
            &site,
            TWO,
            &Inputs {
                catalog: Some(&two_targets),
                lock: Some(lock_input),
                ..Inputs::default()
            },
        );
        planned.blockers.iter().find_map(|b| match &b.kind {
            BlockerKind::LockArtifactUnknown { reason, .. } => Some(reason.as_code()),
            _ => None,
        })
    };
    assert_eq!(
        reason(&changed(
            &|s| s.artifacts[0].sha256 = "ef".repeat(32).parse().expect("a digest")
        )),
        Some("not_in_catalog")
    );
    assert_eq!(
        reason(&changed(
            &|s| s.version = "9.1.9".parse().expect("a version")
        )),
        Some("version_mismatch")
    );
    assert_eq!(
        reason(&changed(&|s| s.artifacts[0].sha256 = s.artifacts[1].sha256)),
        Some("target_mismatch")
    );
    assert_eq!(
        reason(&changed(&|s| s.artifacts[0].id = "valkey-other".into())),
        Some("id_mismatch")
    );
}

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
";

// covers: AC-8
#[test]
fn a_member_with_another_runtime_than_the_lock_is_kept_with_a_lock_difference() {
    let mut site = Site::new();
    let api = site.instance("shop-api", 18080, InstanceLifecycle::Running);
    let env = EnvironmentBuilder::new()
        .name("shop")
        .manifest_location(site.location("stackly.yaml").as_str())
        .member("api", api.id(), Ownership::Dedicated)
        .build();
    site.registry
        .commit(WriteBatch::new(vec![WriteOp::InsertEnvironment(env)]))
        .expect("commits");
    let own = match site.runtime.install() {
        InstallMode::Managed { artifact_sha256 } => *artifact_sha256,
        InstallMode::External { .. } => panic!("the fake runtime is managed"),
    };
    let entry = |sha256| LockedService {
        alias: "api".parse().expect("an alias"),
        provider: "testing/fake".parse().expect("a provider"),
        provider_revision: site.fake_key.revision().clone(),
        provider_manifest_sha256: *site.fake_key.manifest_sha256(),
        selector: "0.1".into(),
        version: site.runtime.version().clone(),
        line: site.runtime.line().clone(),
        artifacts: vec![LockedArtifact {
            target: site.host().target,
            id: "fake-other".into(),
            sha256,
        }],
    };
    for locked in [false, true] {
        let other = input(&EnvironmentLock::new(vec![entry(
            "aa".repeat(32).parse().expect("a digest"),
        )]));
        let planned = plan(
            &site,
            SHOP,
            &Inputs {
                lock: Some(&other),
                locked,
                ..Inputs::default()
            },
        );
        assert!(planned.applicable, "{:#?}", planned.blockers);
        let item = &planned.items[0];
        assert_eq!(item.action, PlanAction::Keep);
        let lock: Vec<_> = item
            .differences
            .iter()
            .filter(|d| d.field == "lock")
            .collect();
        assert_eq!(lock.len(), 1, "{:#?}", item.differences);

        let same = input(&EnvironmentLock::new(vec![entry(own)]));
        let planned = plan(
            &site,
            SHOP,
            &Inputs {
                lock: Some(&same),
                locked,
                ..Inputs::default()
            },
        );
        assert!(
            planned.items[0]
                .differences
                .iter()
                .all(|d| d.field != "lock")
        );
    }
}

// covers: AC-12
#[test]
fn the_plan_digest_changes_with_locked_the_lock_bytes_and_the_provider_revision() {
    let site = Site::new();
    let lock = lock_on(&site, &catalog(), TWO, &mac());
    let lock_input = input(&lock);
    let digest = |extra: &Inputs<'_>| plan(&site, TWO, extra).digest.expect("applicable");
    let free = digest(&Inputs::default());
    let with_lock = digest(&Inputs {
        lock: Some(&lock_input),
        ..Inputs::default()
    });
    let locked = digest(&Inputs {
        lock: Some(&lock_input),
        locked: true,
        ..Inputs::default()
    });
    assert_ne!(free, with_lock);
    assert_ne!(with_lock, locked);
    // Other lock bytes for the same choice: another digest.
    let mut services = lock.services().to_vec();
    services[0].artifacts.push(LockedArtifact {
        target: Target::new(Os::Linux, Arch::Aarch64),
        id: "valkey-linux".into(),
        sha256: "12".repeat(32).parse().expect("a digest"),
    });
    let wider = input(&EnvironmentLock::new(services));
    assert_ne!(
        digest(&Inputs {
            lock: Some(&wider),
            ..Inputs::default()
        }),
        with_lock
    );
    // Another provider revision in the catalog: another digest.
    let changed = valkey_catalog(|_| {});
    assert_ne!(
        digest(&Inputs {
            catalog: Some(&changed),
            ..Inputs::default()
        }),
        free
    );
    let _ = site.registry.environments();
}
