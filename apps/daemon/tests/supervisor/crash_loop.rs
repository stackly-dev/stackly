//! A crash loop made visible through the real daemon (spec 0033,
//! milestone 1): the six spawns of both crash loop paths and the pauses
//! between them in the durable log markers (AC-1), the stage `backoff`
//! with the next attempt's time (AC-2), the time and exit of the last
//! failure until a daemon restart (AC-3), the failure envelope with its
//! operation ID, static hint, and log line (AC-5), a stop or cancel during
//! a pause, nothing repaired on its own (AC-14), and a credential the fake
//! prints that never leaves the daemon (AC-15).

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use serde_json::json;
use stackly_protocol::events::{EventKind, ShutdownReason, SubscribeParams};
use stackly_protocol::instance::config::GetConfigParams;
use stackly_protocol::instance::{
    EndpointPortMode, InstanceLifecycle, InstanceSummary, SecretFieldInput,
};
use stackly_protocol::json::DecodedFrame;
use stackly_protocol::logs::{LogEntry, LogEntryKind, LogPage, LogsReadParams};
use stackly_protocol::operation::{
    CancelOperationParams, ListOperationsParams, OperationDetail, OperationKind, OperationState,
    OperationSummary, TriggerOrigin,
};
use stackly_protocol::secret::{SecretInput, SecretText};
use stackly_protocol::{Call, Reply};
use stackly_testkit::evidence::{FixtureCredential, Kind, evidence};
use stacklyd::server::DaemonConfig;
use uuid::Uuid;

use crate::support::{Api, PATIENCE, Site, fast, install_logs, log_text, serving};

/// The pause of the first respawn in these tests; the next ones double.
const BASE: Duration = Duration::from_millis(20);
/// The longest pause in these tests.
const MAX: Duration = Duration::from_millis(400);
/// The exit code of every crash of the fake.
const EXIT_CODE: i32 = 3;
/// The static hint of `supervisor.retry_budget_exhausted` (spec 0033).
const BUDGET_HINT: &str = "It kept crashing, so Stackly stopped restarting it. Read its logs, fix the cause, then start it again.";

/// The fast daemon config with the restart pauses of these tests.
fn config(base: Duration, max: Duration) -> DaemonConfig {
    let mut config = fast();
    config.supervisor.restart_base_delay = base;
    config.supervisor.restart_max_delay = max;
    config
}

/// The fake settings of a process that crashes `after_ms` after each
/// spawn, before it is ready.
fn crashes_before_ready(after_ms: u64) -> serde_json::Value {
    json!({
        "crashAfterMs": after_ms,
        "crashExitCode": EXIT_CODE,
        "readyDelayMs": 60_000,
    })
}

pub(crate) async fn started_operation(api: &Api, id: Uuid) -> Uuid {
    api.start(id)
        .await
        .expect("the start is accepted")
        .accepted
        .expect("a start of a stopped instance begins an operation")
        .operation
        .id
}

/// Every entry of the instance's durable log, oldest first.
async fn log_entries(api: &Api, id: Uuid) -> Vec<LogEntry> {
    let params = LogsReadParams {
        instance: id,
        streams: Vec::new(),
        contains: None,
        position: None,
        limit: Some(1000),
    };
    let reply = api
        .client
        .call(Call::InstanceLogsRead(params))
        .await
        .expect("the log reads");
    let Reply::InstanceLogsRead(page) = reply else {
        panic!("not a logs_read reply: {reply:?}");
    };
    let page: LogPage = *page;
    assert!(page.earlier.is_none(), "one page holds the whole log");
    page.entries
}

/// The run markers of the log: `true` for `started`, `false` for
/// `exited`, with the time the daemon saw each.
pub(crate) async fn markers(api: &Api, id: Uuid) -> Vec<(bool, jiff::Timestamp)> {
    log_entries(api, id)
        .await
        .into_iter()
        .filter_map(|entry| match entry.kind {
            LogEntryKind::Started(_) => Some((true, entry.at.as_jiff())),
            LogEntryKind::Exited(_) => Some((false, entry.at.as_jiff())),
            _ => None,
        })
        .collect()
}

