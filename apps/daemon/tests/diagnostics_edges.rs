//! The edges of `doctor.run` and the diagnostic bundle through the real
//! daemon (spec 0034 AC-5, AC-12, AC-14): the fake provider in a test root
//! on the macOS platform. A long daemon log and a long instance log keep
//! their newest lines and are marked truncated; past 50 instances the bundle
//! keeps the 50 newest logs and names the rest; a sentinel credential in a
//! readiness detail never reaches the report, the preview, the bundle, or
//! the daemon log; and two port conflicts in one report each get their own
//! suggestion, which the doctor never applies.
#![cfg(target_os = "macos")]

#[path = "supervisor/support.rs"]
mod support;

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use jiff::{SignedDuration, Timestamp};
use serde_json::json;
use stackly_core::supervisor::{LogBatch, LogLine};
use stackly_domain::InstanceId;
use stackly_ipc::CallOptions;
use stackly_platform::OutputStream;
use stackly_protocol::diagnostics::{
    BundleEntry, BundleEntryKind, BundlePreview, ExportBundleParams,
};
use stackly_protocol::doctor::{
    DoctorActionKind, DoctorCheck, DoctorCheckId, DoctorFinding, DoctorFindingCode, DoctorParams,
    DoctorReport, DoctorSeverity,
};
use stackly_protocol::instance::config::{ChangeEndpointsParams, ChangeEndpointsReply};
use stackly_protocol::instance::{
    EndpointRequest, InstanceLifecycle, InstanceReadinessState, PortRequest, SecretFieldInput,
};
use stackly_protocol::operation::OperationState;
use stackly_protocol::secret::{SecretInput, SecretText};
use stackly_protocol::{Call, ConfigRevisionNumber, ErrorBody, Reply};
use stackly_testkit::allocate_port;
use stackly_testkit::evidence::{FixtureCredential, Kind, evidence};
use stacklyd::server::DaemonHandle;
use uuid::Uuid;

use crate::support::{Api, Site};

async fn preview(api: &Api) -> BundlePreview {
    match api
        .client
        .call(Call::DiagnosticsPreviewBundle)
        .await
        .expect("the preview is built")
    {
        Reply::DiagnosticsPreviewBundle(preview) => *preview,
        other => panic!("not a preview reply: {other:?}"),
    }
}

async fn doctor(api: &Api, params: DoctorParams) -> DoctorReport {
    match api
        .client
        .call(Call::DoctorRun(params))
        .await
        .expect("doctor.run answers")
    {
        Reply::DoctorRun(report) => *report,
        other => panic!("not a doctor reply: {other:?}"),
    }
}

fn check(report: &DoctorReport, id: DoctorCheckId) -> &DoctorCheck {
    report
        .checks
        .iter()
        .find(|check| check.id == id)
        .unwrap_or_else(|| panic!("the report has {id:?}"))
}

/// Every entry of the staged zip of `preview`, decompressed, as text.
fn staged_entries(site: &Site, preview: &BundlePreview) -> BTreeMap<String, String> {
    let staged = site
        .prepared
        .layout()
        .diagnostics_cache_dir()
        .join(format!("{}.zip", preview.preview_id));
    let zip = std::fs::read(&staged).expect("the preview is staged");
    stackly_core::diagnostics::testing::unzip(&zip)
        .into_iter()
        .map(|(path, bytes)| (path, String::from_utf8_lossy(&bytes).into_owned()))
        .collect()
}

fn entry<'a>(preview: &'a BundlePreview, path: &str) -> &'a BundleEntry {
    preview
        .entries
        .iter()
        .find(|entry| entry.path == path)
        .unwrap_or_else(|| panic!("{path} in {:?}", preview.entries))
}

/// Writes `count` stdout lines `<prefix> <n>` (four digits) of `id` into
/// the durable log store of the running daemon, read at `read_at`, and
/// waits until the writer stored them. The text holds no secret, so it
/// needs no redaction.
fn store_lines(daemon: &DaemonHandle, id: Uuid, prefix: &str, count: usize, read_at: Timestamp) {
    let logs = daemon.core().logs().expect("the log store is open");
    let lines = (0..count)
        .map(|n| LogLine::new(format!("{prefix} {n:04}")))
        .collect();
    let id = InstanceId::from_uuid(id).expect("a v7 instance id");
    logs.log()
        .accept(id, LogBatch::new(OutputStream::Stdout, read_at, lines));
    assert!(
        logs.log().wait_idle(Duration::from_secs(10)),
        "the log writer drained its queue"
    );
}

