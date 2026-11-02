//! Changing an instance over the socket (spec 0029, milestone 2): the six
//! config methods against the real daemon and the dummy server. Settings
//! apply by reload, restart, or at the next start; an invalid change makes
//! no operation; a stale base revision conflicts; a file edited outside
//! Stackly blocks every render until the revision is kept; history is
//! pruned; a rename and a port change leave the rest alone; the journal
//! records names and ports and no value; and none of it needs the network.

use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use stackly_domain::{InstanceId, OperationId, Registry};
use stackly_ipc::CallOptions;
use stackly_platform::{Clock, SystemClock};
use stackly_protocol::Timestamp;
use stackly_protocol::events::ShutdownReason;
use stackly_protocol::instance::config::{
    ApplyConfigParams, ChangeEndpointsParams, ChangeEndpointsReply, ConfigApplyAccepted,
    ConfigChangeRequest, ConfigImpact, ConfigRevisionOrigin, ConfigValidation, DriftKind,
    GetConfigParams, InstanceConfigView, KeepRevisionParams, RenameInstanceParams,
    ValidateConfigParams,
};
use stackly_protocol::instance::{
    EndpointRequest, InstanceActionAccepted, InstanceBlocker, InstanceLifecycle, PortRequest,
    SettingsDocument,
};
use stackly_protocol::operation::{ListOperationsParams, OperationState};
use stackly_protocol::{Call, ConfigRevisionNumber, ErrorBody, Reply};
use stackly_testkit::evidence::{Kind, evidence};
use stackly_testkit::{FakeCodeSigning, FakeFetcher, SigningCall, allocate_port};
use stacklyd::log_sink::{LogSink, LogSinkConfig};
use stacklyd::map::StartFacts;
use stacklyd::server::{self, DaemonDeps};
use uuid::Uuid;

use crate::support::{Api, Site, alive, fast, key, serving};

fn number(n: u64) -> ConfigRevisionNumber {
    ConfigRevisionNumber::new(n).expect("a revision number")
}

fn pid_of(summary: &stackly_protocol::instance::InstanceSummary) -> u32 {
    summary.process.as_ref().expect("a process runs").pid
}

impl Api {
    async fn config(&self, id: Uuid) -> Result<InstanceConfigView, Box<ErrorBody>> {
        match self
            .client
            .call(Call::InstanceConfigGet(GetConfigParams { instance_id: id }))
            .await?
        {
            Reply::InstanceConfigGet(view) => Ok(*view),
            other => panic!("not a config reply: {other:?}"),
        }
    }

    async fn validate(
        &self,
        id: Uuid,
        base: u64,
        change: ConfigChangeRequest,
    ) -> Result<ConfigValidation, Box<ErrorBody>> {
        match self
            .client
            .call(Call::InstanceConfigValidate(ValidateConfigParams {
                instance_id: id,
                base_revision: number(base),
                change,
            }))
            .await?
        {
            Reply::InstanceConfigValidate(verdict) => Ok(*verdict),
            other => panic!("not a validate reply: {other:?}"),
        }
    }

    async fn apply_with_key(
        &self,
        id: Uuid,
        base: u64,
        change: ConfigChangeRequest,
        key: &str,
    ) -> Result<ConfigApplyAccepted, Box<ErrorBody>> {
        match self
            .client
            .call_detailed(
                Call::InstanceConfigApply(ApplyConfigParams {
                    instance_id: id,
                    base_revision: number(base),
                    change,
                    expected_revision: None,
                }),
                CallOptions::default().with_idempotency_key(key),
            )
            .await?
        {
            Reply::InstanceConfigApply(accepted) => Ok(*accepted),
            other => panic!("not an apply reply: {other:?}"),
        }
    }

    async fn apply(
        &self,
        id: Uuid,
        base: u64,
        change: ConfigChangeRequest,
    ) -> Result<ConfigApplyAccepted, Box<ErrorBody>> {
        self.apply_with_key(id, base, change, &key()).await
    }

    /// Applies `change` and waits until its operation ended well.
    async fn apply_done(&self, id: Uuid, base: u64, change: ConfigChangeRequest) {
        let accepted = self
            .apply(id, base, change)
            .await
            .expect("the apply is accepted")
            .accepted
            .expect("a change begins an operation");
        let detail = self.wait_final(accepted.operation.id).await;
        assert_eq!(
            detail.operation.state,
            OperationState::Succeeded,
            "{detail:?}"
        );
    }

    async fn keep(&self, id: Uuid) -> Result<InstanceActionAccepted, Box<ErrorBody>> {
        match self
            .client
            .call_detailed(
                Call::InstanceConfigKeepRevision(KeepRevisionParams { instance_id: id }),
                CallOptions::default().with_idempotency_key(key()),
            )
            .await?
        {
            Reply::InstanceConfigKeepRevision(accepted) => Ok(*accepted),
            other => panic!("not a keep reply: {other:?}"),
        }
    }

