//! The durable log store over the socket (spec 0027, milestone 2):
//! `instance.logs_read`, `instance.logs_follow`, and
//! `instance.logs_unfollow` against real processes of the dummy server,
//! the rotation, a store that cannot be written, and the fixture
//! credential the dummy server prints.

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::json;
use stackly_ipc::EventStream;
use stackly_protocol::codes::events;
use stackly_protocol::daemon::{SubsystemStatus, subsystem};
use stackly_protocol::events::SubscribeParams;
use stackly_protocol::json::DecodedFrame;
use stackly_protocol::logs::{
    LogAnchor, LogCursor, LogEntryKind, LogFrame, LogGapReason, LogPage, LogPosition, LogStream,
    LogsFollowParams, LogsReadParams, LogsUnfollowParams,
};
use stackly_protocol::operation::OperationState;
use stackly_protocol::{Call, ErrorBody, LogSeq, Reply};
use stackly_testkit::evidence::{Kind, evidence};
use uuid::Uuid;

use crate::support::{Api, PATIENCE, Site, TOKEN, fast};

fn params(id: Uuid) -> LogsReadParams {
    LogsReadParams {
        instance: id,
        streams: Vec::new(),
        contains: None,
        position: None,
        limit: Some(1000),
    }
}

async fn read(api: &Api, params: LogsReadParams) -> Result<LogPage, Box<ErrorBody>> {
    match api.client.call(Call::InstanceLogsRead(params)).await? {
        Reply::InstanceLogsRead(page) => Ok(*page),
        other => panic!("not a logs_read reply: {other:?}"),
    }
}

/// Reads every page from the newest back through `earlier`, and returns
/// the newest page with the entries of all of them, oldest first.
async fn read_all(api: &Api, params: LogsReadParams) -> LogPage {
    let mut page = read(api, params.clone()).await.expect("the log reads");
    let mut earlier = page.earlier;
    while let Some(cursor) = earlier {
        let mut before = params.clone();
        before.position = Some(LogPosition::Before(LogAnchor { cursor }));
        let older = read(api, before).await.expect("an older page reads");
        let mut entries = older.entries;
        entries.append(&mut page.entries);
        page.entries = entries;
        earlier = older.earlier;
    }
    page
}

