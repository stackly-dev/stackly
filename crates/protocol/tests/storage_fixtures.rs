//! Checks the storage frames in `protocols/fixtures/ipc/` and the instance
//! test data in `protocols/fixtures/instance/` that the CLI goldens and the
//! Swift tests read (spec 0022 AC-1 and AC-12).

use std::collections::BTreeSet;
use std::path::PathBuf;

use serde::Deserialize;
use serde::de::DeserializeOwned;
use stackly_protocol::instance::{InstanceLifecycle, InstancePage, InstanceReadinessState};
use stackly_protocol::operation::{OperationPage, OperationState, RecoveryHint};
use stackly_protocol::storage::{CapacityLevel, CapacityReport};
use stackly_protocol::{Call, ErrorFrame, Reply, ServerFrame, codes};

#[derive(Debug, Deserialize)]
struct Index {
    fixtures: Vec<Entry>,
}

#[derive(Debug, Deserialize)]
struct Entry {
    file: String,
}

#[derive(Debug, Deserialize)]
struct GroupCases {
    cases: Vec<GroupCase>,
}

#[derive(Debug, Deserialize)]
struct GroupCase {
    lifecycle: InstanceLifecycle,
    readiness: InstanceReadinessState,
    group: Option<String>,
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../protocols/fixtures")
}

fn decode<T: DeserializeOwned>(path: &str) -> T {
    let full = fixtures_dir().join(path);
    let bytes = std::fs::read(&full).unwrap_or_else(|err| panic!("{}: {err}", full.display()));
    serde_json::from_slice(&bytes).unwrap_or_else(|err| panic!("{path}: {err}"))
}

fn frame(file: &str) -> ServerFrame {
    decode(&format!("ipc/{file}"))
}

fn report(file: &str) -> CapacityReport {
    let ServerFrame::Response(response) = frame(file) else {
        panic!("{file}: expected a response");
    };
    let Reply::StorageGetCapacity(report) = response.reply else {
        panic!("{file}: expected a storage.get_capacity reply");
    };
    report
}

fn error(file: &str) -> stackly_protocol::ErrorBody {
    let ServerFrame::Error(ErrorFrame { error, .. }) = frame(file) else {
        panic!("{file}: expected an error frame");
    };
    error
}

#[test]
fn storage_get_capacity_is_a_unit_method_allowed_in_recovery() {
    let request: stackly_protocol::Request = decode("ipc/request-storage-get-capacity.json");

    assert_eq!(request.call, Call::StorageGetCapacity);
    assert!(request.call.allowed_in_recovery());
    assert!(request.idempotency_key.is_none());
}

#[test]
fn a_capacity_report_carries_the_sizes_the_percent_and_the_level() {
    let ok = report("frame-response-storage-get-capacity.json");
    let critical = report("frame-response-storage-get-capacity-critical.json");

    assert_eq!(ok.level, CapacityLevel::Ok);
    assert_eq!(ok.used_percent, 58);
    assert_eq!(ok.total_bytes.get(), 994_662_584_320);
    assert_eq!(ok.available_bytes.get(), 412_317_048_832);
    assert_eq!(critical.level, CapacityLevel::Critical);
    assert!(critical.available_bytes.get() < 2_000_000_000);
}

#[test]
fn a_foreign_capacity_level_decodes_as_unrecognized() {
    let report = report("frame-response-storage-get-capacity-unknown-level.json");

    assert_eq!(report.level, CapacityLevel::Unrecognized);
}

#[test]
fn every_storage_error_fixture_is_retryable_unavailable_and_carries_the_hint_of_its_details() {
    for (file, reason) in [
        (
            "frame-error-storage-capacity-unavailable-timeout.json",
            "timeout",
        ),
        ("frame-error-storage-capacity-unavailable-io.json", "io"),
        (
            "frame-error-storage-capacity-unavailable-no-size.json",
            "no_size",
        ),
    ] {
        let error = error(file);

        assert_eq!(
            error.code(),
            codes::storage::CAPACITY_UNAVAILABLE.code,
            "{file}"
        );
        assert!(error.retryable(), "{file}");
        assert_eq!(
            error
                .details()
                .iter()
                .find(|d| d.key() == "reason")
                .map(|d| d.value()),
            Some(reason),
            "{file}"
        );
        assert_eq!(
            error.hint(),
            codes::storage::hint(error.code(), error.details()),
            "{file}"
        );
    }
}

#[test]
fn only_a_timeout_has_a_static_hint() {
    let timeout = error("frame-error-storage-capacity-unavailable-timeout.json");
    let io = error("frame-error-storage-capacity-unavailable-io.json");

    let hint = timeout.hint().expect("a timeout has a hint");

    assert!(hint.len() <= 200 && !hint.contains('{'));
    assert_eq!(io.hint(), None);
}

#[test]
fn the_instance_fixture_index_lists_every_file_in_the_folder() {
    let index: Index = decode("instance/index.json");
    let listed: BTreeSet<String> = index.fixtures.into_iter().map(|e| e.file).collect();
    let mut on_disk = BTreeSet::new();
    for entry in std::fs::read_dir(fixtures_dir().join("instance")).expect("the folder exists") {
        let name = entry
            .expect("a readable entry")
            .file_name()
            .to_string_lossy()
            .into_owned();
        if name != "index.json" {
            on_disk.insert(name);
        }
    }

    assert_eq!(listed, on_disk);
}

