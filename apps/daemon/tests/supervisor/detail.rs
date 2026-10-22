//! `instance.get_detail` through the real daemon (spec 0021 AC-4, AC-8,
//! AC-18, AC-19): the facts of the registry and the pinned package, the
//! connection of an instance with a fixed and an `auto` port, a package
//! that is broken and repaired, and the answer time.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::json;
use stackly_protocol::instance::connection::ConnectionPartKind;
use stackly_protocol::instance::{
    ConnectionAvailability, ConnectionUnavailableReason, EndpointRequest, InstanceLifecycle,
    InstanceReadinessState, PortRequest, StartupPolicyKind,
};
use stackly_protocol::secret::SecretStatus;
use stackly_testkit::evidence::{Kind, evidence};
use stackly_testkit::{SecretOp, allocate_port};
use uuid::Uuid;

use crate::support::{Api, Site, TOKEN};

/// Creates `name` with a fixed `http` port and returns its ID and port.
async fn create_fixed(api: &Api, name: &str, extra: serde_json::Value) -> (Uuid, u16) {
    let port = allocate_port().expect("a free port");
    let mut params = api.params(name, extra);
    params.endpoints = vec![Api::http_port(port)];
    let accepted = api
        .create(params)
        .await
        .expect("the create is accepted")
        .accepted
        .expect("a create starts an operation");
    let id = accepted
        .operation
        .resource
        .as_ref()
        .map(|resource| resource.id.parse::<Uuid>().expect("an instance id"))
        .expect("the operation names the instance");
    api.wait_state(
        accepted.operation.id,
        stackly_protocol::operation::OperationState::Succeeded,
    )
    .await;
    (id, port)
}

/// Creates `name` with an `auto` `http` port.
async fn create_auto(api: &Api, name: &str) -> Uuid {
    let mut params = api.params(name, json!({}));
    params.endpoints = vec![EndpointRequest {
        name: "http".to_owned(),
        port: PortRequest::Auto,
    }];
    let accepted = api
        .create(params)
        .await
        .expect("the create is accepted")
        .accepted
        .expect("a create starts an operation");
    let id = accepted
        .operation
        .resource
        .as_ref()
        .map(|resource| resource.id.parse::<Uuid>().expect("an instance id"))
        .expect("the operation names the instance");
    api.wait_state(
        accepted.operation.id,
        stackly_protocol::operation::OperationState::Succeeded,
    )
    .await;
    id
}

/// The `provider.yaml` of the pinned fake package.
fn pinned_manifest(site: &Site) -> PathBuf {
    fn find(dir: &Path) -> Option<PathBuf> {
        for entry in std::fs::read_dir(dir).ok()? {
            let path = entry.ok()?.path();
            if path.is_dir() {
                if let Some(found) = find(&path) {
                    return Some(found);
                }
            } else if path.file_name().is_some_and(|name| name == "provider.yaml") {
                return Some(path);
            }
        }
        None
    }
    find(&site.prepared.layout().providers_dir()).expect("the fake package is pinned")
}

