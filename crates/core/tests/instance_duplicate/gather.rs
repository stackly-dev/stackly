//! `gather_duplicate` over fakes (spec 0024 AC-1 and the core part of
//! AC-7): the checks in their order, and a source that leaves no trace.

use stackly_core::catalog::CatalogState;
use stackly_core::instance::duplicate::{DuplicateDeps, DuplicateError, gather_duplicate};
use stackly_domain::{
    InstanceId, InstanceLifecycle, OperationId, Registry, RuntimeState, WriteBatch, WriteOp,
};
use stackly_evidence::{Kind, evidence};
use stackly_platform::Clock;
use stackly_testkit::RuntimeBuilder;

use crate::common::{Site, commit_runtime};
use crate::support::{
    add_source, change_digest, deps, facts, pin_package, set_config, single_instance, use_secret,
};

fn site_with_source() -> (Site, stackly_domain::Instance) {
    let site = Site::new();
    pin_package(&site, "fake", |_| {});
    let source = add_source(&site, "fake-a");
    (site, source)
}

fn gather(site: &Site, id: InstanceId) -> Result<(), DuplicateError> {
    gather_duplicate(&deps(site), id).map(|_| ())
}

#[test]
fn an_unknown_instance_is_not_found() {
    let (site, _) = site_with_source();

    assert!(matches!(
        gather(&site, InstanceId::generate()),
        Err(DuplicateError::NotFound)
    ));
}

#[test]
fn a_retained_instance_is_refused_before_anything_else_is_read() {
    let site = Site::new();
    // No pinned package: the retained check must come first.
    let retained = site.add_instance(|builder| {
        builder
            .name("fake-old")
            .retained(OperationId::generate(), site.fake.clock().now())
    });

    assert!(matches!(
        gather(&site, retained.id()),
        Err(DuplicateError::Retained)
    ));
}

#[test]
fn a_missing_invalid_or_changed_pinned_package_is_unavailable_with_the_start_reason() {
    let ev = evidence!(
        covers = ["REQ-025", "AC-025"],
        kind = Kind::Fixture,
        expected = "a pinned package that is missing, invalid, or has another digest than the instance records is supervisor.package_unavailable with the provider revision key and the three reasons load_package gives the start",
    );
    let site = Site::new();
    let source = add_source(&site, "fake-a");
    let key = source.provider_revision().to_string();
    let reason = |site: &Site| match gather_duplicate(&deps(site), source.id()) {
        Err(DuplicateError::PackageUnavailable { provider, reason }) => {
            assert_eq!(provider, key);
            reason
        }
        other => panic!("expected package_unavailable, got {other:?}"),
    };

    assert!(reason(&site).starts_with("the pinned package cannot be read: "));

    pin_package(&site, "fake", |dir| {
        std::fs::write(dir.join("provider.yaml"), "not: [a provider").expect("the file writes");
    });
    assert!(reason(&site).starts_with("the pinned package is invalid: "));

    pin_package(&site, "fake", |dir| {
        std::fs::write(dir.join("provider.yaml"), "not: [a provider").expect("the file writes");
    });
    pin_package(&site, "fake", change_digest);
    assert_eq!(
        reason(&site),
        "the pinned package has another digest than the instance records"
    );
    ev.pass();
}

#[test]
fn a_catalog_package_with_the_same_revision_label_and_another_digest_is_ambiguous() {
    let site = Site::new();
    pin_package(&site, "fake", |_| {});
    let source = add_source(&site, "fake-a");
    let other = Site::with_provider_edit("fake", change_digest);
    let key = source.provider_revision();

    let ambiguous = DuplicateDeps {
        catalog: &other.catalog,
        ..deps(&site)
    };

    match gather_duplicate(&ambiguous, source.id()) {
        Err(DuplicateError::ProviderAmbiguous { provider, revision }) => {
            assert_eq!(&provider, key.provider());
            assert_eq!(&revision, key.revision());
        }
        other => panic!("expected provider_ambiguous, got {other:?}"),
    }
}

#[test]
fn a_catalog_with_the_same_package_or_without_the_entry_is_no_problem() {
    let (site, source) = site_with_source();
    let same = deps(&site);
    assert!(gather_duplicate(&same, source.id()).is_ok());

    let catalog = CatalogState::NotConfigured;
    let without = DuplicateDeps {
        catalog: &catalog,
        ..deps(&site)
    };
    assert!(gather_duplicate(&without, source.id()).is_ok());
}

#[test]
fn a_package_without_multi_instance_cannot_be_duplicated() {
    let site = Site::with_provider_edit("fake", single_instance);
    pin_package(&site, "fake", single_instance);
    let source = add_source(&site, "fake-a");

    match gather(&site, source.id()) {
        Err(DuplicateError::CapabilityUnsupported { provider, existing }) => {
            assert_eq!(&provider, source.provider_revision().provider());
            assert_eq!(existing.as_str(), "fake-a");
        }
        other => panic!("expected capability_unsupported, got {other:?}"),
    }
}