/// The table of the spec, written out once more so the data test is checked
/// against an independent copy.
fn expected_group(
    lifecycle: InstanceLifecycle,
    readiness: InstanceReadinessState,
) -> Option<&'static str> {
    use InstanceLifecycle::{
        Failed, Initializing, Recovering, Retained, Running, Starting, Stopped, Stopping,
        Uninitialized, Unknown,
    };
    match lifecycle {
        Running if readiness == InstanceReadinessState::Ready => Some("ready"),
        Running | Initializing | Starting | Recovering | Stopping => Some("notReady"),
        Stopped | Uninitialized => Some("stopped"),
        Failed => Some("failed"),
        Unknown => Some("unknown"),
        Retained => None,
    }
}

#[test]
fn groups_json_covers_every_lifecycle_times_every_readiness_state() {
    let cases: GroupCases = decode("instance/groups.json");

    let pairs: BTreeSet<(String, String)> = cases
        .cases
        .iter()
        .map(|case| {
            (
                serde_json::to_string(&case.lifecycle).expect("encodes"),
                serde_json::to_string(&case.readiness).expect("encodes"),
            )
        })
        .collect();

    assert_eq!(cases.cases.len(), 30);
    assert_eq!(pairs.len(), 30);
    for case in &cases.cases {
        assert_eq!(
            case.group.as_deref(),
            expected_group(case.lifecycle, case.readiness),
            "{:?} with {:?}",
            case.lifecycle,
            case.readiness
        );
    }
    assert!(
        cases
            .cases
            .iter()
            .filter(|case| case.lifecycle == InstanceLifecycle::Retained)
            .all(|case| case.group.is_none())
    );
}

#[test]
fn list_mixed_holds_twenty_instances_in_every_group() {
    let page: InstancePage = decode("instance/list-mixed.json");

    let mut counts = std::collections::BTreeMap::new();
    for item in &page.items {
        let group = expected_group(item.lifecycle, item.readiness.state)
            .expect("the list holds no retained instance");
        *counts.entry(group).or_insert(0_u32) += 1;
    }

    assert_eq!(page.items.len(), 20);
    assert_eq!(page.next, None);
    assert_eq!(
        counts.into_iter().collect::<Vec<_>>(),
        [
            ("failed", 3),
            ("notReady", 8),
            ("ready", 4),
            ("stopped", 4),
            ("unknown", 1)
        ]
    );
    let names: BTreeSet<_> = page.items.iter().map(|item| item.name.as_str()).collect();
    let ids: BTreeSet<_> = page.items.iter().map(|item| item.id).collect();
    assert_eq!((names.len(), ids.len()), (20, 20));
}

#[test]
fn list_mixed_holds_one_instance_for_each_attention_rule_with_the_smallest_rule_winning() {
    let page: InstancePage = decode("instance/list-mixed.json");
    let by_name = |name: &str| {
        page.items
            .iter()
            .find(|item| item.name.as_str() == name)
            .unwrap_or_else(|| panic!("no instance named {name}"))
    };

    assert!(by_name("broken-db").last_failure.is_some());
    assert_eq!(by_name("half-made").initialized, Some(false));
    assert!(by_name("mystery").blocker.is_some());
    assert!(by_name("locked-db").blocker.is_some());
    assert!(by_name("secretive").blocker.is_some());
    assert!(by_name("flapping").restart.next_attempt_at.is_some());
    assert_eq!(
        by_name("stray-db")
            .process
            .as_ref()
            .expect("a process")
            .adoptable,
        Some(false)
    );
    let double = by_name("double-trouble");
    assert!(double.blocker.is_some() && double.last_failure.is_none());
    assert_eq!(double.lifecycle, InstanceLifecycle::Failed);
}

#[test]
fn attention_operations_hold_every_recovery_hint_and_the_active_states() {
    let page: OperationPage = decode("instance/attention-operations.json");

    let hints: Vec<_> = page
        .items
        .iter()
        .filter(|op| op.state == OperationState::NeedsRecovery)
        .map(|op| op.recovery.as_ref().expect("a recovery").hint)
        .collect();
    let states: BTreeSet<String> = page
        .items
        .iter()
        .map(|op| serde_json::to_string(&op.state).expect("encodes"))
        .collect();

    assert_eq!(
        hints,
        [
            RecoveryHint::Resume,
            RecoveryHint::Abandon,
            RecoveryHint::Manual,
            RecoveryHint::AutoCompensate,
            RecoveryHint::Unrecognized
        ]
    );
    for state in [
        "queued",
        "running",
        "cancel_requested",
        "needs_recovery",
        "succeeded",
    ] {
        assert!(states.contains(&format!("\"{state}\"")), "{state}");
    }
}

#[test]
fn the_mixed_instance_list_is_in_the_canonical_wire_form() {
    let page = serde_json::to_value(decode::<InstancePage>("instance/list-mixed.json"));
    let original: serde_json::Value = decode("instance/list-mixed.json");

    assert_eq!(page.expect("encodes"), original);
}
