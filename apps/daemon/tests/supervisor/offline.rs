//! Start, status, detail, and connection without a network (spec 0021
//! AC-10, AC-18): the daemon runs in process with no catalog, a fetcher and
//! a code signing port that record every call, and the real `stackly`
//! binary asks it over the socket.

use std::io;
use std::sync::Arc;

use serde_json::{Value, json};
use stackly_domain::{AuditQuery, Registry};
use stackly_ipc::{CallOptions, Client, ConnectPolicy};
use stackly_platform::{Clock, SystemClock};
use stackly_protocol::handshake::{ClientInfo, ClientKind};
use stackly_protocol::{Call, Reply, Timestamp};
use stackly_testkit::evidence::{Kind, evidence};
use stackly_testkit::{
    CliRunner, FakeCodeSigning, FakeFetcher, SigningCall, allocate_port, cargo_bin,
};
use stacklyd::log_sink::{LogSink, LogSinkConfig};
use stacklyd::map::StartFacts;
use stacklyd::server::{self, DaemonDeps};
use uuid::Uuid;

use crate::support::{Api, Site, TOKEN, fast};

/// A writer the test can read back: the lines of the daemon log.
#[derive(Clone, Default)]
struct Lines(Arc<std::sync::Mutex<Vec<u8>>>);

