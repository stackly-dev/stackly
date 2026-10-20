//! `instance.create` and the secret fields of the package (spec 0011 AC-6,
//! spec 0014): a field the client does not mention gets a generated value
//! under `local:instance/<id>/<purpose>`, and a field the client does give
//! keeps the source it was sent with. The real `stacklyd` runs on a root
//! seeded with the fake package; only the file keychain of that root is
//! touched.
#![cfg(target_os = "macos")]

mod common;

use std::process::Command;
use std::time::Duration;

use common::{client, daemon_paths, start_daemon};
use stackly_ipc::{CallOptions, Client};
use stackly_protocol::instance::{
    CreateInstanceParams, ProviderSelector, SecretFieldInput, SettingsDocument,
};
use stackly_protocol::operation::{GetOperationParams, OperationState};
use stackly_protocol::secret::{
    ListSecretsParams, RevealPurpose, RevealSecretParams, SecretInput, SecretRefSummary,
    SecretStatus, SecretText, SecretValueOrigin,
};
use stackly_protocol::{Call, Reply};
use stackly_testkit::{ReapGuard, TestRoot, cargo_example, fake_server};
use uuid::Uuid;

const PATIENCE: Duration = Duration::from_secs(30);
const PROVIDER: &str = "testing/fake";
/// A fixture value for the explicit secret; it is not a credential.
const EXPLICIT: &str = "explicit-fixture-value-for-the-fake-provider";

/// Seeds `root` with the example and returns the runtime ID and revision.
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

fn params(
    name: &str,
    runtime_id: Uuid,
    revision: &str,
    secrets: Vec<SecretFieldInput>,
) -> CreateInstanceParams {
    CreateInstanceParams {
        name: name.to_owned(),
        display_name: None,
        provider: ProviderSelector {
            name: PROVIDER.to_owned(),
            revision: Some(revision.to_owned()),
        },
        runtime_id,
        endpoints: Vec::new(),
        settings: SettingsDocument::new(serde_json::Map::new()),
        secrets,
    }
}

/// Creates the instance, waits for the operation to succeed, and returns
/// the instance ID.
async fn create(client: &Client, params: CreateInstanceParams, key: &str) -> Uuid {
    let Reply::InstanceCreate(accepted) = client
        .call_detailed(
            Call::InstanceCreate(params),
            CallOptions::default().with_idempotency_key(key),
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
    let id = operation
        .resource
        .as_ref()
        .map(|resource| resource.id.parse::<Uuid>().expect("an instance ID"))
        .expect("the operation names the instance");
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
            return id;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the create did not succeed: {detail:?}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn references(client: &Client) -> Vec<SecretRefSummary> {
    let Ok(Reply::SecretList(listed)) = client
        .call(Call::SecretList(ListSecretsParams::default()))
        .await
    else {
        panic!("secret.list failed");
    };
    listed.items
}

async fn reveal(client: &Client, reference: &str) -> String {
    let Ok(Reply::SecretReveal(revealed)) = client
        .call(Call::SecretReveal(RevealSecretParams {
            reference: reference.to_owned(),
            purpose: RevealPurpose::Show,
        }))
        .await
    else {
        panic!("secret.reveal failed");
    };
    revealed.value.expose().to_owned()
}

#[tokio::test]
async fn a_create_without_secrets_generates_every_secret_field() {
    let root = TestRoot::new().expect("a test root");
    let (runtime_id, revision) = seed(&root);
    let _reap = ReapGuard::new(root.path(), &daemon_paths(&root).runtime);
    let daemon = start_daemon(&root);
    let (client, _) = client(daemon.socket()).await;

    let id = create(
        &client,
        params("generated", runtime_id, &revision, Vec::new()),
        "create-generated",
    )
    .await;

    let listed = references(&client).await;
    assert_eq!(listed.len(), 1, "{listed:?}");
    let summary = &listed[0];
    assert!(
        summary
            .reference
            .starts_with(&format!("local:instance/{id}/")),
        "{}",
        summary.reference
    );
    assert_eq!(summary.status, SecretStatus::Present);
    assert_eq!(summary.value_origin, Some(SecretValueOrigin::Generated));
    let value = reveal(&client, &summary.reference).await;
    assert_eq!(value.chars().count(), 32, "within the field bounds");
    assert!(value.chars().all(|c| c.is_ascii_alphanumeric()));
    let log = std::fs::read_to_string(
        common::daemon_paths(&root)
            .layout
            .daemon_log_dir()
            .join("stacklyd.log"),
    )
    .unwrap_or_default();
    assert!(
        !log.contains(&value),
        "the generated value is not in the daemon log"
    );
}

#[tokio::test]
async fn an_explicit_secret_keeps_the_source_the_client_sent() {
    let root = TestRoot::new().expect("a test root");
    let (runtime_id, revision) = seed(&root);
    let _reap = ReapGuard::new(root.path(), &daemon_paths(&root).runtime);
    let daemon = start_daemon(&root);
    let (client, _) = client(daemon.socket()).await;

    let id = create(
        &client,
        params(
            "explicit",
            runtime_id,
            &revision,
            vec![SecretFieldInput {
                field: "authToken".to_owned(),
                input: SecretInput::Value(SecretText::new(EXPLICIT.to_owned())),
            }],
        ),
        "create-explicit",
    )
    .await;

    let listed = references(&client).await;
    assert_eq!(listed.len(), 1, "{listed:?}");
    let summary = &listed[0];
    assert!(
        summary
            .reference
            .starts_with(&format!("local:instance/{id}/"))
    );
    assert_eq!(summary.value_origin, Some(SecretValueOrigin::Entered));
    assert_eq!(reveal(&client, &summary.reference).await, EXPLICIT);
}