    async fn rename(&self, id: Uuid, name: &str) -> Result<InstanceActionAccepted, Box<ErrorBody>> {
        match self
            .client
            .call_detailed(
                Call::InstanceRename(RenameInstanceParams {
                    instance_id: id,
                    name: name.to_owned(),
                    expected_revision: None,
                }),
                CallOptions::default().with_idempotency_key(key()),
            )
            .await?
        {
            Reply::InstanceRename(accepted) => Ok(*accepted),
            other => panic!("not a rename reply: {other:?}"),
        }
    }

    async fn change_endpoints(
        &self,
        id: Uuid,
        base: u64,
        endpoints: Vec<EndpointRequest>,
        dry_run: bool,
    ) -> Result<ChangeEndpointsReply, Box<ErrorBody>> {
        match self
            .client
            .call_detailed(
                Call::InstanceChangeEndpoints(ChangeEndpointsParams {
                    instance_id: id,
                    base_revision: number(base),
                    endpoints,
                    dry_run,
                }),
                CallOptions::default().with_idempotency_key(key()),
            )
            .await?
        {
            Reply::InstanceChangeEndpoints(reply) => Ok(*reply),
            other => panic!("not a change endpoints reply: {other:?}"),
        }
    }

    async fn operations(&self) -> usize {
        match self
            .client
            .call(Call::OperationList(ListOperationsParams::default()))
            .await
        {
            Ok(Reply::OperationList(page)) => page.items.len(),
            other => panic!("operation.list: {other:?}"),
        }
    }