// covers: AC-4
#[tokio::test(flavor = "multi_thread")]
async fn the_detail_of_an_instance_comes_from_the_registry_and_the_pinned_package() {
    let site = Site::new();
    let _daemon = site.start().await;
    let api = site.api().await;
    let (id, port) = create_fixed(&api, "orders", json!({})).await;
    let reads = site.secrets.count(SecretOp::Read);

    let detail = api.detail(id).await.expect("the detail is answered");

    let summary = api.get(id).await.expect("the instance");
    assert_eq!(detail.instance_id, id);
    assert_eq!(detail.revision, summary.revision);
    assert_eq!(detail.provider.name, "testing/fake");
    assert_eq!(detail.provider.revision, "0.3.0");
    assert_eq!(
        detail.provider.manifest_sha256,
        summary.provider.manifest_sha256
    );
    assert_eq!(
        detail.provider.display_name.as_deref(),
        Some("Fake Provider")
    );
    assert!(detail.provider.package_readable);
    assert_eq!(detail.runtime.id, api.runtime_id);
    assert_eq!(
        detail.data_path,
        site.instance_paths(id).data.to_string_lossy()
    );
    assert!(Path::new(&detail.data_path).is_absolute());
    assert_eq!(detail.startup, StartupPolicyKind::Manual);
    assert!(detail.dependents.is_empty());
    assert_eq!(detail.secrets.len(), 1);
    assert_eq!(detail.secrets[0].field, "authToken");
    assert_eq!(detail.secrets[0].title, "Auth token");
    assert!(
        matches!(
            detail.secrets[0].status,
            SecretStatus::Present | SecretStatus::Unknown
        ),
        "{:?}",
        detail.secrets[0].status
    );
    assert_eq!(detail.connections.len(), 1);
    let uri = &detail.connections[0];
    assert!(uri.is_consistent());
    assert_eq!(
        (uri.name.as_str(), uri.title.as_str()),
        ("uri", "Connection string")
    );
    assert_eq!(uri.state, ConnectionAvailability::Available);
    assert_eq!(
        uri.masked.as_deref(),
        Some(format!("http://stackly:****@127.0.0.1:{port}/").as_str())
    );
    assert!(!uri.port_may_change, "a fixed port never changes");
    let kinds: Vec<_> = uri.parts.iter().map(|part| part.kind).collect();
    assert_eq!(
        kinds,
        [
            ConnectionPartKind::Text,
            ConnectionPartKind::Secret,
            ConnectionPartKind::Text
        ]
    );
    let secret = uri.parts[1].secret.as_ref().expect("the secret part");
    assert_eq!(secret.field, "authToken");
    assert_eq!(secret.reference, detail.secrets[0].reference);
    assert_eq!(
        site.secrets.count(SecretOp::Read),
        reads,
        "the detail never reads a secret value"
    );
}

// covers: AC-4
#[tokio::test(flavor = "multi_thread")]
async fn an_instance_that_does_not_exist_is_not_found() {
    let site = Site::new();
    let _daemon = site.start().await;
    let api = site.api().await;

    let error = api
        .detail(Uuid::now_v7())
        .await
        .expect_err("no such instance");

    assert_eq!(error.code(), "resource.not_found");
}

// covers: AC-4
#[tokio::test(flavor = "multi_thread")]
async fn a_broken_pinned_package_is_unreadable_and_a_repaired_one_is_read_again() {
    let site = Site::new();
    let _daemon = site.start().await;
    let api = site.api().await;
    let (id, _) = create_fixed(&api, "orders", json!({})).await;
    let manifest = pinned_manifest(&site);
    let good = std::fs::read(&manifest).expect("the manifest is readable");
    assert!(
        api.detail(id)
            .await
            .expect("detail")
            .provider
            .package_readable
    );

    // The same size, so only the content tells the file changed.
    let mut damaged = good.clone();
    damaged[0] = b'#';
    damaged[1] = b'#';
    std::fs::write(&manifest, &damaged).expect("the manifest is damaged");
    let broken = api.detail(id).await.expect("a broken package is no error");
    assert!(!broken.provider.package_readable);
    assert_eq!(broken.provider.display_name, None);
    assert!(broken.connections.is_empty());
    assert_eq!(broken.secrets[0].title, "authToken");
    assert_eq!(
        broken.provider.name, "testing/fake",
        "the key is the registry's"
    );

    std::fs::write(&manifest, &good).expect("the manifest is repaired");
    let repaired = api.detail(id).await.expect("detail");
    assert!(repaired.provider.package_readable);
    assert_eq!(
        repaired.provider.display_name.as_deref(),
        Some("Fake Provider")
    );
    assert_eq!(repaired.connections.len(), 1);

    std::fs::remove_dir_all(manifest.parent().expect("the package folder"))
        .expect("the package folder goes");
    let gone = api.detail(id).await.expect("a missing package is no error");
    assert!(!gone.provider.package_readable);
}

// covers: AC-4
#[tokio::test(flavor = "multi_thread")]
async fn a_package_whose_content_is_another_revision_does_not_count_as_the_pinned_one() {
    let site = Site::new();
    let _daemon = site.start().await;
    let api = site.api().await;
    let (id, _) = create_fixed(&api, "orders", json!({})).await;
    let manifest = pinned_manifest(&site);
    let good = std::fs::read_to_string(&manifest).expect("the manifest is readable");

    // A valid manifest with another description has another digest.
    std::fs::write(
        &manifest,
        good.replace("Dummy server for Stackly tests", "Another server"),
    )
    .expect("the manifest changes");
    let detail = api.detail(id).await.expect("detail");

    assert!(!detail.provider.package_readable);
    assert!(detail.connections.is_empty());
}

