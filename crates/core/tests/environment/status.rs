//! The status of a registered environment and its manifest state: AC-14.

use stackly_core::environment::{
    EnvironmentStatus, ManifestRead, ManifestState, NotReadyReason, environment_view,
    manifest_digest, manifest_state,
};
use stackly_domain::{DeclaredService, InstanceLifecycle, Ownership, ReadinessState};
use stackly_testkit::EnvironmentBuilder;

use crate::common::{catalog, fixture};
use crate::site::Site;

fn declared(alias: &str, instance_ref: Option<&str>) -> DeclaredService {
    DeclaredService::new(
        alias.parse().expect("an alias"),
        "testing/fake".parse().expect("a provider"),
        if instance_ref.is_some() {
            Ownership::Shared
        } else {
            Ownership::Dedicated
        },
        None,
        instance_ref.map(|name| name.parse().expect("a name")),
        None,
        Vec::new(),
    )
}

// covers: AC-14 (spec 0044)
#[test]
fn the_status_counts_ready_services_from_the_snapshot_and_instances() {
    let mut site = Site::new();
    let api = site.instance("shop-api", 18080, InstanceLifecycle::Running);
    site.instance("shared-fake", 18081, InstanceLifecycle::Stopped);
    let gone = site.instance("shop-gone", 18082, InstanceLifecycle::Stopped);
    let env = EnvironmentBuilder::new()
        .name("shop")
        .declared(declared("api", None))
        .declared(declared("cache", Some("shared-fake")))
        .declared(declared("web", None))
        .member("api", api.id(), Ownership::Dedicated)
        .member("gone", gone.id(), Ownership::Dedicated)
        .build();

    let view = environment_view(&env, &site.views);
    assert_eq!(view.status, EnvironmentStatus::PartiallyReady);
    assert_eq!((view.ready_count, view.service_count), (1, 3));
    let reasons: Vec<Option<NotReadyReason>> = view
        .services
        .iter()
        .map(|s| s.not_ready.as_ref().map(|r| r.0))
        .collect();
    assert_eq!(
        reasons,
        [
            None,
            Some(NotReadyReason::Stopped),
            Some(NotReadyReason::NotCreated)
        ]
    );
    assert_eq!(view.detached.len(), 1);

    site.views[0].readiness = ReadinessState::NotReady;
    assert_eq!(
        environment_view(&env, &site.views).status,
        EnvironmentStatus::Down
    );
    let empty = EnvironmentBuilder::new().build();
    assert_eq!(
        environment_view(&empty, &[]).status,
        EnvironmentStatus::Down
    );
}

// covers: AC-14 (spec 0044)
#[test]
fn the_manifest_state_takes_the_first_rule_that_matches() {
    let bytes = fixture("graph.yaml");
    let catalog = catalog();
    let synced = EnvironmentBuilder::new()
        .name("graph")
        .manifest_sha256(manifest_digest(&bytes))
        .build();
    let state = |env, read: &ManifestRead| manifest_state(env, read, Some(&catalog)).0;
    assert_eq!(
        state(&synced, &ManifestRead::Missing),
        ManifestState::Missing
    );
    assert_eq!(
        state(&synced, &ManifestRead::Unreadable),
        ManifestState::Unreadable
    );
    assert_eq!(
        state(&synced, &ManifestRead::Bytes(bytes.clone())),
        ManifestState::InSync
    );
    let renamed = EnvironmentBuilder::new()
        .name("other")
        .manifest_sha256(manifest_digest(&bytes))
        .build();
    assert_eq!(
        state(&renamed, &ManifestRead::Bytes(bytes.clone())),
        ManifestState::NameChanged
    );
    let changed = EnvironmentBuilder::new().name("graph").build();
    assert_eq!(
        state(&changed, &ManifestRead::Bytes(bytes)),
        ManifestState::Changed
    );
    let (invalid, problems) = manifest_state(
        &synced,
        &ManifestRead::Bytes(fixture("invalid/dependency_cycle.yaml")),
        Some(&catalog),
    );
    assert_eq!(invalid, ManifestState::Invalid);
    assert!(problems > 0);
}
