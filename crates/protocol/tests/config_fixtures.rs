//! Checks the config DTOs of spec 0029 against the fixtures in
//! `protocols/fixtures/ipc/` (AC-1, AC-2, AC-9, AC-10, AC-14, AC-17): what a
//! view, a verdict, and each refusal carry, and that a bad request is
//! refused with a field path.

use std::path::PathBuf;

use stackly_protocol::instance::config::{
    ChangeEndpointsReply, ConfigChangeRequest, ConfigImpact, ConfigRevisionOrigin, DriftKind,
    InstanceConfigView,
};
use stackly_protocol::instance::{FieldApply, InstanceBlocker, PortRequest};
use stackly_protocol::json::{FieldCode, decode_request};
use stackly_protocol::{Call, ErrorBody, Reply, ServerFrame};

fn read(file: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../protocols/fixtures/ipc")
        .join(file);
    std::fs::read(&path).unwrap_or_else(|err| panic!("cannot read {}: {err}", path.display()))
}

fn reply(file: &str) -> Reply {
    let frame: ServerFrame = serde_json::from_slice(&read(file)).expect(file);
    let ServerFrame::Response(response) = frame else {
        panic!("{file}: expected a response");
    };
    response.reply
}

fn error(file: &str) -> ErrorBody {
    let frame: ServerFrame = serde_json::from_slice(&read(file)).expect(file);
    let ServerFrame::Error(frame) = frame else {
        panic!("{file}: expected an error");
    };
    frame.error
}

fn view(file: &str) -> InstanceConfigView {
    let Reply::InstanceConfigGet(view) = reply(file) else {
        panic!("{file}: expected config get");
    };
    *view
}

// covers: AC-1
#[test]
fn the_view_names_the_active_revision_the_last_known_good_and_the_history() {
    let view = view("frame-response-instance-config-get-fake.json");

    assert_eq!(view.revision.number.get(), 2);
    assert!(view.revision.is_active && view.revision.is_last_known_good);
    assert_eq!(view.last_known_good.map(|n| n.get()), Some(2));
    assert_eq!(view.history.len(), 2);
    assert_eq!(view.history[1].origin, ConfigRevisionOrigin::Created);
    assert!(view.drift.is_none());
    assert!(view.reload_supported);
    assert_eq!(view.effective_files[0].path, "fake.conf");
    assert!(!view.effective_files[0].preview);
}

// covers: AC-1, AC-15
#[test]
fn a_locked_field_is_a_secret_or_set_once_and_every_field_has_its_apply_class() {
    let view = view("frame-response-instance-config-get-fake.json");
    let locked: Vec<&str> = view
        .fields
        .iter()
        .filter(|field| field.locked)
        .map(|field| field.field.key.as_str())
        .collect();

    assert_eq!(locked, ["dataSeed", "adminToken"]);
    assert_eq!(view.fields[0].field.apply, Some(FieldApply::Reload));
    assert_eq!(view.fields[1].field.apply, Some(FieldApply::Restart));
    assert!(view.fields[3].field.secret);
}

// covers: AC-10
#[test]
fn a_drift_names_each_file_how_it_drifted_and_the_diff_lines() {
    let view = view("frame-response-instance-config-get-drift.json");
    let drift = view.drift.expect("the fixture has drift");

    assert_eq!(drift.files.len(), 2);
    assert_eq!(drift.files[0].kind, DriftKind::Modified);
    assert_eq!(drift.files[1].kind, DriftKind::Missing);
    assert_eq!(drift.files[0].diff.len(), 3);
}

// covers: AC-10
#[test]
fn the_drift_blocker_lists_the_files_and_when_the_check_found_them() {
    let blocker: InstanceBlocker =
        serde_json::from_slice(&read("dto-instance-blocker-config-drift.json")).unwrap();

    let InstanceBlocker::ConfigDrift(drift) = blocker else {
        panic!("expected the config drift blocker");
    };
    assert_eq!(drift.files, ["fake.conf", "extra/override.conf"]);
}

// covers: AC-2
#[test]
fn a_verdict_carries_the_diff_the_impact_and_the_changed_lines_of_the_preview() {
    let Reply::InstanceConfigValidate(verdict) =
        reply("frame-response-instance-config-validate-restart.json")
    else {
        panic!("expected a verdict");
    };

    assert!(verdict.valid && verdict.field_errors.is_empty());
    assert_eq!(verdict.impact, ConfigImpact::Restart);
    assert_eq!(verdict.changes[0].key, "workers");
    assert_eq!(verdict.changes[0].old.as_deref(), Some("2"));
    assert_eq!(verdict.preview[0].changed_lines, [2]);
    assert!(verdict.preview[0].preview);
}