async fn stop(daemon: DaemonHandle) {
    daemon
        .shutdown(
            stackly_protocol::events::ShutdownReason::Requested,
            std::future::pending(),
        )
        .await;
}

// covers: AC-12 (spec 0034)
#[tokio::test(flavor = "multi_thread")]
async fn a_long_daemon_log_and_a_long_instance_log_keep_their_newest_2000_lines_and_say_truncated()
{
    let ev = evidence!(
        covers = ["REQ-019", "NFR-016"],
        kind = Kind::Fixture,
        expected = "spec 0034 AC-12: with 2500 lines in a rotated stacklyd log and 2100 entries in the log store of an instance, diagnostics.preview_bundle answers logs/stacklyd.log and logs/instances/chatty.log with 2000 lines each and truncated true, the zip keeps the newest lines and drops the oldest, and README.txt lists both under Truncated",
    );
    let site = Site::new();
    let daemon = site.start().await;
    let api = site.api().await;
    let id = api.create_ready("chatty", json!({})).await;

    let log_dir = site.prepared.layout().daemon_log_dir();
    std::fs::create_dir_all(&log_dir).expect("the daemon log folder can be made");
    let old: String = (0..2500)
        .map(|n| format!("{{\"message\":\"daemon fixture line {n:04}\"}}\n"))
        .collect();
    std::fs::write(log_dir.join("stacklyd.log.4"), old).expect("the rotated log is written");
    store_lines(&daemon, id, "instance fixture line", 2100, Timestamp::now());

    let preview = preview(&api).await;

    let daemon_log = entry(&preview, "logs/stacklyd.log");
    assert_eq!(daemon_log.kind, BundleEntryKind::DaemonLog);
    assert_eq!(daemon_log.lines.map(u64::from), Some(2000));
    assert!(daemon_log.truncated, "{daemon_log:?}");
    let instance_log = entry(&preview, "logs/instances/chatty.log");
    assert_eq!(instance_log.kind, BundleEntryKind::InstanceLog);
    assert_eq!(instance_log.lines.map(u64::from), Some(2000));
    assert!(instance_log.truncated, "{instance_log:?}");

    let entries = staged_entries(&site, &preview);
    let text = &entries["logs/stacklyd.log"];
    assert_eq!(text.lines().count(), 2000);
    assert!(
        text.contains("daemon fixture line 2499"),
        "the newest line stays"
    );
    assert!(
        !text.contains("daemon fixture line 0000"),
        "the oldest line goes"
    );
    let text = &entries["logs/instances/chatty.log"];
    assert_eq!(text.lines().count(), 2000);
    assert!(
        text.contains("instance fixture line 2099"),
        "the newest entry stays"
    );
    assert!(
        !text.contains("instance fixture line 0099"),
        "the 100 oldest entries go"
    );
    assert!(
        text.contains("instance fixture line 0100"),
        "{}",
        &text[..200]
    );
    let readme = &entries["README.txt"];
    assert!(
        readme.contains("logs/stacklyd.log (")
            && readme.contains("2000 lines, truncated): The last 2000 lines of the stacklyd log."),
        "{readme}"
    );
    let truncated = readme
        .lines()
        .find(|line| line.starts_with("Truncated: "))
        .expect("README.txt has a Truncated line");
    assert!(
        truncated.contains("logs/stacklyd.log") && truncated.contains("logs/instances/chatty.log"),
        "{truncated}"
    );
    drop(api);
    stop(daemon).await;
    ev.pass();
}

