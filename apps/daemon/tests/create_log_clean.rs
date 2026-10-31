//! A create keeps the secret value and the free text settings out of the daemon
//! log and its stderr (spec 0020 AC-26, NFR-009). The real `stacklyd` runs on a
//! root seeded with the fake package, with fixture values only.
#![cfg(target_os = "macos")]

mod common;

use std::process::Command;
use std::time::Duration;

use common::{client, daemon_paths, start_daemon};
use serde_json::{Map, json};
use stackly_ipc::CallOptions;
use stackly_protocol::instance::{
    CreateInstanceParams, ProviderSelector, SecretFieldInput, SettingsDocument,
};
use stackly_protocol::operation::{GetOperationParams, OperationState};
use stackly_protocol::secret::{SecretInput, SecretText};
use stackly_protocol::{Call, Reply};
use stackly_testkit::evidence::{Kind, evidence};
use stackly_testkit::{ReapGuard, TestRoot, cargo_example, fake_server};
use uuid::Uuid;

const PATIENCE: Duration = Duration::from_secs(30);
/// Fixture values with markers no other log line contains; they are not credentials.
const SECRET: &str = "fixture-secret-marker-7f3a91c2d4e8b605";
const GREETING: &str = "fixture-greeting-marker-2b9d41e7a3c6f850";

/// Seeds `root` with the fake package and returns the runtime ID and revision.
fn seed(root: &TestRoot) -> (Uuid, String) {
    let output = Command::new(cargo_example("stacklyd", "skeleton_seed"))
        .arg("--root")
        .arg(root.path())
        .arg("--fake-server")
        .arg(fake_server::binary())
        .arg("--json")
        .output()
        .expect("skeleton_seed runs");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let seeded: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("the seed prints JSON");
    (
        seeded["runtimeId"]
            .as_str()
            .expect("a runtime ID")
            .parse()
            .expect("a UUID"),
        seeded["provider"]["revision"]
            .as_str()
            .expect("a revision")
            .to_owned(),
    )
}

#[tokio::test]
async fn the_secret_value_and_the_text_settings_of_a_create_are_not_in_the_daemon_log() {
    let ev = evidence!(
        covers = ["NFR-009"],
        kind = Kind::Fixture,
        expected = "after a create with an entered secret value and a free text setting, neither value appears in the daemon log or its stderr file, while the log itself is not empty",
    );
    // covers: AC-26
    let root = TestRoot::new().expect("a test root");
    let (runtime_id, revision) = seed(&root);
    let _reap = ReapGuard::new(root.path(), &daemon_paths(&root).runtime);
    let daemon = start_daemon(&root);
    let (client, _) = client(daemon.socket()).await;
    let mut settings = Map::new();
    settings.insert("greeting".to_owned(), json!(GREETING));
    let params = CreateInstanceParams {
        name: "logclean".to_owned(),
        display_name: None,
        provider: ProviderSelector {
            name: "testing/fake".to_owned(),
            revision: Some(revision),
        },
        runtime_id,
        endpoints: Vec::new(),
        settings: SettingsDocument::new(settings),
        secrets: vec![SecretFieldInput {
            field: "authToken".to_owned(),
            input: SecretInput::Value(SecretText::new(SECRET.to_owned())),
        }],
    };

    let Reply::InstanceCreate(accepted) = client
        .call_detailed(
            Call::InstanceCreate(params),
            CallOptions::default().with_idempotency_key("create-log-clean"),
        )
        .await
        .expect("the create is accepted")
    else {
        panic!("not a create reply");
    };
    let operation = accepted
        .accepted
        .expect("a create starts an operation")
        .operation;
    let deadline = tokio::time::Instant::now() + PATIENCE;
    loop {
        let Ok(Reply::OperationGet(detail)) = client
            .call(Call::OperationGet(GetOperationParams {
                operation_id: operation.id,
            }))
            .await
        else {
            panic!("operation.get failed");
        };
        if detail.operation.state == OperationState::Succeeded {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the create did not succeed: {detail:?}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    let log = std::fs::read_to_string(
        daemon_paths(&root)
            .layout
            .daemon_log_dir()
            .join("stacklyd.log"),
    )
    .expect("the daemon wrote its log");
    assert!(!log.is_empty(), "an empty log would make the check vacuous");
    let mut texts = vec![("the daemon log", log)];
    for entry in std::fs::read_dir(root.path()).expect("the test root can be read") {
        let path = entry.expect("a directory entry").path();
        if path.extension().is_some_and(|ext| ext == "stderr") {
            texts.push((
                "the daemon stderr",
                std::fs::read_to_string(&path).unwrap_or_default(),
            ));
        }
    }
    for (what, text) in &texts {
        assert!(!text.contains(SECRET), "the secret value is in {what}");
        assert!(!text.contains(GREETING), "the text setting is in {what}");
    }
    ev.pass();
}
