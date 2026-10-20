//! The real `stacklyd` binary and its catalog (spec 0017 AC-2, AC-4,
//! AC-13): `STACKLY_CATALOG_DIR` read once at start, a bad value stopping
//! the daemon with exit 1, no catalog without it, and a built catalog
//! folder served as `development`.
#![cfg(target_os = "macos")]

mod common;

use std::ffi::OsString;
use std::path::Path;
use std::time::Duration;

use common::{
    STACKLYD, client, daemon_paths, field, json_lines, runtime, start_daemon, start_daemon_with,
};
use stackly_core::catalog::testing::{CatalogBuilder, CatalogFixture};
use stackly_core::provider::read_package;
use stackly_platform_macos::MacosPlatform;
use stackly_protocol::catalog::{CatalogEntryState, CatalogSourceKind, TrustLevel};
use stackly_protocol::codes::catalog;
use stackly_protocol::daemon::{DaemonHealth, DaemonHealthStatus, SubsystemStatus, subsystem};
use stackly_protocol::{Call, Reply};
use stackly_testkit::evidence::{Kind, evidence};
use stackly_testkit::{SpawnCatalog, SpawnedDaemon, TestRoot};

fn health(socket: &Path) -> DaemonHealth {
    let reply = runtime().block_on(async {
        let (client, _) = client(socket).await;
        client
            .call(Call::DaemonGetHealth)
            .await
            .expect("get_health is answered")
    });
    let Reply::DaemonGetHealth(health) = reply else {
        panic!("not a health reply");
    };
    health
}

fn catalog_status(health: &DaemonHealth) -> SubsystemStatus {
    health
        .subsystems
        .iter()
        .find(|subsystem| subsystem.name == subsystem::CATALOG)
        .map(|subsystem| subsystem.status)
        .expect("health lists the catalog subsystem")
}

fn call(socket: &Path, call: Call) -> Result<Reply, Box<stackly_protocol::ErrorBody>> {
    runtime().block_on(async {
        let (client, _) = client(socket).await;
        client.call(call).await
    })
}

#[test]
fn a_catalog_dir_that_is_not_an_absolute_folder_stops_the_daemon_with_exit_1() {
    let root = TestRoot::new().unwrap();
    let paths = daemon_paths(&root);
    let file = root.path().join("catalog.json");
    std::fs::write(&file, "{}").unwrap();
    for value in [
        OsString::new(),
        OsString::from("relative/catalog"),
        root.path().join("missing").into_os_string(),
        file.into_os_string(),
    ] {
        let daemon = SpawnedDaemon::spawn_with_catalog(
            Path::new(STACKLYD),
            root.path(),
            &paths.socket,
            &paths.runtime,
            &[],
            SpawnCatalog::Raw(value.clone()),
        )
        .unwrap();
        let stderr_path = daemon.stderr_path().to_path_buf();

        let status = daemon.wait_exit(Duration::from_secs(10)).unwrap();

        assert_eq!(status.code(), Some(1), "{value:?}");
        let stderr = std::fs::read_to_string(stderr_path).unwrap();
        let lines = json_lines(&stderr);
        let expected = format!(
            "STACKLY_CATALOG_DIR {:?} is not an absolute path to a folder",
            value.to_string_lossy()
        );
        assert!(
            lines
                .iter()
                .any(|line| field(line, "error").as_str() == Some(expected.as_str())),
            "{stderr}"
        );
    }
}

#[test]
fn without_a_catalog_dir_the_daemon_starts_and_answers_catalog_unavailable() {
    let ev = evidence!(
        covers = ["REQ-033"],
        kind = Kind::Fixture,
        expected = "stacklyd without STACKLY_CATALOG_DIR or an app bundle serves, reports the catalog subsystem unavailable, and answers catalog.list_providers with catalog.unavailable not_configured and its hint",
    );
    let root = TestRoot::new().unwrap();
    let daemon = start_daemon_with(&root, SpawnCatalog::Unset);

    let error = call(daemon.socket(), Call::CatalogListProviders).unwrap_err();

    assert_eq!(error.code(), catalog::UNAVAILABLE.code);
    assert_eq!(error.detail("reason"), Some("not_configured"));
    assert_eq!(
        error.hint(),
        Some("run stacklyd from Stackly.app or set STACKLY_CATALOG_DIR")
    );
    let health = health(daemon.socket());
    assert_eq!(catalog_status(&health), SubsystemStatus::Unavailable);
    assert_eq!(health.status, DaemonHealthStatus::Degraded);
    assert_eq!(daemon.stop().unwrap().code(), Some(0));
    ev.pass();
}

#[test]
fn the_default_spawned_daemon_has_an_ok_empty_catalog() {
    let root = TestRoot::new().unwrap();
    let daemon = start_daemon(&root);

    let health = health(daemon.socket());

    assert_eq!(catalog_status(&health), SubsystemStatus::Ok);
    assert_eq!(health.status, DaemonHealthStatus::Ready);
    let Ok(Reply::CatalogListProviders(reply)) = call(daemon.socket(), Call::CatalogListProviders)
    else {
        panic!("not a list reply");
    };
    assert!(reply.items.is_empty());
    assert_eq!(daemon.stop().unwrap().code(), Some(0));
}

#[test]
fn a_built_catalog_folder_is_served_as_a_development_catalog() {
    let root = TestRoot::new().unwrap();
    let platform = MacosPlatform::with_roots(root.roots());
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let nats =
        read_package(platform.fs(), &repo.join("providers/nats")).expect("the NATS package reads");
    let dir = root.path().join("catalog");
    CatalogBuilder::new()
        .package("official/nats", nats)
        .fixture(CatalogFixture::IntelOnly)
        .planned(
            "official/valkey",
            "Valkey",
            "cache",
            "In-memory key-value store.",
        )
        .write(platform.fs(), &dir)
        .expect("the test catalog is written");
    let daemon = start_daemon_with(&root, SpawnCatalog::Dir(dir));

    let Ok(Reply::CatalogListProviders(reply)) = call(daemon.socket(), Call::CatalogListProviders)
    else {
        panic!("not a list reply");
    };

    assert_eq!(reply.catalog.source, CatalogSourceKind::Development);
    assert_eq!(reply.catalog.trust, TrustLevel::Development);
    let states: Vec<_> = reply
        .items
        .iter()
        .map(|item| (item.provider.as_str(), item.state))
        .collect();
    assert_eq!(
        states,
        [
            ("official/intelonly", CatalogEntryState::Unsupported),
            ("official/nats", CatalogEntryState::Available),
            ("official/valkey", CatalogEntryState::Planned),
        ]
    );
    let Ok(Reply::CatalogGetProvider(detail)) = call(
        daemon.socket(),
        Call::CatalogGetProvider(stackly_protocol::catalog::GetProviderParams {
            provider: "official/nats".to_owned(),
        }),
    ) else {
        panic!("not a detail reply");
    };
    assert_eq!(
        detail.versions[0].artifacts[0].source.as_deref(),
        Some("github.com/nats-io")
    );
    assert_eq!(
        catalog_status(&health(daemon.socket())),
        SubsystemStatus::Ok
    );
    assert_eq!(daemon.stop().unwrap().code(), Some(0));
}
