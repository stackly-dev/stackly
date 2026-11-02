//! The Open In actions through the real daemon (spec 0070 AC-3, AC-4,
//! AC-14): `openActions` in `instance.get_detail` of the fake package, and
//! `instance.prepare_terminal` writing its script into the temp folder of a
//! ready instance, refusing the rest with the right code and reason, and
//! never writing a secret.

use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::Path;

use serde_json::json;
use stackly_platform::CodeSigning;
use stackly_platform_macos::MacosCodeSigning;
use stackly_protocol::instance::open_with::terminal_script;
use stackly_protocol::instance::{
    ConnectionAvailability, ConnectionUnavailableReason, InstanceLifecycle, InstanceReadinessState,
    OpenActionKind,
};
use stackly_testkit::allocate_port;
use stackly_testkit::evidence::{Kind, evidence};
use uuid::Uuid;

use crate::support::{Api, Site, TOKEN};

/// Creates `name` with a fixed `http` port and `extra` settings.
async fn create(api: &Api, name: &str, extra: serde_json::Value) -> Uuid {
    let port = allocate_port().expect("a free port");
    let mut params = api.params(name, extra);
    params.endpoints = vec![Api::http_port(port)];
    let accepted = api
        .create(params)
        .await
        .expect("the create is accepted")
        .accepted
        .expect("a create starts an operation");
    let id = accepted
        .operation
        .resource
        .as_ref()
        .map(|resource| resource.id.parse::<Uuid>().expect("an instance id"))
        .expect("the operation names the instance");
    api.wait_state(
        accepted.operation.id,
        stackly_protocol::operation::OperationState::Succeeded,
    )
    .await;
    id
}

/// The `reason` detail of an error.
fn detail<'a>(error: &'a stackly_protocol::ErrorBody, key: &str) -> Option<&'a str> {
    error.detail(key)
}

// covers: AC-3
#[tokio::test(flavor = "multi_thread")]
async fn the_detail_lists_the_open_actions_of_the_pinned_package_in_manifest_order() {
    let site = Site::new();
    let _daemon = site.start().await;
    let api = site.api().await;
    let id = create(&api, "orders", json!({})).await;
    api.start_ready(id).await;

    let detail = api.detail(id).await.expect("the detail");

    let actions: Vec<(&str, OpenActionKind, ConnectionAvailability)> = detail
        .open_actions
        .iter()
        .map(|action| (action.name.as_str(), action.kind, action.state))
        .collect();
    assert_eq!(
        actions,
        [
            (
                "client",
                OpenActionKind::App,
                ConnectionAvailability::Available
            ),
            (
                "version",
                OpenActionKind::Terminal,
                ConnectionAvailability::Available
            ),
            (
                "status",
                OpenActionKind::Browser,
                ConnectionAvailability::Available
            ),
            (
                "notes",
                OpenActionKind::Reveal,
                ConnectionAvailability::Available
            ),
        ]
    );
    let terminal = &detail.open_actions[1];
    let command = terminal.command.as_ref().expect("a command");
    assert!(Path::new(&command.executable).is_absolute());
    assert!(command.executable.ends_with("/stackly-fake-server"));
    assert_eq!(command.args, ["version"]);
    let copy = terminal.copy_secret.as_ref().expect("the token is copied");
    assert_eq!(copy.field, "authToken");
    let notes = detail.open_actions[3].path.as_deref().expect("a path");
    assert_eq!(
        Path::new(notes),
        site.instance_paths(id).config.join("notes.txt")
    );
    assert!(Path::new(notes).is_file(), "initialize rendered the notes");
}

