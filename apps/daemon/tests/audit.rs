//! `audit.list` through the real daemon (spec 0035 AC-2, AC-5, AC-6): the
//! fake provider in a test root, the instance methods over the socket, and
//! the rows the operation engine and the secret service write on a real
//! `SqliteRegistry`. The password of every instance is a fixture
//! credential, and no registry byte, reply, or daemon log line holds it.
#![cfg(target_os = "macos")]

#[path = "supervisor/support.rs"]
mod support;

use std::io::{self, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use jiff::SignedDuration;
use serde_json::json;
use stackly_domain::{
    AuditAction, AuditActor, AuditEventId, AuditEventRecord, AuditResult, Registry, WriteBatch,
    WriteOp,
};
use stackly_ipc::CallOptions;
use stackly_platform::{Clock, SystemClock};
use stackly_protocol::audit::{AuditEventInfo, AuditPage, AuditResultKind, ListAuditParams};
use stackly_protocol::instance::config::{ChangeEndpointsParams, ChangeEndpointsReply};
use stackly_protocol::instance::{CreateInstanceParams, EndpointRequest, PortRequest};
use stackly_protocol::operation::{OperationDetail, OperationState};
use stackly_protocol::secret::{SecretInput, SecretText};
use stackly_protocol::settings::{DaemonSettingsView, SettingsPatch, UpdateSettingsParams};
use stackly_protocol::{Call, ConfigRevisionNumber, ErrorBody, Reply, Timestamp};
use stackly_testkit::evidence::FixtureCredential;
use stacklyd::log_sink::{LogSink, LogSinkConfig};
use stacklyd::map::StartFacts;
use stacklyd::server::{self, DaemonDeps, DaemonHandle};
use uuid::Uuid;

use crate::support::{Api, Site, fast, key};

/// A writer that keeps what the daemon's log sink wrote.
#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl Write for Captured {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Captured {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().unwrap_or_else(PoisonError::into_inner)).into_owned()
    }
}

/// Starts a daemon on `site` whose log goes to `log`, with a prune interval
/// of `prune`.
async fn start(site: &Site, log: &Captured, prune: Duration) -> DaemonHandle {
    let sink = LogSink::start(Box::new(log.clone()), LogSinkConfig::DAEMON)
        .expect("the log writer starts");
    let facts = StartFacts::new(
        Uuid::now_v7(),
        Timestamp::from_jiff(SystemClock.now()),
        true,
        std::process::id(),
    );
    let mut config = fast();
    config.prune_interval = prune;
    let deps = DaemonDeps::new(
        site.platform.clone(),
        site.prepared.clone(),
        facts,
        Arc::new(sink),
    )
    .with_config(config);
    let handle = server::start(deps).await.expect("the server starts");
    handle.started().await;
    handle
}

async fn stop(daemon: DaemonHandle) {
    daemon
        .shutdown(
            stackly_protocol::events::ShutdownReason::Requested,
            std::future::pending(),
        )
        .await;
}

async fn audit(api: &Api, params: ListAuditParams) -> Result<AuditPage, Box<ErrorBody>> {
    match api.client.call(Call::AuditList(params)).await? {
        Reply::AuditList(page) => Ok(page),
        other => panic!("not an audit.list reply: {other:?}"),
    }
}

/// Every row of `instance`, oldest first, read a page of two at a time.
async fn rows_of(api: &Api, instance: Uuid) -> Vec<AuditEventInfo> {
    let mut items = Vec::new();
    let mut before = None;
    loop {
        let page = audit(
            api,
            ListAuditParams {
                instance_id: Some(instance),
                limit: Some(2),
                before: before.clone(),
                ..ListAuditParams::default()
            },
        )
        .await
        .expect("audit.list answers");
        assert!(page.items.len() <= 2);
        items.extend(page.items);
        match page.next {
            Some(next) => before = Some(next),
            None => break,
        }
    }
    items.reverse();
    items
}

fn detail<'a>(row: &'a AuditEventInfo, key: &str) -> Option<&'a str> {
    row.details
        .iter()
        .find(|detail| detail.key == key)
        .map(|detail| detail.value.as_str())
}