// covers: AC-12 (spec 0034)
#[tokio::test(flavor = "multi_thread")]
async fn past_fifty_instances_the_bundle_keeps_the_fifty_newest_logs_and_names_the_rest() {
    let ev = evidence!(
        covers = ["REQ-019", "NFR-016"],
        kind = Kind::Fixture,
        expected = "spec 0034 AC-12: with 51 instances that each have one log entry, diagnostics.preview_bundle answers the 50 instance logs with the newest entries and leaves out the one with the oldest, which README.txt names as past the first 50 instances",
    );
    let site = Site::new();
    let daemon = site.start().await;
    let api = site.api().await;
    let base = Timestamp::now() - SignedDuration::from_hours(1);
    for n in 0..51_i64 {
        let name = format!("many-{n:02}");
        let id = api.create_ready(&name, json!({})).await;
        // many-00 has the oldest entry, many-50 the newest.
        let at = base + SignedDuration::from_secs(n);
        store_lines(&daemon, id, &format!("{name} says"), 1, at);
    }

    let preview = preview(&api).await;

    let logs: Vec<&str> = preview
        .entries
        .iter()
        .filter(|entry| entry.kind == BundleEntryKind::InstanceLog)
        .map(|entry| entry.path.as_str())
        .collect();
    assert_eq!(logs.len(), 50, "{logs:?}");
    assert!(
        !logs.contains(&"logs/instances/many-00.log"),
        "the oldest log is left out: {logs:?}"
    );
    for n in 1..51 {
        let path = format!("logs/instances/many-{n:02}.log");
        assert!(logs.contains(&path.as_str()), "{path} in {logs:?}");
    }
    let entries = staged_entries(&site, &preview);
    assert!(!entries.contains_key("logs/instances/many-00.log"));
    assert!(
        entries["README.txt"].contains("logs/instances/many-00.log (past the first 50 instances)"),
        "{}",
        entries["README.txt"]
    );
    assert!(entries["logs/instances/many-50.log"].contains("many-50 says 0000"));
    drop(api);
    stop(daemon).await;
    ev.pass();
}