#[test]
fn a_runtime_that_is_not_installed_is_refused() {
    let (site, _) = site_with_source();
    let key = site.package.key();

    // The runtime exists and is not installed.
    let broken = RuntimeBuilder::new()
        .software("fake")
        .version("0.2.0", "0.2")
        .build();
    assert_ne!(broken.state(), RuntimeState::Installed);
    let broken_id = commit_runtime(&site, broken);
    let stuck = stackly_testkit::InstanceBuilder::new(
        &site.registry.runtime(broken_id).unwrap().value.unwrap(),
        key,
    )
    .name("fake-stuck")
    .lifecycle(InstanceLifecycle::Stopped)
    .build();
    site.registry
        .commit(WriteBatch::new(vec![WriteOp::InsertInstance(
            stuck.clone(),
        )]))
        .expect("the instance commits");
    match gather(&site, stuck.id()) {
        Err(DuplicateError::RuntimeUnavailable { runtime_id, state }) => {
            assert_eq!(runtime_id, broken_id);
            assert_ne!(state, RuntimeState::Installed);
        }
        other => panic!("expected runtime_unavailable, got {other:?}"),
    }
}

#[test]
fn settings_that_are_not_a_json_object_are_a_corrupt_registry() {
    let (site, source) = site_with_source();
    let revision = stackly_domain::ConfigRevisionRecord::first(
        stackly_domain::ConfigRevisionId::generate(),
        source.id(),
        stackly_domain::ConfigValuesJson::new("{not json}".to_owned()).expect("braces"),
        None,
        site.fake.clock().now(),
    );
    site.registry
        .commit(WriteBatch::new(vec![WriteOp::InsertConfigRevision(
            revision,
        )]))
        .expect("the revision commits");

    match gather(&site, source.id()) {
        Err(DuplicateError::Registry(stackly_domain::RegistryError::Corrupt {
            table,
            column,
            ..
        })) => {
            assert_eq!((table, column), ("config_revisions", "values"));
        }
        other => panic!("expected a corrupt registry, got {other:?}"),
    }
}

#[test]
fn the_checks_run_in_the_order_of_the_spec() {
    let ev = evidence!(
        covers = ["REQ-025"],
        kind = Kind::Fixture,
        expected = "an instance with a bad package and a bad runtime reports the package first, and a package that is both ambiguous and single instance reports ambiguous first",
    );
    // A runtime that is not installed and a package that is missing: the
    // package comes first.
    let site = Site::new();
    let broken = RuntimeBuilder::new()
        .software("fake")
        .version("0.2.0", "0.2")
        .build();
    let broken_id = commit_runtime(&site, broken);
    let runtime = site.registry.runtime(broken_id).unwrap().value.unwrap();
    let both = stackly_testkit::InstanceBuilder::new(&runtime, site.package.key())
        .name("fake-both")
        .lifecycle(InstanceLifecycle::Stopped)
        .build();
    site.registry
        .commit(WriteBatch::new(vec![WriteOp::InsertInstance(both.clone())]))
        .expect("the instance commits");
    assert!(matches!(
        gather(&site, both.id()),
        Err(DuplicateError::PackageUnavailable { .. })
    ));

    // Ambiguous and single instance: ambiguous first.
    let single = Site::with_provider_edit("fake", single_instance);
    pin_package(&single, "fake", single_instance);
    let source = add_source(&single, "fake-a");
    let other = Site::with_provider_edit("fake", |dir| {
        single_instance(dir);
        change_digest(dir);
    });
    let ambiguous = DuplicateDeps {
        catalog: &other.catalog,
        ..deps(&single)
    };
    assert!(matches!(
        gather_duplicate(&ambiguous, source.id()),
        Err(DuplicateError::ProviderAmbiguous { .. })
    ));
    ev.pass();
}

#[test]
fn a_source_without_any_folder_gives_facts_and_leaves_no_trace() {
    let ev = evidence!(
        covers = ["REQ-009", "AC-025"],
        kind = Kind::Fixture,
        expected = "gather_duplicate succeeds for a source whose instances, log, and run folders do not exist, and changes nothing: no registry commit, no file system mutation, no Keychain call, no process spawn",
    );
    let (site, source) = site_with_source();
    set_config(
        &site,
        &source,
        &serde_json::json!({ "greeting": "hello from fake-a" }),
    );
    let reference = use_secret(&site, &source, "authToken");
    let paths = site.prepared.layout().instance_paths(source.id());
    for folder in [&paths.root, &paths.data, &paths.logs, &paths.run] {
        assert!(
            !folder.exists(),
            "the fixture source has no folder: {}",
            folder.display()
        );
    }
    let generation = site
        .registry
        .instances(stackly_domain::InstanceFilter {
            include_retained: true,
        })
        .unwrap()
        .generation;
    let mutations = site.fake.fs().mutations().len();

    let gathered = facts(&site, &source);

    assert_eq!(gathered.source.id(), source.id());
    assert_eq!(
        gathered.values,
        Some(serde_json::json!({ "greeting": "hello from fake-a" }))
    );
    assert_eq!(gathered.uses.len(), 1);
    assert_eq!(gathered.uses[0].reference, reference);
    assert_eq!(gathered.taken.len(), 1);
    assert_eq!(
        site.registry
            .instances(stackly_domain::InstanceFilter {
                include_retained: true
            })
            .unwrap()
            .generation,
        generation
    );
    assert_eq!(site.fake.fs().mutations().len(), mutations);
    assert!(site.fake.secrets().calls().is_empty());
    assert!(site.fake.processes().spawns().is_empty());
    ev.pass();
}

#[test]
fn a_source_without_an_active_config_revision_has_no_values() {
    let (site, source) = site_with_source();

    let gathered = facts(&site, &source);

    assert_eq!(gathered.values, None);
    assert!(gathered.uses.is_empty());
}
