//! `runtime.install` through the daemon (spec 0018 AC-1, AC-2, AC-3, AC-6,
//! AC-13, AC-14): the operation end to end on a real registry, the reply
//! when the artifact is installed already or a repeated key names the
//! operation, the refusal for a runtime in another state, and a failure
//! that leaves no runtime and carries its hint.

use stackly_domain::{Registry, RuntimeState, WriteBatch, WriteOp};
use stackly_protocol::codes::runtime as runtime_codes;
use stackly_protocol::events::{EventKind, SubscribeParams};
use stackly_protocol::json::DecodedFrame;
use stackly_protocol::operation::{ListOperationsParams, OperationState};
use stackly_protocol::runtime::{
    GetRuntimeParams, InstallOutcome, InstallRuntimeParams, ListRuntimesParams,
    RuntimeState as WireState, SigningPolicy,
};
use stackly_protocol::{Call, Reply, ResourceKind};
use stackly_testkit::RuntimeBuilder;
use stackly_testkit::evidence::{Kind, evidence};

use crate::support::{
    PATIENCE, Site, URL, archive, call_with_key, catalog, client, digest, nats, package, wait_state,
};

async fn operation_count(client: &stackly_ipc::Client) -> usize {
    let Reply::OperationList(page) = client
        .call(Call::OperationList(ListOperationsParams::default()))
        .await
        .expect("operations can be listed")
    else {
        panic!("expected an operation.list reply");
    };
    page.items.len()
}

