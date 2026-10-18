//! Native runtime tests (spec 0003, AC-10): each provider line runs from
//! its managed artifact as a child of a LaunchAgent worker, through
//! `cargo xtask runtime spike`, and writes a `stackly.evidence/v1` record.
//!
//! They need the network, macOS 27, a GUI login session, and the
//! artifacts, so they are ignored by default. Run them as evidence on a
//! clean tree:
//!
//! ```text
//! cargo xtask evidence run --kind native -- cargo test -p xtask --test native_runtime -- --ignored
//! cargo xtask runtime record <the report path the wrapper prints>
//! ```
//!
//! Spikes started in parallel run one after another: the driver holds the
//! spike lock.

use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::Value;
use stackly_evidence::{Evidence, Kind, Source, evidence};

/// Who can unblock a line that has no managed artifact.
const OWNER: &str = "@ojiepermana";

#[test]
#[ignore = "native: needs network, macOS 27, and provider artifacts"]
fn nats_2_15_runs_from_managed_artifact() {
    let ev = evidence!(
        covers = ["AC-078", "REQ-005"],
        kind = Kind::Native,
        expected = "nats-server 2.15 runs from its managed artifact under a LaunchAgent, is ready within 120 s, and stops within 60 s",
    );
    spike(&ev, "nats", Some("2.15"));
}

#[test]
#[ignore = "native: needs network, macOS 27, and provider artifacts"]
fn traefik_3_7_runs_from_managed_artifact() {
    let ev = evidence!(
        covers = ["AC-078", "REQ-005"],
        kind = Kind::Native,
        expected = "traefik 3.7 runs from its managed artifact under a LaunchAgent, is ready within 120 s, and stops within 60 s",
    );
    spike(&ev, "traefik", Some("3.7"));
}

#[test]
#[ignore = "native: needs network, macOS 27, and provider artifacts"]
fn clickhouse_runs_from_managed_artifact() {
    let ev = evidence!(
        covers = ["AC-078", "REQ-005"],
        kind = Kind::Native,
        expected = "clickhouse server runs from its managed artifact under a LaunchAgent, is ready within 120 s, and stops within 60 s",
    );
    spike(&ev, "clickhouse", None);
}

#[test]
#[ignore = "native: needs network, macOS 27, and provider artifacts"]
fn mysql_8_4_runs_from_managed_artifact() {
    let ev = evidence!(
        covers = ["AC-078", "REQ-005"],
        kind = Kind::Native,
        expected = "mysqld 8.4 runs from its managed artifact under a LaunchAgent, is ready within 120 s, and stops within 60 s",
    );
    spike(&ev, "mysql", Some("8.4"));
}

#[test]
#[ignore = "native: needs network, macOS 27, and provider artifacts"]
fn mysql_9_7_runs_from_managed_artifact() {
    let ev = evidence!(
        covers = ["AC-078", "REQ-005"],
        kind = Kind::Native,
        expected = "mysqld 9.7 runs from its managed artifact under a LaunchAgent, is ready within 120 s, and stops within 60 s",
    );
    spike(&ev, "mysql", Some("9.7"));
}

#[test]
#[ignore = "native: needs network, macOS 27, and provider artifacts"]
fn postgresql_17_runs_from_managed_artifact() {
    let ev = evidence!(
        covers = ["AC-078", "REQ-005"],
        kind = Kind::Native,
        expected = "postgres 17 runs from its managed artifact under a LaunchAgent, passes the ICU, pgcrypto, lz4, and zstd probes, is ready within 120 s, and stops within 60 s",
    );
    spike(&ev, "postgresql", Some("17"));
}

#[test]
#[ignore = "native: needs network, macOS 27, and provider artifacts"]
fn postgresql_18_runs_from_managed_artifact() {
    let ev = evidence!(
        covers = ["AC-078", "REQ-005"],
        kind = Kind::Native,
        expected = "postgres 18 runs from its managed artifact under a LaunchAgent, passes the ICU, pgcrypto, lz4, and zstd probes, is ready within 120 s, and stops within 60 s",
    );
    spike(&ev, "postgresql", Some("18"));
}