/// Waits until the log holds `count` run markers.
async fn wait_markers(api: &Api, id: Uuid, count: usize) -> Vec<(bool, jiff::Timestamp)> {
    let started = std::time::Instant::now();
    loop {
        let found = markers(api, id).await;
        if found.len() >= count {
            return found;
        }
        assert!(
            started.elapsed() < PATIENCE,
            "only {} of {count} markers: {found:?}",
            found.len()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// Checks the markers of `spawns` runs: they alternate `started`,
/// `exited`, so two main processes never ran at once, and the pause from
/// the exit of run n to the spawn of run n+1 is at least the shortest
/// backoff of respawn n (half of `BASE × 2^(n−1)`, at most `MAX`).
fn assert_runs_in_turn(markers: &[(bool, jiff::Timestamp)], spawns: usize) {
    let kinds: Vec<bool> = markers.iter().map(|(started, _)| *started).collect();
    let expected: Vec<bool> = (0..spawns).flat_map(|_| [true, false]).collect();
    assert_eq!(
        kinds, expected,
        "every exited comes before the next started"
    );
    for respawn in 1..spawns {
        let exited = markers[2 * respawn - 1].1;
        let started = markers[2 * respawn].1;
        let full = (BASE * 2_u32.pow(u32::try_from(respawn - 1).expect("a small count"))).min(MAX);
        // The markers keep milliseconds, so allow one for the rounding.
        let shortest = (full / 2).saturating_sub(Duration::from_millis(1));
        let pause = started.duration_since(exited);
        assert!(
            pause >= jiff::SignedDuration::try_from(shortest).expect("a short pause"),
            "respawn {respawn} came {pause:?} after the exit, under {shortest:?}"
        );
    }
}

/// The start operations of the instance, newest first.
async fn start_operations(api: &Api, id: Uuid) -> Vec<OperationSummary> {
    let reply = api
        .client
        .call(Call::OperationList(ListOperationsParams {
            resource: Some(stackly_protocol::ResourceRef {
                kind: stackly_protocol::ResourceKind::Instance,
                id: id.to_string(),
            }),
            limit: Some(50),
            ..ListOperationsParams::default()
        }))
        .await
        .expect("the operations list");
    let Reply::OperationList(page) = reply else {
        panic!("not an operation.list reply: {reply:?}");
    };
    page.items
        .into_iter()
        .filter(|summary| summary.kind == OperationKind::StartInstance)
        .collect()
}

/// The details of the failed operation's error, as text pairs.
pub(crate) fn error_details(detail: &OperationDetail) -> Vec<(String, String)> {
    detail
        .operation
        .failure
        .as_ref()
        .expect("the failed operation says why")
        .error
        .details()
        .iter()
        .map(|detail| (detail.key().to_owned(), detail.value().to_owned()))
        .collect()
}

/// What a failure must never change: the port requests and the config
/// revisions the registry holds, every file of the data folder, the
/// instance lock, and the names in the run folder. Left out are what every
/// run writes by design: the spools and logs, the `argv-serve.json` the
/// fake records at each spawn, and the port an `Auto` endpoint is bound
/// to at its spawn.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Untouched {
    endpoints: Vec<(String, Option<u16>, String)>,
    config_revision: String,
    data: BTreeMap<String, Vec<u8>>,
    lock: Option<(u64, Vec<u8>)>,
    run: Vec<String>,
}

fn files(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut found = BTreeMap::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(next) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&next) else {
            continue;
        };
        for entry in entries {
            let path = entry.expect("a folder entry").path();
            if path.is_dir() {
                pending.push(path);
            } else {
                let relative = path
                    .strip_prefix(dir)
                    .expect("the file is under the folder")
                    .display()
                    .to_string();
                found.insert(relative, std::fs::read(&path).unwrap_or_default());
            }
        }
    }
    found
}

pub(crate) async fn untouched(site: &Site, api: &Api, id: Uuid) -> Untouched {
    untouched_at(&site.instance_paths(id), api, id).await
}