// covers: AC-1, AC-13
#[tokio::test]
async fn an_install_runs_five_stages_and_leaves_one_runtime_that_can_be_read() {
    let ev = evidence!(
        covers = ["AC-001", "AC-004", "AC-013"],
        kind = Kind::Fixture,
        expected = "runtime.install starts the operation, it succeeds with the new runtime as its result, runtime.get and runtime.list show it, runtime.state_changed is published, and an install of the same artifact answers alreadyInstalled without an operation",
    );
    let site = Site::new();
    let archive = archive();
    site.fake.fetcher().serve(URL, archive.clone());
    let answering = site.answer_checks("nats-server: v2.15.1\n");
    let _daemon = site.start(catalog(vec![package(&archive)])).await;
    let client = client(&site).await;
    let (_, mut events) = client
        .subscribe(SubscribeParams {
            kinds: vec!["runtime.state_changed".to_owned()],
            ..SubscribeParams::default()
        })
        .await
        .expect("the client subscribes");

    let reply = call_with_key(
        &client,
        Call::RuntimeInstall(nats("stable")),
        "install-nats-0001",
    )
    .await
    .expect("the install starts");
    let Reply::RuntimeInstall(started) = reply else {
        panic!("expected a runtime.install reply");
    };
    assert_eq!(started.resolved.artifact_id, "nats-2.15.1-macos-aarch64");
    let InstallOutcome::Started(accepted) = started.outcome else {
        panic!("expected a started outcome");
    };
    assert!(!accepted.replayed);
    assert_eq!(
        accepted.operation.kind,
        stackly_protocol::operation::OperationKind::InstallRuntime
    );
    let runtime_id = accepted
        .operation
        .resource
        .as_ref()
        .expect("the operation names the runtime")
        .id
        .clone();

    let done = wait_state(&client, accepted.operation.id, OperationState::Succeeded).await;
    answering.abort();
    let resources = done.operation.result.expect("a result").resources;
    assert_eq!(resources.len(), 1);
    assert_eq!(
        (resources[0].kind, resources[0].id.as_str()),
        (ResourceKind::Runtime, runtime_id.as_str())
    );

    let event = tokio::time::timeout(PATIENCE, events.next())
        .await
        .expect("the event comes")
        .expect("the stream is open")
        .expect("an event arrives");
    let DecodedFrame::Event(event) = event else {
        panic!("expected an event");
    };
    assert_eq!(event.kind, EventKind::RuntimeStateChanged);
    assert_eq!(
        event.resource.map(|resource| resource.id),
        Some(runtime_id.clone())
    );

    let id = runtime_id.parse().expect("a runtime id");
    let Reply::RuntimeGet(detail) = client
        .call(Call::RuntimeGet(GetRuntimeParams { runtime_id: id }))
        .await
        .expect("the runtime can be read")
    else {
        panic!("expected a runtime.get reply");
    };
    assert_eq!(detail.summary.state, WireState::Installed);
    assert_eq!(detail.summary.version, "2.15.1");
    assert_eq!(detail.summary.display_name, "NATS Server");
    assert_eq!(
        detail.summary.artifact_sha256,
        Some(digest(&archive).to_string())
    );
    assert!(
        detail.path.ends_with(&digest(&archive).to_string()),
        "{}",
        detail.path
    );
    let artifact = detail.artifact.expect("an artifact");
    assert_eq!(
        artifact.catalog_id.as_deref(),
        Some("nats-2.15.1-macos-aarch64")
    );
    assert_eq!(
        artifact.signature.as_ref().map(|s| s.policy),
        Some(SigningPolicy::KeepOrAdhoc)
    );
    assert_eq!(detail.executables, ["nats-server"]);
    assert!(
        artifact
            .dependencies
            .iter()
            .any(|d| d.name == "libSystem.B.dylib")
    );

    let Reply::RuntimeList(page) = client
        .call(Call::RuntimeList(ListRuntimesParams::default()))
        .await
        .expect("runtimes can be listed")
    else {
        panic!("expected a runtime.list reply");
    };
    assert_eq!(page.items, std::slice::from_ref(&detail.summary));
    assert!(page.installing.is_empty());

    // The artifact is installed: a new key gets the runtime, no operation.
    let again = call_with_key(
        &client,
        Call::RuntimeInstall(nats("2.15")),
        "install-nats-0002",
    )
    .await
    .expect("an installed artifact answers");
    let Reply::RuntimeInstall(again) = again else {
        panic!("expected a runtime.install reply");
    };
    assert_eq!(
        again.outcome,
        InstallOutcome::AlreadyInstalled(Box::new(detail.summary.clone()))
    );
    assert_eq!(operation_count(&client).await, 1);

    // The first key names the operation the first request started.
    let replay = call_with_key(
        &client,
        Call::RuntimeInstall(nats("stable")),
        "install-nats-0001",
    )
    .await
    .expect("the key replays");
    let Reply::RuntimeInstall(replay) = replay else {
        panic!("expected a runtime.install reply");
    };
    let InstallOutcome::Started(replayed) = replay.outcome else {
        panic!("a replay names its operation");
    };
    assert!(replayed.replayed);
    assert_eq!(replayed.operation.id, accepted.operation.id);
    assert_eq!(replay.resolved.sha256, digest(&archive).to_string());
    ev.pass();
}

// covers: AC-4
#[tokio::test]
async fn a_download_that_is_not_the_catalogs_fails_the_operation_with_its_code_and_hint() {
    let ev = evidence!(
        covers = ["AC-004"],
        kind = Kind::Fixture,
        expected = "a changed byte fails the operation with runtime.digest_mismatch and its static hint, and no runtime is left",
    );
    let site = Site::new();
    let archive = archive();
    let mut tampered = archive.clone();
    let last = tampered.len() - 1;
    tampered[last] ^= 0xff;
    site.fake.fetcher().serve(URL, tampered);
    let _daemon = site.start(catalog(vec![package(&archive)])).await;
    let client = client(&site).await;

    let Reply::RuntimeInstall(reply) = call_with_key(
        &client,
        Call::RuntimeInstall(nats("stable")),
        "install-nats-0003",
    )
    .await
    .expect("the install starts") else {
        panic!("expected a runtime.install reply");
    };
    let InstallOutcome::Started(accepted) = reply.outcome else {
        panic!("expected a started outcome");
    };

    let failed = wait_state(&client, accepted.operation.id, OperationState::Failed).await;

    let error = failed.operation.failure.expect("a failure").error;
    assert_eq!(error.code(), runtime_codes::DIGEST_MISMATCH.code);
    assert_eq!(
        error.hint(),
        Some(
            "The download does not match the catalog. Try again; if it repeats, report this catalog entry."
        )
    );
    let Reply::RuntimeList(page) = client
        .call(Call::RuntimeList(ListRuntimesParams::default()))
        .await
        .expect("runtimes can be listed")
    else {
        panic!("expected a runtime.list reply");
    };
    assert!(page.items.is_empty());
    ev.pass();
}