impl io::Write for Lines {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Lines {
    fn text(&self) -> String {
        String::from_utf8_lossy(
            &self
                .0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )
        .into_owned()
    }
}

fn result_of(run: &stackly_testkit::CliOutput) -> Value {
    assert_eq!(run.code(), Some(0), "{run:?}");
    run.json().expect("the envelope is JSON")["result"].clone()
}

// covers: AC-10, AC-18
#[tokio::test(flavor = "multi_thread")]
async fn start_status_detail_and_connection_need_no_catalog_fetch_or_signature_check() {
    let ev = evidence!(
        covers = ["REQ-046", "AC-046", "AC-016"],
        kind = Kind::Fixture,
        expected = "with no catalog, a fetcher that was never asked, and a code signing port that never assessed a signature, instance.start to ready, instance.get, instance.get_detail, instance.get_data_usage, stackly instance connection, and stackly instance inspect succeed for the fake instance with zero catalog accesses, and neither the daemon log nor any output holds the secret value",
    );
    let site = Site::for_cli();
    let fetcher = FakeFetcher::new();
    fetcher.go_offline();
    let signing = FakeCodeSigning::new();
    let platform = site
        .platform
        .clone()
        .with_fetcher(Arc::new(fetcher.clone()))
        .with_code_signing(Arc::new(signing.clone()));
    let log = Lines::default();
    let sink = LogSink::start(Box::new(log.clone()), LogSinkConfig::DAEMON)
        .expect("the log writer starts");
    let facts = StartFacts::new(
        Uuid::now_v7(),
        Timestamp::from_jiff(SystemClock.now()),
        true,
        std::process::id(),
    );
    let daemon = server::start(
        DaemonDeps::new(platform, site.prepared.clone(), facts, Arc::new(sink)).with_config(fast()),
    )
    .await
    .expect("the server starts");
    daemon.started().await;
    let (client, _) = Client::connect(
        &site.socket(),
        ConnectPolicy::same_user(site.uid()),
        ClientInfo {
            kind: ClientKind::Cli,
            version: "0.1.0".to_owned(),
            pid: Some(std::process::id()),
            interactive: Some(true),
        },
    )
    .await
    .expect("the client connects");
    let api = Api::new(client, site.runtime_id);
    let mut params = api.params("orders", json!({}));
    params.endpoints = vec![Api::http_port(allocate_port().expect("a free port"))];
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
    let before = daemon.core().catalog_accesses();

    api.start_ready(id).await;
    api.get(id).await.expect("instance.get");
    let detail = api.detail(id).await.expect("instance.get_detail");
    api.data_usage(id, false, false)
        .await
        .expect("instance.get_data_usage");
    let cli = CliRunner::new(&cargo_bin("stackly-cli", "stackly"), site.root.path());
    let connection = result_of(
        &cli.run(&["instance", "connection", "orders"])
            .expect("runs"),
    );
    let inspect = result_of(&cli.run(&["instance", "inspect", "orders"]).expect("runs"));

    let masked = detail.connections[0].masked.clone().expect("available");
    assert_eq!(connection["connection"]["masked"], masked);
    assert_eq!(inspect["detail"]["connections"][0]["masked"], masked);
    assert_eq!(inspect["instance"]["readiness"]["state"], "ready");
    assert_eq!(
        daemon.core().catalog_accesses(),
        before,
        "the catalog was looked at"
    );
    assert!(fetcher.requests().is_empty(), "{:?}", fetcher.requests());
    // The start reads the quarantine flag of the runtime from its files; it
    // never assesses a signature.
    let assessed: Vec<_> = signing
        .calls()
        .into_iter()
        .filter(|call| matches!(call, SigningCall::Assess(_)))
        .collect();
    assert!(assessed.is_empty(), "{assessed:?}");
    for text in [
        log.text(),
        connection.to_string(),
        inspect.to_string(),
        serde_json::to_string(&detail).expect("encodes"),
    ] {
        assert!(
            !text.contains(&TOKEN[..12]),
            "a prefix of the secret leaked"
        );
    }
    let revealed = client_reveal(&api, &detail.secrets[0].reference).await;
    assert_eq!(revealed, TOKEN, "the value is only in secret.reveal");
    ev.pass();
}

/// `secret.reveal` for `reference`, the one place the value may appear.
async fn client_reveal(api: &Api, reference: &str) -> String {
    let reply = api
        .client
        .call_detailed(
            Call::SecretReveal(stackly_protocol::secret::RevealSecretParams {
                reference: reference.to_owned(),
                purpose: stackly_protocol::secret::RevealPurpose::Show,
            }),
            CallOptions::default(),
        )
        .await
        .expect("the reveal is answered");
    match reply {
        Reply::SecretReveal(reply) => reply.value.expose().to_owned(),
        other => panic!("not a reveal reply: {other:?}"),
    }
}

// covers: AC-6, AC-18
#[tokio::test(flavor = "multi_thread")]
async fn show_password_writes_the_real_string_and_each_reveal_is_audited() {
    let ev = evidence!(
        covers = ["REQ-028", "REQ-016", "AC-016"],
        kind = Kind::Fixture,
        expected = "with a real daemon stackly instance connection writes the masked string by default, writes the string with the real token only with --show-password, and every secret.reveal it made is an audit row",
    );
    let site = Site::for_cli();
    let daemon = site.start().await;
    let api = site.api().await;
    let port = allocate_port().expect("a free port");
    let mut params = api.params("orders", json!({}));
    params.endpoints = vec![Api::http_port(port)];
    let accepted = api
        .create(params)
        .await
        .expect("the create is accepted")
        .accepted
        .expect("a create starts an operation");
    api.wait_state(
        accepted.operation.id,
        stackly_protocol::operation::OperationState::Succeeded,
    )
    .await;
    let cli = CliRunner::new(&cargo_bin("stackly-cli", "stackly"), site.root.path());

    let masked = result_of(
        &cli.run(&["instance", "connection", "orders"])
            .expect("runs"),
    );
    let shown = result_of(
        &cli.run(&["instance", "connection", "orders", "--show-password"])
            .expect("runs"),
    );

    assert_eq!(masked["value"], Value::Null);
    assert_eq!(
        masked["connection"]["masked"],
        format!("http://stackly:****@127.0.0.1:{port}/")
    );
    assert!(!masked.to_string().contains(TOKEN));
    assert_eq!(
        shown["value"],
        format!("http://stackly:{TOKEN}@127.0.0.1:{port}/")
    );
    let registry = daemon.core().registry().expect("the registry is ready");
    let page = registry
        .audit_events(AuditQuery {
            limit: 50,
            before: None,
        })
        .expect("the audit rows read")
        .value;
    let reveals = page
        .items
        .iter()
        .filter(|row| row.action.to_string() == "secret.reveal")
        .count();
    assert_eq!(reveals, 1, "one reveal, audited once");
    ev.pass();
}