#[test]
#[ignore = "native: needs network, macOS 27, and provider artifacts"]
fn valkey_9_1_runs_from_managed_artifact() {
    let ev = evidence!(
        covers = ["AC-078", "REQ-005"],
        kind = Kind::Native,
        expected = "valkey-server 9.1 runs from its managed artifact under a LaunchAgent, is ready within 120 s, and stops within 60 s",
    );
    spike(&ev, "valkey", Some("9.1"));
}

/// Finds the inventory entry of `provider` (and `line`, when given), runs
/// its spike, and turns the spike result into the test outcome.
fn spike(ev: &Evidence, provider: &str, line: Option<&str>) {
    let dir = ev.artifact_dir();
    let Some(entry) = inventory_entry(provider, line) else {
        let what = line.map_or_else(|| provider.to_owned(), |line| format!("{provider} {line}"));
        ev.blocked(&format!("no inventory entry for {what}"), OWNER);
        return;
    };
    let text = |key: &str| {
        entry
            .get(key)
            .and_then(toml::Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    if text("status") == "blocked" {
        let owner = Some(text("blocked_owner")).filter(|owner| !owner.is_empty());
        ev.blocked(&text("blocked_reason"), owner.as_deref().unwrap_or(OWNER));
        return;
    }
    let result_path = dir.join("spike-result.json");
    let status = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args(["runtime", "spike", "--id", &text("id"), "--result"])
        .arg(&result_path)
        .status()
        .expect("the xtask binary runs");
    let result: Value = match fs::read(&result_path) {
        Ok(bytes) => serde_json::from_slice(&bytes).expect("the spike result is JSON"),
        Err(_) => panic!("spike: the driver ended with {status} without a result"),
    };
    let failure = |key: &str| {
        result["failure"][key]
            .as_str()
            .unwrap_or_default()
            .to_owned()
    };
    if status.code() == Some(2) {
        let reason = Some(failure("reason")).filter(|reason| !reason.is_empty());
        ev.blocked(reason.as_deref().unwrap_or("no gui login session"), OWNER);
        return;
    }
    let version = result["observedVersion"]
        .as_str()
        .map_or_else(|| text("version"), str::to_owned);
    ev.subject(provider, &version, Some(&text("sha256")), Source::Managed);
    if result["status"] != "PASS" {
        panic!("{}: {}", failure("step"), failure("reason"));
    }
    let ready_ms = result["steps"]
        .as_array()
        .and_then(|steps| steps.iter().find(|step| step["name"] == "ready"))
        .and_then(|step| step["durationMs"].as_u64())
        .unwrap_or_default();
    let stopped_ms = result["stop"]["durationMs"].as_u64().unwrap_or_default();
    ev.pass_with(&format!(
        "{provider} {version} ready in {ready_ms} ms, stopped in {stopped_ms} ms"
    ));
}

/// The `[[artifact]]` of `provider` and `line` (any line when `None`) in
/// `runtimes/inventory.toml`, as a TOML table.
fn inventory_entry(provider: &str, line: Option<&str>) -> Option<toml::Table> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../runtimes/inventory.toml");
    let text = fs::read_to_string(&path).expect("runtimes/inventory.toml is readable");
    let inventory: toml::Table = toml::from_str(&text).expect("runtimes/inventory.toml is TOML");
    inventory
        .get("artifact")
        .and_then(toml::Value::as_array)?
        .iter()
        .filter_map(toml::Value::as_table)
        .find(|entry| {
            entry.get("provider").and_then(toml::Value::as_str) == Some(provider)
                && line.is_none_or(|line| {
                    entry.get("line").and_then(toml::Value::as_str) == Some(line)
                })
        })
        .cloned()
}