// covers: AC-13
#[tokio::test]
async fn a_runtime_in_another_state_refuses_the_install_and_the_list_pages_by_id() {
    let ev = evidence!(
        covers = ["AC-013"],
        kind = Kind::Fixture,
        expected = "a runtime of the artifact that is not installed is refused with runtime.already_registered, and runtime.list pages by id with a next cursor",
    );
    let site = Site::new();
    let archive = archive();
    let daemon = site.start(catalog(vec![package(&archive)])).await;
    let client = client(&site).await;
    let registry = daemon.core().registry().expect("the registry is ready");
    let stuck = RuntimeBuilder::new()
        .software("nats")
        .version("2.15.1", "2.15")
        .managed(digest(&archive))
        .state(RuntimeState::Failed);
    let others: Vec<RuntimeBuilder> = (0..2)
        .map(|_| RuntimeBuilder::new().state(RuntimeState::Installed))
        .collect();
    let mut ops = vec![
        WriteOp::InsertArtifact(stuck.artifact()),
        WriteOp::InsertRuntime(stuck.build()),
    ];
    for other in &others {
        ops.push(WriteOp::InsertArtifact(other.artifact()));
        ops.push(WriteOp::InsertRuntime(other.clone().build()));
    }
    let writer = registry.clone();
    tokio::task::spawn_blocking(move || writer.commit(WriteBatch::new(ops)))
        .await
        .expect("the commit task ends")
        .expect("the runtimes are written");

    let error = call_with_key(
        &client,
        Call::RuntimeInstall(nats("stable")),
        "install-nats-0004",
    )
    .await
    .unwrap_err();
    assert_eq!(error.code(), runtime_codes::ALREADY_REGISTERED.code);
    assert!(
        error
            .details()
            .iter()
            .any(|d| d.key() == "state" && d.value() == "failed")
    );
    assert!(error.hint().is_some());
    assert_eq!(operation_count(&client).await, 0);

    let list = |after, limit| {
        let client = &client;
        async move {
            let Reply::RuntimeList(page) = client
                .call(Call::RuntimeList(ListRuntimesParams {
                    limit: stackly_protocol::Count::new(limit),
                    after,
                }))
                .await
                .expect("runtimes can be listed")
            else {
                panic!("expected a runtime.list reply");
            };
            page
        }
    };
    let first = list(None, 2).await;
    assert_eq!(first.items.len(), 2);
    let next = first.next.expect("one more follows");
    assert_eq!(next, first.items[1].id);
    let second = list(Some(next), 2).await;
    assert_eq!(second.items.len(), 1);
    assert!(second.next.is_none());
    assert!(first.items[1].id < second.items[0].id);
    let ids: Vec<_> = first
        .items
        .iter()
        .chain(&second.items)
        .map(|r| r.id)
        .collect();
    let mut sorted = ids.clone();
    sorted.sort();
    assert_eq!(ids, sorted);
    ev.pass();
}

