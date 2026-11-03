//! `settings.get` and `settings.update` through the real daemon (spec 0032
//! AC-16, AC-17, AC-18, AC-21): every setting with its revision, a partial
//! patch, a stale revision that writes nothing, a change that changes
//! nothing, the event that follows a real change, and the default startup
//! that a create takes.

use std::time::Duration;

use serde_json::json;
use stackly_ipc::CallOptions;
use stackly_protocol::events::{EventKind, SubscribeParams};
use stackly_protocol::instance::{InstanceSummary, StartupPolicyKind};
use stackly_protocol::json::DecodedFrame;
use stackly_protocol::settings::{
    DaemonSettingsView, SettingsPatch, UpdateChannel, UpdateSettingsParams,
};
use stackly_protocol::{Call, ErrorBody, Reply, Revision};

use crate::support::{Api, Site, fast_with_catalog, key};

pub(crate) async fn stop_daemon(daemon: stacklyd::server::DaemonHandle) {
    daemon
        .shutdown(
            stackly_protocol::events::ShutdownReason::Requested,
            std::future::pending(),
        )
        .await;
}

pub(crate) async fn settings_of(api: &Api) -> DaemonSettingsView {
    match api
        .client
        .call_detailed(Call::SettingsGet, CallOptions::default())
        .await
        .expect("the settings read")
    {
        Reply::SettingsGet(view) => *view,
        other => panic!("not a settings reply: {other:?}"),
    }
}

pub(crate) async fn update(
    api: &Api,
    expected: Revision,
    patch: SettingsPatch,
    key: &str,
) -> Result<DaemonSettingsView, Box<ErrorBody>> {
    match api
        .client
        .call_detailed(
            Call::SettingsUpdate(UpdateSettingsParams {
                expected_revision: expected,
                patch,
            }),
            CallOptions::default().with_idempotency_key(key),
        )
        .await?
    {
        Reply::SettingsUpdate(view) => Ok(*view),
        other => panic!("not an update reply: {other:?}"),
    }
}

fn rev(number: u64) -> Revision {
    Revision::new(number).expect("a small revision")
}