/// A readiness endpoint of this test on `127.0.0.1:<port>`: it answers
/// `200 OK` until [`Self::leak`], then a status line that starts with the
/// credential. Stopped and joined on drop.
struct ReadinessResponder {
    port: u16,
    stop: Arc<AtomicBool>,
    leaking: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl ReadinessResponder {
    fn start(port: u16, credential: &str) -> Self {
        let listener =
            TcpListener::bind((Ipv4Addr::LOCALHOST, port)).expect("the test listens on the port");
        let stop = Arc::new(AtomicBool::new(false));
        let leaking = Arc::new(AtomicBool::new(false));
        let leak_line = format!("{credential} is the readiness status\r\n\r\n");
        let (thread_stop, thread_leaking) = (Arc::clone(&stop), Arc::clone(&leaking));
        let thread = std::thread::spawn(move || {
            for stream in listener.incoming() {
                if thread_stop.load(Ordering::SeqCst) {
                    return;
                }
                let Ok(mut stream) = stream else { continue };
                let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                let mut request = Vec::new();
                let mut buffer = [0_u8; 1024];
                while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                    match stream.read(&mut buffer) {
                        Ok(0) | Err(_) => break,
                        Ok(read) => request.extend_from_slice(&buffer[..read]),
                    }
                }
                let answer = if thread_leaking.load(Ordering::SeqCst) {
                    leak_line.as_str()
                } else {
                    "HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                };
                let _ = stream.write_all(answer.as_bytes());
            }
        });
        Self {
            port,
            stop,
            leaking,
            thread: Some(thread),
        }
    }

    fn leak(&self) {
        self.leaking.store(true, Ordering::SeqCst);
    }
}

impl Drop for ReadinessResponder {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect((Ipv4Addr::LOCALHOST, self.port));
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// The readiness probe of the fake package moved to a `probe` endpoint, so
/// this test can answer it.
const PROBE_ENDPOINT: &str = "    probe: { transport: tcp, purpose: monitoring, defaultHost: \"127.0.0.1\", defaultPort: 18090 }\n";

// covers: AC-14, AC-22 (spec 0034)
#[tokio::test(flavor = "multi_thread")]
async fn a_credential_in_a_readiness_detail_reaches_neither_the_report_nor_the_bundle_nor_the_daemon_log()
 {
    let ev = evidence!(
        covers = ["REQ-019", "NFR-016", "AC-016"],
        kind = Kind::Fixture,
        expected = "spec 0034 AC-14: a readiness endpoint that answers a status line holding the fixture credential the start learned turns a running instance not_ready with a redacted detail; doctor.run reports not_ready with that detail, and neither the report, the preview, any decompressed entry of the exported bundle, nor the daemon log written during the run holds any form of the credential",
    );
    support::install_logs();
    let (_scratch, package) = support::edited_fake_package(|dir| {
        let path = dir.join("provider.yaml");
        let text = std::fs::read_to_string(&path).expect("the manifest reads");
        let http = text
            .lines()
            .find(|line| line.trim_start().starts_with("http: { transport: tcp"))
            .expect("the manifest has the http endpoint")
            .to_owned();
        let text = text
            .replacen(
                &format!("{http}\n"),
                &format!("{http}\n{PROBE_ENDPOINT}"),
                1,
            )
            .replacen(
                "probe: { type: http, endpoint: http, path: /ready }",
                "probe: { type: http, endpoint: probe, path: /ready }",
                1,
            );
        assert!(text.contains("endpoint: probe, path: /ready"), "{text}");
        std::fs::write(&path, text).expect("the manifest writes");
    });
    let site = Site::with_package(&package);
    let daemon = site.start_with(support::fast()).await;
    let api = site.api().await;
    let credential = FixtureCredential::new();
    let probe_port = allocate_port().expect("a free port");
    let mut params = api.params("probed", json!({}));
    params.endpoints = vec![EndpointRequest {
        name: "probe".to_owned(),
        port: PortRequest::Fixed(probe_port),
    }];
    params.secrets = vec![SecretFieldInput {
        field: "authToken".to_owned(),
        input: SecretInput::Value(SecretText::new(credential.value().to_owned())),
    }];
    let accepted = api
        .create(params)
        .await
        .expect("the create is accepted")
        .accepted
        .expect("a create starts an operation");
    api.wait_state(accepted.operation.id, OperationState::Succeeded)
        .await;
    let id: Uuid = accepted
        .operation
        .resource
        .as_ref()
        .map(|resource| resource.id.parse().expect("an instance id"))
        .expect("the operation names the instance");
    let started = api
        .start(id)
        .await
        .expect("the start is accepted")
        .accepted
        .expect("a start begins an operation")
        .operation
        .id;
    // Listen only once the process runs, so the start's port check of the
    // probe endpoint found the port free; the readiness probe retries.
    api.wait_instance(id, "a process", |summary| summary.process.is_some())
        .await;
    let responder = ReadinessResponder::start(probe_port, credential.value());
    let done = api.wait_final(started).await;
    assert_eq!(done.operation.state, OperationState::Succeeded, "{done:?}");
    api.wait_instance(id, "running and ready", |summary| {
        summary.lifecycle == InstanceLifecycle::Running
            && summary.readiness.state == InstanceReadinessState::Ready
    })
    .await;

    responder.leak();
    let summary = api
        .wait_instance(id, "not ready", |summary| {
            summary.readiness.state == InstanceReadinessState::NotReady
        })
        .await;
    let detail = summary
        .readiness
        .detail
        .clone()
        .expect("a not ready readiness has a detail");
    assert!(
        detail.contains("invalid status line") && detail.contains("****"),
        "the detail holds the redacted status line: {detail}"
    );
    credential.assert_absent(&detail, "the readiness detail");

    let report = doctor(&api, DoctorParams::default()).await;
    let failures = check(&report, DoctorCheckId::InstanceFailures);
    let finding = failures
        .findings
        .iter()
        .find(|finding| finding.code == DoctorFindingCode::NotReady)
        .expect("not_ready is reported");
    assert_eq!(finding.severity, DoctorSeverity::Warning);
    let shown = serde_json::to_string(finding).expect("a finding serializes");
    assert!(
        shown.contains("invalid status line") && shown.contains("****"),
        "the finding carries the redacted detail: {shown}"
    );
    credential.assert_absent(
        &serde_json::to_string(&report).expect("the report serializes"),
        "the doctor report",
    );

    let preview = preview(&api).await;
    credential.assert_absent(
        &serde_json::to_string(&preview).expect("the preview serializes"),
        "the preview",
    );
    let out = site.root.path().join("out");
    std::fs::create_dir(&out).expect("the export folder can be made");
    let target = out.join("probed.zip");
    match api
        .client
        .call(Call::DiagnosticsExportBundle(ExportBundleParams {
            preview_id: preview.preview_id,
            destination: target.display().to_string(),
            replace: false,
        }))
        .await
        .expect("the export succeeds")
    {
        Reply::DiagnosticsExportBundle(_) => {}
        other => panic!("not an export reply: {other:?}"),
    }
    let entries =
        stackly_core::diagnostics::testing::unzip(&std::fs::read(&target).expect("it is written"));
    for (path, bytes) in &entries {
        credential.assert_absent(&String::from_utf8_lossy(bytes), path);
    }
    let instances = String::from_utf8_lossy(&entries["instances.json"]).into_owned();
    assert!(
        instances.contains("invalid status line") && instances.contains("****"),
        "instances.json holds the redacted readiness detail"
    );
    let doctor_json = String::from_utf8_lossy(&entries["doctor.json"]).into_owned();
    assert!(doctor_json.contains("not_ready"), "{doctor_json}");

    drop(responder);
    drop(api);
    stop(daemon).await;
    credential.assert_absent(&support::log_text(), "the daemon log");
    ev.pass();
}

/// A port `p` with `p + 1` and `p + 2` free as well on the loopback, from
/// 30000 to 38999: below the ephemeral range, where `allocate_port` and the
/// automatic ports of the instances other tests start come from, and away
/// from the default ports of the fake package.
fn three_free_ports() -> u16 {
    let seed = std::process::id()
        ^ std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("the clock is after 1970")
            .subsec_nanos();
    for attempt in 0..300_u32 {
        let offset = (seed.wrapping_add(attempt.wrapping_mul(7919))) % 9000;
        let port = 30_000 + u16::try_from(offset).expect("an offset below 9000 fits");
        let last = port + 2;
        if (port..=last)
            .all(|candidate| TcpListener::bind((Ipv4Addr::LOCALHOST, candidate)).is_ok())
        {
            return port;
        }
    }
    panic!("no three free ports in a row on the loopback");
}

async fn create_on(api: &Api, name: &str, port: u16) -> Uuid {
    let mut params = api.params(name, json!({}));
    params.endpoints = vec![Api::http_port(port)];
    let accepted = api
        .create(params)
        .await
        .expect("the create is accepted")
        .accepted
        .expect("a create starts an operation");
    api.wait_state(accepted.operation.id, OperationState::Succeeded)
        .await;
    accepted
        .operation
        .resource
        .as_ref()
        .map(|resource| resource.id.parse().expect("an instance id"))
        .expect("the operation names the instance")
}

async fn change_http_port(
    api: &Api,
    id: Uuid,
    port: u16,
    dry_run: bool,
) -> Result<ChangeEndpointsReply, Box<ErrorBody>> {
    match api
        .client
        .call_detailed(
            Call::InstanceChangeEndpoints(ChangeEndpointsParams {
                instance_id: id,
                base_revision: ConfigRevisionNumber::new(1).expect("a revision number"),
                endpoints: vec![Api::http_port(port)],
                dry_run,
                override_leases: false,
            }),
            CallOptions::default().with_idempotency_key(support::key()),
        )
        .await?
    {
        Reply::InstanceChangeEndpoints(reply) => Ok(*reply),
        other => panic!("not a change endpoints reply: {other:?}"),
    }
}

fn port_finding(report: &DoctorReport, id: Uuid) -> &DoctorFinding {
    check(report, DoctorCheckId::Endpoints)
        .findings
        .iter()
        .find(|finding| {
            finding.code == DoctorFindingCode::PortInUse
                && finding.instance.as_ref().map(|instance| instance.id) == Some(id)
        })
        .unwrap_or_else(|| panic!("port_in_use for {id} in {report:#?}"))
}

/// The port the `change_port` action of `finding` suggests.
fn suggestion(finding: &DoctorFinding) -> u16 {
    let action = finding.action.as_ref().expect("a change_port action");
    assert_eq!(action.kind, DoctorActionKind::ChangePort);
    assert_eq!(action.endpoint.as_deref(), Some("http"));
    action.port.expect("a suggested port")
}

// covers: AC-5 (spec 0034)
#[tokio::test(flavor = "multi_thread")]
async fn two_port_conflicts_in_one_report_each_get_a_suggestion_and_only_the_first_applies() {
    let ev = evidence!(
        covers = ["REQ-019"],
        kind = Kind::Fixture,
        expected = "spec 0034 AC-5: two stopped instances on ports p and p+1 that this test holds each get port_in_use with their own change_port action, and the doctor suggests the same first free port for both; applying the first works, a dry run of the second with that port is refused as an endpoint conflict with another suggestion, the doctor moved no port, and the next run suggests the second instance a new port",
    );
    let site = Site::new();
    let daemon = site.start().await;
    let api = site.api().await;
    let port = three_free_ports();
    let first = create_on(&api, "port-a", port).await;
    let second = create_on(&api, "port-b", port + 1).await;
    let _held = [
        TcpListener::bind((Ipv4Addr::LOCALHOST, port)).expect("the test holds p"),
        TcpListener::bind((Ipv4Addr::LOCALHOST, port + 1)).expect("the test holds p + 1"),
    ];

    let report = doctor(
        &api,
        DoctorParams {
            instance_id: None,
            checks: vec![DoctorCheckId::Endpoints],
        },
    )
    .await;
    let (a, b) = (port_finding(&report, first), port_finding(&report, second));
    assert_eq!(a.severity, DoctorSeverity::Warning, "a stopped instance");
    assert_eq!(b.severity, DoctorSeverity::Warning, "a stopped instance");
    let (for_a, for_b) = (suggestion(a), suggestion(b));
    // The search skips the other instance's port, not another finding's
    // suggestion, so both name the first free port after p + 1.
    assert!(for_a > port + 1, "{for_a}");
    assert_eq!(for_a, for_b, "each finding is judged on its own");
    for finding in [a, b] {
        assert!(
            finding
                .details
                .iter()
                .any(|detail| detail.key == "suggestedPort" && detail.value == for_a.to_string()),
            "{:?}",
            finding.details
        );
    }
    for (id, own) in [(first, port), (second, port + 1)] {
        let summary = api.get(id).await.expect("the instance reads");
        assert_eq!(
            summary.endpoints[0].port,
            Some(own),
            "the doctor moved no port"
        );
    }

    let ChangeEndpointsReply::Applied(applied) = change_http_port(&api, first, for_a, false)
        .await
        .expect("the first suggestion applies")
    else {
        panic!("not an applied reply");
    };
    let operation = applied
        .accepted
        .expect("a port change begins an operation")
        .operation
        .id;
    let done = api.wait_final(operation).await;
    assert_eq!(done.operation.state, OperationState::Succeeded, "{done:?}");

    let ChangeEndpointsReply::DryRun(dry) = change_http_port(&api, second, for_b, true)
        .await
        .expect("a dry run answers")
    else {
        panic!("not a dry run");
    };
    assert!(!dry.valid, "{dry:?}");
    assert_eq!(dry.problems[0].code, "supervisor.endpoint_conflict");
    let other = dry.problems[0]
        .suggested_port
        .expect("the dry run suggests another port");
    assert_ne!(other, for_b);
    assert!(dry.changes.is_empty(), "{dry:?}");
    let summary = api.get(second).await.expect("the instance reads");
    assert_eq!(summary.endpoints[0].port, Some(port + 1), "nothing moved");

    let again = doctor(
        &api,
        DoctorParams {
            instance_id: Some(second),
            checks: vec![DoctorCheckId::Endpoints],
        },
    )
    .await;
    let renewed = suggestion(port_finding(&again, second));
    assert_ne!(renewed, for_b, "the taken suggestion is not offered again");
    assert!(
        check(&again, DoctorCheckId::Endpoints)
            .findings
            .iter()
            .all(|finding| finding.instance.as_ref().map(|instance| instance.id) != Some(first)),
        "the scope keeps the other instance out"
    );
    drop(api);
    stop(daemon).await;
    ev.pass();
}