// covers: AC-17
#[tokio::test]
async fn an_install_that_has_not_ended_is_listed_on_the_first_page_only() {
    let ev = evidence!(
        covers = ["AC-017"],
        kind = Kind::Fixture,
        expected = "runtime.list names the running install with its operation, runtime, and digest on the first page, and not on a later one",
    );
    let site = Site::new();
    let archive = archive();
    site.fake.fetcher().serve(URL, archive.clone());
    site.fake.fetcher().set_chunk(8);
    site.fake
        .fetcher()
        .after_chunk(|_| std::thread::sleep(std::time::Duration::from_millis(20)));
    let _daemon = site.start(catalog(vec![package(&archive)])).await;
    let client = client(&site).await;
    let Reply::RuntimeInstall(reply) = call_with_key(
        &client,
        Call::RuntimeInstall(nats("stable")),
        "install-nats-0006",
    )
    .await
    .expect("the install starts") else {
        panic!("expected a runtime.install reply");
    };
    let InstallOutcome::Started(accepted) = reply.outcome else {
        panic!("expected a started outcome");
    };

    let Reply::RuntimeList(page) = client
        .call(Call::RuntimeList(ListRuntimesParams::default()))
        .await
        .expect("runtimes can be listed")
    else {
        panic!("expected a runtime.list reply");
    };

    assert!(page.items.is_empty());
    let [installing] = page.installing.as_slice() else {
        panic!("expected one install: {page:?}");
    };
    assert_eq!(installing.operation_id, accepted.operation.id);
    assert_eq!(installing.provider, "official/nats");
    assert_eq!(installing.display_name, "NATS Server");
    assert_eq!(installing.version, "2.15.1");
    assert_eq!(installing.sha256, digest(&archive).to_string());
    assert_eq!(
        accepted.operation.resource.map(|resource| resource.id),
        Some(installing.runtime_id.to_string())
    );
    let Reply::RuntimeList(later) = client
        .call(Call::RuntimeList(ListRuntimesParams {
            limit: None,
            after: Some(uuid::Uuid::nil()),
        }))
        .await
        .expect("a later page lists")
    else {
        panic!("expected a runtime.list reply");
    };
    assert!(later.installing.is_empty());
    ev.pass();
}

// covers: AC-14
#[tokio::test]
async fn a_cancel_during_the_download_ends_cancelled_and_keeps_the_partial_file() {
    let ev = evidence!(
        covers = ["AC-014"],
        kind = Kind::Fixture,
        expected = "operation.cancel in the download stage ends the operation cancelled, leaves no runtime, and keeps the .part file for a later install",
    );
    let site = Site::new();
    let archive = archive();
    site.fake.fetcher().serve(URL, archive.clone());
    site.fake.fetcher().set_chunk(8);
    site.fake
        .fetcher()
        .after_chunk(|_| std::thread::sleep(std::time::Duration::from_millis(30)));
    let _daemon = site.start(catalog(vec![package(&archive)])).await;
    let client = client(&site).await;
    let Reply::RuntimeInstall(reply) = call_with_key(
        &client,
        Call::RuntimeInstall(nats("stable")),
        "install-nats-0008",
    )
    .await
    .expect("the install starts") else {
        panic!("expected a runtime.install reply");
    };
    let InstallOutcome::Started(accepted) = reply.outcome else {
        panic!("expected a started outcome");
    };
    let deadline = tokio::time::Instant::now() + PATIENCE;
    while site.fake.fetcher().requests().is_empty() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the download never began"
        );
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }

    client
        .call(Call::OperationCancel(
            stackly_protocol::operation::CancelOperationParams {
                operation_id: accepted.operation.id,
            },
        ))
        .await
        .expect("the cancel is accepted");
    wait_state(&client, accepted.operation.id, OperationState::Cancelled).await;

    let part = site
        .prepared
        .layout()
        .downloads_dir()
        .join(format!("{}.part", digest(&archive)));
    assert!(part.is_file(), "{} is kept", part.display());
    let Reply::RuntimeList(page) = client
        .call(Call::RuntimeList(ListRuntimesParams::default()))
        .await
        .expect("runtimes can be listed")
    else {
        panic!("expected a runtime.list reply");
    };
    assert!(page.items.is_empty() && page.installing.is_empty());
    ev.pass();
}