/// The one `operation.finished` row of `operation` among `rows`, checked
/// against the operation it closes: the same correlation ID as the
/// accepted reply and the error, and the result and error code of its end.
fn finished<'a>(rows: &'a [AuditEventInfo], operation: &OperationDetail) -> &'a AuditEventInfo {
    let id = operation.operation.id;
    let mine: Vec<&AuditEventInfo> = rows
        .iter()
        .filter(|row| row.operation_id == Some(id) && row.action == "operation.finished")
        .collect();
    assert_eq!(mine.len(), 1, "one finished row of {id}: {rows:?}");
    let row = mine[0];
    let expected = match operation.operation.state {
        OperationState::Succeeded => AuditResultKind::Succeeded,
        OperationState::Failed => AuditResultKind::Failed,
        OperationState::Cancelled => AuditResultKind::Cancelled,
        other => panic!("{id} ended {other:?}"),
    };
    assert_eq!(row.result, expected);
    if let Some(failure) = &operation.operation.failure {
        assert_eq!(detail(row, "errorCode"), Some(failure.error.code()));
        assert_eq!(
            failure.error.operation_id(),
            Some(id),
            "the same correlation ID"
        );
    }
    row
}

fn with_credential(
    mut params: CreateInstanceParams,
    credential: &FixtureCredential,
) -> CreateInstanceParams {
    params.secrets[0].input = SecretInput::Value(SecretText::new(credential.value().to_owned()));
    params
}

fn free_port() -> u16 {
    TcpListener::bind(("127.0.0.1", 0))
        .expect("a free port binds")
        .local_addr()
        .expect("the listener has an address")
        .port()
}

async fn create(api: &Api, params: CreateInstanceParams) -> (Uuid, OperationDetail) {
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
        .and_then(|resource| resource.id.parse().ok())
        .expect("the operation names the instance");
    (id, api.wait_final(accepted.operation.id).await)
}

async fn change_port(api: &Api, id: Uuid, base: u64, port: u16) -> Result<Uuid, Box<ErrorBody>> {
    let reply = api
        .client
        .call_detailed(
            Call::InstanceChangeEndpoints(ChangeEndpointsParams {
                instance_id: id,
                base_revision: ConfigRevisionNumber::new(base).expect("a revision number"),
                endpoints: vec![EndpointRequest {
                    name: "http".to_owned(),
                    port: PortRequest::Fixed(port),
                }],
                dry_run: false,
                override_leases: false,
            }),
            CallOptions::default().with_idempotency_key(key()),
        )
        .await?;
    match reply {
        Reply::InstanceChangeEndpoints(reply) => match *reply {
            ChangeEndpointsReply::Applied(applied) => Ok(applied
                .accepted
                .expect("a change starts an operation")
                .operation
                .id),
            other => panic!("not an applied change: {other:?}"),
        },
        other => panic!("not a change endpoints reply: {other:?}"),
    }
}