/// [`untouched`] for an instance whose folders are `paths`, for a root that
/// the `stacklyd` binary serves rather than a [`Site`].
pub(crate) async fn untouched_at(
    paths: &stackly_platform::InstancePaths,
    api: &Api,
    id: Uuid,
) -> Untouched {
    let summary = api.get(id).await.expect("the instance reads");
    let config = match api
        .client
        .call(Call::InstanceConfigGet(GetConfigParams { instance_id: id }))
        .await
        .expect("the config can be read")
    {
        Reply::InstanceConfigGet(view) => *view,
        other => panic!("not a config reply: {other:?}"),
    };
    let lock = std::fs::metadata(&paths.lock).ok().map(|metadata| {
        use std::os::unix::fs::MetadataExt;
        (
            metadata.ino(),
            std::fs::read(&paths.lock).unwrap_or_default(),
        )
    });
    // The spools and the log files are written by every run; the names
    // of everything else in the run folder must stay.
    let run = files(&paths.run)
        .into_keys()
        .filter(|name| !name.ends_with(".spool") && !name.contains("log"))
        .collect();
    Untouched {
        endpoints: summary
            .endpoints
            .iter()
            .map(|endpoint| {
                let fixed = endpoint.port_mode == EndpointPortMode::Fixed;
                (
                    endpoint.name.clone(),
                    endpoint.port.filter(|_| fixed),
                    format!("{:?}", endpoint.port_mode),
                )
            })
            .collect(),
        config_revision: serde_json::to_string(&(&config.revision, &config.history))
            .expect("the config revisions encode"),
        data: files(&paths.data)
            .into_iter()
            .filter(|(name, _)| name != "argv-serve.json")
            .collect(),
        lock,
        run,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_start_that_crashes_before_ready_spawns_six_times_then_fails_with_its_operation_id() {
    let ev = evidence!(
        covers = ["AC-068", "NFR-011"],
        kind = Kind::Fixture,
        expected = "a fake that crashes 50 ms after each spawn, before ready, is spawned six times with every exit before the next start and every pause at least half its backoff, then ends failed with crash_loop, an exhausted budget, the time and exit code 3 of the sixth crash, and a failed start whose error carries attempts 6, lastExitCode 3, the static hint, and the operation ID that the daemon log line of the failure names too",
    );
    install_logs();
    let site = Site::new();
    let daemon = site.start_with(config(BASE, MAX)).await;
    let api = site.api().await;
    let id = api.create_ready("crashy", crashes_before_ready(50)).await;
    let before = untouched(&site, &api, id).await;

    let operation = started_operation(&api, id).await;
    let detail = api.wait_final(operation).await;

    assert_eq!(detail.operation.state, OperationState::Failed);
    assert_eq!(
        Api::failure_code(&detail),
        "supervisor.retry_budget_exhausted"
    );
    let error = &detail.operation.failure.as_ref().expect("a failure").error;
    assert_eq!(error.operation_id(), Some(operation), "the correlation ID");
    assert_eq!(error.hint(), Some(BUDGET_HINT));
    let details = error_details(&detail);
    assert!(
        details.contains(&("attempts".to_owned(), "6".to_owned())),
        "{details:?}"
    );
    assert!(
        details.contains(&("lastExitCode".to_owned(), EXIT_CODE.to_string())),
        "{details:?}"
    );

    let failed = api.get(id).await.expect("the instance reads");
    assert_eq!(failed.lifecycle, InstanceLifecycle::Failed);
    assert!(failed.restart.exhausted);
    assert!(failed.process.is_none());
    let failure = failed
        .last_failure
        .clone()
        .expect("a failed instance says why");
    assert_eq!(failure.code, "crash_loop");
    assert_eq!(failure.exit_code, Some(EXIT_CODE));
    assert_eq!(failure.signal, None);
    let at = failure.at.expect("the time of the sixth crash").as_jiff();

    let runs = wait_markers(&api, id, 12).await;
    assert_runs_in_turn(&runs, 6);
    let last_exit = runs.last().expect("the sixth exit").1;
    assert!(
        at >= last_exit,
        "the failure is recorded after the exit it follows: {at} before {last_exit}"
    );
    assert!(serving(&site.instance_paths(id).data).is_empty());

    // AC-14: a crash loop repairs nothing.
    assert_eq!(untouched(&site, &api, id).await, before);
    tokio::time::sleep(BASE * 4).await;
    assert_eq!(
        markers(&api, id).await.len(),
        12,
        "no spawn after the budget"
    );

    // AC-5: the daemon log line of the failure names the same operation.
    let log = log_text();
    let id_text = operation.to_string();
    assert!(
        log.lines()
            .any(|line| line.contains("operation work failed")
                && line.contains(&id_text)
                && line.contains("supervisor.retry_budget_exhausted")),
        "no log line of the failure names {id_text}"
    );
    drop(api);
    daemon
        .shutdown(ShutdownReason::Requested, std::future::pending())
        .await;
    ev.pass();
}

#[tokio::test(flavor = "multi_thread")]
async fn every_pause_inside_the_start_is_the_backoff_stage_with_the_next_attempt_time() {
    let ev = evidence!(
        covers = ["AC-068"],
        kind = Kind::Fixture,
        expected = "each of the five pauses of a start that crashes before ready enters the stage backoff once, an instance.get read right after shows nextAttemptAt in the future and attempts equal to the respawn waited for, and the next attempt enters lock again",
    );
    let site = Site::new();
    // Pauses long enough that a read after the event lands inside them.
    let daemon = site
        .start_with(config(
            Duration::from_millis(600),
            Duration::from_millis(900),
        ))
        .await;
    let api = site.api().await;
    let (_, mut events) = api
        .client
        .subscribe(SubscribeParams::default())
        .await
        .expect("the subscription is accepted");
    let id = api.create_ready("backoff", crashes_before_ready(50)).await;

    let operation = started_operation(&api, id).await;
    let mut stages: Vec<String> = Vec::new();
    let mut seen = Vec::new();
    let deadline = tokio::time::Instant::now() + PATIENCE;
    loop {
        let frame = tokio::time::timeout_at(deadline, events.next())
            .await
            .expect("the start ends in time")
            .expect("the subscription stays open")
            .expect("a frame");
        let DecodedFrame::Event(event) = frame else {
            continue;
        };
        let (EventKind::OperationProgress(summary) | EventKind::OperationStateChanged(summary)) =
            event.kind
        else {
            continue;
        };
        if summary.id != operation {
            continue;
        }
        if let Some(stage) = summary.stage.clone()
            && stages.last() != Some(&stage)
        {
            if stage == "backoff" {
                let read = api.get(id).await.expect("the instance reads");
                seen.push(read.restart.clone());
                let next = read
                    .restart
                    .next_attempt_at
                    .expect("backoff is entered after nextAttemptAt is shown")
                    .as_jiff();
                assert!(next > jiff::Timestamp::now(), "the next attempt is ahead");
                assert_eq!(read.lifecycle, InstanceLifecycle::Recovering);
            }
            stages.push(stage);
        }
        if summary.state.is_final() {
            break;
        }
    }

    let attempts: Vec<u32> = seen.iter().map(|restart| restart.attempts).collect();
    assert_eq!(attempts, [1, 2, 3, 4, 5], "respawn n of the budget");
    assert!(seen.iter().all(|restart| restart.budget == 5));
    let after: Vec<&str> = stages
        .windows(2)
        .filter(|pair| pair[0] == "backoff")
        .map(|pair| pair[1].as_str())
        .collect();
    assert_eq!(
        after, ["lock"; 5],
        "each attempt enters lock again: {stages:?}"
    );
    drop((api, events));
    daemon
        .shutdown(ShutdownReason::Requested, std::future::pending())
        .await;
    ev.pass();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_process_that_crashes_after_ready_ends_failed_after_six_spawns_of_system_respawns() {
    let ev = evidence!(
        covers = ["AC-068"],
        kind = Kind::Fixture,
        expected = "a fake that becomes ready and crashes 700 ms after each spawn is spawned six times, every respawn is a system start_instance that succeeds and never pauses inside itself, and the sixth crash ends the instance failed with crash_loop, an exhausted budget, and its exit code, without a failed operation",
    );
    let site = Site::new();
    let daemon = site.start_with(config(BASE, MAX)).await;
    let api = site.api().await;
    let id = api
        .create_ready(
            "flaky",
            json!({ "crashAfterMs": 700, "crashExitCode": EXIT_CODE, "readyDelayMs": 0 }),
        )
        .await;

    let operation = started_operation(&api, id).await;
    let first = api.wait_final(operation).await;
    assert_eq!(first.operation.state, OperationState::Succeeded);
    let failed = api
        .wait_instance(id, "failed with an exhausted budget", |summary| {
            summary.lifecycle == InstanceLifecycle::Failed && summary.restart.exhausted
        })
        .await;

    let failure = failed.last_failure.expect("a failed instance says why");
    assert_eq!(failure.code, "crash_loop");
    assert_eq!(failure.exit_code, Some(EXIT_CODE));
    assert!(failure.at.is_some());
    let runs = wait_markers(&api, id, 12).await;
    assert_runs_in_turn(&runs, 6);
    let starts = start_operations(&api, id).await;
    let respawns: Vec<_> = starts
        .iter()
        .filter(|summary| summary.id != operation)
        .collect();
    assert_eq!(
        respawns.len(),
        5,
        "one system start per respawn: {starts:?}"
    );
    for respawn in &respawns {
        let detail = api.wait_final(respawn.id).await;
        assert_eq!(
            detail.operation.state,
            OperationState::Succeeded,
            "the respawn became ready"
        );
        assert_eq!(
            detail.operation.trigger.origin,
            TriggerOrigin::Background,
            "the restart policy starts it"
        );
        assert!(
            detail.steps.iter().all(|step| step.name != "journal"),
            "a respawn writes no journal step"
        );
    }
    assert!(
        starts
            .iter()
            .all(|summary| summary.state != OperationState::Failed),
        "the sixth crash came after ready, so no operation failed"
    );
    drop(api);
    daemon
        .shutdown(ShutdownReason::Requested, std::future::pending())
        .await;
    ev.pass();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stop_during_the_actor_pause_ends_stopped_and_keeps_the_last_failure() {
    let ev = evidence!(
        covers = ["AC-068"],
        kind = Kind::Fixture,
        expected = "after a ready fake crashes, the instance is recovering with nextAttemptAt set and the crash's time and exit code; a stop during that pause ends it stopped without a respawn and the last failure keeps its code, time, and exit code",
    );
    let site = Site::new();
    let daemon = site
        .start_with(config(Duration::from_secs(30), Duration::from_secs(30)))
        .await;
    let api = site.api().await;
    let id = api
        .create_ready(
            "pausing",
            json!({ "crashAfterMs": 700, "crashExitCode": EXIT_CODE, "readyDelayMs": 0 }),
        )
        .await;
    let operation = started_operation(&api, id).await;
    assert_eq!(
        api.wait_final(operation).await.operation.state,
        OperationState::Succeeded
    );
    let waiting = api
        .wait_instance(id, "recovering with a next attempt", |summary| {
            summary.lifecycle == InstanceLifecycle::Recovering
                && summary.restart.next_attempt_at.is_some()
        })
        .await;
    let crashed = waiting.last_failure.clone().expect("the crash is shown");
    assert_eq!(crashed.code, "process_exited");
    assert_eq!(crashed.exit_code, Some(EXIT_CODE));
    assert!(crashed.at.is_some());
    assert_eq!(waiting.restart.attempts, 1);

    let stop = api
        .stop(id)
        .await
        .expect("the stop is accepted")
        .accepted
        .expect("a stop of a recovering instance begins an operation");
    assert_eq!(
        api.wait_final(stop.operation.id).await.operation.state,
        OperationState::Succeeded
    );
    let stopped = api.get(id).await.expect("the instance reads");
    assert_eq!(stopped.lifecycle, InstanceLifecycle::Stopped);
    assert_eq!(stopped.restart.next_attempt_at, None);
    assert_eq!(
        stopped.last_failure,
        Some(stackly_protocol::instance::InstanceLastFailure {
            excerpt: stopped
                .last_failure
                .as_ref()
                .map(|failure| failure.excerpt.clone())
                .unwrap_or_default(),
            ..crashed
        }),
        "the stop keeps the facts of the crash"
    );
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(wait_markers(&api, id, 2).await.len(), 2, "no respawn");
    drop(api);
    daemon
        .shutdown(ShutdownReason::Requested, std::future::pending())
        .await;
    ev.pass();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_cancel_in_the_backoff_stage_ends_the_start_cancelled_without_another_spawn() {
    let ev = evidence!(
        covers = ["AC-068"],
        kind = Kind::Fixture,
        expected = "a start of a fake that crashes before ready is in the stage backoff after the first crash; a cancel ends it cancelled and the instance stopped with one spawn and its last failure kept",
    );
    let site = Site::new();
    let daemon = site
        .start_with(config(Duration::from_secs(30), Duration::from_secs(30)))
        .await;
    let api = site.api().await;
    let id = api
        .create_ready("cancelled", crashes_before_ready(50))
        .await;

    let operation = started_operation(&api, id).await;
    let started = std::time::Instant::now();
    while api.operation(operation).await.operation.stage.as_deref() != Some("backoff") {
        assert!(started.elapsed() < PATIENCE, "the start never paused");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let reply = api
        .client
        .call(Call::OperationCancel(CancelOperationParams {
            operation_id: operation,
        }))
        .await
        .expect("the cancel is accepted");
    assert!(matches!(reply, Reply::OperationCancel(_)));
    let cancelled = api.wait_final(operation).await;

    assert_eq!(cancelled.operation.state, OperationState::Cancelled);
    let stopped = api.get(id).await.expect("the instance reads");
    assert_eq!(stopped.lifecycle, InstanceLifecycle::Stopped);
    assert_eq!(stopped.restart.next_attempt_at, None);
    assert_eq!(
        stopped
            .last_failure
            .as_ref()
            .and_then(|failure| failure.exit_code),
        Some(EXIT_CODE)
    );
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(markers(&api, id).await.len(), 2, "one run only");
    drop(api);
    daemon
        .shutdown(ShutdownReason::Requested, std::future::pending())
        .await;
    ev.pass();
}

#[tokio::test(flavor = "multi_thread")]
async fn after_a_daemon_restart_the_last_failure_keeps_its_code_without_time_or_exit() {
    let site = Site::new();
    let daemon = site.start_with(config(BASE, MAX)).await;
    let api = site.api().await;
    let id = api
        .create_ready("forgetful", crashes_before_ready(50))
        .await;
    let operation = started_operation(&api, id).await;
    api.wait_final(operation).await;
    let before = api.get(id).await.expect("the instance reads");
    let failure = before.last_failure.expect("the crash loop is shown");
    assert_eq!(failure.code, "crash_loop");
    assert!(failure.at.is_some() && failure.exit_code.is_some());
    drop(api);
    daemon
        .shutdown(ShutdownReason::Requested, std::future::pending())
        .await;

    let daemon = site.start_with(config(BASE, MAX)).await;
    let api = site.api().await;
    let loaded = api.get(id).await.expect("the loaded instance reads");
    let failure = loaded.last_failure.expect("the stored failure stays");
    assert_eq!(loaded.lifecycle, InstanceLifecycle::Failed);
    assert_eq!(failure.code, "crash_loop");
    assert_eq!(
        (failure.at, failure.exit_code, failure.signal),
        (None, None, None),
        "the facts lived in the memory of the daemon before"
    );
    assert!(loaded.restart.exhausted);
    drop(api);
    daemon
        .shutdown(ShutdownReason::Requested, std::future::pending())
        .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_credential_the_fake_prints_before_it_crashes_reaches_no_failure_text() {
    let ev = evidence!(
        covers = ["AC-068", "NFR-011"],
        kind = Kind::Fixture,
        expected = "a fake that prints its fixture credential to stdout and stderr and then crashes before ready ends in a crash loop whose lastFailure excerpt, operation error message, hint, and details, and the instance.get and operation.get replies hold no trace of the credential, while the excerpt still shows output",
    );
    let site = Site::new();
    let daemon = site.start_with(config(BASE, MAX)).await;
    let api = site.api().await;
    let credential = FixtureCredential::new();
    let mut params = api.params(
        "leaky",
        json!({
            "crashAfterMs": 150,
            "crashExitCode": EXIT_CODE,
            "readyDelayMs": 60_000,
            "leakToken": true,
        }),
    );
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

    let operation = started_operation(&api, id).await;
    let detail = api.wait_final(operation).await;
    assert_eq!(
        Api::failure_code(&detail),
        "supervisor.retry_budget_exhausted"
    );
    let failed: InstanceSummary = api.get(id).await.expect("the instance reads");
    let failure = failed
        .last_failure
        .clone()
        .expect("the crash loop is shown");
    assert!(
        !failure.excerpt.is_empty(),
        "the excerpt shows the redacted output"
    );

    credential.assert_absent(&failure.excerpt.join("\n"), "the failure excerpt");
    let error = &detail.operation.failure.as_ref().expect("a failure").error;
    credential.assert_absent(error.message(), "the error message");
    credential.assert_absent(error.hint().unwrap_or(""), "the error hint");
    credential.assert_absent(
        &serde_json::to_string(error.details()).expect("the details encode"),
        "the error details",
    );
    credential.assert_absent(
        &serde_json::to_string(&failed).expect("the summary encodes"),
        "the instance.get reply",
    );
    credential.assert_absent(
        &serde_json::to_string(&detail).expect("the detail encodes"),
        "the operation.get reply",
    );
    drop(api);
    daemon
        .shutdown(ShutdownReason::Requested, std::future::pending())
        .await;
    ev.pass();
}
