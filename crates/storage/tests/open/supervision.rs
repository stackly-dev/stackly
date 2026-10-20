//! The feature 10 rows of the frozen version 3 fixture read back through
//! the registry (spec 0005 "Tambahan fitur 10"): its process observation,
//! written in the column text spec 0005 fixes, and the active config
//! revision the instance points to.

use std::path::PathBuf;

use stackly_domain::{ConfigRevisionId, ConfigRevisionStatus, InstanceId, Registry};

use crate::common::{Harness, ready};
use crate::fixture::fixture;

const INSTANCE: &str = "0199f000-0000-7000-8000-000000000030";

#[test]
fn the_v3_fixture_observation_and_active_config_read_back_through_the_registry() {
    let harness = Harness::new();
    harness.load_fixture(&fixture(3));
    let (registry, _) = ready(harness.open());
    let instance = INSTANCE.parse::<InstanceId>().unwrap();

    let observation = registry
        .latest_process_observation(instance)
        .unwrap()
        .value
        .expect("the fixture holds one observation");
    assert_eq!(observation.instance, instance);
    assert_eq!(observation.identity.pid.get(), 4242);
    assert_eq!(observation.identity.start.to_string(), "1700000000.000000");
    assert_eq!(
        observation.identity.executable,
        PathBuf::from(
            "/Users/example/Library/Application Support/Stackly/runtimes/postgresql/17.6/\
             macos-aarch64/cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc/bin/\
             postgres"
        )
    );
    assert_eq!(observation.identity.session.get(), 4242);
    assert_eq!(
        observation.identity.boot.to_string(),
        "6f1d2c3b-4a59-4e8f-9a0b-1c2d3e4f5a6b"
    );
    assert_eq!(
        observation.observed_at.to_string(),
        "2026-01-02T03:04:05.123456789Z"
    );

    let config = registry
        .active_config_revision(instance)
        .unwrap()
        .value
        .expect("the fixture instance points to a config revision");
    assert_eq!(
        config.id,
        "0199f000-0000-7000-8000-000000000040"
            .parse::<ConfigRevisionId>()
            .unwrap()
    );
    assert_eq!(config.instance, instance);
    assert_eq!(
        config.values.as_str(),
        r#"{"adminPassword":"local:instance/0199f000-0000-7000-8000-000000000030/admin-password","max_connections":100}"#
    );
    assert_eq!(
        config.rendered_sha256.map(|digest| digest.to_string()),
        Some("e".repeat(64))
    );
    assert_eq!(
        config.previous,
        Some(
            "0199f000-0000-7000-8000-000000000041"
                .parse::<ConfigRevisionId>()
                .unwrap()
        )
    );
    assert_eq!(config.status, ConfigRevisionStatus::Active);
    assert_eq!(
        config
            .created_by_operation
            .map(|id| id.to_string())
            .as_deref(),
        Some("0199f000-0000-7000-8000-000000000050")
    );
    assert_eq!(
        config.created_at.to_string(),
        "2026-01-02T03:04:05.123456789Z"
    );
    drop(registry);
}