// covers: AC-4, AC-14
#[tokio::test(flavor = "multi_thread")]
async fn a_ready_instance_gets_a_private_script_in_its_temp_folder_without_a_secret() {
    let ev = evidence!(
        covers = ["AC-016", "REQ-016"],
        kind = Kind::Fixture,
        expected = "instance.prepare_terminal writes open-version.command with mode 0700, owned by the user, without quarantine, holding the absolute runtime executable and no prefix of the secret",
    );
    let site = Site::new();
    let _daemon = site.start().await;
    let api = site.api().await;
    let id = create(&api, "orders", json!({})).await;
    api.start_ready(id).await;
    let detail = api.detail(id).await.expect("the detail");

    let path = api
        .prepare_terminal(id, "version")
        .await
        .expect("the script is written");
    let again = api
        .prepare_terminal(id, "version")
        .await
        .expect("a second call writes it again");

    let temp = site.instance_paths(id).temp;
    assert_eq!(Path::new(&path), temp.join("open-version.command"));
    assert_eq!(again, path);
    let metadata = std::fs::metadata(&path).expect("the script exists");
    assert_eq!(metadata.permissions().mode() & 0o777, 0o700);
    assert_eq!(metadata.uid(), site.uid());
    let quarantine = MacosCodeSigning::new()
        .quarantine(Path::new(&path))
        .expect("the attribute can be read");
    assert!(quarantine.is_none(), "{quarantine:?}");
    let text = std::fs::read_to_string(&path).expect("the script reads");
    let command = detail.open_actions[1].command.as_ref().expect("a command");
    assert_eq!(text, terminal_script(id, "Fake Server", command));
    assert!(!text.contains(&TOKEN[..12]));
    let leftovers: Vec<_> = std::fs::read_dir(&temp)
        .expect("the temp folder reads")
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
    ev.pass();
}

// covers: AC-4
#[tokio::test(flavor = "multi_thread")]
async fn an_unknown_or_non_terminal_action_is_not_found_with_the_terminal_names() {
    let site = Site::new();
    let _daemon = site.start().await;
    let api = site.api().await;
    let id = create(&api, "orders", json!({})).await;

    for action in ["nope", "status", "client"] {
        let error = api
            .prepare_terminal(id, action)
            .await
            .expect_err("only a terminal action has a script");
        assert_eq!(error.code(), "instance.open_action_not_found", "{action}");
        assert_eq!(detail(&error, "action"), Some(action));
        assert_eq!(detail(&error, "available"), Some("version"));
    }
    let missing = api
        .prepare_terminal(Uuid::now_v7(), "version")
        .await
        .expect_err("no instance");
    assert_eq!(missing.code(), "resource.not_found");
}

// covers: AC-4
#[tokio::test(flavor = "multi_thread")]
async fn a_stopped_or_not_ready_instance_is_unavailable_with_its_reason() {
    let site = Site::new();
    let _daemon = site.start().await;
    let api = site.api().await;
    let stopped = create(&api, "stopped", json!({})).await;
    let slow = create(&api, "slow", json!({ "readyDelayMs": 3000 })).await;

    let error = api
        .prepare_terminal(stopped, "version")
        .await
        .expect_err("a stopped instance has no terminal");
    assert_eq!(error.code(), "instance.open_action_unavailable");
    assert_eq!(detail(&error, "reason"), Some("not_running"));
    assert!(error.retryable());

    api.start(slow).await.expect("the start is accepted");
    api.wait_instance(slow, "running and not ready", |summary| {
        summary.lifecycle == InstanceLifecycle::Running
            && summary.process.is_some()
            && summary.readiness.state != InstanceReadinessState::Ready
    })
    .await;
    let error = api
        .prepare_terminal(slow, "version")
        .await
        .expect_err("not ready yet");
    assert_eq!(error.code(), "instance.open_action_unavailable");
    assert_eq!(detail(&error, "reason"), Some("not_ready"));
    assert!(
        !site
            .instance_paths(slow)
            .temp
            .join("open-version.command")
            .exists()
    );
}

// covers: AC-3
#[tokio::test(flavor = "multi_thread")]
async fn a_stopped_instance_keeps_its_actions_available_in_the_detail() {
    let site = Site::new();
    let _daemon = site.start().await;
    let api = site.api().await;
    let id = create(&api, "orders", json!({})).await;

    let detail = api.detail(id).await.expect("the detail");

    // The daemon only reports its own reasons; the GUI gates on lifecycle.
    assert!(
        detail
            .open_actions
            .iter()
            .all(|action| action.state == ConnectionAvailability::Available
                && action.reason.is_none()),
        "{:?}",
        detail.open_actions
    );
    let _ = ConnectionUnavailableReason::Retained;
}