/// Reads all pages until `done` holds, or panics after [`PATIENCE`].
async fn wait_page(api: &Api, params: LogsReadParams, done: impl Fn(&LogPage) -> bool) -> LogPage {
    let started = Instant::now();
    loop {
        let page = read_all(api, params.clone()).await;
        if done(&page) {
            return page;
        }
        assert!(
            started.elapsed() < PATIENCE,
            "the log never got there: {page:#?}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn texts(entries: &[stackly_protocol::logs::LogEntry]) -> Vec<String> {
    entries
        .iter()
        .filter_map(|entry| match &entry.kind {
            LogEntryKind::Line(line) => Some(line.text.clone()),
            _ => None,
        })
        .collect()
}

/// Waits for the next `log` frame of `stream`.
async fn next_frame(stream: &mut EventStream) -> LogFrame {
    let frame = tokio::time::timeout(PATIENCE, stream.next())
        .await
        .expect("a log frame in time")
        .expect("the stream reads")
        .expect("the stream is open");
    match frame {
        DecodedFrame::Log(frame) => frame,
        other => panic!("not a log frame: {other:?}"),
    }
}

/// Reads frames until one holds an entry that `found` accepts.
async fn frame_until(
    stream: &mut EventStream,
    found: impl Fn(&LogEntryKind) -> bool,
) -> Vec<LogFrame> {
    let mut frames = Vec::new();
    loop {
        let frame = next_frame(stream).await;
        let hit = frame.entries.iter().any(|entry| found(&entry.kind));
        frames.push(frame);
        if hit {
            return frames;
        }
    }
}

/// Every byte of the files in the store folder of `id`.
fn stored_bytes(site: &Site, id: Uuid) -> Vec<u8> {
    let dir = site.instance_paths(id).logs.join("captured");
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).expect("the store folder lists") {
        out.extend(std::fs::read(entry.expect("an entry").path()).expect("a store file reads"));
    }
    out
}

async fn health_of(api: &Api) -> SubsystemStatus {
    match api
        .client
        .call(Call::DaemonGetHealth)
        .await
        .expect("the health reads")
    {
        Reply::DaemonGetHealth(health) => health
            .subsystems
            .iter()
            .find(|entry| entry.name == subsystem::INSTANCE_LOGS)
            .map(|entry| entry.status)
            .expect("the subsystem is listed"),
        other => panic!("not a health reply: {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn output_is_stored_merged_and_redacted_and_a_follow_sees_stop_and_start() {
    let ev = evidence!(
        covers = ["REQ-017"],
        kind = Kind::Fixture,
        expected = "spec 0027 AC-1, AC-5 to AC-7, AC-9: the dummy server's stdout and stderr land in the store and come back merged by seq, filtered by stream and text, the fixture credential it prints (whole and across the 64 KiB cut) is in no segment and no reply, and one follow sees the exit and the next start",
    );
    let site = Site::new();
    let daemon = site.start_with(fast()).await;
    let api = site.api().await;
    let id = api.create_ready("logs", json!({ "leakToken": true })).await;
    api.start_ready(id).await;

    let page = wait_page(&api, params(id), |page| {
        let texts = texts(&page.entries);
        texts.iter().any(|text| text.starts_with("listening on"))
            && texts.iter().any(|text| text.starts_with("token is"))
            && texts.iter().filter(|text| text.contains("bbbb")).count() >= 1
    })
    .await;
    let encoded = serde_json::to_string(&page).expect("a page encodes");
    assert!(!encoded.contains(TOKEN), "the token reached a reply");
    assert!(encoded.contains("token is ****"));
    let stored = String::from_utf8_lossy(&stored_bytes(&site, id)).into_owned();
    assert!(!stored.contains(TOKEN), "the token reached a segment");
    assert!(
        !stored.contains("fixture-token"),
        "half of the token reached a segment"
    );
    assert!(matches!(page.entries[0].kind, LogEntryKind::Started(_)));
    assert!(
        page.entries
            .windows(2)
            .all(|pair| pair[0].seq < pair[1].seq)
    );
    assert_eq!(page.retention.segments_per_stream.get(), 5);

    let mut only = params(id);
    only.streams = vec![LogStream::Stdout];
    let stdout = read(&api, only).await.expect("the stdout page reads");
    assert!(
        stdout
            .entries
            .iter()
            .all(|entry| entry.stream != Some(LogStream::Stderr))
    );
    let mut grep = params(id);
    grep.contains = Some("LISTENING".to_owned());
    let found = read(&api, grep).await.expect("the filtered page reads");
    assert_eq!(texts(&found.entries).len(), 1);

    let (following, mut stream) = api
        .client
        .follow_logs(LogsFollowParams {
            instance: id,
            streams: Vec::new(),
            contains: None,
            after: None,
        })
        .await
        .expect("the follow starts");
    assert_eq!(
        following.head.map(|head| head.seq),
        page.head.map(|head| head.seq)
    );
    let stop = api
        .stop(id)
        .await
        .expect("the stop")
        .accepted
        .expect("an operation");
    api.wait_state(stop.operation.id, OperationState::Succeeded)
        .await;
    frame_until(&mut stream, |kind| matches!(kind, LogEntryKind::Exited(_))).await;
    api.start_ready(id).await;
    let frames = frame_until(&mut stream, |kind| matches!(kind, LogEntryKind::Started(_))).await;
    assert!(
        frames
            .iter()
            .all(|frame| frame.subscription_id == following.subscription_id)
    );

    // The two kinds of subscription share IDs and the limit, but each
    // unsubscribe ends only its own kind.
    let (subscribed, _events) = api
        .client
        .subscribe(SubscribeParams::default())
        .await
        .expect("an event subscription");
    let refused = api
        .client
        .call(Call::InstanceLogsUnfollow(LogsUnfollowParams {
            subscription_id: subscribed.subscription_id,
        }))
        .await
        .expect_err("an event subscription is not a follow");
    assert!(refused.is(&events::UNKNOWN_SUBSCRIPTION));
    let mut held = Vec::new();
    for _ in 0..6 {
        held.push(
            api.client
                .follow_logs(LogsFollowParams {
                    instance: id,
                    streams: Vec::new(),
                    contains: None,
                    after: None,
                })
                .await
                .expect("a follow under the limit"),
        );
    }
    let ninth = api
        .client
        .follow_logs(LogsFollowParams {
            instance: id,
            streams: Vec::new(),
            contains: None,
            after: None,
        })
        .await
        .expect_err("the ninth subscription is refused");
    assert!(ninth.is(&events::TOO_MANY_SUBSCRIPTIONS));
    let ended = api
        .client
        .call(Call::InstanceLogsUnfollow(LogsUnfollowParams {
            subscription_id: following.subscription_id,
        }))
        .await
        .expect("the follow ends");
    assert!(matches!(ended, Reply::InstanceLogsUnfollow));
    assert_eq!(health_of(&api).await, SubsystemStatus::Ok);

    drop(held);
    drop(stream);
    let stop = api
        .stop(id)
        .await
        .expect("the stop")
        .accepted
        .expect("an operation");
    api.wait_state(stop.operation.id, OperationState::Succeeded)
        .await;
    drop(api);
    daemon
        .shutdown(
            stackly_protocol::events::ShutdownReason::Requested,
            std::future::pending(),
        )
        .await;
    ev.pass();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_chatty_process_rotates_each_stream_on_its_own_and_an_old_cursor_sees_the_gap() {
    let ev = evidence!(
        covers = ["REQ-017", "AC-017"],
        kind = Kind::Fixture,
        expected = "spec 0027 AC-2, AC-5: with 32 KiB segments and three per stream, a fake writing 2000 lines a second keeps at most three segments per stream, and a cursor at the first record reads the gap rotated",
    );
    let site = Site::new();
    let mut config = fast();
    config.durable_logs.segment_max_bytes = 32 * 1024;
    config.durable_logs.segments_per_stream = 3;
    let daemon = site.start_with(config).await;
    let api = site.api().await;
    let id = api
        .create_ready("rotating", json!({ "logLinesPerSecond": 2000 }))
        .await;
    api.start_ready(id).await;
    let first = wait_page(&api, params(id), |page| page.store_id.is_some()).await;
    let store = first.store_id.expect("a store");
    tokio::time::sleep(Duration::from_secs(3)).await;

    let dir = site.instance_paths(id).logs.join("captured");
    let names: Vec<String> = std::fs::read_dir(&dir)
        .expect("the store lists")
        .map(|entry| {
            entry
                .expect("an entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    for stream in ["stdout.", "stderr."] {
        let count = names.iter().filter(|name| name.starts_with(stream)).count();
        assert!(
            (1..=3).contains(&count),
            "{stream} has {count} segments: {names:?}"
        );
    }
    let mut old = params(id);
    old.position = Some(LogPosition::After(LogAnchor {
        cursor: LogCursor {
            store_id: store,
            seq: LogSeq::new(1).expect("a seq"),
        },
    }));
    old.limit = Some(10);
    let page = read(&api, old).await.expect("the old cursor reads");
    assert_eq!(page.gap.map(|gap| gap.reason), Some(LogGapReason::Rotated));
    assert!(
        page.entries
            .first()
            .is_some_and(|entry| entry.seq.get() > 2)
    );

    let stop = api
        .stop(id)
        .await
        .expect("the stop")
        .accepted
        .expect("an operation");
    api.wait_state(stop.operation.id, OperationState::Succeeded)
        .await;
    drop(api);
    daemon
        .shutdown(
            stackly_protocol::events::ShutdownReason::Requested,
            std::future::pending(),
        )
        .await;
    ev.pass();
}

fn set_mode(path: &Path, mode: u32) {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
        .expect("the mode changes");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_store_that_cannot_be_written_degrades_the_health_and_the_process_runs_on() {
    let ev = evidence!(
        covers = ["NFR-004"],
        kind = Kind::Fixture,
        expected = "spec 0027 AC-12: with captured/ at mode 0500 the instance still starts and serves, instance_logs reads degraded, and once the mode is back the waiting lines are written without loss and the health turns ok after the recovery time",
    );
    let site = Site::new();
    let mut config = fast();
    config.durable_logs.health_recovery = Duration::from_secs(1);
    let daemon = site.start_with(config).await;
    let api = site.api().await;
    let id = api.create_ready("readonly", json!({})).await;
    let dir = site.instance_paths(id).logs.join("captured");
    std::fs::create_dir(&dir).expect("the store folder");
    set_mode(&dir, 0o500);

    api.start_ready(id).await;
    let started = Instant::now();
    while health_of(&api).await != SubsystemStatus::Degraded {
        assert!(
            started.elapsed() < PATIENCE,
            "the health never turned degraded"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(
        api.get(id)
            .await
            .expect("the instance reads")
            .process
            .is_some()
    );

    set_mode(&dir, 0o700);
    let page = wait_page(&api, params(id), |page| {
        texts(&page.entries)
            .iter()
            .any(|text| text.starts_with("listening on"))
    })
    .await;
    assert!(matches!(page.entries[0].kind, LogEntryKind::Started(_)));
    let started = Instant::now();
    while health_of(&api).await != SubsystemStatus::Ok {
        assert!(started.elapsed() < PATIENCE, "the health never recovered");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    let stop = api
        .stop(id)
        .await
        .expect("the stop")
        .accepted
        .expect("an operation");
    api.wait_state(stop.operation.id, OperationState::Succeeded)
        .await;
    drop(api);
    daemon
        .shutdown(
            stackly_protocol::events::ShutdownReason::Requested,
            std::future::pending(),
        )
        .await;
    ev.pass();
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unknown_instance_is_not_found_and_an_empty_store_reads_empty() {
    let site = Site::new();
    let daemon = site.start_with(fast()).await;
    let api = site.api().await;

    let missing = read(&api, params(Uuid::now_v7()))
        .await
        .expect_err("no such instance");
    assert!(missing.is(&stackly_protocol::codes::resource::NOT_FOUND));
    let id = api.create_ready("quiet", json!({})).await;
    let page = read(&api, params(id)).await.expect("an empty store reads");
    assert_eq!(page.store_id, None);
    assert!(page.entries.is_empty());
    assert_eq!(page.head, None);

    drop(api);
    daemon
        .shutdown(
            stackly_protocol::events::ShutdownReason::Requested,
            std::future::pending(),
        )
        .await;
}