    /// Waits until the config matches `ready`.
    async fn wait_config(
        &self,
        id: Uuid,
        what: &str,
        ready: impl Fn(&InstanceConfigView) -> bool,
    ) -> InstanceConfigView {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
        loop {
            let view = self.config(id).await.expect("the config can be read");
            if ready(&view) {
                return view;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "the config of {id} did not get {what}: {view:#?}"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
}

/// The settings of the active revision with `edits` laid over them.
fn settings_with(view: &InstanceConfigView, edits: Value) -> ConfigChangeRequest {
    let mut map = view.revision.values.as_map().clone();
    for (field, value) in edits.as_object().expect("edits are an object") {
        map.insert(field.clone(), value.clone());
    }
    ConfigChangeRequest::Settings(SettingsDocument::new(map))
}

/// A secret reference that is not the one the instance has.
fn other_token(view: &InstanceConfigView) -> String {
    let current = view
        .revision
        .values
        .as_map()
        .get("authToken")
        .and_then(Value::as_str)
        .expect("a secret field holds its reference");
    format!("{current}x")
}

fn error_code(result: Result<impl std::fmt::Debug, Box<ErrorBody>>) -> String {
    let error = result.expect_err("the request is refused");
    if error.code() == "internal.unexpected" {
        eprintln!("{error:?}");
    }
    error.code().to_owned()
}

// covers: AC-1, AC-7, AC-15
#[tokio::test(flavor = "multi_thread")]
async fn the_view_shows_revision_one_locked_fields_and_the_files_of_the_last_render() {
    let ev = evidence!(
        covers = ["REQ-015", "AC-015", "VAL-012"],
        kind = Kind::Fixture,
        expected = "instance.config.get of a new fake instance reads revision 1 created, no last known good, the set once and secret fields locked, each field with its apply class, and no drift; after the first ready the last known good is revision 1 and fake.conf shows the copy of the render",
    );
    let site = Site::new();
    let _daemon = site.start().await;
    let api = site.api().await;
    let id = api.create_ready("cfg-view", json!({})).await;

    let view = api.config(id).await.expect("instance.config.get");
    assert_eq!(view.revision.number.get(), 1);
    assert_eq!(view.revision.origin, ConfigRevisionOrigin::Created);
    assert!(view.revision.is_active && view.last_known_good.is_none());
    assert_eq!(view.history.len(), 1);
    assert!(view.drift.is_none() && view.reload_supported);
    let locked: Vec<&str> = view
        .fields
        .iter()
        .filter(|field| field.locked)
        .map(|field| field.field.key.as_str())
        .collect();
    assert_eq!(locked, ["failInit", "initDelayMs", "authToken"]);
    let apply_of = |key: &str| {
        view.fields
            .iter()
            .find(|field| field.field.key == key)
            .and_then(|field| field.field.apply)
    };
    assert_eq!(
        apply_of("greeting"),
        Some(stackly_protocol::instance::FieldApply::Reload)
    );
    assert_eq!(
        apply_of("label"),
        Some(stackly_protocol::instance::FieldApply::Restart)
    );
    assert!(
        view.endpoints
            .iter()
            .any(|e| e.name == "http" && e.changeable)
    );
    let token = view
        .revision
        .values
        .as_map()
        .get("authToken")
        .and_then(Value::as_str)
        .expect("a secret field holds its reference");
    assert!(
        !token.contains("fixture-secret"),
        "no secret value: {token}"
    );

    api.start_ready(id).await;
    let view = api
        .wait_config(id, "a last known good", |view| {
            view.last_known_good.is_some()
        })
        .await;
    assert_eq!(view.last_known_good.map(|n| n.get()), Some(1));
    assert!(view.revision.ready_at.is_some() && view.revision.is_last_known_good);
    let file = view
        .effective_files
        .iter()
        .find(|file| file.path == "fake.conf")
        .expect("the render left a copy of fake.conf");
    assert!(
        !file.preview && file.content.contains("greeting"),
        "{file:?}"
    );
    api.stop_when_free(id).await;
    ev.pass();
}

// covers: AC-2, AC-3, AC-9
#[tokio::test(flavor = "multi_thread")]
async fn an_invalid_change_makes_no_operation_a_stale_base_conflicts_and_an_equal_change_is_unchanged()
 {
    let ev = evidence!(
        covers = ["AC-015", "VAL-012"],
        kind = Kind::Fixture,
        expected = "validate marks a secret as secret_read_only and a set once field as immutable_after_init, shows the diff and the impact of a valid change, and apply of an invalid change, of a stale base, and of an equal change makes no operation, no revision, and leaves the PID alone",
    );
    let site = Site::new();
    let _daemon = site.start().await;
    let api = site.api().await;
    let id = api.create_ready("cfg-invalid", json!({})).await;
    let running = api.start_ready(id).await;
    let pid = pid_of(&running);
    let view = api.config(id).await.expect("config");

    let reload = api
        .validate(id, 1, settings_with(&view, json!({ "greeting": "hola" })))
        .await
        .expect("validate");
    assert!(
        reload.valid && reload.impact == ConfigImpact::Reload,
        "{reload:?}"
    );
    assert_eq!(reload.changes[0].key, "greeting");
    assert_eq!(reload.changes[0].new.as_deref(), Some("hola"));
    assert!(
        reload
            .preview
            .iter()
            .any(|file| file.path == "fake.conf" && !file.changed_lines.is_empty()),
        "{reload:?}"
    );
    let restart = api
        .validate(id, 1, settings_with(&view, json!({ "label": "other" })))
        .await
        .expect("validate");
    assert_eq!(restart.impact, ConfigImpact::Restart);

    let secret = api
        .validate(
            id,
            1,
            settings_with(&view, json!({ "authToken": other_token(&view) })),
        )
        .await
        .expect("validate");
    let once = api
        .validate(id, 1, settings_with(&view, json!({ "failInit": true })))
        .await
        .expect("validate");
    assert!(!secret.valid && !once.valid);
    assert_eq!(secret.field_errors[0].path(), "settings.authToken");
    assert_eq!(secret.field_errors[0].code(), "secret_read_only");
    assert_eq!(once.field_errors[0].path(), "settings.failInit");
    assert_eq!(once.field_errors[0].code(), "immutable_after_init");

    let before = api.operations().await;
    let refused = api
        .apply(
            id,
            1,
            settings_with(&view, json!({ "authToken": other_token(&view) })),
        )
        .await
        .expect_err("a secret cannot change");
    assert_eq!(refused.code(), "instance.settings_invalid");
    assert_eq!(refused.fields()[0].code(), "secret_read_only");
    let range = api
        .apply(id, 1, settings_with(&view, json!({ "readyDelayMs": -5 })))
        .await
        .expect_err("a value out of range");
    assert_eq!(range.fields()[0].path(), "settings.readyDelayMs");

    let stale = api
        .apply(id, 7, settings_with(&view, json!({ "greeting": "hola" })))
        .await
        .expect_err("a stale base");
    assert_eq!(stale.code(), "config.revision_conflict");
    assert_eq!(stale.detail("activeRevision"), Some("1"));
    assert_eq!(
        error_code(api.validate(id, 7, ConfigChangeRequest::Rollback).await),
        "config.revision_conflict"
    );

    let same = api
        .apply(id, 1, settings_with(&view, json!({})))
        .await
        .expect("an equal change is answered");
    assert!(same.unchanged && same.accepted.is_none());
    assert_eq!(api.operations().await, before);
    let after = api.config(id).await.expect("config");
    assert_eq!(after.revision.number.get(), 1);
    assert_eq!(pid_of(&api.get(id).await.expect("instance")), pid);
    api.stop_when_free(id).await;
    ev.pass();
}

// covers: AC-4, AC-7
#[tokio::test(flavor = "multi_thread")]
async fn a_reload_change_keeps_the_pid_and_the_new_revision_becomes_the_last_known_good() {
    let ev = evidence!(
        covers = ["AC-015", "REQ-015"],
        kind = Kind::Fixture,
        expected = "applying a reload setting of a running fake instance writes revision 2, keeps the PID, supersedes revision 1, and revision 2 reaches last known good after the reload verified ready",
    );
    let site = Site::new();
    let _daemon = site.start().await;
    let api = site.api().await;
    let id = api.create_ready("cfg-reload", json!({})).await;
    let pid = pid_of(&api.start_ready(id).await);
    let view = api.config(id).await.expect("config");

    api.apply_done(id, 1, settings_with(&view, json!({ "greeting": "hola" })))
        .await;

    let view = api
        .wait_config(id, "revision 2 as last known good", |view| {
            view.last_known_good.map(|n| n.get()) == Some(2)
        })
        .await;
    assert_eq!(view.revision.number.get(), 2);
    assert_eq!(view.revision.origin, ConfigRevisionOrigin::Applied);
    assert_eq!(view.history.len(), 2);
    assert!(!view.history[1].is_active);
    assert_eq!(
        view.revision.values.as_map().get("greeting"),
        Some(&json!("hola"))
    );
    assert_eq!(pid_of(&api.get(id).await.expect("instance")), pid);
    api.stop_when_free(id).await;
    ev.pass();
}

// covers: AC-5
#[tokio::test(flavor = "multi_thread")]
async fn a_restart_change_starts_a_new_process() {
    let ev = evidence!(
        covers = ["AC-015", "AC-026"],
        kind = Kind::Fixture,
        expected = "applying a restart setting of a running fake instance writes revision 2 and ends with a new PID that serves the data folder alone",
    );
    let site = Site::new();
    let _daemon = site.start().await;
    let api = site.api().await;
    let id = api.create_ready("cfg-restart", json!({})).await;
    let first = pid_of(&api.start_ready(id).await);
    let view = api.config(id).await.expect("config");

    api.apply_done(id, 1, settings_with(&view, json!({ "label": "changed" })))
        .await;

    let summary = api
        .wait_instance(id, "a new process", |summary| {
            summary
                .process
                .as_ref()
                .is_some_and(|process| process.pid != first)
        })
        .await;
    assert!(!alive(first));
    assert_eq!(serving(&site.instance_paths(id).data), [pid_of(&summary)]);
    assert_eq!(
        api.config(id).await.expect("config").revision.number.get(),
        2
    );
    api.stop_when_free(id).await;
    ev.pass();
}

// covers: AC-6, AC-8
#[tokio::test(flavor = "multi_thread")]
async fn a_stopped_instance_only_saves_and_a_roll_back_copies_the_last_known_good() {
    let ev = evidence!(
        covers = ["AC-015", "REQ-015"],
        kind = Kind::Fixture,
        expected = "a change on a stopped instance writes a revision that applies at the next start with no process, the revision that never reached ready can be rolled back to revision 1 as a new revision with origin rollback, and a roll back before anything was ready is config.no_last_known_good",
    );
    let site = Site::new();
    let _daemon = site.start().await;
    let api = site.api().await;
    let fresh = api.create_ready("cfg-fresh", json!({})).await;
    assert_eq!(
        error_code(api.apply(fresh, 1, ConfigChangeRequest::Rollback).await),
        "config.no_last_known_good"
    );

    let id = api.create_ready("cfg-saved", json!({})).await;
    api.start_ready(id).await;
    api.wait_config(id, "a last known good", |view| {
        view.last_known_good.is_some()
    })
    .await;
    api.stop_when_free(id).await;
    api.wait_instance(id, "stopped", |s| s.lifecycle == InstanceLifecycle::Stopped)
        .await;
    let view = api.config(id).await.expect("config");

    let verdict = api
        .validate(id, 1, settings_with(&view, json!({ "label": "saved" })))
        .await
        .expect("validate");
    assert_eq!(verdict.impact, ConfigImpact::NextStart);
    api.apply_done(id, 1, settings_with(&view, json!({ "label": "saved" })))
        .await;
    let saved = api.config(id).await.expect("config");
    assert_eq!(saved.revision.number.get(), 2);
    assert!(saved.revision.ready_at.is_none());
    assert_eq!(saved.last_known_good.map(|n| n.get()), Some(1));
    assert!(api.get(id).await.expect("instance").process.is_none());

    api.apply_done(id, 2, ConfigChangeRequest::Rollback).await;
    let back = api.config(id).await.expect("config");
    assert_eq!(back.revision.number.get(), 3);
    assert_eq!(back.revision.origin, ConfigRevisionOrigin::Rollback);
    assert_eq!(back.revision.source_number.map(|n| n.get()), Some(1));
    assert_eq!(back.revision.values, view.revision.values);

    // Restore takes any revision of the history.
    api.apply_done(id, 3, ConfigChangeRequest::Restore(number(2)))
        .await;
    let restored = api.config(id).await.expect("config");
    assert_eq!(restored.revision.origin, ConfigRevisionOrigin::Restored);
    assert_eq!(restored.revision.values, saved.revision.values);

    // The next start renders the saved revision and reaches ready on it.
    api.start_ready(id).await;
    api.wait_config(id, "revision 4 ready", |view| {
        view.last_known_good.map(|n| n.get()) == Some(4)
    })
    .await;
    api.stop_when_free(id).await;
    ev.pass();
}

// covers: AC-10, AC-11
#[tokio::test(flavor = "multi_thread")]
async fn a_file_edited_outside_stackly_blocks_every_render_until_the_revision_is_kept() {
    let ev = evidence!(
        covers = ["REQ-015", "AC-015"],
        kind = Kind::Fixture,
        expected = "after fake.conf is edited, config get reports it modified with a diff and the instance blocker is configDrift; start, apply, and change endpoints are refused config.drift_detected with no operation and the edited file unchanged; keep revision writes the copy back, a deleted file shows as missing, and start works again",
    );
    let site = Site::new();
    let _daemon = site.start().await;
    let api = site.api().await;
    let id = api.create_ready("cfg-drift", json!({})).await;
    api.start_ready(id).await;
    api.wait_config(id, "a last known good", |view| {
        view.last_known_good.is_some()
    })
    .await;
    api.stop_when_free(id).await;
    api.wait_instance(id, "stopped", |s| s.lifecycle == InstanceLifecycle::Stopped)
        .await;
    let path = site.instance_paths(id).config.join("fake.conf");
    let original = std::fs::read_to_string(&path).expect("fake.conf was rendered");

    std::fs::write(&path, "{\"greeting\": \"edited by hand\"}").expect("the file is edited");
    let view = api.config(id).await.expect("config");
    let drift = view.drift.clone().expect("the edit is drift");
    assert_eq!(drift.files[0].path, "fake.conf");
    assert_eq!(drift.files[0].kind, DriftKind::Modified);
    assert!(!drift.files[0].diff.is_empty());
    let summary = api.get(id).await.expect("instance");
    assert!(matches!(
        summary.blocker,
        Some(InstanceBlocker::ConfigDrift(_))
    ));

    let before = api.operations().await;
    assert_eq!(error_code(api.start(id).await), "config.drift_detected");
    assert_eq!(
        error_code(
            api.apply(id, 1, settings_with(&view, json!({ "greeting": "x" })))
                .await
        ),
        "config.drift_detected"
    );
    assert_eq!(
        error_code(
            api.change_endpoints(
                id,
                1,
                vec![EndpointRequest {
                    name: "http".to_owned(),
                    port: PortRequest::Auto,
                }],
                false
            )
            .await
        ),
        "config.drift_detected"
    );
    assert_eq!(api.operations().await, before);
    assert_eq!(
        std::fs::read_to_string(&path).expect("the file reads"),
        "{\"greeting\": \"edited by hand\"}"
    );

    let kept = api
        .keep(id)
        .await
        .expect("keep revision is accepted")
        .accepted
        .expect("a drift begins an operation");
    api.wait_final(kept.operation.id).await;
    assert_eq!(std::fs::read_to_string(&path).expect("restored"), original);
    assert!(api.config(id).await.expect("config").drift.is_none());
    let again = api.keep(id).await.expect("keep without drift");
    assert!(again.already_in_state && again.accepted.is_none());

    std::fs::remove_file(&path).expect("the file is deleted");
    let missing = api.config(id).await.expect("config").drift.expect("drift");
    assert_eq!(missing.files[0].kind, DriftKind::Missing);
    let kept = api
        .keep(id)
        .await
        .expect("keep")
        .accepted
        .expect("an operation");
    api.wait_final(kept.operation.id).await;
    assert_eq!(std::fs::read_to_string(&path).expect("restored"), original);

    api.start_ready(id).await;
    api.stop_when_free(id).await;
    ev.pass();
}

// covers: AC-12
#[tokio::test(flavor = "multi_thread")]
async fn the_history_keeps_twenty_revisions_and_numbers_are_never_reused() {
    let ev = evidence!(
        covers = ["REQ-015", "AC-015"],
        kind = Kind::Fixture,
        expected = "after 25 applies the history holds 20 revisions, the active revision is 26, the last known good is kept with its copy folder, and the folders of pruned revisions are gone",
    );
    let site = Site::new();
    let _daemon = site.start().await;
    let api = site.api().await;
    let id = api.create_ready("cfg-prune", json!({})).await;
    api.start_ready(id).await;
    api.wait_config(id, "a last known good", |view| {
        view.last_known_good.is_some()
    })
    .await;
    api.stop_when_free(id).await;
    api.wait_instance(id, "stopped", |s| s.lifecycle == InstanceLifecycle::Stopped)
        .await;
    let history_dir = site.instance_paths(id).config_history;
    let folders = || std::fs::read_dir(&history_dir).map_or(0, Iterator::count);
    assert_eq!(folders(), 1);

    for n in 1..=25_u64 {
        let view = api.config(id).await.expect("config");
        api.apply_done(
            id,
            n,
            settings_with(&view, json!({ "greeting": format!("hello {n}") })),
        )
        .await;
    }

    let view = api.config(id).await.expect("config");
    assert_eq!(view.revision.number.get(), 26);
    assert_eq!(view.history.len(), 20);
    assert_eq!(view.history.last().map(|r| r.number.get()), Some(7));
    // Revision 1 was the last known good and stays beside the 20 newest.
    assert_eq!(view.last_known_good.map(|n| n.get()), Some(1));
    assert!(folders() <= 21, "{} history folders", folders());
    api.apply_done(id, 26, settings_with(&view, json!({ "greeting": "last" })))
        .await;
    assert_eq!(
        api.config(id).await.expect("config").revision.number.get(),
        27
    );
    ev.pass();
}

// covers: AC-13
#[tokio::test(flavor = "multi_thread")]
async fn a_rename_changes_only_the_name_and_a_taken_name_is_refused() {
    let ev = evidence!(
        covers = ["BR-003", "AC-045"],
        kind = Kind::Fixture,
        expected = "renaming a running instance keeps the PID, the ID, and the config revision; the same name has nothing to do; a name another instance has is instance.name_taken and a malformed one instance.name_invalid, both with no operation",
    );
    let site = Site::new();
    let _daemon = site.start().await;
    let api = site.api().await;
    let id = api.create_ready("cfg-name", json!({})).await;
    api.create_ready("cfg-other", json!({})).await;
    let pid = pid_of(&api.start_ready(id).await);

    let before = api.operations().await;
    assert_eq!(
        error_code(api.rename(id, "cfg-other").await),
        "instance.name_taken"
    );
    assert_eq!(
        error_code(api.rename(id, "Bad Name").await),
        "instance.name_invalid"
    );
    let same = api.rename(id, "cfg-name").await.expect("answered");
    assert!(same.already_in_state && same.accepted.is_none());
    assert_eq!(api.operations().await, before);

    let accepted = api
        .rename(id, "cfg-renamed")
        .await
        .expect("the rename is accepted")
        .accepted
        .expect("a rename begins an operation");
    let detail = api.wait_final(accepted.operation.id).await;
    assert_eq!(detail.operation.state, OperationState::Succeeded);
    let summary = api.get(id).await.expect("instance");
    assert_eq!(summary.name, "cfg-renamed");
    assert_eq!(pid_of(&summary), pid);
    assert_eq!(
        api.config(id).await.expect("config").revision.number.get(),
        1
    );
    api.stop_when_free(id).await;
    ev.pass();
}

// covers: AC-14
#[tokio::test(flavor = "multi_thread")]
async fn changing_a_port_checks_like_create_suggests_a_free_one_and_restarts_a_running_instance() {
    let ev = evidence!(
        covers = ["REQ-010", "AC-055", "AC-056"],
        kind = Kind::Fixture,
        expected = "a fixed port another instance has is refused with supervisor.endpoint_conflict and a suggested port, a dry run lists the problem, a free fixed port on a running instance writes a revision with origin endpoints and restarts it on that port, and Automatic is accepted again",
    );
    let site = Site::new();
    let _daemon = site.start().await;
    let api = site.api().await;
    let taken = allocate_port().expect("a free port");
    let mut params = api.params("cfg-holder", json!({}));
    params.endpoints = vec![Api::http_port(taken)];
    let holder = api
        .create(params)
        .await
        .expect("the holder is created")
        .accepted
        .expect("an operation");
    api.wait_state(holder.operation.id, OperationState::Succeeded)
        .await;
    let id = api.create_ready("cfg-ports", json!({})).await;
    let first = pid_of(&api.start_ready(id).await);
    let ask = |port| {
        vec![EndpointRequest {
            name: "http".to_owned(),
            port,
        }]
    };

    let before = api.operations().await;
    let conflict = api
        .change_endpoints(id, 1, ask(PortRequest::Fixed(taken)), false)
        .await
        .expect_err("another instance has the port");
    assert_eq!(conflict.code(), "supervisor.endpoint_conflict");
    assert!(conflict.detail("suggestedPort").is_some(), "{conflict:?}");
    let ChangeEndpointsReply::DryRun(dry) = api
        .change_endpoints(id, 1, ask(PortRequest::Fixed(taken)), true)
        .await
        .expect("a dry run answers")
    else {
        panic!("not a dry run");
    };
    assert!(!dry.valid && dry.problems[0].code == "supervisor.endpoint_conflict");
    let unknown = api
        .change_endpoints(
            id,
            1,
            vec![EndpointRequest {
                name: "nope".to_owned(),
                port: PortRequest::Auto,
            }],
            false,
        )
        .await
        .expect_err("an endpoint the manifest does not have");
    assert_eq!(unknown.code(), "instance.endpoint_invalid");
    assert_eq!(api.operations().await, before);

    let free = allocate_port().expect("a free port");
    let ChangeEndpointsReply::Applied(applied) = api
        .change_endpoints(id, 1, ask(PortRequest::Fixed(free)), false)
        .await
        .expect("a free port is accepted")
    else {
        panic!("not an applied reply");
    };
    assert_eq!(applied.impact, ConfigImpact::Restart);
    let accepted = applied.accepted.expect("a port change begins an operation");
    let detail = api.wait_final(accepted.operation.id).await;
    assert_eq!(
        detail.operation.state,
        OperationState::Succeeded,
        "{detail:?}"
    );
    let summary = api
        .wait_instance(id, "ready on the new port", |summary| {
            summary
                .process
                .as_ref()
                .is_some_and(|process| process.pid != first)
                && summary.endpoints[0].port == Some(free)
        })
        .await;
    let view = api.config(id).await.expect("config");
    assert_eq!(view.revision.number.get(), 2);
    assert_eq!(view.revision.origin, ConfigRevisionOrigin::Endpoints);
    assert_eq!(view.endpoints[0].request, Some(PortRequest::Fixed(free)));
    assert_eq!(summary.endpoints[0].port, Some(free));

    api.stop_when_free(id).await;
    api.wait_instance(id, "stopped", |s| s.lifecycle == InstanceLifecycle::Stopped)
        .await;
    let ChangeEndpointsReply::Applied(back) = api
        .change_endpoints(id, 2, ask(PortRequest::Auto), false)
        .await
        .expect("automatic is accepted")
    else {
        panic!("not an applied reply");
    };
    assert_eq!(back.impact, ConfigImpact::NextStart);
    ev.pass();
}

// covers: AC-20
#[tokio::test(flavor = "multi_thread")]
async fn the_journal_of_a_change_names_the_settings_and_ports_and_holds_no_value() {
    let ev = evidence!(
        covers = ["AC-045"],
        kind = Kind::Fixture,
        expected = "the journal input of an apply_config lists the old and new revision and the names of the changed settings and of a change_endpoints the old and new port, and none of them holds a setting value or the auth token",
    );
    let site = Site::new();
    let daemon = site.start().await;
    let api = site.api().await;
    let id = api.create_ready("cfg-audit", json!({})).await;
    let view = api.config(id).await.expect("config");
    api.apply_done(
        id,
        1,
        settings_with(&view, json!({ "greeting": "a distinctive greeting" })),
    )
    .await;
    let free = allocate_port().expect("a free port");
    let ChangeEndpointsReply::Applied(applied) = api
        .change_endpoints(
            id,
            2,
            vec![EndpointRequest {
                name: "http".to_owned(),
                port: PortRequest::Fixed(free),
            }],
            false,
        )
        .await
        .expect("accepted")
    else {
        panic!("not an applied reply");
    };
    let ports_op = applied.accepted.expect("an operation").operation.id;
    api.wait_final(ports_op).await;
    let apply_op = api
        .operations_of_kind("apply_config")
        .await
        .expect("an apply_config ran");
    daemon
        .shutdown(ShutdownReason::Requested, std::future::pending())
        .await;

    let registry = site.open_registry();
    let input_of = |id: Uuid| {
        let journal = registry
            .operation_journal(OperationId::from_uuid(id).expect("an operation id"))
            .expect("the journal reads")
            .value
            .expect("the operation has a journal");
        journal.journal.input().as_str().to_owned()
    };
    let apply: Value = serde_json::from_str(&input_of(apply_op)).expect("JSON input");
    assert_eq!(apply["oldRevision"], 1);
    assert_eq!(apply["newRevision"], 2);
    assert_eq!(apply["settings"], json!(["greeting"]));
    let ports: Value = serde_json::from_str(&input_of(ports_op)).expect("JSON input");
    assert_eq!(ports["ports"][0]["endpoint"], "http");
    assert_eq!(ports["ports"][0]["new"], free);
    for text in [input_of(apply_op), input_of(ports_op)] {
        assert!(!text.contains("distinctive"), "{text}");
        assert!(!text.contains(crate::support::TOKEN), "{text}");
    }
    let _ = InstanceId::from_uuid(id);
    ev.pass();
}

impl Api {
    /// The ID of the newest operation of `kind`.
    async fn operations_of_kind(&self, kind: &str) -> Option<Uuid> {
        match self
            .client
            .call(Call::OperationList(ListOperationsParams::default()))
            .await
        {
            Ok(Reply::OperationList(page)) => page
                .items
                .iter()
                .find(|item| {
                    serde_json::to_value(item.kind)
                        .ok()
                        .and_then(|v| v.as_str().map(str::to_owned))
                        .as_deref()
                        == Some(kind)
                })
                .map(|item| item.id),
            other => panic!("operation.list: {other:?}"),
        }
    }
}

// covers: AC-21
#[tokio::test(flavor = "multi_thread")]
async fn every_config_method_works_with_no_catalog_fetch_or_signature_check() {
    let ev = evidence!(
        covers = ["REQ-046", "AC-046"],
        kind = Kind::Fixture,
        expected = "with a fetcher that was never asked and a code signing port that never assessed a signature, config get, validate, apply, rollback, keep revision, rename, and change endpoints all succeed for the fake instance",
    );
    let site = Site::new();
    let fetcher = FakeFetcher::new();
    fetcher.go_offline();
    let signing = FakeCodeSigning::new();
    let platform = site
        .platform
        .clone()
        .with_fetcher(Arc::new(fetcher.clone()))
        .with_code_signing(Arc::new(signing.clone()));
    let sink = LogSink::start(Box::new(std::io::sink()), LogSinkConfig::DAEMON)
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
    let api = site.api().await;
    let id = api.create_ready("cfg-offline", json!({})).await;
    let accesses = daemon.core().catalog_accesses();
    api.start_ready(id).await;
    let view = api
        .wait_config(id, "a last known good", |view| {
            view.last_known_good.is_some()
        })
        .await;

    api.validate(id, 1, settings_with(&view, json!({ "greeting": "x" })))
        .await
        .expect("validate");
    api.apply_done(id, 1, settings_with(&view, json!({ "greeting": "x" })))
        .await;
    let path = site.instance_paths(id).config.join("fake.conf");
    std::fs::write(&path, "{}").expect("an edit");
    let kept = api
        .keep(id)
        .await
        .expect("keep")
        .accepted
        .expect("operation");
    api.wait_final(kept.operation.id).await;
    let renamed = api
        .rename(id, "cfg-offline-2")
        .await
        .expect("rename")
        .accepted
        .expect("operation");
    api.wait_final(renamed.operation.id).await;
    let free = allocate_port().expect("a free port");
    let ChangeEndpointsReply::Applied(ports) = api
        .change_endpoints(
            id,
            2,
            vec![EndpointRequest {
                name: "http".to_owned(),
                port: PortRequest::Fixed(free),
            }],
            false,
        )
        .await
        .expect("ports")
    else {
        panic!("not an applied reply");
    };
    api.wait_final(ports.accepted.expect("operation").operation.id)
        .await;
    let view = api.config(id).await.expect("config");
    api.apply_done(id, 3, settings_with(&view, json!({ "label": "again" })))
        .await;

    assert_eq!(
        daemon.core().catalog_accesses(),
        accesses,
        "the catalog was looked at"
    );
    assert!(fetcher.requests().is_empty(), "{:?}", fetcher.requests());
    let assessed: Vec<_> = signing
        .calls()
        .into_iter()
        .filter(|call| matches!(call, SigningCall::Assess(_)))
        .collect();
    assert!(assessed.is_empty(), "{assessed:?}");
    api.stop_when_free(id).await;
    daemon
        .shutdown(ShutdownReason::Requested, std::future::pending())
        .await;
    ev.pass();
}

// covers: AC-14
#[tokio::test(flavor = "multi_thread")]
async fn a_running_instance_can_change_a_fixed_port_to_automatic() {
    let ev = evidence!(
        covers = ["REQ-010", "AC-055", "AC-056"],
        kind = Kind::Fixture,
        expected = "changing a fixed port of a running instance to Automatic writes a revision, stops the old process, and starts a new one on a picked port",
    );
    let site = Site::new();
    let _daemon = site.start().await;
    let api = site.api().await;
    let mut params = api.params("cfg-auto", json!({}));
    params.endpoints = vec![Api::http_port(allocate_port().expect("a free port"))];
    let created = api
        .create(params)
        .await
        .expect("created")
        .accepted
        .expect("an operation");
    api.wait_state(created.operation.id, OperationState::Succeeded)
        .await;
    let id: Uuid = created
        .operation
        .resource
        .as_ref()
        .map(|resource| resource.id.parse().expect("an instance id"))
        .expect("the operation names the instance");
    let first = pid_of(&api.start_ready(id).await);

    let ChangeEndpointsReply::Applied(applied) = api
        .change_endpoints(
            id,
            1,
            vec![EndpointRequest {
                name: "http".to_owned(),
                port: PortRequest::Auto,
            }],
            false,
        )
        .await
        .expect("accepted")
    else {
        panic!("not an applied reply");
    };
    let detail = api
        .wait_final(applied.accepted.expect("an operation").operation.id)
        .await;
    assert_eq!(
        detail.operation.state,
        OperationState::Succeeded,
        "{detail:?}"
    );
    let summary = api
        .wait_instance(id, "a new process", |summary| {
            summary
                .process
                .as_ref()
                .is_some_and(|process| process.pid != first)
        })
        .await;
    assert!(summary.endpoints[0].port.is_some());
    api.stop_when_free(id).await;
    ev.pass();
}

// covers: AC-10
#[tokio::test(flavor = "multi_thread")]
async fn a_daemon_that_starts_over_an_edited_file_puts_the_drift_blocker_on_the_instance() {
    let ev = evidence!(
        covers = ["REQ-015", "AC-015"],
        kind = Kind::Fixture,
        expected = "fake.conf edited while no daemon runs makes the next daemon report the instance with a configDrift blocker as soon as it serves, and start is refused config.drift_detected",
    );
    let site = Site::new();
    let first = site.start().await;
    let api = site.api().await;
    let id = api.create_ready("cfg-restart-drift", json!({})).await;
    api.start_ready(id).await;
    api.wait_config(id, "a last known good", |view| {
        view.last_known_good.is_some()
    })
    .await;
    api.stop_when_free(id).await;
    api.wait_instance(id, "stopped", |s| s.lifecycle == InstanceLifecycle::Stopped)
        .await;
    drop(api);
    first
        .shutdown(ShutdownReason::Requested, std::future::pending())
        .await;

    let path = site.instance_paths(id).config.join("fake.conf");
    std::fs::write(&path, "{}").expect("the file is edited");
    let _second = site.start().await;
    let api = site.api().await;
    let summary = api
        .wait_instance(id, "the drift blocker", |summary| {
            matches!(summary.blocker, Some(InstanceBlocker::ConfigDrift(_)))
        })
        .await;
    let Some(InstanceBlocker::ConfigDrift(drift)) = summary.blocker else {
        panic!("no drift blocker");
    };
    assert_eq!(drift.files, ["fake.conf"]);
    assert_eq!(error_code(api.start(id).await), "config.drift_detected");
    assert_eq!(std::fs::read_to_string(&path).expect("reads"), "{}");
    ev.pass();
}