fn telemetry(on: bool) -> SettingsPatch {
    SettingsPatch {
        telemetry: Some(on),
        ..SettingsPatch::default()
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_new_registry_reads_the_defaults_with_the_built_in_locations_and_the_log_retention() {
    // covers: AC-16, AC-21
    let site = Site::new();
    let daemon = site.start().await;
    let api = site.api().await;

    let view = settings_of(&api).await;

    assert_eq!(view.revision, rev(1));
    assert_eq!(view.default_startup, StartupPolicyKind::Manual);
    assert_eq!(view.update_channel, UpdateChannel::Stable);
    assert!(!view.telemetry);
    assert_eq!(view.telemetry_copy_version.get(), 1);
    assert_eq!(view.log_retention.segments_per_stream.get(), 5);
    assert_eq!(view.log_retention.segment_max_bytes.get(), 20 * 1024 * 1024);
    for location in [
        &view.locations.data,
        &view.locations.logs,
        &view.locations.cache,
    ] {
        assert!(location.built_in && location.is_default && location.reachable);
        assert!(std::path::Path::new(&location.path).is_absolute());
    }
    assert_eq!(view.locations.data.instance_count.get(), 0);
    drop(api);
    stop_daemon(daemon).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn an_update_changes_only_what_it_names_raises_the_revision_and_sends_one_event() {
    // covers: AC-16
    let site = Site::new();
    let daemon = site.start().await;
    let api = site.api().await;
    let (_, mut events) = api
        .client
        .subscribe(SubscribeParams::default())
        .await
        .expect("the subscription is accepted");

    let before = settings_of(&api).await;
    let patch = SettingsPatch {
        update_channel: Some(UpdateChannel::Beta),
        telemetry: Some(true),
        ..SettingsPatch::default()
    };
    let once = key();
    let after = update(&api, before.revision, patch, &once)
        .await
        .expect("the update applies");

    assert_eq!(after.revision, rev(2));
    assert_eq!(after.update_channel, UpdateChannel::Beta);
    assert!(after.telemetry);
    assert_eq!(after.default_startup, before.default_startup);
    assert_eq!(settings_of(&api).await, after);

    // The same request again gets the same answer and no second event.
    let again = update(&api, before.revision, patch, &once)
        .await
        .expect("the replay answers");
    assert_eq!(again, after);

    let mut revisions = Vec::new();
    while let Ok(Ok(Some(frame))) =
        tokio::time::timeout(Duration::from_millis(300), events.next()).await
    {
        if let DecodedFrame::Event(event) = frame
            && let EventKind::SettingsChanged(changed) = event.kind
        {
            revisions.push(changed.revision);
        }
    }
    assert_eq!(revisions, [rev(2)]);
    drop((api, events));
    stop_daemon(daemon).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stale_revision_is_refused_and_writes_nothing() {
    // covers: AC-17
    let site = Site::new();
    let daemon = site.start().await;
    let api = site.api().await;
    let first = settings_of(&api).await;
    update(&api, first.revision, telemetry(true), &key())
        .await
        .expect("the first update applies");

    let error = update(&api, first.revision, telemetry(false), &key())
        .await
        .expect_err("the stale revision is refused");

    assert_eq!(error.code(), "resource.revision_mismatch");
    let now = settings_of(&api).await;
    assert_eq!(now.revision, rev(2));
    assert!(now.telemetry, "nothing was written");
    drop(api);
    stop_daemon(daemon).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_patch_that_changes_nothing_answers_the_same_view_without_an_operation_or_an_event() {
    // covers: AC-16
    let site = Site::new();
    let daemon = site.start().await;
    let api = site.api().await;
    let (_, mut events) = api
        .client
        .subscribe(SubscribeParams::default())
        .await
        .expect("the subscription is accepted");
    let before = settings_of(&api).await;
    let operations = operation_count(&api).await;

    let same = update(&api, before.revision, telemetry(false), &key())
        .await
        .expect("the update answers");

    assert_eq!(same, before);
    assert_eq!(operation_count(&api).await, operations);
    while let Ok(Ok(Some(frame))) =
        tokio::time::timeout(Duration::from_millis(300), events.next()).await
    {
        if let DecodedFrame::Event(event) = frame {
            assert!(
                !matches!(event.kind, EventKind::SettingsChanged(_)),
                "no settings.changed for no change"
            );
        }
    }
    drop((api, events));
    stop_daemon(daemon).await;
}

async fn operation_count(api: &Api) -> usize {
    match api
        .client
        .call(Call::OperationList(
            stackly_protocol::operation::ListOperationsParams::default(),
        ))
        .await
        .expect("the operations list")
    {
        Reply::OperationList(page) => page.items.len(),
        other => panic!("not an operations reply: {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn an_empty_patch_and_an_unknown_value_are_refused_as_invalid_params() {
    // covers: AC-16
    let site = Site::new();
    let daemon = site.start().await;
    let api = site.api().await;
    let before = settings_of(&api).await;

    let empty = update(&api, before.revision, SettingsPatch::default(), &key())
        .await
        .expect_err("an empty patch is refused");
    let unknown = update(
        &api,
        before.revision,
        SettingsPatch {
            update_channel: Some(UpdateChannel::Unrecognized),
            ..SettingsPatch::default()
        },
        &key(),
    )
    .await
    .expect_err("an unknown value is refused");

    assert_eq!(empty.code(), "request.invalid_params");
    assert_eq!(unknown.code(), "request.invalid_params");
    assert_eq!(settings_of(&api).await, before);
    drop(api);
    stop_daemon(daemon).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_create_without_a_startup_takes_the_default_and_an_existing_instance_keeps_its_policy() {
    // covers: AC-18
    let site = Site::new();
    site.unpin_fake_package();
    let daemon = site.start_with(fast_with_catalog()).await;
    let api = site.api().await;
    let before = api.create_ready("first", json!({})).await;
    let form = api
        .form(api.runtime_id, None)
        .await
        .expect("the create form");
    assert_eq!(form.startup_default, StartupPolicyKind::Manual);
    let mut manual_draft = api.preflight_params("preview", json!({}));
    manual_draft.startup = Some(StartupPolicyKind::Manual);
    let command = api
        .preflight(manual_draft)
        .await
        .expect("a preflight")
        .command;
    assert!(
        !command.contains("autostart"),
        "manual against a manual default needs no flag: {command}"
    );

    let current = settings_of(&api).await;
    update(
        &api,
        current.revision,
        SettingsPatch {
            default_startup: Some(StartupPolicyKind::AtLogin),
            ..SettingsPatch::default()
        },
        &key(),
    )
    .await
    .expect("the default changes");
    let form = api
        .form(api.runtime_id, None)
        .await
        .expect("the create form");
    assert_eq!(form.startup_default, StartupPolicyKind::AtLogin);

    // The equivalent command says `--no-autostart` only when the draft asks for manual
    // against an at login default, and `--autostart` when it asks for at login.
    let command_for = |startup: Option<StartupPolicyKind>| {
        let mut params = api.preflight_params("preview", json!({}));
        params.startup = startup;
        let api = &api;
        async move { api.preflight(params).await.expect("a preflight").command }
    };
    let manual = command_for(Some(StartupPolicyKind::Manual)).await;
    assert!(manual.contains(" --no-autostart"), "{manual}");
    assert!(!manual.contains(" --autostart"), "{manual}");
    let at_login = command_for(Some(StartupPolicyKind::AtLogin)).await;
    assert!(at_login.contains(" --autostart"), "{at_login}");
    assert!(!at_login.contains("--no-autostart"), "{at_login}");
    let unspecified = command_for(None).await;
    assert!(!unspecified.contains("autostart"), "{unspecified}");

    let follows = api.create_ready("second", json!({})).await;
    let policy_of = |summary: InstanceSummary| summary.autostart.policy;
    assert_eq!(
        policy_of(api.get(follows).await.expect("reads")),
        StartupPolicyKind::AtLogin
    );
    assert_eq!(
        policy_of(api.get(before).await.expect("reads")),
        StartupPolicyKind::Manual,
        "an instance that exists keeps its policy"
    );

    let mut explicit = api.params("third", json!({}));
    explicit.startup = Some(StartupPolicyKind::Manual);
    let accepted = api
        .create(explicit)
        .await
        .expect("the create is accepted")
        .accepted
        .expect("a create starts an operation");
    let third: uuid::Uuid = accepted
        .operation
        .resource
        .as_ref()
        .expect("the operation names the instance")
        .id
        .parse()
        .expect("an instance id");
    api.wait_state(
        accepted.operation.id,
        stackly_protocol::operation::OperationState::Succeeded,
    )
    .await;
    assert_eq!(
        policy_of(api.get(third).await.expect("reads")),
        StartupPolicyKind::Manual,
        "an explicit manual wins over the default"
    );
    drop(api);
    stop_daemon(daemon).await;
}
