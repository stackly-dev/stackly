//! Durability per persistence mode (spec 0040 AC-6 to AC-9): a sentinel
//! written before a normal stop, one written after it and before a
//! `SIGKILL` to the ready `valkey-server`, which files each mode leaves in
//! the data folder, and a mode that `instance.config.validate` refuses to
//! change. Each mode writes one evidence record with what it kept.

use std::time::Duration;

use serde_json::json;
use stackly_protocol::instance::InstanceReadinessState;
use stackly_protocol::instance::SettingsDocument;
use stackly_protocol::instance::config::ConfigChangeRequest;
use stackly_testkit::evidence::{Evidence, Kind, evidence};

use crate::config_calls::Cfg;
use crate::harness::{
    Reply, Site, Valkey, alive, create_instance, files_under, pid_of, port_of, signal,
    stop_and_shutdown, stop_instance,
};

/// One persistence mode and what it is expected to keep.
#[derive(Debug, Clone, Copy)]
struct Mode {
    name: &'static str,
    /// Whether a sentinel survives a normal stop and start.
    keeps_on_stop: bool,
    /// Whether a sentinel written after the last snapshot survives a crash.
    keeps_on_crash: bool,
}

const RDB: Mode = Mode {
    name: "rdb",
    keeps_on_stop: true,
    keeps_on_crash: false,
};
const AOF: Mode = Mode {
    name: "aof",
    keeps_on_stop: true,
    keeps_on_crash: true,
};
const VOLATILE: Mode = Mode {
    name: "volatile",
    keeps_on_stop: false,
    keeps_on_crash: false,
};

fn get(valkey: &mut Valkey, key: &str) -> Option<String> {
    match valkey.cmd(&["GET", key]) {
        Reply::Bulk(value) => value,
        other => panic!("GET {key}: {other:?}"),
    }
}

/// The files Valkey writes in the data folder in `mode`, after a stop.
fn assert_data_files(mode: Mode, data: &std::path::Path) {
    let files = files_under(data);
    match mode.name {
        "rdb" => assert_eq!(files, ["dump.rdb"], "{}", data.display()),
        "aof" => {
            assert!(
                files.iter().all(|file| file.starts_with("appendonlydir/")),
                "{files:?}"
            );
            for part in [".manifest", ".base.rdb", ".incr.aof"] {
                assert!(
                    files.iter().any(|file| file.ends_with(part)),
                    "no {part} in {files:?}"
                );
            }
        }
        _ => assert!(files.is_empty(), "volatile writes nothing: {files:?}"),
    }
}

/// The files of the instance folder outside data, config, its history, and
/// temp: only what the daemon itself keeps there.
fn assert_nothing_else(site: &Site, id: uuid::Uuid) {
    let paths = site.instance_paths(id);
    let others: Vec<String> = files_under(&paths.root)
        .into_iter()
        .filter(|file| {
            let path = paths.root.join(file);
            ![
                &paths.data,
                &paths.config,
                &paths.config_history,
                &paths.temp,
                &paths.logs,
            ]
            .iter()
            .any(|dir| path.starts_with(dir))
                && path != paths.lock
                && path != paths.manifest
        })
        .collect();
    assert!(
        others
            .iter()
            .all(|file| !file.contains("dump.rdb") && !file.contains("appendonly")),
        "Valkey files outside the instance folders: {others:?}"
    );
}

