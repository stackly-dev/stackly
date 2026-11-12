//! A normal stop of Valkey (spec 0040 AC-11): `SIGTERM` with a client
//! blocked in `BLPOP`, done well within `stopSeconds`, exit 0, no
//! `SIGKILL`, and the final snapshot of mode `rdb` in the log.

use std::time::{Duration, Instant};

use serde_json::json;
use stackly_protocol::instance::InstanceLifecycle;
use stackly_protocol::logs::{LogEntryKind, LogsReadParams};
use stackly_protocol::{Call, Reply as Answer};
use stackly_testkit::evidence::{Kind, evidence};
use uuid::Uuid;

use crate::harness::{Site, Valkey, alive, create_instance, pid_of, port_of, stop_instance};
use crate::support::Api;

async fn log_entries(api: &Api, id: Uuid, contains: Option<&str>) -> Vec<LogEntryKind> {
    let reply = api
        .client
        .call(Call::InstanceLogsRead(LogsReadParams {
            instance: id,
            streams: Vec::new(),
            contains: contains.map(str::to_owned),
            position: None,
            limit: Some(1000),
        }))
        .await
        .expect("the log reads");
    let Answer::InstanceLogsRead(page) = reply else {
        panic!("not a logs_read reply: {reply:?}");
    };
    page.entries.into_iter().map(|entry| entry.kind).collect()
}

// covers: AC-11 (spec 0040)
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed Valkey 9.1.2 artifact in the runtime cache"]
async fn valkey_9_1_stops_on_sigterm_with_a_blocked_client_and_saves_its_snapshot() {
    let ev = evidence!(
        covers = ["REQ-038"],
        kind = Kind::Native,
        expected = "with one client waiting in BLPOP q 600, instance.stop of a Valkey rdb instance ends within 10 s, the instance is stopped, the process exited with code 0 and no signal, and the log holds DB saved on disk",
    );
    let Some(site) = Site::open(&ev) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let made = create_instance(&site, &api, "stopped", json!({})).await;
    let running = api.start_ready(made.id).await;
    let pid = pid_of(&running);
    let mut blocked = Valkey::signed_in(port_of(&running), &made.password);
    let version = blocked.info_server("valkey_version");
    site.subject(&ev, &version);
    blocked.send(&["BLPOP", "q", "600"]);
    tokio::time::sleep(Duration::from_millis(200)).await;

    let began = Instant::now();
    stop_instance(&api, made.id).await;
    let took = began.elapsed();
    assert!(took < Duration::from_secs(10), "the stop took {took:?}");
    assert!(!alive(pid));
    let stopped = api.get(made.id).await.expect("the instance can be read");
    assert_eq!(stopped.lifecycle, InstanceLifecycle::Stopped);
    assert!(stopped.last_failure.is_none(), "{stopped:#?}");

    let mut exited = None;
    let mut saved = false;
    for _ in 0..100 {
        exited = log_entries(&api, made.id, None)
            .await
            .into_iter()
            .rev()
            .find_map(|kind| match kind {
                LogEntryKind::Exited(exit) => Some(exit),
                _ => None,
            });
        saved = !log_entries(&api, made.id, Some("DB saved on disk"))
            .await
            .is_empty();
        if exited.is_some() && saved {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let exited = exited.expect("the log records the exit");
    assert_eq!(exited.code, Some(0), "{exited:?}");
    assert_eq!(exited.signal, None, "no SIGKILL: {exited:?}");
    assert!(saved, "the log holds DB saved on disk");
    daemon.shutdown().await;
    ev.pass_with(&format!(
        "Valkey {version} stopped in {took:?} with a client in BLPOP, exit 0, DB saved on disk"
    ));
}