// covers: AC-2
#[test]
fn an_invalid_verdict_has_the_two_new_field_codes_and_no_diff() {
    let Reply::InstanceConfigValidate(verdict) =
        reply("frame-response-instance-config-validate-invalid.json")
    else {
        panic!("expected a verdict");
    };
    let codes: Vec<&str> = verdict.field_errors.iter().map(|f| f.code()).collect();

    assert!(!verdict.valid);
    assert_eq!(
        codes,
        ["secret_read_only", "immutable_after_init", "out_of_range"]
    );
    assert!(verdict.changes.is_empty() && verdict.preview.is_empty());
    assert_eq!(FieldCode::SecretReadOnly.as_str(), "secret_read_only");
    assert_eq!(
        FieldCode::ImmutableAfterInit.as_str(),
        "immutable_after_init"
    );
}

// covers: AC-9
#[test]
fn a_stale_base_revision_names_the_active_one_and_an_equal_change_is_unchanged() {
    let conflict = error("frame-error-config-revision-conflict.json");
    let Reply::InstanceConfigApply(unchanged) =
        reply("frame-response-instance-config-apply-unchanged.json")
    else {
        panic!("expected an apply reply");
    };

    assert_eq!(conflict.code(), "config.revision_conflict");
    assert_eq!(conflict.detail("activeRevision"), Some("3"));
    assert!(unchanged.unchanged && unchanged.accepted.is_none());
    assert_eq!(unchanged.impact, ConfigImpact::None);
}

// covers: AC-14
#[test]
fn a_dry_run_lists_the_problem_with_a_free_port_to_try() {
    let Reply::InstanceChangeEndpoints(reply) =
        reply("frame-response-instance-change-endpoints-dry-run.json")
    else {
        panic!("expected a change endpoints reply");
    };
    let ChangeEndpointsReply::DryRun(verdict) = *reply else {
        panic!("expected a dry run");
    };

    assert!(!verdict.valid);
    assert_eq!(verdict.problems[0].suggested_port, Some(5434));
    let conflict = error("frame-error-supervisor-endpoint-conflict-suggestion.json");
    assert_eq!(conflict.detail("suggestedPort"), Some("5434"));
}

// covers: AC-17
#[test]
fn the_requests_decode_to_the_calls_with_their_changes() {
    let apply = decode_request(&read("request-instance-config-apply.json")).unwrap();
    let rollback = decode_request(&read("request-instance-config-apply-rollback.json")).unwrap();
    let restore = decode_request(&read("request-instance-config-apply-restore.json")).unwrap();
    let ports = decode_request(&read("request-instance-change-endpoints.json")).unwrap();

    let Call::InstanceConfigApply(apply) = apply.call else {
        panic!("expected apply");
    };
    assert!(matches!(apply.change, ConfigChangeRequest::Settings(_)));
    assert_eq!(apply.expected_revision.map(|r| r.get()), Some(4));
    let Call::InstanceConfigApply(rollback) = rollback.call else {
        panic!("expected apply");
    };
    assert_eq!(rollback.change, ConfigChangeRequest::Rollback);
    let Call::InstanceConfigApply(restore) = restore.call else {
        panic!("expected apply");
    };
    assert!(matches!(restore.change, ConfigChangeRequest::Restore(n) if n.get() == 1));
    let Call::InstanceChangeEndpoints(ports) = ports.call else {
        panic!("expected change endpoints");
    };
    assert_eq!(ports.endpoints[0].port, PortRequest::Fixed(5433));
    assert_eq!(ports.endpoints[1].port, PortRequest::Auto);
}

// covers: AC-17
#[test]
fn a_bad_request_is_refused_with_the_path_of_the_field() {
    let zero = decode_request(&read(
        "reject-instance-config-apply-zero-base-revision.json",
    ))
    .expect_err("a zero base revision is refused");
    let empty = decode_request(&read("reject-instance-change-endpoints-empty.json"))
        .expect_err("an empty endpoint list is refused");

    assert_eq!(zero.error.fields()[0].path(), "baseRevision");
    assert_eq!(empty.error.fields()[0].path(), "endpoints");
}
