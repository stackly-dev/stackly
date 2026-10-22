//! The `create_instance` operation through the real daemon (spec 0020
//! AC-8, AC-10, AC-11, AC-12, AC-14, AC-16): the races, the compensation of
//! every failure, the hooks that prove a folder is never initialized twice,
//! and the row of the catalog source.

use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde_json::json;
use stackly_core::instance::create::Hook;
use stackly_domain::{InstanceId, ProviderSourceKind, Registry, SecretRef};
use stackly_protocol::Call;
use stackly_protocol::events::{EventKind, SubscribeParams};
use stackly_protocol::instance::{PortRequest, SecretFieldInput};
use stackly_protocol::json::DecodedFrame;
use stackly_protocol::operation::{CancelOperationParams, OperationState};
use stackly_protocol::secret::SecretInput;
use stackly_testkit::evidence::{Kind, evidence};

use crate::support::{Api, Site, fast_with_catalog, serving};

fn instance_id(uuid: uuid::Uuid) -> InstanceId {
    InstanceId::from_uuid(uuid).expect("a daemon instance ID is a version 7 UUID")
}

async fn stop(daemon: stacklyd::server::DaemonHandle) {
    daemon
        .shutdown(
            stackly_protocol::events::ShutdownReason::Requested,
            std::future::pending(),
        )
        .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_create_without_a_revision_pins_the_catalog_and_the_source_row_is_recorded_once() {
    let ev = evidence!(
        covers = ["REQ-008", "BR-016"],
        kind = Kind::Fixture,
        expected = "a create without provider.revision uses the revision of the loaded catalog, the instance keeps that exact revision, one provider_sources row exists for the catalog and a restart of the daemon finds the same row",
    );
    let site = Site::new();
    site.unpin_fake_package();
    let daemon = site.start_with(fast_with_catalog()).await;
    let api = site.api().await;

    let mut params = api.params("demo", json!({}));
    params.provider.revision = None;
    let accepted = api
        .create(params)
        .await
        .expect("the create is accepted")
        .accepted
        .expect("a create starts an operation");
    api.wait_state(accepted.operation.id, OperationState::Succeeded)
        .await;
    let id: uuid::Uuid = accepted
        .operation
        .resource
        .as_ref()
        .expect("the operation names the instance")
        .id
        .parse()
        .expect("an instance id");
    let created = api.get(id).await.expect("the instance can be read");
    assert_eq!(created.provider.revision, "0.3.0");
    drop(api);
    stop(daemon).await;

    let read = || {
        let stackly_storage::RegistryOpen::Ready { registry, .. } =
            stackly_storage::OpenOptions::new()
                .open(&site.platform, site.prepared.layout(), "0.1.0")
                .expect("the registry opens")
        else {
            panic!("the registry opens ready");
        };
        registry
            .provider_source_by_location(ProviderSourceKind::Development, "/stackly-test-catalog")
            .expect("the source can be read")
            .value
            .expect("the source row exists")
    };
    let first = read();
    assert!(first.last_sync_at.is_none());

    let daemon = site.start_with(fast_with_catalog()).await;
    stop(daemon).await;
    assert_eq!(read().id, first.id, "a restart finds the same row");
    ev.pass();
}

#[tokio::test(flavor = "multi_thread")]
async fn ten_creates_of_one_name_make_one_instance_and_the_rest_are_refused_at_once() {
    let ev = evidence!(
        covers = ["REQ-008", "BR-023"],
        kind = Kind::Fixture,
        expected = "ten concurrent creates with one name make exactly one instance, the others fail with resource.busy or instance.name_taken without an operation, and no folder, reference, or row of a loser is left",
    );
    let site = Site::new();
    site.unpin_fake_package();
    let daemon = site.start_with(fast_with_catalog()).await;
    let api = Arc::new(site.api().await);

    let mut tasks = Vec::new();
    for _ in 0..10 {
        let api = Arc::clone(&api);
        tasks.push(tokio::spawn(async move {
            api.create(api.params("same", json!({}))).await
        }));
    }
    let mut won = Vec::new();
    for task in tasks {
        match task.await.expect("a create task finishes") {
            Ok(reply) => won.push(reply.accepted.expect("a create starts an operation")),
            Err(error) => assert!(
                matches!(error.code(), "resource.busy" | "instance.name_taken"),
                "{error:?}"
            ),
        }
    }
    assert_eq!(won.len(), 1, "exactly one create won");
    api.wait_state(won[0].operation.id, OperationState::Succeeded)
        .await;
    assert_eq!(api.list(Default::default()).await.items.len(), 1);
    let folders = std::fs::read_dir(site.prepared.layout().instances_dir())
        .expect("the instances folder exists")
        .count();
    assert_eq!(folders, 1, "only the winner has a folder");
    let late = api.create(api.params("same", json!({}))).await.unwrap_err();
    assert_eq!(late.code(), "instance.name_taken", "{late:?}");
    drop(api);
    stop(daemon).await;
    ev.pass();
}

#[tokio::test(flavor = "multi_thread")]
async fn two_creates_with_the_same_fixed_port_make_one_instance() {
    let ev = evidence!(
        covers = ["REQ-010", "AC-056"],
        kind = Kind::Fixture,
        expected = "two concurrent creates of different names with one fixed port make exactly one instance, the loser fails with supervisor.endpoint_conflict, and the loser leaves no folder, reference, or row",
    );
    let site = Site::new();
    site.unpin_fake_package();
    let daemon = site.start_with(fast_with_catalog()).await;
    let api = Arc::new(site.api().await);
    // A free port for both drafts.
    let port = {
        let probe = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("a free port binds");
        probe.local_addr().expect("an address").port()
    };

    let mut tasks = Vec::new();
    for name in ["left", "right"] {
        let api = Arc::clone(&api);
        tasks.push(tokio::spawn(async move {
            let mut params = api.params(name, json!({}));
            params.endpoints = vec![Api::http_port(port)];
            match api.create(params).await {
                Ok(reply) => {
                    let accepted = reply.accepted.expect("a create starts an operation");
                    Ok(api.wait_final(accepted.operation.id).await)
                }
                Err(error) => Err(error),
            }
        }));
    }
    let mut winners = 0;
    for task in tasks {
        match task.await.expect("a create task finishes") {
            Ok(detail) if detail.operation.state == OperationState::Succeeded => winners += 1,
            Ok(detail) => assert_eq!(
                Api::failure_code(&detail),
                "supervisor.endpoint_conflict",
                "{detail:?}"
            ),
            Err(error) => assert_eq!(error.code(), "supervisor.endpoint_conflict", "{error:?}"),
        }
    }
    assert_eq!(winners, 1);
    assert_eq!(api.list(Default::default()).await.items.len(), 1);
    let folders = std::fs::read_dir(site.prepared.layout().instances_dir())
        .expect("the instances folder exists")
        .count();
    assert_eq!(folders, 1);
    drop(api);
    stop(daemon).await;
    ev.pass();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_cancel_during_init_waits_for_it_and_then_undoes_the_whole_create() {
    let ev = evidence!(
        covers = ["BR-023"],
        kind = Kind::Fixture,
        expected = "a cancel while init runs waits for init to return and ends cancelled with the instance row, the instance folder, and the secret reference removed and instance.removed sent",
    );
    let site = Site::new();
    site.unpin_fake_package();
    let daemon = site.start_with(fast_with_catalog()).await;
    let api = site.api().await;
    let (_, mut events) = api
        .client
        .subscribe(SubscribeParams::default())
        .await
        .expect("the subscription is accepted");

    let accepted = api
        .create(api.params("slow", json!({ "initDelayMs": 800 })))
        .await
        .expect("the create is accepted")
        .accepted
        .expect("a create starts an operation");
    let id: uuid::Uuid = accepted
        .operation
        .resource
        .as_ref()
        .expect("the operation names the instance")
        .id
        .parse()
        .expect("an instance id");
    // Cancel once the operation reached init.
    let started = Instant::now();
    loop {
        let detail = api.operation(accepted.operation.id).await;
        if detail.operation.stage.as_deref() == Some("init") {
            break;
        }
        assert!(started.elapsed() < crate::support::PATIENCE, "{detail:?}");
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    api.client
        .call(Call::OperationCancel(CancelOperationParams {
            operation_id: accepted.operation.id,
        }))
        .await
        .expect("the cancel is accepted");
    let detail = api
        .wait_state(accepted.operation.id, OperationState::Cancelled)
        .await;
    assert!(detail.operation.recovery.is_none(), "{detail:?}");

    assert_eq!(api.get(id).await.unwrap_err().code(), "resource.not_found");
    assert!(
        !site
            .prepared
            .layout()
            .instance_paths(instance_id(id))
            .root
            .exists()
    );
    let reference: SecretRef = format!("local:instance/{id}/auth-token")
        .parse()
        .expect("the reference of the token");
    assert!(!site.secrets.contains(&reference), "the reference is gone");

    let mut removed = false;
    while let Ok(Ok(Some(frame))) =
        tokio::time::timeout(std::time::Duration::from_millis(300), events.next()).await
    {
        if let DecodedFrame::Event(event) = frame
            && let EventKind::InstanceRemoved(gone) = event.kind
            && gone.instance_id == id
        {
            removed = true;
        }
    }
    assert!(removed, "instance.removed was sent for the instance");
    drop((api, events));
    stop(daemon).await;
    ev.pass();
}

#[tokio::test(flavor = "multi_thread")]
async fn data_that_appears_after_the_layout_stops_the_create_before_init_and_is_never_touched() {
    let ev = evidence!(
        covers = ["AC-008", "BR-008"],
        kind = Kind::Fixture,
        expected = "a file that appears in the new data folder between the layout and the content check fails the create with instance.data_not_empty before init, nothing is spawned, the file stays as it was, and the secret reference of the operation is removed",
    );
    let site = Site::new();
    site.unpin_fake_package();
    let planted: Arc<Mutex<Option<std::path::PathBuf>>> = Arc::new(Mutex::new(None));
    let hook: Hook = {
        let planted = Arc::clone(&planted);
        Arc::new(move |paths| {
            std::fs::write(paths.data.join("keep.txt"), b"mine").expect("the file is planted");
            *planted.lock().expect("the lock is not poisoned") = Some(paths.data.clone());
        })
    };
    let mut config = fast_with_catalog();
    config.create_hooks.after_layout = Some(hook);
    let daemon = site.start_with(config).await;
    let api = site.api().await;

    let accepted = api
        .create(api.params("planted", json!({})))
        .await
        .expect("the create is accepted")
        .accepted
        .expect("a create starts an operation");
    let detail = api
        .wait_state(accepted.operation.id, OperationState::Failed)
        .await;
    assert_eq!(Api::failure_code(&detail), "instance.data_not_empty");
    let data = planted
        .lock()
        .expect("the lock is not poisoned")
        .clone()
        .expect("the hook ran");
    assert_eq!(
        std::fs::read(data.join("keep.txt")).expect("the file is still there"),
        b"mine",
        "the folder the create did not leave empty is not deleted"
    );
    assert!(serving(&data).is_empty(), "nothing was spawned");
    let id: uuid::Uuid = accepted
        .operation
        .resource
        .as_ref()
        .expect("the operation names the instance")
        .id
        .parse()
        .expect("an instance id");
    let reference: SecretRef = format!("local:instance/{id}/auth-token")
        .parse()
        .expect("the reference of the token");
    assert!(!site.secrets.contains(&reference), "the reference is gone");
    assert!(api.list(Default::default()).await.items.is_empty());
    drop(api);
    stop(daemon).await;
    ev.pass();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_locked_store_fails_a_generated_secret_before_any_folder_exists() {
    let ev = evidence!(
        covers = ["REQ-016", "AC-008"],
        kind = Kind::Fixture,
        expected = "a create whose secret the locked store cannot write fails with secret.store_locked at the secrets step, before any instance folder or row exists",
    );
    let site = Site::new();
    site.unpin_fake_package();
    let daemon = site.start_with(fast_with_catalog()).await;
    let api = site.api().await;
    site.secrets.lock();

    let mut params = api.params("locked", json!({}));
    params.secrets = vec![SecretFieldInput {
        field: "authToken".to_owned(),
        input: SecretInput::Generate,
    }];
    let accepted = api
        .create(params)
        .await
        .expect("a locked store is judged only when it is written")
        .accepted
        .expect("a create starts an operation");
    let detail = api
        .wait_state(accepted.operation.id, OperationState::Failed)
        .await;
    assert_eq!(Api::failure_code(&detail), "secret.store_locked");
    let folders = std::fs::read_dir(site.prepared.layout().instances_dir())
        .map(|entries| entries.count())
        .unwrap_or(0);
    assert_eq!(folders, 0);
    assert!(api.list(Default::default()).await.items.is_empty());
    site.secrets.unlock();
    drop(api);
    stop(daemon).await;
    ev.pass();
}

#[tokio::test(flavor = "multi_thread")]
async fn an_auto_port_is_final_after_the_start() {
    let ev = evidence!(
        covers = ["REQ-010", "AC-055"],
        kind = Kind::Fixture,
        expected = "an instance created with an Auto endpoint reads bound false until it starts and bound true with its port after readiness",
    );
    let site = Site::new();
    site.unpin_fake_package();
    let daemon = site.start_with(fast_with_catalog()).await;
    let api = site.api().await;

    let mut auto = api.params("auto", json!({}));
    auto.endpoints = vec![stackly_protocol::instance::EndpointRequest {
        name: "http".to_owned(),
        port: PortRequest::Auto,
    }];
    let accepted = api
        .create(auto)
        .await
        .expect("the create is accepted")
        .accepted
        .expect("a create starts an operation");
    api.wait_state(accepted.operation.id, OperationState::Succeeded)
        .await;
    let id: uuid::Uuid = accepted
        .operation
        .resource
        .as_ref()
        .expect("the operation names the instance")
        .id
        .parse()
        .expect("an instance id");
    let before = api.get(id).await.expect("the instance can be read");
    assert!(!before.endpoints[0].bound);
    let running = api.start_ready(id).await;
    assert!(running.endpoints[0].bound, "{running:?}");
    assert!(running.endpoints[0].port.is_some());
    drop(api);
    stop(daemon).await;
    ev.pass();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_symlink_in_the_providers_folder_or_for_instances_is_refused_and_nothing_is_overwritten()
{
    let ev = evidence!(
        covers = ["NFR-009", "VAL-007"],
        kind = Kind::Fixture,
        expected = "a symlink where the provider folder of the catalog package belongs fails the pin with catalog.pin_failed and leaves its target alone, and a symlink in place of the instances folder fails the data row of the preflight with supervisor.data_unavailable",
    );
    let site = Site::new();
    site.unpin_fake_package();
    // A symlink where `providers/testing/fake` would be written.
    let outside = stackly_testkit::TestRoot::new().expect("a folder outside the data root");
    let providers = site.prepared.layout().providers_dir();
    std::fs::create_dir_all(providers.join("testing")).expect("the namespace folder exists");
    std::os::unix::fs::symlink(outside.path(), providers.join("testing/fake"))
        .expect("the symlink is made");
    let daemon = site.start_with(fast_with_catalog()).await;
    let api = site.api().await;

    let mut params = api.params("linked", json!({}));
    params.provider.revision = None;
    let accepted = api
        .create(params)
        .await
        .expect("the create is accepted")
        .accepted
        .expect("a create starts an operation");
    let detail = api
        .wait_state(accepted.operation.id, OperationState::Failed)
        .await;
    assert_eq!(Api::failure_code(&detail), "catalog.pin_failed");
    assert_eq!(
        std::fs::read_dir(outside.path())
            .expect("the target is still a folder")
            .count(),
        0,
        "nothing was written through the symlink"
    );
    assert!(api.list(Default::default()).await.items.is_empty());

    // The instances folder replaced by a symlink is not the private folder
    // of the user the data row asks for.
    let instances = site.prepared.layout().instances_dir();
    std::fs::remove_dir_all(&instances).expect("the instances folder is removed");
    std::os::unix::fs::symlink(outside.path(), &instances).expect("the symlink is made");
    let preflight = api
        .preflight(api.preflight_params("viasymlink", json!({})))
        .await
        .expect("a preflight is answered");
    assert!(!preflight.ready);
    let row = preflight
        .checks
        .iter()
        .find(|check| check.id == "data")
        .expect("the data row is there");
    assert_eq!(
        row.error.as_ref().map(|error| error.code()),
        Some("supervisor.data_unavailable"),
        "{row:?}"
    );
    drop(api);
    stop(daemon).await;
    ev.pass();
}

/// Spec 0020 AC-24: with ten instances registered, a create is answered in
/// 500 ms and a preflight in 300 ms at the 95th percentile. Only with
/// `STACKLY_MEASURE=1`, because the numbers belong to the evidence run:
/// `STACKLY_MEASURE=1 cargo test -p stacklyd --test supervisor measure_create -- --nocapture`.
#[tokio::test(flavor = "multi_thread")]
async fn measure_create_and_preflight_with_ten_instances() {
    if std::env::var_os("STACKLY_MEASURE").is_none() {
        eprintln!("skipped: set STACKLY_MEASURE=1 to time create and preflight");
        return;
    }
    let site = Site::new();
    site.unpin_fake_package();
    let daemon = site.start_with(fast_with_catalog()).await;
    let api = site.api().await;
    for n in 0..10 {
        api.create_ready(&format!("seed-{n}"), json!({})).await;
    }
    let mut preflights = Vec::new();
    for n in 0..40 {
        let started = Instant::now();
        api.preflight(api.preflight_params(&format!("probe-{n}"), json!({})))
            .await
            .expect("a preflight is answered");
        preflights.push(started.elapsed());
    }
    let mut creates = Vec::new();
    for n in 0..20 {
        let started = Instant::now();
        let accepted = api
            .create(api.params(&format!("timed-{n}"), json!({})))
            .await
            .expect("the create is accepted")
            .accepted
            .expect("a create starts an operation");
        creates.push(started.elapsed());
        api.wait_state(accepted.operation.id, OperationState::Succeeded)
            .await;
    }
    let p95 = |mut times: Vec<std::time::Duration>| {
        times.sort();
        times[times.len() * 95 / 100]
    };
    let preflight = p95(preflights);
    let create = p95(creates);
    eprintln!("preflight p95 {preflight:?}, create p95 {create:?}");
    assert!(
        preflight < std::time::Duration::from_millis(300),
        "{preflight:?}"
    );
    assert!(create < std::time::Duration::from_millis(500), "{create:?}");
    drop(api);
    stop(daemon).await;
}