// covers: AC-2, AC-4, AC-9
#[tokio::test(flavor = "multi_thread")]
async fn an_auto_port_has_no_connection_until_the_instance_runs() {
    let site = Site::new();
    let _daemon = site.start().await;
    let api = site.api().await;
    let id = create_auto(&api, "auto").await;

    let before = api.detail(id).await.expect("detail");

    let connection = &before.connections[0];
    assert!(connection.is_consistent());
    assert_eq!(connection.state, ConnectionAvailability::Unavailable);
    assert_eq!(
        connection.reason,
        Some(ConnectionUnavailableReason::PortNotAssigned)
    );
    assert!(connection.port_may_change);

    let running = api.start_ready(id).await;
    let port = running.endpoints[0].port.expect("the bound port");
    let after = api.detail(id).await.expect("detail");

    let connection = &after.connections[0];
    assert_eq!(connection.state, ConnectionAvailability::Available);
    assert_eq!(
        connection.masked.as_deref(),
        Some(format!("http://stackly:****@127.0.0.1:{port}/").as_str())
    );
    assert!(
        !connection.port_may_change,
        "a running instance keeps its port"
    );
    assert!(after.revision >= before.revision);
}

// covers: AC-8
#[tokio::test(flavor = "multi_thread")]
async fn a_running_instance_whose_probe_has_not_succeeded_is_never_ready() {
    let ev = evidence!(
        covers = ["AC-012", "REQ-012", "BR-010"],
        kind = Kind::Fixture,
        expected = "an instance running with readyDelayMs set shows lifecycle running and readiness not_ready while instance.get_detail answers, and ready only once the probe succeeds",
    );
    let site = Site::new();
    let _daemon = site.start().await;
    let api = site.api().await;
    let (id, _) = create_fixed(&api, "slow", json!({ "readyDelayMs": 1500 })).await;

    api.start(id).await.expect("the start is accepted");
    let waiting = api
        .wait_instance(id, "running and not ready", |summary| {
            summary.lifecycle == InstanceLifecycle::Running
                && summary.process.is_some()
                && summary.readiness.state != InstanceReadinessState::Ready
        })
        .await;
    let detail = api
        .detail(id)
        .await
        .expect("detail while waiting for the probe");
    assert_ne!(waiting.readiness.state, InstanceReadinessState::Ready);
    assert_eq!(detail.instance_id, id);

    let ready = api
        .wait_instance(id, "ready", |summary| {
            summary.readiness.state == InstanceReadinessState::Ready
        })
        .await;
    assert_eq!(ready.lifecycle, InstanceLifecycle::Running);
    ev.pass();
}

// covers: AC-18
#[tokio::test(flavor = "multi_thread")]
async fn no_part_of_the_detail_carries_the_secret_value() {
    let ev = evidence!(
        covers = ["AC-016", "REQ-016"],
        kind = Kind::Fixture,
        expected = "the encoded detail of a running instance holds the secret reference but never the secret value or a prefix of it",
    );
    let site = Site::new();
    let _daemon = site.start().await;
    let api = site.api().await;
    let (id, _) = create_fixed(&api, "orders", json!({})).await;
    api.start_ready(id).await;

    let detail = api.detail(id).await.expect("detail");

    let text = serde_json::to_string(&detail).expect("the detail encodes");
    assert!(text.contains("authToken"));
    assert!(
        !text.contains(&TOKEN[..12]),
        "a prefix of the value is in the detail"
    );
    assert!(!text.contains(TOKEN));
    ev.pass();
}

// covers: AC-19
#[tokio::test(flavor = "multi_thread")]
async fn ten_instances_are_detailed_within_the_time_bounds() {
    let site = Site::new();
    let _daemon = site.start().await;
    let api = site.api().await;
    let mut ids = Vec::new();
    for n in 0..10 {
        ids.push(create_fixed(&api, &format!("db-{n}"), json!({})).await.0);
    }

    let cold = Instant::now();
    api.detail(ids[0]).await.expect("detail");
    let cold = cold.elapsed();
    let mut warm = Vec::new();
    for _ in 0..10 {
        for id in &ids {
            let started = Instant::now();
            api.detail(*id).await.expect("detail");
            warm.push(started.elapsed());
        }
    }
    warm.sort();
    let p95 = warm[warm.len() * 95 / 100 - 1];

    assert!(cold < Duration::from_millis(300), "cold: {cold:?}");
    assert!(p95 < Duration::from_millis(100), "warm p95: {p95:?}");
}