// covers: AC-2, AC-4, AC-5 (spec 0035)
#[tokio::test(flavor = "multi_thread")]
async fn create_start_and_a_port_change_each_leave_one_row_with_their_correlation_id_and_no_secret()
{
    let site = Site::new();
    let log = Captured::default();
    let daemon = start(&site, &log, Duration::from_secs(3600)).await;
    let api = site.api().await;
    let credential = FixtureCredential::new();

    // Create: one that succeeds, one that fails at init.
    let first_port = free_port();
    let mut params = with_credential(api.params("audited", json!({})), &credential);
    params.endpoints = vec![Api::http_port(first_port)];
    let (id, created) = create(&api, params).await;
    assert_eq!(created.operation.state, OperationState::Succeeded);
    let (broken, failed_create) = create(
        &api,
        with_credential(
            api.params("broken", json!({ "failInit": true })),
            &credential,
        ),
    )
    .await;
    assert_eq!(failed_create.operation.state, OperationState::Failed);

    // Start: one that succeeds, one whose fixed port a listener holds.
    let started = api
        .start(id)
        .await
        .expect("the start is accepted")
        .accepted
        .expect("a start begins an operation");
    let started = api.wait_final(started.operation.id).await;
    assert_eq!(started.operation.state, OperationState::Succeeded);
    let held_port = free_port();
    let mut params = with_credential(api.params("held", json!({})), &credential);
    params.endpoints = vec![Api::http_port(held_port)];
    let (held, _) = create(&api, params).await;
    let listener = TcpListener::bind(("127.0.0.1", held_port)).expect("the port binds");
    let refused = api
        .start(held)
        .await
        .expect("the start is accepted")
        .accepted
        .expect("a start begins an operation");
    let refused = api.wait_final(refused.operation.id).await;
    assert_eq!(Api::failure_code(&refused), "supervisor.endpoint_in_use");
    drop(listener);

    // Port change: one that succeeds, and a second one read at the same
    // revision, which the engine queues behind the first and fails.
    let moved_port = free_port();
    let moved = change_port(&api, held, 1, moved_port)
        .await
        .expect("the change is accepted");
    let moved = api.wait_final(moved).await;
    assert_eq!(moved.operation.state, OperationState::Succeeded);
    let mut stale = None;
    for base in 2..12 {
        let (one, two) = tokio::join!(
            change_port(&api, held, base, free_port()),
            change_port(&api, held, base, free_port()),
        );
        let one = one.expect("the first change is accepted");
        api.wait_final(one).await;
        if let Ok(two) = two {
            let two = api.wait_final(two).await;
            if two.operation.state == OperationState::Failed {
                stale = Some(two);
                break;
            }
        }
    }
    let stale = stale.expect("a queued change read at a stale revision fails in its operation");
    assert_eq!(Api::failure_code(&stale), "config.revision_conflict");

    // The rows of each instance.
    let rows = rows_of(&api, id).await;
    let create_row = finished(&rows, &created);
    assert_eq!(create_row.config_revision.map(|n| n.get()), Some(1));
    assert_eq!(create_row.instance_id, Some(id));
    assert_eq!(detail(create_row, "provider"), Some(support::PROVIDER));
    let start_row = finished(&rows, &started);
    assert_eq!(start_row.config_revision.map(|n| n.get()), Some(1));
    assert_eq!(
        detail(start_row, "ports"),
        Some(format!("http {first_port}").as_str())
    );
    assert!(
        rows.iter().any(|row| row.action == "secret.set"),
        "the instance's secret row shows with the instance filter: {rows:?}"
    );

    let rows = rows_of(&api, broken).await;
    let row = finished(&rows, &failed_create);
    assert_eq!(row.config_revision.map(|n| n.get()), Some(1));
    assert_eq!(
        row.instance_id,
        Some(broken),
        "a removed instance keeps its rows"
    );

    let rows = rows_of(&api, held).await;
    let row = finished(&rows, &refused);
    assert_eq!(row.config_revision.map(|n| n.get()), Some(1));
    let row = finished(&rows, &moved);
    assert_eq!(row.config_revision.map(|n| n.get()), Some(2));
    assert_eq!(detail(row, "oldRevision"), Some("1"));
    assert_eq!(
        detail(row, "ports"),
        Some(format!("http {held_port} \u{2192} {moved_port}").as_str())
    );
    finished(&rows, &stale);

    // AC-5: both filters together, an exclusive cursor, and the refusals.
    let both = audit(
        &api,
        ListAuditParams {
            instance_id: Some(held),
            operation_id: Some(moved.operation.id),
            ..ListAuditParams::default()
        },
    )
    .await
    .expect("audit.list answers");
    assert_eq!(both.items.len(), 1);
    assert_eq!(both.next, None, "the last page has no next");
    let first = audit(
        &api,
        ListAuditParams {
            limit: Some(1),
            ..ListAuditParams::default()
        },
    )
    .await
    .expect("audit.list answers");
    let after = audit(
        &api,
        ListAuditParams {
            limit: Some(1),
            before: first.next.clone(),
            ..ListAuditParams::default()
        },
    )
    .await
    .expect("audit.list answers");
    assert_ne!(after.items[0].id, first.items[0].id, "before is exclusive");
    assert!(after.items[0].occurred_at <= first.items[0].occurred_at);
    for bad in [
        ListAuditParams {
            limit: Some(0),
            ..ListAuditParams::default()
        },
        ListAuditParams {
            limit: Some(201),
            ..ListAuditParams::default()
        },
        ListAuditParams {
            before: Some("not-a-cursor".to_owned()),
            ..ListAuditParams::default()
        },
    ] {
        let error = audit(&api, bad).await.expect_err("refused");
        assert_eq!(error.code(), "request.invalid_params");
    }

    // AC-2, AC-4: the credential is in no row, no registry byte, no log.
    let all = audit(
        &api,
        ListAuditParams {
            limit: Some(200),
            ..ListAuditParams::default()
        },
    )
    .await
    .expect("audit.list answers");
    credential.assert_absent(&format!("{all:?}"), "the audit.list reply");
    credential.assert_absent(
        &serde_json::to_string(&all).expect("a page encodes"),
        "the audit.list reply as JSON",
    );
    drop(api);
    stop(daemon).await;
    credential.assert_absent(&log.text(), "the daemon log");
    let registry = site.prepared.layout().registry_db();
    for suffix in ["", "-wal", "-shm"] {
        let mut name = registry.clone().into_os_string();
        name.push(suffix);
        let bytes = std::fs::read(&name).unwrap_or_default();
        credential.assert_absent(
            &String::from_utf8_lossy(&bytes),
            &format!("the registry file{suffix}"),
        );
    }
}