async fn keeps_what_its_mode_promises(mode: Mode, ev: Evidence) {
    let Some(site) = Site::open(&ev) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let made = create_instance(&site, &api, mode.name, json!({ "persistence": mode.name })).await;
    let running = api.start_ready(made.id).await;
    let mut valkey = Valkey::signed_in(port_of(&running), &made.password);
    let version = valkey.info_server("valkey_version");
    site.subject(&ev, &version);
    assert_eq!(
        valkey.cmd(&["SET", "before-stop", "one"]),
        Reply::Simple("OK".to_owned())
    );
    drop(valkey);

    stop_instance(&api, made.id).await;
    let paths = site.instance_paths(made.id);
    assert_data_files(mode, &paths.data);
    assert_nothing_else(&site, made.id);

    let running = api.start_ready(made.id).await;
    let port = port_of(&running);
    let mut valkey = Valkey::signed_in(port, &made.password);
    let after_stop = get(&mut valkey, "before-stop");
    assert_eq!(
        after_stop.is_some(),
        mode.keeps_on_stop,
        "{}: before-stop after a normal stop",
        mode.name
    );
    if let Some(value) = &after_stop {
        assert_eq!(value, "one");
    }

    assert_eq!(
        valkey.cmd(&["SET", "before-crash", "two"]),
        Reply::Simple("OK".to_owned())
    );
    drop(valkey);
    // More than the one second of appendfsync everysec.
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let pid = pid_of(&running);
    signal(pid, "KILL");
    let respawned = api
        .wait_instance(made.id, "respawned and ready", |summary| {
            summary.readiness.state == InstanceReadinessState::Ready
                && summary
                    .process
                    .as_ref()
                    .is_some_and(|process| process.pid != pid)
        })
        .await;
    assert!(!alive(pid));
    let mut valkey = Valkey::signed_in(port_of(&respawned), &made.password);
    let crash_kept = get(&mut valkey, "before-crash").is_some();
    let stop_kept_after_crash = get(&mut valkey, "before-stop").is_some();
    assert_eq!(
        crash_kept, mode.keeps_on_crash,
        "{}: before-crash after SIGKILL",
        mode.name
    );
    assert_eq!(
        stop_kept_after_crash, mode.keeps_on_stop,
        "{}: before-stop after SIGKILL",
        mode.name
    );
    drop(valkey);

    if mode.name == "rdb" {
        // spec 0040 AC-9: the mode is fixed after init.
        let cfg = Cfg { api: &api };
        let view = cfg.view(made.id).await;
        let config_before = files_under(&paths.config);
        let mut map = view.revision.values.as_map().clone();
        map.insert("persistence".to_owned(), json!("aof"));
        let verdict = cfg
            .validate(
                made.id,
                view.revision.number.get(),
                ConfigChangeRequest::Settings(SettingsDocument::new(map)),
            )
            .await;
        assert!(!verdict.valid, "{verdict:#?}");
        assert_eq!(verdict.field_errors.len(), 1, "{verdict:#?}");
        assert_eq!(verdict.field_errors[0].path(), "settings.persistence");
        assert_eq!(verdict.field_errors[0].code(), "immutable_after_init");
        assert_eq!(files_under(&paths.config), config_before);
    }

    stop_and_shutdown(&api, daemon, made.id).await;
    let stop = if mode.keeps_on_stop { "kept" } else { "lost" };
    let crash = match (mode.keeps_on_stop, crash_kept) {
        (_, true) => "kept",
        (true, false) => "lost since last snapshot",
        (false, false) => "lost",
    };
    ev.pass_with(&format!(
        "valkey {version} {}: normal stop {stop}, process crash {crash}",
        mode.name
    ));
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed Valkey 9.1.2 artifact in the runtime cache"]
async fn valkey_9_1_rdb_keeps_data_over_a_stop_and_loses_writes_since_the_snapshot_on_a_crash() {
    let ev = evidence!(
        covers = ["REQ-038", "AC-038", "AC-077"],
        kind = Kind::Native,
        expected = "mode rdb leaves dump.rdb in the data folder, keeps a sentinel over a normal stop and start, loses a sentinel written after the last snapshot when valkey-server gets SIGKILL and respawns, keeps the one from before the stop, and refuses another mode with immutable_after_init",
    );
    keeps_what_its_mode_promises(RDB, ev).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed Valkey 9.1.2 artifact in the runtime cache"]
async fn valkey_9_1_aof_keeps_data_over_a_stop_and_over_a_crash() {
    let ev = evidence!(
        covers = ["REQ-038", "AC-038", "AC-077"],
        kind = Kind::Native,
        expected = "mode aof leaves appendonlydir/ with manifest, base, and incr files in the data folder, keeps a sentinel over a normal stop and start, and keeps one written more than a second before valkey-server gets SIGKILL and respawns",
    );
    keeps_what_its_mode_promises(AOF, ev).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed Valkey 9.1.2 artifact in the runtime cache"]
async fn valkey_9_1_volatile_writes_nothing_and_loses_everything_on_a_stop_or_crash() {
    let ev = evidence!(
        covers = ["REQ-038", "AC-038", "AC-077"],
        kind = Kind::Native,
        expected = "mode volatile leaves no file in the data folder, loses a sentinel over a normal stop and start, and loses everything when valkey-server gets SIGKILL and respawns",
    );
    keeps_what_its_mode_promises(VOLATILE, ev).await;
}
