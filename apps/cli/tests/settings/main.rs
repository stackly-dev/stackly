//! `stackly settings` and `stackly storage` against a scripted daemon (spec
//! 0032, milestone 1): the output of each verb, the params it sends, the
//! exit code of a stale revision, and the rule sentence of a refused folder.
#![cfg(target_os = "macos")]

#[path = "../common/mod.rs"]
mod common;

mod settings;
mod storage;

use serde_json::json;
use stackly_protocol::settings::{DaemonSettingsView, StorageLocationView};

const DATA: &str = "00000000-0000-7000-8000-000000000001";
const LOGS: &str = "00000000-0000-7000-8000-000000000002";
const CACHE: &str = "00000000-0000-7000-8000-000000000003";

/// A location the scripted daemon reports.
pub fn location(
    id: &str,
    kind: &str,
    path: &str,
    built_in: bool,
    count: u64,
) -> StorageLocationView {
    serde_json::from_value(json!({
        "id": id, "kind": kind, "path": path, "builtIn": built_in,
        "isDefault": true, "instanceCount": count, "reachable": true
    }))
    .expect("a location view")
}

/// The settings the scripted daemon reports at `revision`, with the given
/// autostart and telemetry.
pub fn view(revision: u64, autostart: &str, channel: &str, telemetry: bool) -> DaemonSettingsView {
    serde_json::from_value(json!({
        "revision": revision,
        "defaultStartup": autostart,
        "updateChannel": channel,
        "telemetry": telemetry,
        "telemetryCopyVersion": 1,
        "locations": {
            "data": location(DATA, "data", "/Users/dev/Library/Application Support/Stackly", true, 2),
            "logs": location(LOGS, "logs", "/Users/dev/Library/Logs/Stackly", true, 2),
            "cache": location(CACHE, "cache", "/Users/dev/Library/Caches/Stackly", true, 0),
        },
        "logRetention": { "segmentsPerStream": 5, "segmentMaxBytes": 20971520 },
        "auditRetention": { "days": 30, "maxRows": 100000 },
        "updatedAt": "2026-11-03T08:00:00.000Z"
    }))
    .expect("a settings view")
}