fn old_row(days: i64) -> AuditEventRecord {
    let at = SystemClock
        .now()
        .checked_sub(SignedDuration::from_hours(24 * days))
        .expect("a time some days ago");
    AuditEventRecord::new(
        AuditEventId::generate(),
        AuditActor::User,
        AuditAction::from_static("secret.reveal"),
        AuditResult::Succeeded,
        at,
    )
}

async fn settings(api: &Api) -> DaemonSettingsView {
    match api
        .client
        .call(Call::SettingsGet)
        .await
        .expect("settings.get answers")
    {
        Reply::SettingsGet(view) => *view,
        other => panic!("not a settings reply: {other:?}"),
    }
}

async fn retention(api: &Api, days: u32) -> Result<DaemonSettingsView, Box<ErrorBody>> {
    let revision = settings(api).await.revision;
    match api
        .client
        .call_detailed(
            Call::SettingsUpdate(UpdateSettingsParams {
                expected_revision: revision,
                patch: SettingsPatch {
                    audit_retention_days: Some(days),
                    ..SettingsPatch::default()
                },
            }),
            CallOptions::default().with_idempotency_key(key()),
        )
        .await?
    {
        Reply::SettingsUpdate(view) => Ok(*view),
        other => panic!("not a settings reply: {other:?}"),
    }
}

async fn known(api: &Api, ids: &[AuditEventId]) -> Vec<bool> {
    let page = audit(
        api,
        ListAuditParams {
            limit: Some(200),
            ..ListAuditParams::default()
        },
    )
    .await
    .expect("audit.list answers");
    ids.iter()
        .map(|id| page.items.iter().any(|row| row.id == id.as_uuid()))
        .collect()
}

// covers: AC-6 (spec 0035)
#[tokio::test(flavor = "multi_thread")]
async fn a_shorter_audit_retention_removes_older_rows_within_10_seconds_without_a_restart() {
    let site = Site::new();
    let rows = [old_row(20), old_row(60), old_row(1)];
    {
        let registry = site.open_registry();
        registry
            .commit(WriteBatch::new(
                rows.iter()
                    .cloned()
                    .map(WriteOp::InsertAuditEvent)
                    .collect(),
            ))
            .expect("old rows are seeded");
    }
    let ids: Vec<AuditEventId> = rows.iter().map(|row| row.id).collect();
    let log = Captured::default();
    // An interval far beyond the test: only the start and a wake prune.
    let daemon = start(&site, &log, Duration::from_secs(3600)).await;
    let api = site.api().await;

    let view = settings(&api).await;
    let shown = view
        .audit_retention
        .expect("the view carries the retention");
    assert_eq!((shown.days, shown.max_rows.get()), (30, 100_000));
    assert_eq!(
        known(&api, &ids).await,
        [true, false, true],
        "the start prune used 30 days"
    );

    let error = retention(&api, 14).await.expect_err("14 days is refused");
    assert_eq!(error.code(), "request.invalid_params");
    let view = retention(&api, 7).await.expect("7 days is accepted");
    assert_eq!(view.audit_retention.map(|shown| shown.days), Some(7));
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let seen = known(&api, &ids).await;
        if seen == [false, false, true] {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the 20 day old row is still there: {seen:?}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let view = retention(&api, 90).await.expect("90 days is accepted");
    assert_eq!(view.audit_retention.map(|shown| shown.days), Some(90));
    drop(api);
    stop(daemon).await;
}