#[tokio::test]
async fn an_install_refuses_a_changed_expected_digest_before_any_operation() {
    let ev = evidence!(
        covers = ["AC-001"],
        kind = Kind::Fixture,
        expected = "expectedSha256 of another artifact is refused with runtime.selector_changed and no operation",
    );
    let site = Site::new();
    let archive = archive();
    let _daemon = site.start(catalog(vec![package(&archive)])).await;
    let client = client(&site).await;

    let error = call_with_key(
        &client,
        Call::RuntimeInstall(InstallRuntimeParams {
            expected_sha256: Some("2".repeat(64)),
            ..nats("stable")
        }),
        "install-nats-0005",
    )
    .await
    .unwrap_err();

    assert_eq!(error.code(), runtime_codes::SELECTOR_CHANGED.code);
    assert_eq!(operation_count(&client).await, 0);
    ev.pass();
}

#[tokio::test]
async fn an_install_without_a_key_is_refused_before_the_catalog_is_read() {
    let ev = evidence!(
        covers = ["AC-001"],
        kind = Kind::Fixture,
        expected = "runtime.install without an idempotency key is refused with operation.idempotency_key_required",
    );
    let site = Site::new();
    let _daemon = site.start(catalog(Vec::new())).await;
    let client = client(&site).await;

    let error = client
        .call(Call::RuntimeInstall(nats("stable")))
        .await
        .unwrap_err();

    assert_eq!(error.code(), "operation.idempotency_key_required");
    ev.pass();
}

// covers: AC-3
#[test]
fn an_install_cut_short_by_a_restart_is_rebuilt_from_its_journal_and_finishes() {
    let ev = evidence!(
        covers = ["AC-003"],
        kind = Kind::Fixture,
        expected = "a daemon that dies in the middle of the download starts again, rebuilds the install from the frozen input, and the operation succeeds with one installed runtime",
    );
    let site = Site::new();
    let archive = archive();
    site.fake.fetcher().serve(URL, archive.clone());
    site.fake.fetcher().set_chunk(8);
    site.fake
        .fetcher()
        .after_chunk(|_| std::thread::sleep(std::time::Duration::from_millis(30)));

    let crashed = runtime();
    let operation = crashed.block_on(async {
        let daemon = site.start(catalog(vec![package(&archive)])).await;
        let client = client(&site).await;
        let Reply::RuntimeInstall(reply) = call_with_key(
            &client,
            Call::RuntimeInstall(nats("stable")),
            "install-nats-0007",
        )
        .await
        .expect("the install starts") else {
            panic!("expected a runtime.install reply");
        };
        let InstallOutcome::Started(accepted) = reply.outcome else {
            panic!("expected a started outcome");
        };
        // The download has begun when the fetcher saw its request.
        let deadline = tokio::time::Instant::now() + PATIENCE;
        while site.fake.fetcher().requests().is_empty() {
            assert!(
                tokio::time::Instant::now() < deadline,
                "the download never began"
            );
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        // No stop runs: the handle and the connection go, and the runtime
        // drops every task of the run, as in a crash.
        drop(daemon);
        drop(client);
        accepted.operation.id
    });
    crashed.shutdown_timeout(std::time::Duration::from_secs(5));

    let restarted = runtime();
    restarted.block_on(async {
        // The catalog may have changed since: the work comes from the journal.
        let _daemon = site.start(catalog(Vec::new())).await;
        let answering = site.answer_checks("nats-server: v2.15.1\n");
        let client = client(&site).await;

        let done = wait_state(&client, operation, OperationState::Succeeded).await;
        answering.abort();

        assert_eq!(done.operation.result.expect("a result").resources.len(), 1);
        let Reply::RuntimeList(page) = client
            .call(Call::RuntimeList(ListRuntimesParams::default()))
            .await
            .expect("runtimes can be listed")
        else {
            panic!("expected a runtime.list reply");
        };
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].state, WireState::Installed);
        assert!(page.installing.is_empty());
    });
    restarted.shutdown_timeout(std::time::Duration::from_secs(5));
    ev.pass();
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("a tokio runtime builds")
}
